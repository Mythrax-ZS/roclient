//! Controller input through gilrs (XInput on Windows; evdev on Linux, where Steam Input
//! presents the Steam Deck's controls as an Xbox pad). Each frame yields the stick and
//! trigger state plus the buttons pressed since the last frame.

use gilrs::{Axis, Button, EventType, Gilrs};
use glam::Vec2;
use serde::{Deserialize, Serialize};

/// How the sticks, triggers and touchpad respond (Settings > Controls > Controller).
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
#[serde(default)]
pub struct Tuning {
    /// Stick travel ignored around the centre (share of full travel), per physical stick;
    /// motion starts smoothly past it.
    pub left_deadzone: f32,
    pub right_deadzone: f32,
    /// Travel short of the rim that already reads as full tilt (worn sticks that never
    /// reach the edge still turn at full speed).
    pub outer_deadzone: f32,
    /// Camera turn and tilt speed (1: the default).
    pub turn_speed: f32,
    pub tilt_speed: f32,
    pub invert_turn: bool,
    pub invert_tilt: bool,
    /// Camera stick response: 1 linear; higher is gentler near the centre, for fine aim.
    pub curve: f32,
    /// How far a trigger presses before it counts (share of full travel).
    pub trigger: f32,
    /// The right stick walks and the left turns the camera.
    pub swap_sticks: bool,
    /// Touchpad pointer speed (1: the default).
    pub touch_speed: f32,
}

impl Default for Tuning {
    fn default() -> Self {
        Self { left_deadzone: 0.22, right_deadzone: 0.22, outer_deadzone: 0.0, turn_speed: 1.0, tilt_speed: 1.0, invert_turn: false, invert_tilt: false, curve: 1.0, trigger: 0.4, swap_sticks: false, touch_speed: 1.0 }
    }
}

#[derive(Default, Clone)]
pub struct PadFrame {
    /// Walking: the direction (and how far) the walking stick leans.
    pub left: Vec2,
    /// The camera: turn and tilt, with the tuning's curve, speeds and inversion applied.
    pub right: Vec2,
    /// Held: the shortcut bar modifiers (LT, RT, and both together in battle mode).
    pub lt: bool,
    pub rt: bool,
    /// Pressed this frame.
    pub pressed: Vec<Button>,
}

impl PadFrame {
    pub fn just(&self, b: Button) -> bool {
        self.pressed.contains(&b)
    }
}

pub struct Pad {
    gilrs: Option<Gilrs>,
    active: Option<gilrs::GamepadId>,
    /// Set once the controller is actually touched (hints stay hidden for mouse players).
    used: bool,
    /// The connected controller's family, for its button glyphs.
    pub detected: crate::padglyph::Style,
    /// Rumble: a constant strong and weak effect on the active pad, their gains set to the
    /// strength wanted, and the strengths last set.
    rumble: Option<(gilrs::GamepadId, gilrs::ff::Effect, gilrs::ff::Effect)>,
    rumble_now: (f32, f32),
    /// Set before each poll from the settings.
    pub tuning: Tuning,
    /// The physical left and right sticks as read, before any deadzone (the settings'
    /// stick preview).
    pub raw: [Vec2; 2],
}

/// A stick's reading with a radial deadzone: nothing inside `inner`, full tilt from
/// `outer` short of the rim, rescaled in between so motion starts smoothly.
pub fn deadzone(v: Vec2, inner: f32, outer: f32) -> Vec2 {
    let len = v.length();
    if len <= inner || len == 0.0 {
        return Vec2::ZERO;
    }
    let span = (1.0 - outer - inner).max(0.05);
    v / len * ((len - inner) / span).min(1.0)
}

impl Pad {
    pub fn new() -> Self {
        // No default filters: gilrs's own per-device deadzone would sit under ours and make
        // smaller settings do nothing. The D-pad hat is still turned into buttons (poll).
        let gilrs = gilrs::GilrsBuilder::new().with_default_filters(false).build().inspect_err(|e| tracing::warn!("controller support unavailable: {e}")).ok();
        let active = gilrs.as_ref().and_then(|g| g.gamepads().inspect(|(_, p)| tracing::info!(name = p.name(), "controller found")).last().map(|(id, pad)| {
            tracing::info!(name = pad.name(), "controller connected");
            id
        }));
        let detected = gilrs.as_ref().map_or(crate::padglyph::Style::Xbox, |g| family(g, active));
        Self { gilrs, active, used: false, detected, rumble: None, rumble_now: (0.0, 0.0), tuning: Tuning::default(), raw: [Vec2::ZERO; 2] }
    }

    /// Runs the active pad's motors at these strengths (0..1): the heavy one and the light one.
    pub fn set_rumble(&mut self, strong: f32, weak: f32) {
        let want = ((strong * 50.0).round() / 50.0, (weak * 50.0).round() / 50.0);
        let (Some(g), Some(id)) = (self.gilrs.as_mut(), self.active) else { return };
        if self.rumble.as_ref().is_some_and(|r| r.0 == id) && want == self.rumble_now {
            return;
        }
        if self.rumble.as_ref().is_none_or(|r| r.0 != id) {
            self.rumble = None;
            if want == (0.0, 0.0) || !g.gamepad(id).is_ff_supported() {
                return;
            }
            use gilrs::ff::{BaseEffect, BaseEffectType, EffectBuilder};
            let make = |g: &mut Gilrs, kind| EffectBuilder::new().add_effect(BaseEffect { kind, ..Default::default() }).gamepads(&[id]).finish(g).ok();
            let (Some(s), Some(w)) = (make(g, BaseEffectType::Strong { magnitude: u16::MAX }), make(g, BaseEffectType::Weak { magnitude: u16::MAX })) else {
                tracing::info!("controller rumble unavailable");
                return;
            };
            let _ = (s.set_gain(0.0), w.set_gain(0.0), s.play(), w.play());
            self.rumble = Some((id, s, w));
            self.rumble_now = (0.0, 0.0);
        }
        if let Some((_, s, w)) = &self.rumble {
            let _ = (s.set_gain(want.0), w.set_gain(want.1));
            self.rumble_now = want;
        }
    }

    /// A controller is connected.
    pub fn connected(&self) -> bool {
        self.active.is_some()
    }

    /// The USB vendor and product ids of the pad in use, when the system reports them.
    pub fn active_ids(&self) -> Option<(u16, u16)> {
        let p = self.gilrs.as_ref()?.gamepad(self.active?);
        Some((p.vendor_id()?, p.product_id()?))
    }

    /// True once a controller has been used (the UI then shows button hints).
    pub fn in_use(&self) -> bool {
        self.used && self.active.is_some()
    }

    pub fn poll(&mut self) -> PadFrame {
        let mut frame = PadFrame::default();
        let Some(g) = self.gilrs.as_mut() else { return frame };
        let mut any = false;
        while let Some(ev) = g.next_event() {
            use gilrs::ev::filter::{axis_dpad_to_button, Filter};
            // A D-pad reported as a hat axis arrives as its buttons.
            let Some(ev) = ev.filter_ev(&axis_dpad_to_button, g).filter(|e| !e.is_dropped()) else { continue };
            any = true;
            match ev.event {
                EventType::ButtonPressed(b, _) => {
                    if !self.used {
                        tracing::info!(name = g.gamepad(ev.id).name(), button = ?b, "controller in use");
                    }
                    self.active = Some(ev.id);
                    self.used = true;
                    frame.pressed.push(b);
                }
                EventType::Connected => {
                    let p = g.gamepad(ev.id);
                    tracing::info!(name = p.name(), vendor = ?p.vendor_id(), product = ?p.product_id(), "controller connected");
                    self.active.get_or_insert(ev.id);
                }
                EventType::Disconnected if self.active == Some(ev.id) => self.active = None,
                EventType::AxisChanged(..) if self.active.is_none() => self.active = Some(ev.id),
                _ => {}
            }
        }
        // Controllers announce themselves after start-up: label for them as they do.
        if any {
            self.detected = family(g, self.active);
        }
        let t = self.tuning;
        let Some(pad) = self.active.map(|id| g.gamepad(id)) else {
            self.raw = [Vec2::ZERO; 2];
            return frame;
        };
        self.raw = [Vec2::new(pad.value(Axis::LeftStickX), pad.value(Axis::LeftStickY)), Vec2::new(pad.value(Axis::RightStickX), pad.value(Axis::RightStickY))];
        let left = deadzone(self.raw[0], t.left_deadzone, t.outer_deadzone);
        let right = deadzone(self.raw[1], t.right_deadzone, t.outer_deadzone);
        let (walk, look) = if t.swap_sticks { (right, left) } else { (left, right) };
        frame.left = walk;
        let look = if look == Vec2::ZERO { look } else { look.normalize() * look.length().powf(t.curve.max(0.1)) };
        let sign = |inv: bool| if inv { -1.0 } else { 1.0 };
        frame.right = Vec2::new(look.x * t.turn_speed * sign(t.invert_turn), look.y * t.tilt_speed * sign(t.invert_tilt));
        let trigger = |b: Button| pad.button_data(b).is_some_and(|d| d.value() > t.trigger.clamp(0.05, 0.95));
        frame.lt = trigger(Button::LeftTrigger2);
        frame.rt = trigger(Button::RightTrigger2);
        // A stick only counts as picking up the controller when clearly pushed (with a small
        // deadzone, resting drift would otherwise wake the controller hints).
        if self.raw.iter().any(|v| v.length() > 0.35) && (frame.left != Vec2::ZERO || frame.right != Vec2::ZERO) {
            if !self.used {
                tracing::info!(name = pad.name(), left = ?frame.left, right = ?frame.right, "controller in use (sticks)");
            }
            self.used = true;
        }
        frame
    }
}

/// A pad's family, by its maker's USB id, else its name.
fn pad_family(p: &gilrs::Gamepad) -> Option<crate::padglyph::Style> {
    match p.vendor_id() {
        Some(0x054c) => Some(crate::padglyph::Style::PlayStation),
        Some(0x057e) => Some(crate::padglyph::Style::Nintendo),
        _ => crate::padglyph::detect(p.name()),
    }
}

/// The family to label buttons for: the pad in use. Steam Input can put a virtual Xbox pad in
/// front of a PlayStation or Nintendo one, so failing that, any connected pad that is one.
fn family(g: &Gilrs, active: Option<gilrs::GamepadId>) -> crate::padglyph::Style {
    active
        .and_then(|id| pad_family(&g.gamepad(id)))
        .or_else(|| g.gamepads().find_map(|(_, p)| pad_family(&p)))
        .unwrap_or(crate::padglyph::Style::Xbox)
}

#[cfg(test)]
mod tests {
    use super::deadzone;
    use glam::Vec2;

    #[test]
    fn deadzone_rescales_between_inner_and_outer() {
        assert_eq!(deadzone(Vec2::new(0.2, 0.0), 0.22, 0.0), Vec2::ZERO);
        // Just past the deadzone: barely moving; halfway: half; the rim: full.
        assert!(deadzone(Vec2::new(0.23, 0.0), 0.22, 0.0).x < 0.02);
        assert!((deadzone(Vec2::new(0.61, 0.0), 0.22, 0.0).x - 0.5).abs() < 1e-5);
        assert!((deadzone(Vec2::new(0.0, -1.0), 0.22, 0.0).y + 1.0).abs() < 1e-5);
        // An outer deadzone reads 90% travel as full tilt.
        assert!((deadzone(Vec2::new(0.9, 0.0), 0.2, 0.1).x - 1.0).abs() < 1e-5);
        // No deadzone at all passes the stick through.
        assert_eq!(deadzone(Vec2::new(0.3, 0.4), 0.0, 0.0), Vec2::new(0.3, 0.4));
    }
}
