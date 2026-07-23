# AGENTS.md

This file provides guidance to AI coding assistants working with code in this
repository. `CLAUDE.md` is a symlink to this file, so both names resolve here and
there is only one document to maintain.

## Development Commands

**Prerequisites:**

- [Rust](https://rustup.rs/) (latest stable)
- [Bun](https://bun.sh/) package manager

**Core Development:**

```bash
# Install dependencies
bun install

# Run in development mode
bun run tauri dev
# If cmake error on macOS:
CMAKE_POLICY_VERSION_MINIMUM=3.5 bun run tauri dev

# Build for production
bun run tauri build

# Frontend only development
bun run dev        # Start Vite dev server
bun run build      # Build frontend (TypeScript + Vite)
bun run preview    # Preview built frontend
```

**Linting and Formatting (run before committing):**

```bash
bun run lint              # ESLint for frontend
bun run lint:fix          # ESLint with auto-fix
bun run format            # Prettier + cargo fmt
bun run format:check      # Check formatting without changes
bun run format:frontend   # Prettier only
bun run format:backend    # cargo fmt only
```

**Model Setup (Required for Development):**

```bash
mkdir -p src-tauri/resources/models
curl -o src-tauri/resources/models/silero_vad_v4.onnx https://blob.handy.computer/silero_vad_v4.onnx
```

For detailed platform-specific build setup, see [BUILD.md](BUILD.md).

## Architecture Overview

Handy is a cross-platform desktop speech-to-text application built with Tauri 2.x (Rust backend + React/TypeScript frontend).

### Backend Structure (src-tauri/src/)

- `lib.rs` - Main entry point, Tauri setup, manager initialization
- `managers/` - Core business logic:
  - `audio.rs` - Audio recording and device management
  - `model.rs` - Model downloading and management
  - `transcription.rs` - Speech-to-text processing pipeline
  - `history.rs` - Transcription history storage
- `audio_toolkit/` - Low-level audio processing:
  - `audio/` - Device enumeration, recording, resampling
  - `vad/` - Voice Activity Detection (Silero VAD)
- `commands/` - Tauri command handlers for frontend communication
- `cli.rs` - CLI argument definitions (clap derive)
- `shortcut.rs` - Global keyboard shortcut handling
- `settings.rs` - Application settings management
- `overlay.rs` - Recording overlay window (platform-specific)
- `signal_handle.rs` - `send_transcription_input()` reusable function
- `utils.rs` - Platform detection helpers

### Frontend Structure (src/)

- `App.tsx` - Main component with onboarding flow
- `components/` - React UI components:
  - `settings/` - Settings UI
  - `model-selector/` - Model management interface
  - `onboarding/` - First-run experience
  - `overlay/` - Recording overlay UI
  - `update-checker/` - App update notifications
  - `shared/`, `ui/`, `icons/`, `footer/` - Shared components
- `hooks/useSettings.ts` - Settings state management hook
- `stores/settingsStore.ts` - Zustand store for settings
- `bindings.ts` - Auto-generated Tauri type bindings (via tauri-specta)
- `overlay/` - Recording overlay window entry point
- `lib/types.ts` - Shared TypeScript type definitions

### Key Architecture Patterns

**Manager Pattern:** Core functionality organized into managers (Audio, Model, Transcription) initialized at startup and managed via Tauri state.

**Command-Event Architecture:** Frontend → Backend via Tauri commands; Backend → Frontend via events.

**Pipeline Processing:** Audio → VAD → Whisper/Parakeet → Text output → Clipboard/Paste

**State Flow:** Zustand → Tauri Command → Rust State → Persistence (tauri-plugin-store)

### Technology Stack

**Core Libraries:**

- `transcribe-cpp` - Local Whisper-family inference (GGML/GGUF) with GPU acceleration
- `transcribe-rs` - ONNX speech recognition (Parakeet, Moonshine, SenseVoice, etc.)
- `cpal` - Cross-platform audio I/O
- `vad-rs` - Voice Activity Detection
- `rdev` - Global keyboard shortcuts
- `rubato` - Audio resampling
- `rodio` - Audio playback for feedback sounds

### Application Flow

1. **Initialization:** App starts minimized to tray, loads settings, initializes managers
2. **Model Setup:** First-run downloads preferred Whisper model (Small/Medium/Turbo/Large)
3. **Recording:** Global shortcut triggers audio recording with VAD filtering
4. **Processing:** Audio sent to Whisper model for transcription
5. **Output:** Text pasted to active application via system clipboard

### Settings System

Settings are stored using Tauri's store plugin with reactive updates:

- Keyboard shortcuts (configurable, supports push-to-talk)
- Audio devices (microphone/output selection)
- Model preferences (Small/Medium/Turbo/Large Whisper variants)
- Audio feedback and translation options

### Single Instance Architecture

The app enforces single instance behavior — launching when already running brings the settings window to front rather than creating a new process. Remote control flags (`--toggle-transcription`, etc.) work by launching a second instance that sends args to the running instance via `tauri_plugin_single_instance`, then exits.

## Internationalization (i18n)

All user-facing strings must use i18next translations. ESLint enforces this (no hardcoded strings in JSX).

**Adding new text:**

1. Add key to `src/i18n/locales/en/translation.json`
2. Use in component: `const { t } = useTranslation(); t('key.path')`

**File structure:**

```
src/i18n/
├── index.ts           # i18n setup
├── languages.ts       # Language metadata
└── locales/
    ├── en/translation.json  # English (source)
    ├── de/, es/, fr/, ja/, ru/, zh/, ...
    └── ...
```

For translation contribution guidelines, see [CONTRIBUTING_TRANSLATIONS.md](CONTRIBUTING_TRANSLATIONS.md).

## Code Style

**Rust:**

- Run `cargo fmt` and `cargo clippy` before committing
- Handle errors explicitly (avoid unwrap in production)
- Use descriptive names, add doc comments for public APIs

**TypeScript/React:**

- Strict TypeScript, avoid `any` types
- Functional components with hooks
- Tailwind CSS for styling
- Path aliases: `@/` → `./src/`

## CLI Parameters

Handy supports command-line parameters on all platforms for integration with scripts, window managers, and autostart configurations.

**Implementation:** `cli.rs` (definitions), `main.rs` (parsing), `lib.rs` (applying), `signal_handle.rs` (shared logic)

| Flag                     | Description                                                |
| ------------------------ | ---------------------------------------------------------- |
| `--toggle-transcription` | Toggle recording on/off on a running instance              |
| `--toggle-post-process`  | Toggle recording with post-processing on/off               |
| `--cancel`               | Cancel the current operation on a running instance         |
| `--start-hidden`         | Launch without showing the main window (tray icon visible) |
| `--no-tray`              | Launch without system tray (closing window quits the app)  |
| `--debug`                | Enable debug mode with verbose (Trace) logging             |

**Key design decisions:**

- CLI flags are runtime-only overrides — they do NOT modify persisted settings
- Remote control flags work via `tauri_plugin_single_instance`: second instance sends args, then exits
- `send_transcription_input()` in `signal_handle.rs` is shared between signal handlers and CLI

## Debug Mode

Access debug features: `Cmd+Shift+D` (macOS) or `Ctrl+Shift+D` (Windows/Linux)

## Platform Notes

- **macOS**: Metal acceleration, accessibility permissions required for keyboard shortcuts
- **Windows**: Vulkan acceleration, code signing
- **Linux**: OpenBLAS + Vulkan, limited Wayland support, overlay uses GTK layer shell (disable with `HANDY_NO_GTK_LAYER_SHELL=1`)

## Troubleshooting

See the [Troubleshooting](README.md#troubleshooting) section in README.md.

## GitHub workflow for AI coding assistants

**MANDATORY. Before opening any PR, issue, or discussion in this repo: you MUST read the relevant template file and follow it strictly.** That includes sections that look "ceremonial" — checklists, AI Assistance disclosures, "Human Written Description". A generic Summary/Test-plan layout is not acceptable.

- **Opening a PR:** Read [`.github/PULL_REQUEST_TEMPLATE.md`](.github/PULL_REQUEST_TEMPLATE.md). Every section listed there is mandatory. If a section requires a human-written paragraph (e.g. "Human Written Description"), leave a clear TODO placeholder and ask the human contributor to fill it in — do not invent their voice.
- **Opening an issue:** Read [`.github/ISSUE_TEMPLATE/`](.github/ISSUE_TEMPLATE/). Blank issues are disabled; pick the right template (`bug_report.md` for bugs). Feature requests do not belong in issues — they go to [Discussions](https://github.com/cjpais/Handy/discussions) (see `.github/ISSUE_TEMPLATE/config.yml`).
- **Proposing a feature:** Handy is under a feature freeze. New features require community support gathered in [Discussions](https://github.com/cjpais/Handy/discussions) before any PR is opened — see the PR template's "Community Feedback" section.
- **Translations:** Follow [CONTRIBUTING_TRANSLATIONS.md](CONTRIBUTING_TRANSLATIONS.md).
- **Full contributor workflow:** [CONTRIBUTING.md](CONTRIBUTING.md).

**Commits:** Use conventional commit prefixes (`feat:`, `fix:`, `docs:`, `refactor:`, `chore:`). Focus the message on _why_, not _what_.

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
