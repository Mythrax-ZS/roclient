//! Controller feedback for what happens in game: a thud when we are hit (harder the more it
//! hurts), a sharp tick for our critical hits, a rising buzz on level up, a long fade when we
//! die, a tap on picking up an item and a heartbeat at low HP.
//!
//! Each cue is a short pattern of steps, each a low band (a heavy thud) and a high band (a
//! light buzz) with a frequency and a strength. Pads with two motors play the strengths;
//! the Switch Pro Controller plays both bands as HD rumble; a DualSense on USB plays them as
//! waveforms on its haptic actuators (its third and fourth audio channels), the way PS5
//! games drive it.

use crate::hidpad::Band;
use std::sync::{Arc, Mutex};

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Cue {
    /// We took damage: how much of our max HP it was (0..1).
    Hit(f32),
    /// We landed a critical hit.
    Crit,
    LevelUp,
    Death,
    Pickup,
    /// Low HP: one beat (two thumps).
    Heartbeat,
}

#[derive(Clone, Copy, Debug)]
struct Step {
    ms: f32,
    low: Band,
    high: Band,
}

fn band(freq: f32, amp: f32) -> Band {
    Band { freq, amp }
}

const OFF: Band = Band { freq: 160.0, amp: 0.0 };

fn pattern(c: Cue) -> Vec<Step> {
    let s = |ms: f32, low: Band, high: Band| Step { ms, low, high };
    match c {
        Cue::Hit(share) => {
            let k = share.clamp(0.0, 1.0).sqrt();
            vec![
                s(50.0 + 90.0 * k, band(150.0 - 60.0 * k, 0.35 + 0.6 * k), band(320.0, 0.15 + 0.35 * k)),
                s(60.0, band(110.0, (0.35 + 0.6 * k) * 0.4), band(320.0, 0.05)),
            ]
        }
        Cue::Crit => vec![s(24.0, OFF, band(720.0, 0.95)), s(60.0, band(140.0, 0.55), band(380.0, 0.3)), s(40.0, band(120.0, 0.2), OFF)],
        Cue::LevelUp => (0..6).map(|i| s(70.0, band(180.0, 0.18), band(300.0 + 120.0 * i as f32, 0.45 + 0.08 * i as f32))).collect(),
        Cue::Death => [1.0, 0.8, 0.6, 0.4, 0.25, 0.12].iter().map(|a| s(130.0, band(70.0, *a), band(200.0, a * 0.3))).collect(),
        Cue::Pickup => vec![s(22.0, OFF, band(620.0, 0.35))],
        Cue::Heartbeat => vec![s(70.0, band(60.0, 0.7), OFF), s(120.0, OFF, OFF), s(80.0, band(55.0, 0.45), OFF)],
    }
}

/// The cues playing now, mixed into one output.
#[derive(Default)]
pub struct Haptics {
    playing: Vec<(Vec<Step>, f32)>,
}

impl Haptics {
    pub fn play(&mut self, c: Cue) {
        // A handful at once; the oldest gives way.
        if self.playing.len() >= 8 {
            self.playing.remove(0);
        }
        self.playing.push((pattern(c), 0.0));
    }

    /// Advances `dt` ms; returns the low and high bands to play now, scaled by `gain`.
    pub fn frame(&mut self, dt: f32, gain: f32) -> (Band, Band) {
        let (mut low, mut high) = (Band::default(), Band::default());
        self.playing.retain_mut(|(steps, t)| {
            *t += dt;
            let mut at = *t;
            for st in steps.iter() {
                if at < st.ms {
                    // The strongest band of all that play wins, with its frequency.
                    if st.low.amp > low.amp {
                        low = st.low;
                    }
                    if st.high.amp > high.amp {
                        high = st.high;
                    }
                    return true;
                }
                at -= st.ms;
            }
            false
        });
        low.amp = (low.amp * gain).min(1.0);
        high.amp = (high.amp * gain).min(1.0);
        (low, high)
    }
}

/// The DualSense's haptic actuators driven as audio: over USB it is also a four-channel sound
/// card whose third and fourth channels move the left and right actuators.
pub struct AudioHaptics {
    target: Arc<Mutex<(Band, Band)>>,
    _stream: rodio::cpal::Stream,
}

impl AudioHaptics {
    /// Opens the controller's audio device if it is there (USB only).
    pub fn open() -> Option<AudioHaptics> {
        use rodio::cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
        let host = rodio::cpal::default_host();
        let device = host.output_devices().ok()?.find(|d| {
            let name = d.description().map(|n| n.name().to_ascii_lowercase()).unwrap_or_default();
            (name.contains("dualsense") || name.contains("wireless controller")) && d.supported_output_configs().is_ok_and(|mut c| c.any(|c| c.channels() >= 4))
        })?;
        // Four channels or more, preferably at 48 kHz in floating point (the controller's own
        // format), else as near to it as the device goes.
        let ranges: Vec<_> = device.supported_output_configs().ok()?.filter(|c| c.channels() >= 4).collect();
        let fits = |c: &&rodio::cpal::SupportedStreamConfigRange| (c.min_sample_rate()..=c.max_sample_rate()).contains(&48_000);
        let range = ranges
            .iter()
            .filter(fits)
            .find(|c| c.sample_format() == rodio::cpal::SampleFormat::F32)
            .or_else(|| ranges.iter().find(fits))
            .or_else(|| ranges.iter().min_by_key(|c| c.min_sample_rate().abs_diff(48_000)))?
            .clone();
        let rate = 48_000u32.clamp(range.min_sample_rate(), range.max_sample_rate());
        let channels = range.channels() as usize;
        let config = rodio::cpal::StreamConfig { channels: range.channels(), sample_rate: rate, buffer_size: rodio::cpal::BufferSize::Default };
        let target = Arc::new(Mutex::new((Band::default(), Band::default())));
        let shared = target.clone();
        // Phases and smoothed strengths, so changes never click.
        let (mut pl, mut ph, mut al, mut ah) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
        let step = 1.0 / rate as f32;
        let smooth = 1.0 - (-step / 0.004f32).exp();
        let stream = device
            .build_output_stream(
                config,
                move |data: &mut [f32], _| {
                    let (low, high) = shared.lock().map(|g| *g).unwrap_or_default();
                    for frame in data.chunks_mut(channels) {
                        al += (low.amp - al) * smooth;
                        ah += (high.amp - ah) * smooth;
                        pl = (pl + low.freq * step).fract();
                        ph = (ph + high.freq * step).fract();
                        let (sl, sh) = ((pl * std::f32::consts::TAU).sin() * al, (ph * std::f32::consts::TAU).sin() * ah);
                        frame.fill(0.0);
                        // The heavy band leans left, the light one right, as the motors do.
                        frame[2] = (sl + sh * 0.4).clamp(-1.0, 1.0);
                        frame[3] = (sh + sl * 0.4).clamp(-1.0, 1.0);
                    }
                },
                |e| tracing::debug!("controller haptics audio: {e}"),
                None,
            )
            .inspect_err(|e| tracing::info!("controller haptics audio not opened: {e}"))
            .ok()?;
        stream.play().ok()?;
        tracing::info!(channels, rate, "controller haptics: playing through the DualSense's actuators");
        Some(AudioHaptics { target, _stream: stream })
    }

    pub fn set(&self, low: Band, high: Band) {
        if let Ok(mut t) = self.target.lock() {
            *t = (low, high);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cues_play_out_and_mix() {
        let mut h = Haptics::default();
        h.play(Cue::Hit(1.0));
        let (low, _) = h.frame(10.0, 1.0);
        assert!(low.amp > 0.9);
        h.play(Cue::Pickup);
        let (_, high) = h.frame(1.0, 1.0);
        assert!(high.amp >= 0.35);
        // Everything has ended a second later.
        let (low, high) = h.frame(1000.0, 1.0);
        assert_eq!((low.amp, high.amp), (0.0, 0.0));
        // Gain scales.
        h.play(Cue::Crit);
        let (_, high) = h.frame(1.0, 0.5);
        assert!((high.amp - 0.475).abs() < 1e-4);
    }
}
