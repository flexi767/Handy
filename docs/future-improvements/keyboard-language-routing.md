# Keyboard-language routing and retained-audio retry

**Status:** Retained-audio retry and direct macOS language-metadata mapping are
implemented.

## Goal

Handy should treat the active keyboard layout as a strong language hint without
losing a dictation when the wrong keyboard was selected. It must not fall back
to unrestricted language detection or languages outside the user's enabled
keyboards and the loaded model's supported languages.

## Current implementation

The implementation keeps the original captured audio and retries inference; it
does not start a second recording or reconstruct audio from the history file.

### Candidate snapshot

At recording start, `TranscriptionManager::snapshot_recording_language` asks
`keyboard_language::language_candidates_for_recording` for an ordered list:

1. The active keyboard language is first.
2. The other supported languages represented by enabled macOS input sources
   follow.
3. Duplicate base languages and input sources without a usable language are
   removed.

Any valid BCP 47 language can enter the initial list.
`TISCreateInputSourceList` is called with `includeAllInstalled = false`, so
disabled keyboards do not become retry candidates. Before inference, the list
is intersected with the loaded model's advertised languages. If the user
selected an explicit language instead of **Follow keyboard**, the list contains
only that language and no cross-language retry occurs.

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
empty text, including whitespace-only or configured filler-only output. In
**Follow keyboard** mode, a non-empty result is also rejected when all of its
letters use a writing system incompatible with the attempted language. The
check is general: ICU/CLDR likely-subtag data resolves the language's expected
script and Unicode properties classify the transcript; there is no hard-coded
language list. Script-neutral, mixed-script, and composite-script cases fail
open. An engine error also advances to the next candidate. If all candidates
fail or remain unusable, the existing empty/error handling runs; the source
recording is still retained in transcription history according to the normal
history settings.

This fallback currently applies to `transcribe-cpp`, which is the backend used
by the installed Parakeet V3 GGUF model. In addition to the script check, Handy
uses CLDR's main and auxiliary exemplar character sets for the attempted locale.
Letters from the expected script that do not belong to that locale (for example
Ukrainian `і` in a Bulgarian result) mark the transcript as suspicious. Letters
from another script, such as `OpenAI` in Bulgarian text, do not trigger this
alphabet-level check.

On macOS, Handy also checks overlapping two-word spans with Apple's offline
Natural Language framework. This catches confidently mixed same-alphabet text
that a whole-sentence classifier can hide, such as a Bulgarian transcript with
a two-word Russian hallucination. A different language must exceed 90%
confidence; single words are deliberately ignored because names and short words
are noisy. The comparison uses BCP-47 primary subtags rather than a hard-coded
language list. Other platforms fail open until an equivalent system detector is
available.

When the first attempt has the expected script but fails the alphabet or text
language check, Handy loads the downloaded prompt-conditioned Nemotron 3.5 Q8
GGUF as a one-shot fallback and retranscribes the retained in-memory audio with
the exact model locale (`bg-BG`, `de-DE`, and so on). Parakeet remains the
selected and cached primary model. A usable Nemotron result is accepted only
when it passes script, alphabet, and text-language validation. If the fallback
model is missing, fails, or returns another suspicious result, Handy preserves
the original Parakeet text rather than discarding the dictation.

### Tests and runtime verification

The implementation is covered by tests for:

- active-first ordering, deduplication, and rejection of unrelated keyboards;
- explicit-language single-attempt behavior;
- reading current and enabled input sources on macOS;
- conversion of strict locales to model-advertised base codes; and
- retry eligibility using the final-output cleanup rules; and
- CLDR/Unicode script compatibility, including explicit script subtags and
  fail-open behavior for script-neutral or composite-script text; and
- CLDR exemplar-set validation for same-script leakage, auxiliary characters,
  and embedded foreign-script names; and
- conservative BCP-47 text-language mismatch decisions plus the exact mixed
  Bulgarian/Russian Parakeet regression on macOS.

During implementation, a recording that had previously failed because Handy
passed unsupported `en-US` to Parakeet was replayed through the installed app.
The corrected run passed `en` and returned a transcript successfully.

### Saved regression: Latinized Bulgarian output

Keep the exact Parakeet output `Št je svârsza spokojna.` as a future end-to-end
test case for Bulgarian Follow Keyboard dictation. Apple Natural Language
classifies the complete text as Croatian with about 92.5% confidence and the
span `svârsza spokojna` as Polish with about 98.2% confidence. The mixture of
`Š`, `â`, and `sz` is not Bulgarian orthography; Bulgarian should use Cyrillic.

The script validator correctly rejects this output for `bg`. The outstanding
routing gap is that wrong-script output advances through the other installed
keyboard languages, while the forced-language Nemotron fallback currently runs
only for a same-script alphabet or lexical-language conflict. If every keyboard
retry fails, the fail-open behavior preserves the first non-empty Parakeet
result. A future end-to-end regression should verify that retained audio is
instead retried through Nemotron with Bulgarian forced before that result can be
returned.

## macOS language-metadata mapping

The general input-source mapper reads `kTISPropertyInputSourceLanguages` from each
`TISInputSourceRef`. Its value is an ordered array of BCP 47 language
identifiers, and the first entry is the language for which the input source is
intended. Handy:

1. Reads `kTISPropertyInputSourceLanguages` for the active and enabled input
   sources.
2. Normalizes the first non-empty BCP 47 identifier while preserving its
   region, script, and other subtags.
3. Uses its base language to intersect it with the loaded model's supported
   languages and passes the model's exact advertised code to inference.
4. Preserves the existing active-first candidate ordering and retained-audio
   retry behavior.
5. Asks macOS to canonicalize the localized input-source name only as a
   fallback when the metadata is absent or empty.

An input source with non-empty metadata is authoritative; its name cannot
override that metadata. The decoder itself does not encode a fixed list of
languages. This supports renamed and third-party layouts, while the later model
intersection prevents unsupported languages from becoming inference attempts.

## Relevant implementation areas

- `src-tauri/src/keyboard_language.rs`: input-source enumeration, strict
  language mapping, and ordered candidate construction.
- `src-tauri/src/managers/transcription.rs`: recording snapshot, model-code
  resolution, usability check, and retained-audio attempt loop.
- `src-tauri/src/actions.rs`: recording-start snapshot and end-of-processing
  cleanup.
- `src-tauri/src/utils.rs`: cancellation cleanup.
