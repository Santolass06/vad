//! Voice Activity Detection (VAD) and silence detection module for VAD.
//!
//! Analyzes PCM audio (16kHz mono `i16`) to detect speech and silence segments.
//! Used by the media player for skip-silence playback and by the AI pipeline.
//!
//! Complies with PLANO_VAD.md §10.2:
//! "teste do VAD detector com áudio sintético (silêncio conhecido) validando que não corta início/fim de fala."

use serde::{Deserialize, Serialize};
use tracing::debug;

use crate::extractor::PcmAudio;

/// Configuration parameters for Voice Activity Detection.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct VadParams {
    /// Frame duration in milliseconds (default: 30 ms = 480 samples at 16kHz).
    pub frame_duration_ms: u32,
    /// Energy threshold in dBFS (default: -38.0 dBFS). Frames with RMS above this are speech.
    pub threshold_db: f64,
    /// Minimum duration of silence in seconds to be treated as skippable silence (default: 0.5s).
    /// Silences shorter than this are bridged to maintain natural speech rhythm.
    pub min_silence_duration_sec: f64,
    /// Pre-speech attack padding in milliseconds (default: 200 ms).
    /// Ensures word onsets and consonants are never clipped (§10.2).
    pub padding_attack_ms: u32,
    /// Post-speech hangover padding in milliseconds (default: 300 ms).
    /// Ensures word tails, trailing vowels, and breath releases are never clipped (§10.2).
    pub padding_release_ms: u32,
}

impl Default for VadParams {
    fn default() -> Self {
        Self {
            frame_duration_ms: 30,
            threshold_db: -38.0,
            min_silence_duration_sec: 0.5,
            padding_attack_ms: 200,
            padding_release_ms: 300,
        }
    }
}

/// A detected segment of active speech.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SpeechSegment {
    /// Start time in seconds.
    pub start_sec: f64,
    /// End time in seconds.
    pub end_sec: f64,
}

impl SpeechSegment {
    pub fn duration_sec(&self) -> f64 {
        (self.end_sec - self.start_sec).max(0.0)
    }
}

/// A detected segment of silence.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SilenceSegment {
    /// Start time in seconds.
    pub start_sec: f64,
    /// End time in seconds.
    pub end_sec: f64,
}

impl SilenceSegment {
    pub fn duration_sec(&self) -> f64 {
        (self.end_sec - self.start_sec).max(0.0)
    }
}

/// Aggregated result of VAD analysis containing speech and silence intervals.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct VadDetectionResult {
    /// Ordered list of active speech intervals.
    pub speech_segments: Vec<SpeechSegment>,
    /// Ordered list of silence intervals.
    pub silence_segments: Vec<SilenceSegment>,
    /// Total duration of analyzed audio in seconds.
    pub total_duration: f64,
}

impl VadDetectionResult {
    /// Returns true if the given playback time (in seconds) falls within a silence segment.
    pub fn is_silence_at(&self, current_sec: f64) -> bool {
        self.find_silence_at(current_sec).is_some()
    }

    /// Finds the silence segment containing `current_sec`, if any.
    pub fn find_silence_at(&self, current_sec: f64) -> Option<&SilenceSegment> {
        self.silence_segments
            .iter()
            .find(|seg| current_sec >= seg.start_sec && current_sec < seg.end_sec)
    }

    /// Finds the speech segment containing `current_sec`, if any.
    pub fn find_speech_at(&self, current_sec: f64) -> Option<&SpeechSegment> {
        self.speech_segments
            .iter()
            .find(|seg| current_sec >= seg.start_sec && current_sec <= seg.end_sec)
    }

    /// If `current_sec` is inside a silence segment of significant duration,
    /// returns the target timestamp to jump forward to (the start of the next speech segment).
    /// Returns None if already in speech, at the end of the file, or if the silence is negligible.
    /// Also returns None when no speech was found anywhere: the absolute threshold then most
    /// likely missed a quiet recording, and skipping "silence" would jump over all of it.
    pub fn next_speech_position(&self, current_sec: f64, min_silence_threshold_sec: f64) -> Option<f64> {
        if self.speech_segments.is_empty() {
            return None;
        }
        let silence = self.find_silence_at(current_sec)?;
        if silence.duration_sec() < min_silence_threshold_sec {
            return None;
        }

        // Target is the start of the next speech segment >= silence.end_sec
        for speech in &self.speech_segments {
            if speech.start_sec >= current_sec {
                return Some(speech.start_sec);
            }
        }

        // If no more speech segments remain, jump to end of silence (or total duration)
        Some(silence.end_sec)
    }
}

/// Voice Activity Detector performing energy and temporal analysis on PCM audio.
#[derive(Clone, Debug)]
pub struct VadDetector {
    params: VadParams,
}

impl Default for VadDetector {
    fn default() -> Self {
        Self::new()
    }
}

impl VadDetector {
    /// Creates a detector with standard parameters.
    pub fn new() -> Self {
        Self {
            params: VadParams::default(),
        }
    }

    /// Creates a detector with custom parameters.
    pub fn with_params(params: VadParams) -> Self {
        Self { params }
    }

    pub fn params(&self) -> &VadParams {
        &self.params
    }

    /// Runs VAD analysis on extracted `PcmAudio`.
    pub fn detect(&self, audio: &PcmAudio) -> VadDetectionResult {
        self.detect_samples(&audio.samples, audio.sample_rate)
    }

    /// Runs VAD analysis on raw `i16` mono samples at `sample_rate`.
    pub fn detect_samples(&self, samples: &[i16], sample_rate: u32) -> VadDetectionResult {
        if samples.is_empty() || sample_rate == 0 {
            return VadDetectionResult::default();
        }

        let total_duration = samples.len() as f64 / sample_rate as f64;
        let frame_samples = ((sample_rate as f64 * (self.params.frame_duration_ms as f64 / 1000.0)).round() as usize).max(1);

        // 1. Frame-by-frame RMS energy computation
        let num_frames = samples.len() / frame_samples;
        let mut frame_is_speech = Vec::with_capacity(num_frames);

        for chunk in samples.chunks_exact(frame_samples) {
            let sum_sq: f64 = chunk.iter().map(|&s| (s as f64) * (s as f64)).sum();
            let mean_sq = sum_sq / (chunk.len() as f64);
            let rms = mean_sq.sqrt();

            // Convert RMS to dBFS (-96 dBFS to 0 dBFS)
            let db = if rms > 1e-6 {
                20.0 * (rms / 32768.0).log10()
            } else {
                -100.0
            };

            frame_is_speech.push(db >= self.params.threshold_db);
        }

        if frame_is_speech.is_empty() {
            return VadDetectionResult {
                speech_segments: Vec::new(),
                silence_segments: vec![SilenceSegment {
                    start_sec: 0.0,
                    end_sec: total_duration,
                }],
                total_duration,
            };
        }

        let frame_dur_sec = frame_samples as f64 / sample_rate as f64;

        // 2. Initial contiguous speech runs from raw frame classification
        let mut raw_speech_ranges: Vec<(f64, f64)> = Vec::new();
        let mut in_speech = false;
        let mut start_sec = 0.0;

        for (i, &is_sp) in frame_is_speech.iter().enumerate() {
            let t = i as f64 * frame_dur_sec;
            if is_sp && !in_speech {
                in_speech = true;
                start_sec = t;
            } else if !is_sp && in_speech {
                in_speech = false;
                raw_speech_ranges.push((start_sec, t));
            }
        }
        if in_speech {
            raw_speech_ranges.push((start_sec, num_frames as f64 * frame_dur_sec));
        }

        if raw_speech_ranges.is_empty() {
            return VadDetectionResult {
                speech_segments: Vec::new(),
                silence_segments: vec![SilenceSegment {
                    start_sec: 0.0,
                    end_sec: total_duration,
                }],
                total_duration,
            };
        }

        // 3. Apply attack and release padding (§10.2: never clip speech onsets or endings)
        let attack_sec = self.params.padding_attack_ms as f64 / 1000.0;
        let release_sec = self.params.padding_release_ms as f64 / 1000.0;

        let mut padded_ranges: Vec<(f64, f64)> = Vec::with_capacity(raw_speech_ranges.len());
        for (st, en) in raw_speech_ranges {
            let padded_st = (st - attack_sec).max(0.0);
            let padded_en = (en + release_sec).min(total_duration);
            padded_ranges.push((padded_st, padded_en));
        }

        // 4. Merge overlapping ranges and bridge gaps smaller than min_silence_duration_sec
        let mut merged_speech: Vec<SpeechSegment> = Vec::new();
        for (st, en) in padded_ranges {
            if let Some(last) = merged_speech.last_mut() {
                // If this segment overlaps or gap is smaller than min_silence_duration_sec, merge
                if st <= last.end_sec + self.params.min_silence_duration_sec {
                    last.end_sec = last.end_sec.max(en);
                } else {
                    merged_speech.push(SpeechSegment {
                        start_sec: st,
                        end_sec: en,
                    });
                }
            } else {
                merged_speech.push(SpeechSegment {
                    start_sec: st,
                    end_sec: en,
                });
            }
        }

        // 5. Derive complementary silence intervals
        let mut silence_segments: Vec<SilenceSegment> = Vec::new();
        let mut cursor = 0.0;

        for speech in &merged_speech {
            if speech.start_sec > cursor + 1e-4 {
                silence_segments.push(SilenceSegment {
                    start_sec: cursor,
                    end_sec: speech.start_sec,
                });
            }
            cursor = speech.end_sec;
        }

        if cursor < total_duration - 1e-4 {
            silence_segments.push(SilenceSegment {
                start_sec: cursor,
                end_sec: total_duration,
            });
        }

        debug!(
            "VAD detected {} speech segments and {} silence segments over {:.2}s",
            merged_speech.len(),
            silence_segments.len(),
            total_duration
        );

        VadDetectionResult {
            speech_segments: merged_speech,
            silence_segments,
            total_duration,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    /// Generates synthetic PCM audio with pure sine tone (speech proxy) or digital silence.
    fn make_synthetic_pcm(duration_sec: f64, sample_rate: u32, is_tone: bool, amplitude: f64) -> Vec<i16> {
        let num_samples = (duration_sec * sample_rate as f64) as usize;
        let mut samples = Vec::with_capacity(num_samples);
        let freq = 440.0; // A4 tone

        for i in 0..num_samples {
            if is_tone {
                let t = i as f64 / sample_rate as f64;
                let val = (2.0 * PI * freq * t).sin() * amplitude * 32767.0;
                samples.push(val.round().clamp(-32768.0, 32767.0) as i16);
            } else {
                samples.push(0);
            }
        }
        samples
    }

    #[test]
    fn test_pure_silence_detection() {
        let detector = VadDetector::new();
        let silence_samples = make_synthetic_pcm(3.0, 16000, false, 0.0);
        let res = detector.detect_samples(&silence_samples, 16000);

        assert!(res.speech_segments.is_empty(), "Pure silence should yield zero speech segments");
        assert_eq!(res.silence_segments.len(), 1);
        assert!((res.silence_segments[0].duration_sec() - 3.0).abs() < 0.05);
        assert!(res.is_silence_at(1.5));
    }

    #[test]
    fn test_pure_speech_detection() {
        let detector = VadDetector::new();
        // High amplitude tone (0.5 = -6 dBFS)
        let tone_samples = make_synthetic_pcm(3.0, 16000, true, 0.5);
        let res = detector.detect_samples(&tone_samples, 16000);

        assert_eq!(res.speech_segments.len(), 1);
        assert!((res.speech_segments[0].duration_sec() - 3.0).abs() < 0.05);
        assert!(res.silence_segments.is_empty());
        assert!(!res.is_silence_at(1.5));
    }

    #[test]
    fn test_no_clipping_of_speech_onset_and_tail_section_10_2() {
        // PLANO_VAD.md §10.2:
        // "teste do VAD detector com áudio sintético (silêncio conhecido) validando que não corta início/fim de fala."
        //
        // Layout:
        // [0.0s .. 1.5s]: Silence (1.5s)
        // [1.5s .. 3.5s]: Speech (2.0s tone)
        // [3.5s .. 5.5s]: Silence (2.0s)
        let sample_rate = 16000;
        let mut audio = make_synthetic_pcm(1.5, sample_rate, false, 0.0);
        audio.extend(make_synthetic_pcm(2.0, sample_rate, true, 0.6));
        audio.extend(make_synthetic_pcm(2.0, sample_rate, false, 0.0));

        let detector = VadDetector::new();
        let res = detector.detect_samples(&audio, sample_rate);

        assert_eq!(res.speech_segments.len(), 1, "Must detect exactly 1 speech segment");
        let speech = &res.speech_segments[0];

        // Speech started at exactly 1.5s. With attack padding of 200ms, start_sec must be <= 1.5s
        // (typically ~1.30s) so the onset of speech is NEVER clipped!
        assert!(
            speech.start_sec <= 1.50,
            "Speech onset must not be clipped: start_sec is {:.3} (expected <= 1.50s)",
            speech.start_sec
        );
        assert!(
            speech.start_sec >= 1.25,
            "Speech onset attack padding must not expand excessively: start_sec is {:.3}",
            speech.start_sec
        );

        // Speech ended at exactly 3.5s. With release padding of 300ms, end_sec must be >= 3.5s
        // (typically ~3.80s) so the ending of speech is NEVER clipped!
        assert!(
            speech.end_sec >= 3.50,
            "Speech ending must not be clipped: end_sec is {:.3} (expected >= 3.50s)",
            speech.end_sec
        );
        assert!(
            speech.end_sec <= 3.85,
            "Speech ending release padding must not expand excessively: end_sec is {:.3}",
            speech.end_sec
        );

        // Verify complementary silences
        assert_eq!(res.silence_segments.len(), 2);
        // First silence covers up to speech.start_sec
        assert!((res.silence_segments[0].end_sec - speech.start_sec).abs() < 1e-3);
        // Second silence starts at speech.end_sec
        assert!((res.silence_segments[1].start_sec - speech.end_sec).abs() < 1e-3);
    }

    #[test]
    fn test_skip_silence_next_speech_position() {
        let sample_rate = 16000;
        let mut audio = make_synthetic_pcm(2.0, sample_rate, false, 0.0);
        audio.extend(make_synthetic_pcm(1.0, sample_rate, true, 0.5));
        audio.extend(make_synthetic_pcm(2.0, sample_rate, false, 0.0));

        let detector = VadDetector::new();
        let res = detector.detect_samples(&audio, sample_rate);

        assert_eq!(res.speech_segments.len(), 1);
        let speech_start = res.speech_segments[0].start_sec;

        // At t = 0.5s (in silence), next_speech_position should jump directly to speech_start
        let jump_target = res.next_speech_position(0.5, 0.5);
        assert_eq!(jump_target, Some(speech_start));

        // At t = speech_start + 0.1s (already in speech), returns None (no jump)
        assert_eq!(res.next_speech_position(speech_start + 0.1, 0.5), None);
    }

    #[test]
    fn test_no_speech_anywhere_never_skips() {
        // A whole quiet recording classified as silence must not be skipped to its end
        let audio = make_synthetic_pcm(10.0, 16000, false, 0.0);
        let res = VadDetector::new().detect_samples(&audio, 16000);
        assert!(res.is_silence_at(5.0));
        assert_eq!(res.next_speech_position(5.0, 0.5), None);
    }

    #[test]
    fn test_small_gap_bridging() {
        // Two speech segments separated by 0.2s of silence (less than min_silence_duration_sec = 0.5s)
        // should be merged into a single smooth speech segment.
        let sample_rate = 16000;
        let mut audio = make_synthetic_pcm(1.0, sample_rate, true, 0.5);
        audio.extend(make_synthetic_pcm(0.2, sample_rate, false, 0.0));
        audio.extend(make_synthetic_pcm(1.0, sample_rate, true, 0.5));

        let detector = VadDetector::new();
        let res = detector.detect_samples(&audio, sample_rate);

        assert_eq!(
            res.speech_segments.len(),
            1,
            "Short silence gap (0.2s) should be bridged into a continuous speech segment"
        );
    }

    /// Real speech (the 19 s "Me at the zoo" clip used by the Whisper e2e test): the default
    /// threshold must find the speech, cover the spoken words, and not call the whole clip silence.
    /// Needs network, yt-dlp and ffmpeg; run with `cargo test -p vad-ai -- --ignored --nocapture`.
    #[test]
    #[ignore = "needs network access, yt-dlp and ffmpeg"]
    fn test_real_speech_is_detected_with_default_threshold() {
        use crate::extractor::{AudioExtractor, ExtractionStatus};
        use std::time::Duration;

        let dir = std::path::PathBuf::from(format!("/tmp/vad_test_vad_real_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let speech = dir.join("speech.m4a");
        let ok = std::process::Command::new("yt-dlp")
            .args(["--no-config", "-q", "-f", "bestaudio[ext=m4a]/bestaudio", "-o"])
            .arg(&speech)
            .arg("https://www.youtube.com/watch?v=jNQXAC9IVRw")
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        assert!(ok && speech.exists(), "yt-dlp could not fetch the speech sample");

        let (rx, _handle) = AudioExtractor::new().extract_async(speech.to_string_lossy().to_string(), Some(19.0));
        let audio = loop {
            match rx.recv_timeout(Duration::from_secs(30)).expect("extraction timed out") {
                ExtractionStatus::Completed(a) => break a,
                ExtractionStatus::Failed(e) => panic!("extraction failed: {e}"),
                _ => {}
            }
        };
        let _ = std::fs::remove_dir_all(&dir);

        let res = VadDetector::new().detect(&audio);
        let speech_total: f64 = res.speech_segments.iter().map(|s| s.duration_sec()).sum();
        println!(
            "real speech: {:.2}s audio, {} speech segments ({:.2}s), {} silence segments; segments = {:?}",
            audio.duration_seconds,
            res.speech_segments.len(),
            speech_total,
            res.silence_segments.len(),
            res.speech_segments
        );
        assert!(!res.speech_segments.is_empty(), "real speech classified as all silence");
        assert!(speech_total > audio.duration_seconds * 0.4, "most of a spoken clip must be speech");
    }
}
