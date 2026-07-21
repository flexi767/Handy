# Future improvement: Core ML inference for Parakeet on macOS

**Status:** Deferred. We are not pursuing this work at this time.

## Motivation

On Apple Silicon, Core ML may improve native efficiency and battery life by
allowing inference to use the CPU, GPU, and Apple Neural Engine (ANE). Handy's
current Parakeet V3 path uses GGUF through `transcribe-cpp`, with Metal
acceleration on macOS. GGUF models cannot be executed directly by Core ML, so a
Core ML path would require a separate model and inference backend.

Handy's cross-platform architecture remains valuable: Metal serves macOS while
CUDA or Vulkan can serve other platforms. Any Core ML work should remain a
macOS-specific optimization rather than replacing the portable backends.

## Lowest-effort experiment: ONNX Runtime Core ML

Handy already supports the ONNX Parakeet V3 model
`parakeet-tdt-0.6b-v3-int8` through `transcribe-rs`. The smallest experiment is
to compile `transcribe-rs` with its `ort-coreml` feature on macOS and keep the
ONNX Runtime accelerator setting on `Auto`. With that feature present,
`transcribe-rs` attempts the Core ML execution provider before falling back to
CPU.

An explicit `CoreMl` option could subsequently be added to Handy's
`OrtAcceleratorSetting`, generated TypeScript bindings, translations, and
settings UI. This is optional for the initial experiment because `Auto` can
select Core ML without a dedicated setting.

The experiment should also configure a persistent Core ML model-cache directory
if the dependency stack exposes one. Otherwise, ONNX-to-Core-ML compilation may
add significant load time after each restart.

### Expected limitations

- ONNX Runtime converts compatible graph partitions to Core ML; it is not the
  same execution path as a native Core ML implementation such as FluidAudio.
- Unsupported operators and dynamic shapes may cause parts of the graph to run
  on CPU.
- Enabling the execution provider does not guarantee that the full model, or
  even every compatible partition, runs on the ANE.
- Initial graph conversion and compilation can add cold-start latency.
- The existing GGUF download cannot be reused; the ONNX model is a separate
  download and cache entry.
- Adoption should depend on measurements, not the presence of a Core ML label.

## Higher-effort native option: FluidAudio bridge

For an inference path closer to VoiceInk's native behavior, add a macOS-only
FluidAudio backend. Handy already builds a Swift bridge for Apple Intelligence,
so the repository has a precedent for linking Swift code into the Rust/Tauri
application. A FluidAudio integration would nevertheless be a substantial
feature, requiring:

- Swift package integration and a stable C ABI between Swift and Rust;
- model download, conversion, versioning, and cache management;
- integration with Handy's transcription-engine lifecycle;
- cancellation, language selection, progress, and error plumbing; and
- macOS-specific packaging, signing, and regression coverage.

This route is more likely to realize native Core ML behavior, but it is not an
easy switch and should only be undertaken if the ONNX experiment is materially
worse in measured efficiency or accuracy.

## Evaluation plan if revisited

Benchmark the same representative English, German, and Bulgarian recordings
against:

1. the current Parakeet V3 GGUF/Metal path;
2. Parakeet V3 ONNX through the Core ML execution provider; and
3. a native FluidAudio backend, if implemented.

Record cold model-load time, first-transcription latency, warm real-time factor,
peak memory, macOS Energy Impact or measured power, word error rate, and observed
CPU/GPU/ANE utilization. Profile execution-provider fallback so good aggregate
latency does not conceal excessive CPU use.

## Relevant implementation areas

- `src-tauri/Cargo.toml`: current `transcribe-rs` feature selection.
- `src-tauri/src/managers/transcription.rs`: ONNX model creation and accelerator
  selection.
- `src-tauri/src/settings.rs`: available ONNX Runtime accelerator settings.
- `src-tauri/build.rs`: existing macOS Swift build bridge.

## References

- [transcribe-rs](https://github.com/cjpais/transcribe-rs)
- [ONNX Runtime CoreML Execution Provider](https://onnxruntime.ai/docs/execution-providers/CoreML-ExecutionProvider.html)
- [Apple Core ML documentation](https://developer.apple.com/documentation/CoreML)

No runtime, model-download, or settings behavior is changed by this note.
