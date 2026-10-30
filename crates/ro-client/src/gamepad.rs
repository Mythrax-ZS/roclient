//! Controller input through gilrs (XInput on Windows; evdev on Linux, where Steam Input
//! presents the Steam Deck's controls as an Xbox pad). Each frame yields the stick and
//! trigger state plus the buttons pressed since the last frame.

use gilrs::{Axis, Button, EventType, Gilrs};
use glam::Vec2;

/// Stick travel ignored around the centre, rescaled so motion starts smoothly past it.
const DEADZONE: f32 = 0.22;

#[derive(Default, Clone)]
pub struct PadFrame {
    pub left: Vec2,
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
}

fn deadzone(v: Vec2) -> Vec2 {
    let len = v.length();
    if len < DEADZONE {
        return Vec2::ZERO;
    }
    v / len * ((len - DEADZONE) / (1.0 - DEADZONE)).min(1.0)
}

impl Pad {
    pub fn new() -> Self {
        let gilrs = Gilrs::new().inspect_err(|e| tracing::warn!("controller support unavailable: {e}")).ok();
        let active = gilrs.as_ref().and_then(|g| g.gamepads().inspect(|(_, p)| tracing::info!(name = p.name(), "controller found")).last().map(|(id, pad)| {
            tracing::info!(name = pad.name(), "controller connected");
            id
        }));
        let detected = gilrs.as_ref().map_or(crate::padglyph::Style::Xbox, |g| family(g, active));
        Self { gilrs, active, used: false, detected, rumble: None, rumble_now: (0.0, 0.0) }
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
        while let Some(ev) = g.next_event() {
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
            // Controllers announce themselves after start-up: label for them as they do.
            self.detected = family(g, self.active);
        }
        let Some(pad) = self.active.map(|id| g.gamepad(id)) else { return frame };
        frame.left = deadzone(Vec2::new(pad.value(Axis::LeftStickX), pad.value(Axis::LeftStickY)));
        frame.right = deadzone(Vec2::new(pad.value(Axis::RightStickX), pad.value(Axis::RightStickY)));
        let trigger = |b: Button| pad.button_data(b).is_some_and(|d| d.value() > 0.4);
        frame.lt = trigger(Button::LeftTrigger2);
        frame.rt = trigger(Button::RightTrigger2);
        if frame.left != Vec2::ZERO || frame.right != Vec2::ZERO {
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
