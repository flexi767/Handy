# Keyboard-language routing and retained-audio retry

**Status:** Retained-audio retry is implemented. Direct macOS language-metadata
mapping is a future improvement.

## Goal

For the local English, German, and Bulgarian setup, Handy should treat the
active keyboard layout as a strong language hint without losing a dictation
when the wrong keyboard was selected. It must not fall back to unrestricted
language detection or languages outside the user's enabled keyboards.

## Current implementation

The implementation keeps the original captured audio and retries inference; it
does not start a second recording or reconstruct audio from the history file.

### Candidate snapshot

At recording start, `TranscriptionManager::snapshot_recording_language` asks
`keyboard_language::language_candidates_for_recording` for an ordered list:

1. The active keyboard language is first.
2. The other supported languages represented by enabled macOS input sources
   follow.
3. Duplicate languages and unsupported layouts are removed.

Only English (`en-US`), German (`de-DE`), and Bulgarian (`bg-BG`) can enter the
list. `TISCreateInputSourceList` is called with `includeAllInstalled = false`,
so disabled keyboards do not become retry candidates. If the user selected an
explicit language instead of **Follow keyboard**, the list contains only that
language and no cross-language retry occurs.

The complete ordered list is frozen for the recording. Changing the keyboard
while speaking therefore cannot change either the first attempt or its
fallback order. The snapshot is cleared after processing, cancellation, or a
failed recording start.

### Model-code resolution

Before inference, every candidate is resolved against the loaded model's
advertised languages. This converts the UI/keyboard locale to the exact code
the model accepts; for the current Parakeet GGUF model, `en-US`, `de-DE`, and
`bg-BG` become `en`, `de`, and `bg`.

`transcribe_cpp_run_plan` performs the same base-language match against the
live `transcribe-cpp` capability metadata and passes the model's advertised
code to the session. A fallback candidate that the live model does not
advertise is skipped rather than silently becoming unrestricted automatic
detection.

### Retained-audio retry

The `LoadedEngine::TranscribeCpp` batch path constructs the ordered attempts
and calls `session.run(&audio, ...)` for each one. Every call borrows the same
in-memory audio vector captured for the original dictation.

Handy stops retrying and returns the first usable transcript. A result is
considered unusable when the same cleanup used for final output reduces it to
empty text, including whitespace-only or configured filler-only output. An
engine error also advances to the next candidate. If all candidates fail or
remain empty, the existing empty/error handling runs; the source recording is
still retained in transcription history according to the normal history
settings.

This fallback currently applies to `transcribe-cpp`, which is the backend used
by the installed Parakeet V3 GGUF model. It deliberately does not retry when a
wrong language produces non-empty but inaccurate text: deciding that such text
is wrong would require confidence scoring or language identification and could
replace a valid transcript incorrectly.

### Tests and runtime verification

The implementation is covered by tests for:

- active-first ordering, deduplication, and rejection of unrelated keyboards;
- explicit-language single-attempt behavior;
- reading current and enabled input sources on macOS;
- conversion of strict locales to model-advertised base codes; and
- retry eligibility using the final-output cleanup rules.

During implementation, a recording that had previously failed because Handy
passed unsupported `en-US` to Parakeet was replayed through the installed app.
The corrected run passed `en` and returned a transcript successfully.

## Future improvement: use macOS language metadata

The current input-source mapper derives the language from the input source ID
and localized name, recognizing values such as `ABC`, `German`, and
`Bulgarian-Phonetic`. This is adequate for the configured Apple layouts but is
less robust for renamed or third-party layouts.

macOS exposes `kTISPropertyInputSourceLanguages` on each `TISInputSourceRef`.
Its value is an ordered array of BCP 47 language identifiers, and the first
entry is the language for which the input source is intended. A future change
should:

1. Read `kTISPropertyInputSourceLanguages` for the active and enabled input
   sources.
2. Canonicalize the first non-empty BCP 47 identifier.
3. Intersect its base language with the locally allowed set (`en`, `de`, `bg`)
   and the loaded model's supported languages.
4. Preserve the existing active-first candidate ordering and retained-audio
   retry behavior.
5. Keep input-source ID/name recognition only as a fallback when the metadata
   is absent or empty.

This would replace layout-name heuristics without broadening the allowed
language set.

## Relevant implementation areas

- `src-tauri/src/keyboard_language.rs`: input-source enumeration, strict
  language mapping, and ordered candidate construction.
- `src-tauri/src/managers/transcription.rs`: recording snapshot, model-code
  resolution, usability check, and retained-audio attempt loop.
- `src-tauri/src/actions.rs`: recording-start snapshot and end-of-processing
  cleanup.
- `src-tauri/src/utils.rs`: cancellation cleanup.

