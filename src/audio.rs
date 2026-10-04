use std::f64::consts::TAU;
use std::time::Duration;

pub const SAMPLE_RATE: u32 = 48_000;
pub const CHANNELS: usize = 2;
pub const DEFAULT_DURATION_MS: u64 = 300;
pub const DEFAULT_FREQUENCY_HZ: f64 = 100.0;
pub const DEFAULT_AMPLITUDE: f64 = 0.0005;

/// Validated signal settings. Amplitude is PCM peak relative to full scale,
/// not the system, speaker, or PipeWire stream volume.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PulseParameters {
    duration_ms: u64,
    frequency_hz: f64,
    amplitude: f64,
}

impl PulseParameters {
    pub fn new(duration_ms: u64, frequency_hz: f64, amplitude: f64) -> Result<Self, String> {
        if !(20..=5000).contains(&duration_ms) {
            return Err("--duration must be an integer between 20 and 5000 milliseconds".into());
        }
        if !frequency_hz.is_finite()
            || !(20.0..=1000.0).contains(&frequency_hz)
            || frequency_hz >= f64::from(SAMPLE_RATE) / 2.0
        {
            return Err(
                "--frequency must be finite, between 20 and 1000 Hz, and below Nyquist".into(),
            );
        }
        if !amplitude.is_finite() || !(0.0..=0.05).contains(&amplitude) {
            return Err("--volume must be a finite PCM peak amplitude between 0 and 0.05".into());
        }
        Ok(Self {
            duration_ms,
            frequency_hz,
            amplitude,
        })
    }

    pub fn duration(&self) -> Duration {
        Duration::from_millis(self.duration_ms)
    }

    /// Generate interleaved stereo f32 samples. Call once and reuse for a run.
    /// This produces samples, not a WAV container or serialized PCM bytes.
    pub fn generate(&self) -> Vec<f32> {
        // Validation caps this at 240,000 frames (1,920,000 bytes of stereo f32).
        let frames = (self.duration_ms * u64::from(SAMPLE_RATE) / 1000) as usize;
        let mut samples = vec![0.0; frames * CHANNELS];
        if self.amplitude == 0.0 {
            return samples;
        }

        // Include a zero endpoint at both ends. Short pulses share the available
        // interior samples between the two ramps; fades never overlap.
        let fade_frames = (SAMPLE_RATE as usize / 100).min((frames - 1) / 2);
        for (frame, stereo) in samples.chunks_exact_mut(CHANNELS).enumerate() {
            let fade = (frame.min(frames - 1 - frame) as f64 / fade_frames as f64).min(1.0);
            let phase = TAU * self.frequency_hz * frame as f64 / f64::from(SAMPLE_RATE);
            let sample = (phase.sin() * self.amplitude * fade) as f32;
            stereo.fill(sample);
        }
        samples
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pulse(duration_ms: u64, amplitude: f64) -> Vec<f32> {
        PulseParameters::new(duration_ms, DEFAULT_FREQUENCY_HZ, amplitude)
            .unwrap()
            .generate()
    }

    #[test]
    fn counts_frames_and_duplicates_stereo_channels() {
        for (duration, frames) in [(20, 960), (300, 14_400), (5000, 240_000)] {
            let samples = pulse(duration, DEFAULT_AMPLITUDE);
            assert_eq!(samples.len(), frames * CHANNELS);
            assert!(samples
                .chunks_exact(CHANNELS)
                .all(|pair| pair[0] == pair[1]));
        }
    }

    #[test]
    fn amplitude_is_applied_once_and_quiet_samples_are_preserved() {
        for amplitude in [1e-8, DEFAULT_AMPLITUDE, 0.05] {
            let samples = pulse(300, amplitude);
            let peak = samples
                .iter()
                .map(|sample| sample.abs())
                .fold(0.0, f32::max);
            assert!(samples.iter().all(|sample| sample.is_finite()));
            assert!((f64::from(peak) - amplitude).abs() <= amplitude * 1e-6);
            assert!(samples.iter().any(|sample| *sample < 0.0));
            assert!(samples.iter().any(|sample| *sample > 0.0));
        }
    }

    #[test]
    fn sine_frequency_and_ten_millisecond_fades() {
        let samples = pulse(300, DEFAULT_AMPLITUDE);
        let left: Vec<_> = samples.chunks_exact(CHANNELS).map(|pair| pair[0]).collect();
        assert_eq!(left[0], 0.0);
        assert_eq!(*left.last().unwrap(), 0.0);
        // At 100 Hz, a positive peak falls at 2.5 ms, a quarter of the fade-in.
        assert!((left[120] - 0.000125).abs() < 1e-10);
        // The next positive peak is past the ramp and reaches full amplitude.
        assert!((left[600] - 0.0005).abs() < 1e-10);
        assert!(left[720].abs() < 1e-10);
        assert!((left[840] + 0.0005).abs() < 1e-10);
        // This negative peak lies 119 frames before the zero-valued endpoint.
        assert!((left[14_280] + 0.0005 * 119.0 / 480.0).abs() < 1e-10);
    }

    #[test]
    fn minimum_duration_has_shortened_fades_and_zero_endpoints() {
        let samples = pulse(20, DEFAULT_AMPLITUDE);
        assert_eq!(samples[0], 0.0);
        assert_eq!(*samples.last().unwrap(), 0.0);
        assert!((samples[120 * CHANNELS] - 0.0005 * 120.0 / 479.0).abs() < 1e-10);
        assert!(samples.iter().any(|sample| *sample != 0.0));
        assert!(samples.iter().all(|sample| sample.abs() <= 0.0005));
    }

    #[test]
    fn zero_amplitude_is_exact_digital_silence() {
        for amplitude in [0.0, -0.0] {
            assert!(pulse(300, amplitude)
                .iter()
                .all(|sample| sample.to_bits() == 0));
        }
    }

    #[test]
    fn rejects_out_of_range_and_non_finite_signal_parameters() {
        for duration in [0, 19, 5001, u64::MAX] {
            assert!(PulseParameters::new(duration, 100.0, 0.0005).is_err());
        }
        for frequency in [
            19.9,
            1000.1,
            24_000.0,
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
        ] {
            assert!(PulseParameters::new(300, frequency, 0.0005).is_err());
        }
        for amplitude in [
            -0.0001,
            0.050001,
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
        ] {
            assert!(PulseParameters::new(300, 100.0, amplitude).is_err());
        }
    }

    #[test]
    fn accepts_inclusive_signal_boundaries() {
        for duration in [20, 5000] {
            for frequency in [20.0, 1000.0] {
                for amplitude in [0.0, 0.05] {
                    let params = PulseParameters::new(duration, frequency, amplitude).unwrap();
                    assert_eq!(params.duration(), Duration::from_millis(duration));
                }
            }
        }
    }
}
