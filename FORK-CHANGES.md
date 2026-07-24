# Fork changes — flexi767/Handy

A detailed record of everything changed in this fork on top of `cjpais/Handy`
(`upstream/main`), in the order it happened. Net diff: **16 files, ~+1522 / −53**
across 17 commits.

The overall goal: make **dictating in multiple languages** work well when the
transcription language should follow the **active keyboard layout** — the
motivating case being Bulgarian, which the stock models transcribe poorly.

---

## 1. Follow Keyboard Language (the core feature)

**Commit:** `d613040` `feat: add Follow Keyboard Language option (macOS)`

Adds a "Follow Keyboard Language" option to the transcription-language picker.
When selected, the language for a recording is taken from the active macOS
keyboard layout at the moment recording starts.

- New module `src-tauri/src/keyboard_language.rs` reads the active input source
  via Carbon TIS (`TISCopyCurrentKeyboardInputSource`) and reduces its BCP-47 tag
  to a base subtag (`en-US` → `en`).
- **No languages are hardcoded.** Whatever tag the layout advertises is passed to
  the existing model-language resolution (`managers::model::effective_language`),
  which intersects it with the selected model's supported languages. An
  unsupported/unavailable language falls back to auto-detect, exactly as an
  explicit choice would.
- Resolution is a snapshot at record start — no background thread, no polling,
  and the persisted setting is never rewritten.
- Frontend: the option appears in the picker on macOS only; other platforms
  resolve it to auto.

This was a deliberate clean re-implementation. An earlier version of the same
idea (preserved on branch `legacy/keyboard-language-v1`) hardcoded EN/DE/BG,
pulled in five dependencies, and silently rewrote the user's persisted language
on load. The clean version has none of those problems.

---

## 2. Wrong-language recovery

**Commit:** `01abea1` `feat: recover follow-keyboard dictation in the wrong language`

A safety net for when the primary model returns a language the user does not
type (e.g. Bulgarian audio mis-detected as Russian). It validates the primary
transcript against the user's enabled keyboard languages and, on a conflict,
retries the retained audio with the language forced.

- Language conflict is detected with Apple's `NLLanguageRecognizer`
  (`src-tauri/src/language_validator.rs`) — generic across languages, no
  per-language rules. Conservative by design: it only reports a conflict on
  strong evidence, so legitimate mixed-language dictation is preserved.
- The retry model is chosen **by capability, not by ID**: any downloaded
  transcribe-cpp model that accepts a forced language and advertises one of the
  user's keyboard languages qualifies, most-accurate first.
- New deps (macOS only): `objc2`, `objc2-foundation`, `objc2-natural-language`,
  and `dispatch` (added later).

This feature then needed a long chain of fixes (sections 3–4, 6–8) before it
actually worked end to end.

---

## 3. The crash saga — Carbon TIS on the wrong thread

Three commits, and the single most instructive episode of the whole session.

**Symptom:** the app aborted with `EXC_BREAKPOINT` on a `tokio-runtime-worker`
thread the first time recovery ran.

**Cause:** Carbon's input-source *enumeration* (`TISCreateInputSourceList`)
asserts it is running on the main dispatch queue and kills the process otherwise.
`transcribe()` runs on a `spawn_blocking` worker, so enumerating the enabled
keyboards from there aborted. (At the time we believed the single *current-source*
lookup used for routing was safe off the main thread — section 13 shows that was
wrong on macOS 26+.)

- `327f57b` — first attempt, marshalling via Tauri's `run_on_main_thread`.
- `78bc0dc` — second attempt, hopping onto the real main dispatch queue via
  libdispatch (`dispatch::Queue::main().exec_async`).
- `a19d5fd` — a `pthread_main_np()` guard so the hop is skipped (and run inline)
  when already on the main thread, avoiding a deadlock on the headless CLI path.

**The lesson (worth remembering):** the first two fixes *looked* correct but the
app kept crashing identically. The real problem was a **stale incremental
build** — the compiler was reusing an old object file, so none of the fixes were
actually in the running binary. A `cargo clean` of our crate finally compiled
them in, and the crash was gone. Multiple "failed" fixes were actually correct
all along. When correct code produces impossible behaviour, suspect the build.

The final mechanism is the libdispatch hop with the main-thread guard.

---

## 4. Fallback selection — the `bg` vs `bg-BG` mismatch

**Commit:** `75ab6ee` `fix: match fallback language on base subtag, not exact locale`

Recovery selected the Nemotron fallback correctly but never ran it. The retry
loop matched candidate base subtags (`bg`) against the loaded session's language
list using exact string equality, and Nemotron advertises full BCP-47 locales
(`bg-BG`). Every candidate was skipped, so recovery always returned `None`.

Fixed by matching on the base subtag and forcing the model's own locale code, so
`bg` resolves to the model's `bg-BG`. This was the change that made recovery
actually run Nemotron for the first time (verified end to end in the logs).

Diagnosis required temporary file-based logging to dump every model's fields and
the per-candidate decision, since the failure was invisible in normal logs.

---

## 5. Adaptive recording waveform

**Commit:** `9a072f5` `feat: auto-calibrate the recording waveform to the microphone`

The overlay waveform barely moved on quieter built-in microphones. The
visualizer normalized each frequency band against a fixed `-55 dB` floor; the
per-band **noise floor was already tracked every frame but never used**.

Now each band is normalized relative to its own adaptively-tracked noise floor —
no fixed sensitivity threshold — with the floor falling quickly toward a new
quiet level and rising only slowly. The waveform calibrates to whatever the
mic's silence level is, so quiet and loud mics animate the same. Loud (speech)
frames never raise the floor, so speech keeps showing. A sustained flat tone is
intentionally absorbed into the floor.

---

## 6. Follow-keyboard routing: auto-detect, not force

**Commit:** `c7f432e` `fix: let keyboard-following auto-detect instead of forcing the layout`

The original design forced the active layout's language on every recording. That
turned out to be actively harmful: **forcing a language the speech is not in
makes Whisper translate rather than transcribe.** Speaking Bulgarian with an
English layout produced fluent English, and recovery could not catch it because
English is itself an enabled keyboard language.

Changed so a detection-capable model is left on **auto**, with recovery
constraining the result to the enabled keyboards afterwards. Only must-pick
models (no detection) still resolve to the active layout, since they must be told
something. Semantically: keyboard-following now means "transcribe whichever of my
keyboard languages I am speaking", not "assume I am speaking the active layout".

---

## 7. Recovery retries on the loaded primary first

**Commit:** `58650eb` `fix: retry on the loaded primary before falling back to another model`

Recovery always loaded a *different* model, so with Whisper as primary it retried
on parakeet — whose Bulgarian is far weaker — and the forced attempts failed
validation, keeping the wrong transcript. The same phrase transcribed correctly
on Whisper seconds later, so the better model was there all along.

Now recovery retries on the **already-loaded primary** first (the model the user
chose, already in memory, and a forced run is genuinely different from the
auto-detect that just failed). It is skipped when the primary cannot be given a
language — a detect-only engine would just repeat itself — falling back to a
different model in that case.

---

## 8. Recovery tries English last

**Commit:** `8bff6cb` `fix: try English last when recovering the dictation language`

Recovery took the first candidate that validated, in active-keyboard-first order.
With an English layout that meant forcing English first — and forcing `en` on
non-English audio yields fluent English (Whisper's translation target) which
always passes validation, masking a correct result from another candidate.

Now English is ordered last so a forced-English pass cannot translate over a
correct result. It stays reachable as a last resort for short/noisy clips where
detection mis-fires on genuinely English speech.

---

## 9. Short-clip handling

Two related fixes for very short recordings, where Whisper is unreliable.

**Commit:** `37fb6cb` `fix: stop one dictation seeding the next on short clips`

Saying "just this" came back as "Bis zum nächsten Mal." (a German farewell) after
an earlier German dictation. That is Whisper conditioning on the previous
transcript's tokens when a clip is too short to constrain the decoder; the
session is cached across recordings, so the bleed carries between dictations.
Disabled `condition_on_prev_tokens` for whisper runs. Previously the whisper
decode extension was only attached when custom words happened to be set, so with
none configured no decode options were passed at all — now attached by
architecture, and carried into the recovery retries too.

**Commit:** `919a99e` `fix: force the keyboard language on short clips instead of auto-detecting`

Auto-detection is unreliable on ~1 second of audio (a short Bulgarian "работи"
gets guessed as Italian). Below **1.5 s** of retained audio, keyboard-following
now forces the active layout's language rather than guessing; longer clips keep
auto-detect. Trade-off: this assumes the keyboard matches the spoken language,
which is the normal case in real use. (Streaming and post-processing have no
fixed clip and keep auto-detect.)

Known residual: a short clip can also be short because a longer phrase was
**truncated** (key released early / mic cold-start). Forcing the language cannot
help incomplete audio — that is a capture problem, not a routing one.

---

## 10. Active-keyboard-language indicator

**Commits:** `485258a` `feat: show the active keyboard language in front of the waveform`,
`9acd73c` `fix: move the language code beside the dot, out of the grid flow`

Shows the active layout's two-letter code (e.g. `BG`, `EN`) in the recording
overlay, so a keyboard/speech mismatch is visible at a glance. The `show-overlay`
event now carries `{ state, language }`; the code reads only the current input
source (safe off the main thread).

Positioned **beside the status dot**, absolutely positioned so it sits out of the
grid flow — putting it in the left grid cell would have grown the equal `1fr`
side columns and squeezed the centered waveform (the same failure mode as the
early "frozen waveform").

---

## 11. Model change (settings, not code)

Switched the primary model to **Whisper Large v3 Turbo** (Q8_0, ~845 MB),
downloaded into the Hugging Face cache with SHA-256 verification.

Why: Whisper is trained on far more multilingual/Cyrillic data than the NVIDIA
models, so it transcribes Bulgarian correctly where parakeet garbled it. The
catalog's general accuracy score (parakeet 88 > Whisper 87) is English-weighted
and misleading for lower-resource languages. Trade-off: Whisper Turbo is slower
(speed 35 vs parakeet 79).

| Model | Role | Params | Catalog acc | Speed | Langs |
| --- | --- | --- | --- | --- | --- |
| Whisper Large v3 Turbo | primary (now) | 809M | 87 | 35 | 100 |
| Parakeet TDT 0.6B v3 | previous primary | 0.6B | 88 | 79 | 25 |
| Nemotron Streaming 3.5 | recovery fallback | 0.6B | 82 | 84 | 28 |

Machine-local settings (in `settings_store.json`, not the repo):
`selected_model` = Whisper Turbo, `selected_language` = `follow_keyboard`,
`extra_recording_buffer_ms` = 100.

---

## 12. Documentation restructure

**Commits:** `c769f92`, `b686a85`, `e283a8d`

Recorded the fork-local setup and language-routing decisions, then arranged
`AGENTS.md` as the single source with `CLAUDE.md` a symlink to it (matching
upstream's AGENTS-as-base convention, but via a real symlink so both names
resolve to one document).

---

## 13. The crash saga, part two — `TISGetInputSourceProperty` also needs main

The section-3 fix routed the *enumeration* to the main queue but left the
*current-source* reads running wherever they were called, on the stated
assumption that reading a property off the current input source is safe off the
main thread. On **macOS 26+ that assumption is false**, and it surfaced as a
fresh-boot `EXC_BREAKPOINT` on a worker thread from the overlay-show path
(`active_language_code`).

**Cause:** `TISGetInputSourceProperty` validates the source ref via
`isValidateInputSourceRef` → `islGetInputSourceListWithAdditions`, which calls
`dispatch_assert_queue(main)` and aborts off the main queue — even when the
property belongs to the *current* source, not an enumeration. It is
**timing-dependent**: the assert only fires while the input-source list cache is
cold (just after login), so an off-main read runs fine for a whole session and
then crashes on the next boot. That is exactly how it presented — the crashes
began immediately after a system restart.

**Fix:** generalize the libdispatch hop into a single `run_on_main` helper
(keeping the `pthread_main_np` guard so nested/CLI calls run inline) and route
**every** TIS access through it — the current-source read as well as the
enumeration. `active_language_code`, `enabled_keyboard_languages`, and the
recovery entry point all now funnel through the main queue.

**The lesson:** "reads the current source, doesn't enumerate" was not a safe
proxy for "doesn't touch the input-source list". On modern macOS, treat *all*
Text Input Source calls as main-queue-only.

---

## Current state

- **Pushed to `origin/main`:** everything through `919a99e`.
- **Committed but NOT pushed:** `485258a` and `9acd73c` (the language indicator).
- `main` is 17 commits over `upstream/main`; `legacy/keyboard-language-v1`
  preserves the original implementation.
- The pre-existing test `catalog::tests::catalog_architectures_are_known_to_capability_probe`
  fails on clean upstream too (unrelated `moss` architecture); everything else
  passes.

## Known limitations (not fixable by routing)

- **Short/truncated clips** — the first recording after idle loses ~57 ms to mic
  stream setup (`build_stream`), and releasing the key early truncates the
  phrase. `always_on_microphone` would remove the cold-start loss (at the cost of
  the mic staying active). Whisper also mishears or hallucinates on very short
  clips regardless of language.
- **Wrong-language output that mimics an enabled keyboard** — recovery only fires
  when the output lands *outside* all enabled keyboards, so Bulgarian misheard as
  English slips through.
- **Bulgarian vs Russian** — Apple's detector can confuse the two (both Cyrillic);
  a correct Bulgarian retry could in principle be rejected as Russian.
