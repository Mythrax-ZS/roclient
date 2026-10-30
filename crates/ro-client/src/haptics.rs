//! Controller feedback for what happens in game: a thud when we are hit (harder the more it
//! hurts), a sharp tick for our critical hits, a rising buzz on level up, a long fade when we
//! die, a tap on picking up an item and a heartbeat at low HP; and for skills, a hum building
//! while we chant, a pulse per hit as a skill lands, a rolling rumble for ground skills, a
//! swell for support, a zap for a hex, and a double tick when a skill fails.
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
    /// We chant for this many ms: a hum building to the release.
    Charge(u32),
    /// Our damaging skill landed this many hits.
    Strike(u16),
    /// We placed a ground skill.
    Ground,
    /// We used a support skill on ourselves or a player.
    Support,
    /// Another player's support skill on us.
    Blessed,
    /// We used a non-damaging skill on a monster (a debuff, a provoke).
    Hex,
    /// A skill of ours failed.
    Fail,
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
        Cue::Charge(ms) => {
            // Rising in 60 ms steps over the chant (a long one plateaus at its peak).
            let n = (ms as f32 / 60.0).ceil().clamp(1.0, 200.0) as usize;
            (0..n)
                .map(|i| {
                    let k = ((i + 1) as f32 / n as f32).min(1.0);
                    s(60.0, band(85.0 + 40.0 * k, 0.06 + 0.2 * k), band(200.0 + 160.0 * k, 0.03 + 0.14 * k))
                })
                .collect()
        }
        Cue::Strike(hits) => {
            let n = hits.clamp(1, 10) as usize;
            let (amp, on) = if n == 1 { (0.7, 45.0) } else { (0.5, 30.0) };
            (0..n).flat_map(|_| [s(on, band(140.0, amp), band(420.0, amp * 0.85)), s(40.0, band(110.0, 0.08), OFF)]).collect()
        }
        Cue::Ground => [0.7, 0.55, 0.4, 0.25, 0.12].iter().map(|a| s(60.0, band(65.0, *a), band(160.0, a * 0.3))).collect(),
        Cue::Support => [0.12, 0.22, 0.3, 0.2, 0.08].iter().enumerate().map(|(i, a)| s(45.0, OFF, band(260.0 + 40.0 * i as f32, *a))).collect(),
        Cue::Blessed => [0.08, 0.15, 0.2, 0.12].iter().enumerate().map(|(i, a)| s(50.0, OFF, band(300.0 + 40.0 * i as f32, *a))).collect(),
        Cue::Hex => vec![s(40.0, OFF, band(520.0, 0.45)), s(50.0, band(110.0, 0.3), OFF)],
        Cue::Fail => vec![s(25.0, OFF, band(150.0, 0.3)), s(50.0, OFF, OFF), s(25.0, OFF, band(150.0, 0.3))],
    }
}

/// The cues playing now, mixed into one output.
#[derive(Default)]
pub struct Haptics {
    /// Each cue playing: what it is, its steps, and how far in (ms).
    playing: Vec<(Cue, Vec<Step>, f32)>,
}

impl Haptics {
    pub fn play(&mut self, c: Cue) {
        // A handful at once; the oldest gives way.
        if self.playing.len() >= 8 {
            self.playing.remove(0);
        }
        self.playing.push((c, pattern(c), 0.0));
    }

    /// Ends a chant's hum: the skill went off, failed or was interrupted.
    pub fn end_charge(&mut self) {
        self.playing.retain(|(c, ..)| !matches!(c, Cue::Charge(_)));
    }

    /// Advances `dt` ms; returns the low and high bands to play now, scaled by `gain`.
    pub fn frame(&mut self, dt: f32, gain: f32) -> (Band, Band) {
        let (mut low, mut high) = (Band::default(), Band::default());
        self.playing.retain_mut(|(_, steps, t)| {
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

    #[test]
    fn charge_builds_and_ends_with_the_skill() {
        let mut h = Haptics::default();
        h.play(Cue::Charge(1200));
        let (early, _) = h.frame(30.0, 1.0);
        let (late, _) = h.frame(1100.0, 1.0);
        assert!(late.amp > early.amp);
        h.end_charge();
        assert_eq!(h.frame(1.0, 1.0).0.amp, 0.0);
        // A multi-hit strike pulses: on, off, on.
        h.play(Cue::Strike(3));
        let on = h.frame(10.0, 1.0).0.amp;
        let off = h.frame(40.0, 1.0).0.amp;
        let on2 = h.frame(40.0, 1.0).0.amp;
        assert!(on > off && on2 > off);
    }
}
