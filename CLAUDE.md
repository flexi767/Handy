Read @AGENTS.md

---

# Fork-local notes (flexi767/Handy)

Everything below is specific to this fork and this machine. It is **not**
upstream behaviour — keep it out of any PR sent to `cjpais/Handy`.

## Local macOS signing config

**`src-tauri/tauri.conf.json` must keep `"signingIdentity": "-"`.** It is the
shared config owned by upstream; a real identity there breaks the build for every
other contributor and leaks into any PR diff that touches the file.

Local signing settings belong in `src-tauri/tauri.local.conf.json` (gitignored),
which `scripts/build-install-macos.sh` passes via `tauri build --config`. Tauri
merges it over the base config, so it only needs the keys that differ
(`signingIdentity`, `entitlements`, `minimumSystemVersion`).

Never "fix" a signing failure by editing `tauri.conf.json`; edit the local config
or the build script instead.

## Runtime settings on this machine

These live in `~/Library/Application Support/com.pais.handy/settings_store.json`
(under a top-level `"settings"` key), **not** in the repo. The app caches them in
memory, so edit the file *and restart the app* — otherwise the change is ignored
or overwritten on the next save.

| setting | value | why |
| --- | --- | --- |
| `selected_model` | `handy-computer/whisper-large-v3-turbo-gguf/whisper-large-v3-turbo-Q8_0.gguf` | Whisper handles Bulgarian far better than parakeet/nemotron, which skew toward English and major languages |
| `selected_language` | `follow_keyboard` | transcribe whichever installed keyboard language is being spoken |
| `extra_recording_buffer_ms` | `100` | keeps the mic open briefly after key release so trailing words survive |

Model IDs are `<hf-repo>/<filename>`. Models are discovered from the Hugging Face
cache (`~/.cache/huggingface/hub/models--<org>--<repo>/`), which needs the full
layout: `blobs/<sha256>`, `snapshots/<commit>/<file>` symlinked to the blob, and
`refs/main` containing the commit. `is_downloaded` is a check for that exact
file, so a model only counts as present if the *default quant* is the one on disk.

Settings backups from past changes: `settings_store.json.bak-*`.

## VAD tuning — do not loosen

Verified against the code (`audio_toolkit/vad/`), frames are 30 ms:

- `VAD_THRESHOLD` 0.3 (permissive; Silero's usual default is 0.5)
- `VAD_ONSET_FRAMES` 2 → only 60 ms of speech needed to trigger
- `VAD_PREFILL_FRAMES` 15 → 450 ms *before* the trigger is retained, so the
  leading edge is already protected
- `VAD_OFFLINE_HANGOVER_FRAMES` 15 → 450 ms kept after speech ends

A `sample count: 0` recording therefore means no speech was detected at all, not
that the VAD was too strict. Loosening these would feed silence and noise to
Whisper, which reliably produces hallucinated text. `always_on_microphone` buys
almost nothing here because of the 450 ms prefill; the only genuinely uncaptured
window is the ~57 ms between keypress and the mic stream opening.

## Follow-keyboard language routing

Design decisions worth not re-litigating:

- A detection-capable model is left on **auto**, not forced to the active layout.
  Forcing a language the speech is not in makes Whisper *translate* into it (the
  `language` token tells the decoder what to emit — the `translate` task is not
  involved and turning it off does not help).
- Wrong-language recovery constrains the result to the enabled keyboard languages
  afterwards, retrying on the **loaded primary** when it accepts a forced
  language, and only otherwise on a different downloaded model.
- Recovery tries **English last**: forcing `en` yields fluent English that always
  passes validation and would mask a correct result from another candidate.
- Candidate languages are matched on the **base subtag** (`bg`), because models
  advertise either base codes (parakeet) or full locales (`bg-BG`, nemotron).
