//! Sony and Nintendo controllers through their own HID reports, for what the generic gamepad
//! layer (gilrs) can't reach: the DualSense and DualShock 4 touchpads, their rumble motors and
//! light bars, and the Switch Pro Controller's HD rumble (a frequency and an amplitude per
//! band). Input still comes through gilrs; this only adds to it.
//!
//! Report layouts follow Linux's hid-playstation driver (DualSense, DualShock 4) and the
//! documented Switch controller protocol (dekuNukem/Nintendo_Switch_Reverse_Engineering).
//! A thread per controller reads its input reports and writes the latest output state.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const SONY: u16 = 0x054c;
const NINTENDO: u16 = 0x057e;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    DualSense,
    DualShock4,
    SwitchPro,
}

fn kind_of(vid: u16, pid: u16) -> Option<Kind> {
    match (vid, pid) {
        (SONY, 0x0ce6 | 0x0df2) => Some(Kind::DualSense),
        (SONY, 0x05c4 | 0x09cc | 0x0ba0) => Some(Kind::DualShock4),
        (NINTENDO, 0x2009) => Some(Kind::SwitchPro),
        _ => None,
    }
}

/// One rumble band: a frequency (Hz) and a strength (0..1). Motor-driven pads use only the
/// strengths: the low band on the heavy motor, the high band on the light one.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct Band {
    pub freq: f32,
    pub amp: f32,
}

/// What the controller should be doing now.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct Output {
    pub low: Band,
    pub high: Band,
    /// Light bar colour (Sony pads); None leaves it as the system set it.
    pub light: Option<[u8; 3]>,
}

/// A finger on the touchpad, scaled to 0..1 across and down.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Finger {
    pub id: u8,
    pub x: f32,
    pub y: f32,
}

#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct Touch {
    pub fingers: [Option<Finger>; 2],
    /// The pad is pressed in.
    pub click: bool,
}

/// A connected Sony or Nintendo controller, worked by its own thread.
pub struct HidPad {
    pub kind: Kind,
    touch: Arc<Mutex<Touch>>,
    out: Sender<Output>,
    alive: Arc<AtomicBool>,
    last: Output,
}

impl HidPad {
    /// Opens the first supported controller, if any.
    pub fn open() -> Option<HidPad> {
        let api = match hidapi::HidApi::new() {
            Ok(a) => a,
            Err(e) => {
                tracing::debug!("hid: {e}");
                return None;
            }
        };
        let info = api.device_list().find(|d| kind_of(d.vendor_id(), d.product_id()).is_some())?;
        let kind = kind_of(info.vendor_id(), info.product_id())?;
        let bt_hint = matches!(info.bus_type(), hidapi::BusType::Bluetooth);
        let dev = match api.open_path(info.path()) {
            Ok(d) => d,
            Err(e) => {
                // Steam Input, or another program, may hold it.
                tracing::info!(?kind, "hid: controller found but not opened: {e}");
                return None;
            }
        };
        tracing::info!(?kind, bluetooth = bt_hint, "hid: controller opened for touchpad, rumble and light");
        let touch = Arc::new(Mutex::new(Touch::default()));
        let alive = Arc::new(AtomicBool::new(true));
        let bluetooth = Arc::new(AtomicBool::new(bt_hint));
        let (tx, rx) = channel();
        let (t, a, b) = (touch.clone(), alive.clone(), bluetooth);
        std::thread::Builder::new()
            .name("hidpad".into())
            .spawn(move || {
                run(dev, kind, b, t, rx);
                a.store(false, Ordering::Relaxed);
            })
            .ok()?;
        Some(HidPad { kind, touch, out: tx, alive, last: Output::default() })
    }

    pub fn alive(&self) -> bool {
        self.alive.load(Ordering::Relaxed)
    }

    pub fn touch(&self) -> Touch {
        self.touch.lock().map(|t| *t).unwrap_or_default()
    }

    /// Sends the state if it changed.
    pub fn set(&mut self, o: Output) {
        if o != self.last {
            self.last = o;
            let _ = self.out.send(o);
        }
    }
}

fn run(dev: hidapi::HidDevice, kind: Kind, bluetooth: Arc<AtomicBool>, touch: Arc<Mutex<Touch>>, rx: Receiver<Output>) {
    // Reading the calibration feature report switches a Bluetooth Sony pad to its full input
    // reports (with the touchpad); over USB it is harmless.
    let mut feature = [0u8; 64];
    match kind {
        Kind::DualSense => feature[0] = 0x05,
        Kind::DualShock4 => feature[0] = 0x02,
        Kind::SwitchPro => {}
    }
    if kind != Kind::SwitchPro {
        let _ = dev.get_feature_report(&mut feature);
    }
    let mut seq: u8 = 0;
    let mut state = Output::default();
    let mut sent_at = Instant::now() - Duration::from_secs(1);
    let mut started = false;
    let mut buf = [0u8; 128];
    loop {
        match dev.read_timeout(&mut buf, 8) {
            Ok(0) => {}
            Ok(n) => {
                if let Some((t, bt)) = parse_input(kind, &buf[..n]) {
                    bluetooth.store(bt, Ordering::Relaxed);
                    if let Ok(mut lock) = touch.lock() {
                        *lock = t;
                    }
                }
            }
            Err(e) => {
                tracing::info!(?kind, "hid: controller gone: {e}");
                return;
            }
        }
        let mut changed = false;
        loop {
            match rx.try_recv() {
                Ok(o) => {
                    state = o;
                    changed = true;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => return,
            }
        }
        let bt = bluetooth.load(Ordering::Relaxed);
        if !started {
            started = true;
            match kind {
                // Fade out the blue start-up light so ours shows.
                Kind::DualSense => {
                    let _ = dev.write(&dualsense_report(bt, &mut seq, &state, true));
                }
                // Vibration must be switched on (subcommand 0x48).
                Kind::SwitchPro => {
                    let mut r = vec![0x01, seq & 0x0f];
                    r.extend_from_slice(&[0x00, 0x01, 0x40, 0x40, 0x00, 0x01, 0x40, 0x40, 0x48, 0x01]);
                    seq = seq.wrapping_add(1);
                    let _ = dev.write(&r);
                }
                Kind::DualShock4 => {}
            }
        }
        // Switch rumble fades unless refreshed; resend it while it plays.
        let refresh = kind == Kind::SwitchPro && (state.low.amp > 0.0 || state.high.amp > 0.0) && sent_at.elapsed() >= Duration::from_millis(30);
        if changed || refresh {
            let report = match kind {
                Kind::DualSense => dualsense_report(bt, &mut seq, &state, false),
                Kind::DualShock4 => dualshock4_report(bt, &state),
                Kind::SwitchPro => switch_rumble_report(&mut seq, &state),
            };
            if let Err(e) = dev.write(&report) {
                tracing::debug!(?kind, "hid: write: {e}");
            }
            sent_at = Instant::now();
        }
    }
}

/// Touchpad point as both Sony pads send it: contact (bit 7 set = no finger), then 12-bit
/// x and y packed in three bytes.
fn point(b: &[u8], w: f32, h: f32) -> Option<Finger> {
    if b.len() < 4 || b[0] & 0x80 != 0 {
        return None;
    }
    let x = b[1] as u16 | ((b[2] as u16 & 0x0f) << 8);
    let y = (b[2] as u16 >> 4) | ((b[3] as u16) << 4);
    Some(Finger { id: b[0] & 0x7f, x: (x as f32 / w).clamp(0.0, 1.0), y: (y as f32 / h).clamp(0.0, 1.0) })
}

/// Touchpad state from an input report, and whether it came over Bluetooth.
fn parse_input(kind: Kind, r: &[u8]) -> Option<(Touch, bool)> {
    let (data, bt, w, h, touch_at) = match (kind, r.first()?) {
        // DualSense: USB 0x01 (data from byte 1), Bluetooth 0x31 (from byte 2); points at 32.
        (Kind::DualSense, 0x01) if r.len() >= 41 => (&r[1..], false, 1920.0, 1080.0, 32),
        (Kind::DualSense, 0x31) if r.len() >= 42 => (&r[2..], true, 1920.0, 1080.0, 32),
        // DualShock 4: USB 0x01 (from byte 1), Bluetooth 0x11 (from byte 3); points at 34.
        (Kind::DualShock4, 0x01) if r.len() >= 43 => (&r[1..], false, 1920.0, 942.0, 34),
        (Kind::DualShock4, 0x11) if r.len() >= 45 => (&r[3..], true, 1920.0, 942.0, 34),
        _ => return None,
    };
    // The touchpad button: buttons[2] bit 1 (data byte 9 on the DualSense, 6 on the DS4).
    let click = match kind {
        Kind::DualSense => data[9] & 0x02 != 0,
        _ => data[6] & 0x02 != 0,
    };
    let fingers = [point(&data[touch_at..], w, h), point(&data[touch_at + 4..], w, h)];
    Some((Touch { fingers, click }, bt))
}

fn amp8(a: f32) -> u8 {
    (a.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// CRC-32 (IEEE) as Sony's Bluetooth output reports carry it, seeded with 0xa2.
fn sony_crc(report: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for &byte in std::iter::once(&0xa2u8).chain(report) {
        crc ^= byte as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xedb8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

/// DualSense output: rumble (emulated on its haptic actuators), light bar and player light.
fn dualsense_report(bt: bool, seq: &mut u8, o: &Output, fade_startup_light: bool) -> Vec<u8> {
    let mut c = [0u8; 47];
    // valid_flag0: compatible vibration, haptics select.
    c[0] = 0x01 | 0x02;
    c[2] = amp8(o.high.amp); // right, light actuator
    c[3] = amp8(o.low.amp); // left, heavy
    if let Some([r, g, b]) = o.light {
        // valid_flag1: light bar and player indicator.
        c[1] = 0x04 | 0x10;
        c[43] = 0x04; // the middle player light
        c[44] = r;
        c[45] = g;
        c[46] = b;
    }
    // valid_flag2: the newer firmware's vibration flag, and the light bar setup.
    c[38] = 0x04;
    if fade_startup_light {
        c[38] |= 0x02;
        c[41] = 0x02;
    }
    if bt {
        let mut r = vec![0u8; 78];
        r[0] = 0x31;
        r[1] = (*seq & 0x0f) << 4;
        r[2] = 0x10;
        r[3..50].copy_from_slice(&c);
        *seq = seq.wrapping_add(1);
        let crc = sony_crc(&r[..74]);
        r[74..78].copy_from_slice(&crc.to_le_bytes());
        r
    } else {
        let mut r = vec![0x02];
        r.extend_from_slice(&c);
        r.resize(63, 0);
        r
    }
}

/// DualShock 4 output: the two motors and the light bar.
fn dualshock4_report(bt: bool, o: &Output) -> Vec<u8> {
    let mut c = [0u8; 10];
    c[0] = 0x01 | if o.light.is_some() { 0x02 } else { 0 };
    c[3] = amp8(o.high.amp); // right, light motor
    c[4] = amp8(o.low.amp); // left, heavy
    if let Some([r, g, b]) = o.light {
        c[5] = r;
        c[6] = g;
        c[7] = b;
    }
    if bt {
        let mut r = vec![0u8; 78];
        r[0] = 0x11;
        r[1] = 0xc0; // HID report with CRC
        r[3..13].copy_from_slice(&c);
        let crc = sony_crc(&r[..74]);
        r[74..78].copy_from_slice(&crc.to_le_bytes());
        r
    } else {
        let mut r = vec![0x05];
        r.extend_from_slice(&c);
        r.resize(32, 0);
        r
    }
}

/// Switch HD rumble for one side: a high band (81-1253 Hz) and a low band (41-626 Hz), each
/// with its own amplitude, packed in four bytes.
pub fn switch_rumble(high: Band, low: Band) -> [u8; 4] {
    let enc_freq = |f: f32| (f.max(1.0) / 10.0).log2() * 32.0;
    let enc_amp = |a: f32| -> u8 {
        let a = a.clamp(0.0, 1.0);
        let v = if a > 0.23 {
            (a * 8.7).log2() * 32.0
        } else if a > 0.12 {
            (a * 17.0).log2() * 16.0
        } else if a > 0.0 {
            ((a.log2() * 32.0) - 96.0) / (4.0 - 2.0 * a)
        } else {
            0.0
        };
        v.round().clamp(0.0, 200.0) as u8
    };
    if high.amp <= 0.0 && low.amp <= 0.0 {
        return [0x00, 0x01, 0x40, 0x40];
    }
    let hf = ((enc_freq(high.freq.clamp(82.0, 1252.0)).round() as i32 - 0x60).max(1) * 4) as u16;
    let lf = (enc_freq(low.freq.clamp(41.0, 626.0)).round() as i32 - 0x40).clamp(1, 0x7f) as u8;
    let hf_amp = enc_amp(high.amp) as u16 * 2;
    let lf_amp = enc_amp(low.amp) as u16 / 2 + 0x40;
    [(hf & 0xff) as u8, (hf_amp + ((hf >> 8) & 0xff)) as u8, lf + ((lf_amp >> 8) & 0xff) as u8, (lf_amp & 0xff) as u8]
}

/// Switch output report 0x10: rumble only, the same on both sides.
fn switch_rumble_report(seq: &mut u8, o: &Output) -> Vec<u8> {
    let side = switch_rumble(o.high, o.low);
    let mut r = vec![0x10, *seq & 0x0f];
    *seq = seq.wrapping_add(1);
    r.extend_from_slice(&side);
    r.extend_from_slice(&side);
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn touch_points() {
        // Finger 3 at x 0x123, y 0x345; second slot empty.
        let f = point(&[0x03, 0x23, 0x51, 0x34], 1920.0, 1080.0).unwrap();
        assert_eq!(f.id, 3);
        assert!((f.x - 0x123 as f32 / 1920.0).abs() < 1e-6);
        assert!((f.y - 0x345 as f32 / 1080.0).abs() < 1e-6);
        assert!(point(&[0x83, 0, 0, 0], 1920.0, 1080.0).is_none());
    }

    #[test]
    fn switch_rumble_encoding() {
        // Silence is the documented neutral value.
        assert_eq!(switch_rumble(Band::default(), Band::default()), [0x00, 0x01, 0x40, 0x40]);
        // 320 Hz high / 160 Hz low: encoded frequencies 0xa0 and 0x80.
        let r = switch_rumble(Band { freq: 320.0, amp: 1.0 }, Band { freq: 160.0, amp: 1.0 });
        assert_eq!(r[0], ((0xa0 - 0x60) * 4) as u8);
        assert_eq!(r[2] & 0x7f, 0x80 - 0x40);
    }

    #[test]
    fn crc() {
        // Standard CRC-32 (zlib.crc32) of the seed byte, then of the seed and a report start.
        assert_eq!(sony_crc(&[]), 0xeada_2d49);
        assert_eq!(sony_crc(&[0x31, 0x00, 0x10]), 0xfe38_9151);
    }
}
