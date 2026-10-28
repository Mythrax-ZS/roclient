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
    /// Held: the shortcut bar modifiers.
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
    /// Face buttons are labelled the PlayStation way.
    pub playstation: bool,
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
        let active = gilrs.as_ref().and_then(|g| g.gamepads().next().map(|(id, pad)| {
            tracing::info!(name = pad.name(), "controller connected");
            id
        }));
        let playstation = active.zip(gilrs.as_ref()).is_some_and(|(id, g)| is_playstation(g.gamepad(id).name()));
        Self { gilrs, active, used: false, playstation }
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
                    if self.active != Some(ev.id) {
                        self.playstation = is_playstation(g.gamepad(ev.id).name());
                    }
                    self.active = Some(ev.id);
                    self.used = true;
                    frame.pressed.push(b);
                }
                EventType::Connected => tracing::info!(name = g.gamepad(ev.id).name(), "controller connected"),
                EventType::Disconnected if self.active == Some(ev.id) => self.active = None,
                EventType::AxisChanged(..) if self.active.is_none() => self.active = Some(ev.id),
                _ => {}
            }
        }
        let Some(pad) = self.active.map(|id| g.gamepad(id)) else { return frame };
        frame.left = deadzone(Vec2::new(pad.value(Axis::LeftStickX), pad.value(Axis::LeftStickY)));
        frame.right = deadzone(Vec2::new(pad.value(Axis::RightStickX), pad.value(Axis::RightStickY)));
        let trigger = |b: Button| pad.button_data(b).is_some_and(|d| d.value() > 0.4);
        frame.lt = trigger(Button::LeftTrigger2);
        frame.rt = trigger(Button::RightTrigger2);
        if frame.left != Vec2::ZERO || frame.right != Vec2::ZERO {
            self.used = true;
        }
        frame
    }
}

fn is_playstation(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    ["ps3", "ps4", "ps5", "dualsense", "dualshock", "sony", "wireless controller"].iter().any(|k| n.contains(k))
}
