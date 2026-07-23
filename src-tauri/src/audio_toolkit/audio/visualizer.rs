use rustfft::{num_complex::Complex32, Fft, FftPlanner};
use std::sync::Arc;

// Dynamic range, in dB, above each band's adaptively-tracked noise floor that
// maps to a full-height bar. This is a perceptual span, not a microphone
// calibration: the floor itself tracks whatever "silence" is for the current
// mic, so a quiet built-in mic and a hot external one animate the same without
// any fixed sensitivity threshold.
const DB_RANGE_ABOVE_FLOOR: f32 = 45.0;
// The per-band noise floor falls quickly toward a newly-observed quiet level
// (so a fresh or quiet mic calibrates within a fraction of a second) and rises
// only very slowly (so sustained speech never drags it up).
const FLOOR_ATTACK: f32 = 0.2;
const FLOOR_RELEASE: f32 = 0.001;
const GAIN: f32 = 1.3;
const CURVE_POWER: f32 = 0.7;

pub struct AudioVisualiser {
    fft: Arc<dyn Fft<f32>>,
    window: Vec<f32>,
    bucket_ranges: Vec<(usize, usize)>,
    fft_input: Vec<Complex32>,
    noise_floor: Vec<f32>,
    buffer: Vec<f32>,
    window_size: usize,
    buckets: usize,
}

impl AudioVisualiser {
    pub fn new(
        sample_rate: u32,
        window_size: usize,
        buckets: usize,
        freq_min: f32,
        freq_max: f32,
    ) -> Self {
        let mut planner = FftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(window_size);

        // Pre-compute Hann window
        let window: Vec<f32> = (0..window_size)
            .map(|i| {
                0.5 * (1.0 - (2.0 * std::f32::consts::PI * i as f32 / window_size as f32).cos())
            })
            .collect();

        // Pre-compute bucket frequency ranges
        let nyquist = sample_rate as f32 / 2.0;
        let freq_min = freq_min.min(nyquist);
        let freq_max = freq_max.min(nyquist);

        let mut bucket_ranges = Vec::with_capacity(buckets);

        for b in 0..buckets {
            // Use logarithmic spacing for better perceptual representation
            let log_start = (b as f32 / buckets as f32).powi(2);
            let log_end = ((b + 1) as f32 / buckets as f32).powi(2);

            let start_hz = freq_min + (freq_max - freq_min) * log_start;
            let end_hz = freq_min + (freq_max - freq_min) * log_end;

            let start_bin = ((start_hz * window_size as f32) / sample_rate as f32) as usize;
            let mut end_bin = ((end_hz * window_size as f32) / sample_rate as f32) as usize;

            // Ensure each bucket has at least one bin
            if end_bin <= start_bin {
                end_bin = start_bin + 1;
            }

            // Clamp to valid range
            let start_bin = start_bin.min(window_size / 2);
            let end_bin = end_bin.min(window_size / 2);

            bucket_ranges.push((start_bin, end_bin));
        }

        Self {
            fft,
            window,
            bucket_ranges,
            fft_input: vec![Complex32::new(0.0, 0.0); window_size],
            noise_floor: vec![-40.0; buckets], // Initialize to reasonable noise floor
            buffer: Vec::with_capacity(window_size * 2),
            window_size,
            buckets,
        }
    }

    pub fn feed(&mut self, samples: &[f32]) -> Option<Vec<f32>> {
        // Add new samples to buffer
        self.buffer.extend_from_slice(samples);

        // Only process if we have enough samples
        if self.buffer.len() < self.window_size {
            return None;
        }

        // Take the required window of samples
        let window_samples = &self.buffer[..self.window_size];

        // Remove DC component
        let mean = window_samples.iter().sum::<f32>() / self.window_size as f32;

        // Apply window function and prepare FFT input
        for (i, &sample) in window_samples.iter().enumerate() {
            let windowed_sample = (sample - mean) * self.window[i];
            self.fft_input[i] = Complex32::new(windowed_sample, 0.0);
        }

        // Perform FFT
        self.fft.process(&mut self.fft_input);

        // Compute power spectrum and bucket levels
        let mut buckets = vec![0.0; self.buckets];

        for (bucket_idx, &(start_bin, end_bin)) in self.bucket_ranges.iter().enumerate() {
            if start_bin >= end_bin || end_bin > self.fft_input.len() / 2 {
                continue;
            }

            // Calculate average power in this frequency range
            let mut power_sum = 0.0;
            for bin_idx in start_bin..end_bin {
                let magnitude = self.fft_input[bin_idx].norm();
                power_sum += magnitude * magnitude;
            }

            let avg_power = power_sum / (end_bin - start_bin) as f32;

            // Convert to dB with proper scaling
            let db = if avg_power > 1e-12 {
                20.0 * (avg_power.sqrt() / self.window_size as f32).log10()
            } else {
                -80.0 // Very low floor for zero power
            };

            // Track the per-band noise floor: adopt a new quiet minimum quickly,
            // drift up slowly, and leave it untouched on loud (speech) frames
            // more than 10 dB above the floor so speech never raises it.
            let floor = self.noise_floor[bucket_idx];
            if db < floor {
                self.noise_floor[bucket_idx] = FLOOR_ATTACK * db + (1.0 - FLOOR_ATTACK) * floor;
            } else if db < floor + 10.0 {
                self.noise_floor[bucket_idx] = FLOOR_RELEASE * db + (1.0 - FLOOR_RELEASE) * floor;
            }

            // Normalize relative to this band's own noise floor — no fixed
            // sensitivity threshold — then apply gain and curve shaping.
            let normalized =
                ((db - self.noise_floor[bucket_idx]) / DB_RANGE_ABOVE_FLOOR).clamp(0.0, 1.0);
            buckets[bucket_idx] = (normalized * GAIN).powf(CURVE_POWER).clamp(0.0, 1.0);
        }

        // Apply light smoothing to reduce jitter
        for i in 1..buckets.len() - 1 {
            buckets[i] = buckets[i] * 0.7 + buckets[i - 1] * 0.15 + buckets[i + 1] * 0.15;
        }

        // Clear processed samples from buffer
        self.buffer.clear();

        Some(buckets)
    }

    pub fn reset(&mut self) {
        self.buffer.clear();
        // Reset noise floor to initial values
        self.noise_floor.fill(-40.0);
    }
}

#[cfg(test)]
mod tests {
    use super::AudioVisualiser;

    fn tone(samples: usize, freq: f32, sample_rate: f32, amplitude: f32) -> Vec<f32> {
        (0..samples)
            .map(|i| (2.0 * std::f32::consts::PI * freq * i as f32 / sample_rate).sin() * amplitude)
            .collect()
    }

    #[test]
    fn silence_stays_flat() {
        let mut vis = AudioVisualiser::new(48_000, 2_048, 16, 400.0, 4_000.0);
        let levels = vis.feed(&vec![0.0; 2_048]).unwrap();
        assert!(
            levels.iter().all(|l| *l == 0.0),
            "silence moved: {levels:?}"
        );
    }

    #[test]
    fn quiet_speech_shows_above_calibrated_silence() {
        // The realistic case that fixed thresholds broke on a soft built-in mic:
        // the floor calibrates to true silence, then a quiet speech-level tone
        // must register clear movement above it — with no fixed sensitivity
        // constant. (A sustained tone with no pauses is intentionally absorbed
        // into the floor; real speech varies and keeps showing.)
        let mut vis = AudioVisualiser::new(48_000, 2_048, 16, 400.0, 4_000.0);
        for _ in 0..30 {
            let _ = vis.feed(&vec![0.0; 2_048]);
        }
        let samples = tone(2_048, 1_000.0, 48_000.0, 0.02);
        let peak = vis
            .feed(&samples)
            .unwrap()
            .iter()
            .copied()
            .fold(0.0_f32, f32::max);
        assert!(peak > 0.2, "quiet speech didn't show above silence: {peak}");
    }

    #[test]
    fn louder_input_reads_higher_than_quieter_input() {
        // Relative-to-floor normalization must still preserve loudness ordering.
        let mut quiet_vis = AudioVisualiser::new(48_000, 2_048, 16, 400.0, 4_000.0);
        let mut loud_vis = AudioVisualiser::new(48_000, 2_048, 16, 400.0, 4_000.0);
        let quiet = tone(2_048, 1_000.0, 48_000.0, 0.01);
        let loud = tone(2_048, 1_000.0, 48_000.0, 0.2);
        let mut quiet_peak: f32 = 0.0;
        let mut loud_peak: f32 = 0.0;
        for _ in 0..10 {
            quiet_peak = quiet_peak.max(
                quiet_vis
                    .feed(&quiet)
                    .unwrap()
                    .iter()
                    .copied()
                    .fold(0.0, f32::max),
            );
            loud_peak = loud_peak.max(
                loud_vis
                    .feed(&loud)
                    .unwrap()
                    .iter()
                    .copied()
                    .fold(0.0, f32::max),
            );
        }
        assert!(
            loud_peak > quiet_peak,
            "louder input did not read higher: loud={loud_peak} quiet={quiet_peak}"
        );
    }
}
