use std::sync::Arc;
use vad_ai::PcmAudio;

/// Target maximum points rendered on screen per PLANO_VAD.md §5 (~1000 points).
pub const TARGET_VISIBLE_POINTS: usize = 1000;

/// Min/max peak amplitude representation for a single display column/bucket.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MinMaxPoint {
    /// Normalized minimum sample value in `[-1.0, 1.0]`.
    pub min: f32,
    /// Normalized maximum sample value in `[-1.0, 1.0]`.
    pub max: f32,
}

impl MinMaxPoint {
    pub const ZERO: Self = Self { min: 0.0, max: 0.0 };

    /// Returns peak absolute amplitude in `[0.0, 1.0]`.
    pub fn amplitude(&self) -> f32 {
        self.max.abs().max(self.min.abs()).clamp(0.0, 1.0)
    }
}

/// Zoom level classification for on-demand MIP-mapping per PLANO_VAD.md §5.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ZoomLevel {
    /// Full overview of the recording (pre-computed on load).
    Global,
    /// Mid-level window (~5 minutes) computed on demand.
    Mid5Min,
    /// Detail window (~10 seconds) computed on demand.
    Detail10Sec,
    /// Arbitrary user-defined zoom window.
    Custom,
}

/// Multi-resolution Waveform Pyramid for fast, zero-lag UI rendering.
/// Only the global overview is computed upfront; zoom windows are generated on-demand (§5).
pub struct WaveformPyramid {
    samples: Arc<[i16]>,
    sample_rate: u32,
    duration_seconds: f64,
    /// Pre-computed global overview (~1000 points spanning 100% of duration).
    global_level: Vec<MinMaxPoint>,
    /// On-demand cache for zoomed-in views.
    zoom_cache: Vec<MinMaxPoint>,
    zoom_cache_range: Option<(f64, f64, usize)>,
}

impl WaveformPyramid {
    /// Creates a new waveform pyramid from decoded PCM audio.
    /// Pre-computes only the global overview level (~1000 points) to avoid delays on opening (§5).
    pub fn from_pcm(pcm: &PcmAudio) -> Self {
        let samples = Arc::clone(&pcm.samples);
        let sample_rate = pcm.sample_rate;
        let duration_seconds = pcm.duration_seconds;

        let global_level = Self::compute_points(&samples, 0, samples.len(), TARGET_VISIBLE_POINTS);

        Self {
            samples,
            sample_rate,
            duration_seconds,
            global_level,
            zoom_cache: Vec::new(),
            zoom_cache_range: None,
        }
    }

    /// Total audio duration in seconds.
    pub fn duration_seconds(&self) -> f64 {
        self.duration_seconds
    }

    /// Pre-computed global overview points.
    pub fn global_points(&self) -> &[MinMaxPoint] {
        &self.global_level
    }

    /// Returns the points to render for the specified time window `[time_start, time_end]`.
    /// Renders at most `max_points` (capped at ~1000 points) in a single draw batch (§5).
    pub fn get_visible_points(
        &mut self,
        time_start: f64,
        time_end: f64,
        max_points: usize,
    ) -> &[MinMaxPoint] {
        let target_points = max_points.clamp(10, TARGET_VISIBLE_POINTS);

        let t_start = time_start.max(0.0);
        let t_end = time_end.min(self.duration_seconds).max(t_start + 0.001);

        // If viewing entire file, return precomputed global level directly (0ms latency)
        if t_start <= 0.001 && t_end >= self.duration_seconds - 0.001 {
            return &self.global_level;
        }

        // Check if cache already matches requested range (within tiny epsilon)
        if let Some((c_start, c_end, c_points)) = self.zoom_cache_range {
            if c_points == target_points
                && (c_start - t_start).abs() < 0.001
                && (c_end - t_end).abs() < 0.001
            {
                return &self.zoom_cache;
            }
        }

        // On-demand computation for zoomed range (§5)
        let total_samples = self.samples.len();
        let start_sample = ((t_start * self.sample_rate as f64).round() as usize).min(total_samples);
        let end_sample = ((t_end * self.sample_rate as f64).round() as usize).min(total_samples);

        self.zoom_cache = Self::compute_points(&self.samples, start_sample, end_sample, target_points);
        self.zoom_cache_range = Some((t_start, t_end, target_points));

        &self.zoom_cache
    }

    /// Classifies current window into ZoomLevel for heuristics / MIP-mapping.
    pub fn classify_zoom(&self, window_duration: f64) -> ZoomLevel {
        if window_duration >= self.duration_seconds * 0.95 {
            ZoomLevel::Global
        } else if window_duration <= 15.0 {
            ZoomLevel::Detail10Sec
        } else if window_duration <= 360.0 {
            ZoomLevel::Mid5Min
        } else {
            ZoomLevel::Custom
        }
    }

    /// Internal helper to calculate min/max buckets from an audio sample slice.
    fn compute_points(
        samples: &[i16],
        start_idx: usize,
        end_idx: usize,
        target_count: usize,
    ) -> Vec<MinMaxPoint> {
        let count = target_count.max(1);
        let slice = if start_idx < end_idx && start_idx < samples.len() {
            let actual_end = end_idx.min(samples.len());
            &samples[start_idx..actual_end]
        } else {
            &[]
        };

        if slice.is_empty() {
            return vec![MinMaxPoint::ZERO; count];
        }

        let slice_len = slice.len();
        let mut points = Vec::with_capacity(count);

        for i in 0..count {
            let bucket_start = (i * slice_len) / count;
            let bucket_end = ((i + 1) * slice_len) / count;

            if bucket_start >= bucket_end || bucket_start >= slice_len {
                points.push(MinMaxPoint::ZERO);
                continue;
            }

            let mut min_val = i16::MAX;
            let mut max_val = i16::MIN;

            for &sample in &slice[bucket_start..bucket_end] {
                if sample < min_val {
                    min_val = sample;
                }
                if sample > max_val {
                    max_val = sample;
                }
            }

            let norm_min = min_val as f32 / 32768.0;
            let norm_max = max_val as f32 / 32768.0;

            points.push(MinMaxPoint {
                min: norm_min.clamp(-1.0, 1.0),
                max: norm_max.clamp(-1.0, 1.0),
            });
        }

        points
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_global_pyramid_computation() {
        // 16000 samples = 1 second
        let mut samples = Vec::with_capacity(16000);
        for i in 0..16000 {
            // Generate triangle wave
            let v = if (i % 200) < 100 { 15000i16 } else { -15000i16 };
            samples.push(v);
        }

        let pcm = PcmAudio {
            path: "test.wav".to_string(),
            samples: samples.into(),
            sample_rate: 16000,
            channels: 1,
            duration_seconds: 1.0,
            is_truncated: false,
        };

        let mut pyramid = WaveformPyramid::from_pcm(&pcm);
        assert_eq!(pyramid.duration_seconds(), 1.0);
        assert_eq!(pyramid.global_points().len(), TARGET_VISIBLE_POINTS);

        let points = pyramid.get_visible_points(0.0, 1.0, 1000);
        assert_eq!(points.len(), 1000);

        for pt in points {
            assert!(pt.min <= pt.max);
            assert!(pt.amplitude() > 0.4 && pt.amplitude() < 0.6);
        }
    }

    #[test]
    fn test_on_demand_zoom_levels() {
        // 48000 samples = 3 seconds
        let samples = vec![10000i16; 48000];
        let pcm = PcmAudio {
            path: "test.wav".to_string(),
            samples: samples.into(),
            sample_rate: 16000,
            channels: 1,
            duration_seconds: 600.0,
            is_truncated: false,
        };

        let mut pyramid = WaveformPyramid::from_pcm(&pcm);

        // Zoom into a 0.5-second sub-window
        let zoomed = pyramid.get_visible_points(1.0, 1.5, 500);
        assert_eq!(zoomed.len(), 500);

        // Zoom classification for a 10-minute audio file
        assert_eq!(pyramid.classify_zoom(600.0), ZoomLevel::Global);
        assert_eq!(pyramid.classify_zoom(180.0), ZoomLevel::Mid5Min);
        assert_eq!(pyramid.classify_zoom(10.0), ZoomLevel::Detail10Sec);
    }

    #[test]
    fn test_empty_audio_waveform() {
        let pcm = PcmAudio {
            path: "empty.wav".to_string(),
            samples: Arc::new([]),
            sample_rate: 16000,
            channels: 1,
            duration_seconds: 0.0,
            is_truncated: false,
        };

        let pyramid = WaveformPyramid::from_pcm(&pcm);
        assert_eq!(pyramid.global_points().len(), TARGET_VISIBLE_POINTS);
        assert_eq!(pyramid.global_points()[0], MinMaxPoint::ZERO);
    }
}
