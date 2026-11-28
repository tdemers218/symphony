//! X11 backend: XInput2 events from the slave devices behind the core
//! pointer, so the app gets one sample stream like on macOS and Wayland.
//!
//! The pen also moves the core pointer on X11, so the window toolkit sees
//! it as a mouse and the UI works without help. We open our own connection
//! and read only the slave devices (the tablet tools and the mice), never
//! the master pointer. That matters: the server gives a button press's
//! implicit grab to the first client that selected the master, and taking
//! it would cut the toolkit off mid-drag. Slave selections don't take it,
//! and each client gets the whole stroke (tested on Xvfb 21.1 with XTEST).
//!
//! Positions arrive in X pixels. The app passes the window scale factor in
//! through `set_scale_factor`, so samples are in logical points like on the
//! other backends.
//!
//! Not read yet: barrel rotation and the airbrush wheel. XWayland sends
//! both on one valuator ("Abs Wheel", rotation × 5 plus the slider), so
//! they can't be told apart.

use std::collections::HashMap;
use std::os::fd::AsFd;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use rustix::event::{poll, PollFd, PollFlags, Timespec};
use x11rb::connection::Connection;
use x11rb::protocol::xinput::{
    self, ConnectionExt as _, DeviceClassData, DeviceType, EventMask, Fp3232, PointerEventFlags,
    XIEventMask,
};
use x11rb::protocol::xproto::{Atom, ConnectionExt as _, Window};
use x11rb::protocol::Event;
use x11rb::rust_connection::RustConnection;

use crate::{DeviceInfo, PenError, PenSample, Phase, Tool};

// Button bits, matching AppKit's buttonMask on macOS.
const TIP: u32 = 1;
const LOWER: u32 = 2;
const UPPER: u32 = 4;
const THIRD: u32 = 8;

type Waker = Box<dyn Fn() + Send + Sync>;

struct Shared {
    samples: Vec<PenSample>,
    device: DeviceInfo,
    waker: Option<Waker>,
    scale: f64,
}

pub struct Backend {
    shared: Arc<Mutex<Shared>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Backend {
    /// `window` is the toolkit's X window. We connect to the same server
    /// the toolkit did, through `$DISPLAY`.
    pub fn attach(window: Window) -> Result<Self, PenError> {
        let (conn, screen) = RustConnection::connect(None).map_err(|_| PenError::MonitorFailed)?;
        // The server's behaviour depends on the version a client asks for;
        // 2.4 is the one the grab behaviour above was tested with.
        let version = conn
            .xinput_xi_query_version(2, 4)
            .ok()
            .and_then(|c| c.reply().ok())
            .ok_or(PenError::Unsupported)?;
        if version.major_version < 2 {
            return Err(PenError::Unsupported);
        }
        let root = conn.setup().roots[screen].root;
        let labels = Labels::intern(&conn).ok_or(PenError::MonitorFailed)?;
        // New tablets and mice show up as hierarchy changes on the root.
        conn.xinput_xi_select_events(
            root,
            &[EventMask {
                deviceid: xinput::Device::ALL.into(),
                mask: vec![XIEventMask::HIERARCHY],
            }],
        )
        .ok()
        .and_then(|c| c.check().ok())
        .ok_or(PenError::MonitorFailed)?;
        let mut devices = HashMap::new();
        scan(&conn, window, &labels, &mut devices).ok_or(PenError::MonitorFailed)?;

        let shared = Arc::new(Mutex::new(Shared {
            samples: Vec::new(),
            device: DeviceInfo::default(),
            waker: None,
            scale: 1.0,
        }));
        let state = State {
            conn,
            window,
            labels,
            devices,
            shared: shared.clone(),
            pending: Vec::new(),
            device: DeviceInfo::default(),
            device_changed: false,
        };
        let stop = Arc::new(AtomicBool::new(false));
        let quit = stop.clone();
        let thread = std::thread::Builder::new()
            .name("symphony-pen-x11".into())
            .spawn(move || run(state, &quit))
            .map_err(|_| PenError::MonitorFailed)?;

        Ok(Backend {
            shared,
            stop,
            thread: Some(thread),
        })
    }

    pub fn drain(&self) -> Vec<PenSample> {
        self.shared
            .lock()
            .map(|mut s| std::mem::take(&mut s.samples))
            .unwrap_or_default()
    }

    pub fn has_pending(&self) -> bool {
        self.shared
            .lock()
            .map(|s| !s.samples.is_empty())
            .unwrap_or(false)
    }

    pub fn device(&self) -> DeviceInfo {
        self.shared
            .lock()
            .map(|s| s.device.clone())
            .unwrap_or_default()
    }

    pub fn set_waker(&self, waker: Waker) {
        if let Ok(mut s) = self.shared.lock() {
            s.waker = Some(waker);
        }
    }

    pub fn set_scale_factor(&self, scale: f64) {
        if !(scale.is_finite() && scale > 0.0) {
            return;
        }
        if let Ok(mut s) = self.shared.lock() {
            s.scale = scale;
        }
    }
}

impl Drop for Backend {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Read events until asked to stop. Each wait is short so a stop request
/// is seen within 100 ms. The connection is ours, so it can close at any
/// time without touching the toolkit's.
fn run(mut state: State, stop: &AtomicBool) {
    let timeout = Timespec::try_from(Duration::from_millis(100)).expect("timeout fits");
    while !stop.load(Ordering::Relaxed) {
        let scale = state.shared.lock().map(|s| s.scale).unwrap_or(1.0);
        loop {
            match state.conn.poll_for_event() {
                Ok(Some(event)) => state.handle(event, scale),
                Ok(None) => break,
                // The server went away; the toolkit sees the same error.
                Err(_) => return,
            }
        }
        state.publish();
        let fd = state.conn.stream().as_fd();
        let mut fds = [PollFd::new(&fd, PollFlags::IN)];
        let _ = poll(&mut fds, Some(&timeout));
    }
}

/// The valuator label atoms we look for. A label is how XInput2 says what
/// an axis means; its number differs between drivers.
struct Labels {
    pressure: Atom,
    tilt_x: Atom,
    tilt_y: Atom,
}

impl Labels {
    fn intern(conn: &RustConnection) -> Option<Self> {
        let atom = |name: &str| -> Option<Atom> {
            // Create the atom if no driver has yet, so a tablet plugged in
            // later still matches.
            Some(
                conn.intern_atom(false, name.as_bytes())
                    .ok()?
                    .reply()
                    .ok()?
                    .atom,
            )
        };
        Some(Labels {
            pressure: atom("Abs Pressure")?,
            tilt_x: atom("Abs Tilt X")?,
            tilt_y: atom("Abs Tilt Y")?,
        })
    }
}

#[derive(Clone, Copy)]
struct Axis {
    number: u16,
    min: f64,
    max: f64,
}

impl Axis {
    /// 0..=1 across the axis range.
    fn unit(&self, values: &[f64]) -> f32 {
        let v = values
            .get(usize::from(self.number))
            .copied()
            .unwrap_or(self.min);
        if self.max > self.min {
            ((v - self.min) / (self.max - self.min)).clamp(0.0, 1.0) as f32
        } else {
            0.0
        }
    }

    fn raw(&self, values: &[f64]) -> f64 {
        values.get(usize::from(self.number)).copied().unwrap_or(0.0)
    }
}

/// One slave pointer device and what it has reported so far.
struct Device {
    kind: Tool,
    pressure: Option<Axis>,
    tilt_x: Option<Axis>,
    tilt_y: Option<Axis>,
    /// Last value of each valuator. Events only carry the ones that changed.
    values: Vec<f64>,
    down: bool,
    buttons: u32,
}

/// Find the mice and tablet tools and select their events on `window`.
/// Devices already known keep their state.
fn scan(
    conn: &RustConnection,
    window: Window,
    labels: &Labels,
    devices: &mut HashMap<u16, Device>,
) -> Option<()> {
    let reply = conn
        .xinput_xi_query_device(xinput::Device::ALL)
        .ok()?
        .reply()
        .ok()?;
    let mut found = HashMap::new();
    for info in &reply.infos {
        if info.type_ != DeviceType::SLAVE_POINTER || !info.enabled {
            continue;
        }
        let axis = |label: Atom| {
            info.classes.iter().find_map(|c| match &c.data {
                DeviceClassData::Valuator(v) if v.label == label => Some(Axis {
                    number: v.number,
                    min: fp3232(v.min),
                    max: fp3232(v.max),
                }),
                _ => None,
            })
        };
        let pressure = axis(labels.pressure);
        let tilt_x = axis(labels.tilt_x);
        let tilt_y = axis(labels.tilt_y);
        // Touchscreens reach the app through the toolkit, as on Wayland.
        let touch = info
            .classes
            .iter()
            .any(|c| matches!(c.data, DeviceClassData::Touch(_)));
        if touch {
            continue;
        }
        let name = String::from_utf8_lossy(&info.name).to_lowercase();
        // A pressure axis means a tablet tool. XInput2 has no tool type, so
        // the eraser is found by its device name, as GTK and Qt do.
        // XWayland names it "xwayland-tablet eraser:N".
        let kind = match pressure {
            Some(_) if name.contains("eraser") => Tool::Eraser,
            Some(_) => Tool::Pen,
            None => Tool::Mouse,
        };
        let valuators = info
            .classes
            .iter()
            .filter_map(|c| match &c.data {
                DeviceClassData::Valuator(v) => Some(usize::from(v.number) + 1),
                _ => None,
            })
            .max()
            .unwrap_or(0);
        let device = devices.remove(&info.deviceid).unwrap_or(Device {
            kind,
            pressure,
            tilt_x,
            tilt_y,
            values: vec![0.0; valuators],
            down: false,
            buttons: 0,
        });
        found.insert(info.deviceid, device);
    }
    *devices = found;

    let mask = XIEventMask::MOTION | XIEventMask::BUTTON_PRESS | XIEventMask::BUTTON_RELEASE;
    let masks: Vec<EventMask> = devices
        .keys()
        .map(|&deviceid| EventMask {
            deviceid,
            mask: vec![mask],
        })
        .collect();
    if !masks.is_empty() {
        conn.xinput_xi_select_events(window, &masks)
            .ok()?
            .check()
            .ok()?;
    }
    Some(())
}

struct State {
    conn: RustConnection,
    window: Window,
    labels: Labels,
    devices: HashMap<u16, Device>,
    shared: Arc<Mutex<Shared>>,
    pending: Vec<PenSample>,
    device: DeviceInfo,
    device_changed: bool,
}

impl State {
    fn handle(&mut self, event: Event, scale: f64) {
        let (e, kind) = match event {
            Event::XinputMotion(e) => (e, Kind::Motion),
            Event::XinputButtonPress(e) => (e, Kind::Press),
            Event::XinputButtonRelease(e) => (e, Kind::Release),
            Event::XinputHierarchy(_) => {
                // A device went away or a new one arrived (a tablet plugged
                // in). A failed rescan keeps the devices we had.
                let mut devices = std::mem::take(&mut self.devices);
                let _ = scan(&self.conn, self.window, &self.labels, &mut devices);
                self.devices = devices;
                return;
            }
            _ => return,
        };
        // Master events and scroll buttons emulated from a wheel or touch
        // aren't ours; the toolkit handles them.
        if e.deviceid != e.sourceid
            || e.event != self.window
            || e.flags.contains(PointerEventFlags::POINTER_EMULATED)
        {
            return;
        }
        let Some(d) = self.devices.get_mut(&e.deviceid) else {
            return;
        };
        let mut set = e
            .valuator_mask
            .iter()
            .enumerate()
            .flat_map(|(word, &bits)| {
                (0..32)
                    .filter(move |&b| bits & (1u32 << b) != 0)
                    .map(move |b| word * 32 + b)
            });
        for value in &e.axisvalues {
            let Some(i) = set.next() else { break };
            if let Some(slot) = d.values.get_mut(i) {
                *slot = fp3232(*value);
            }
        }

        let is_pen = d.kind != Tool::Mouse;
        let phase = match (kind, e.detail) {
            (Kind::Motion, _) if d.down => Phase::Move,
            (Kind::Motion, _) => Phase::Hover,
            (Kind::Press, 1) if !d.down => {
                d.down = true;
                Phase::Down
            }
            (Kind::Release, 1) if d.down => {
                d.down = false;
                Phase::Up
            }
            // Pen side buttons, numbered as XWayland and the Wacom driver
            // do: BTN_STYLUS = 2, BTN_STYLUS2 = 3, BTN_STYLUS3 = 8. They
            // change the bits of the next sample.
            (Kind::Press | Kind::Release, button) if is_pen => {
                let bit = match button {
                    2 => LOWER,
                    3 => UPPER,
                    8 => THIRD,
                    _ => 0,
                };
                if kind == Kind::Press {
                    d.buttons |= bit;
                } else {
                    d.buttons &= !bit;
                }
                return;
            }
            _ => return,
        };

        let sample = if is_pen {
            let up = phase == Phase::Up;
            let tilt = |a: Option<Axis>| {
                // Degrees on XWayland and the Wacom driver; ±90° maps to ±1,
                // as on Wayland.
                a.map_or(0.0, |a| (a.raw(&d.values) / 90.0).clamp(-1.0, 1.0) as f32)
            };
            PenSample {
                x: fp1616(e.event_x) / scale,
                y: fp1616(e.event_y) / scale,
                pressure: if up {
                    0.0
                } else {
                    d.pressure.map_or(1.0, |a| a.unit(&d.values))
                },
                tilt_x: tilt(d.tilt_x),
                tilt_y: tilt(d.tilt_y),
                rotation: 0.0,
                tangential: 0.0,
                phase,
                tool: d.kind,
                buttons: if d.down || phase == Phase::Down {
                    d.buttons | TIP
                } else {
                    d.buttons
                },
                time: f64::from(e.time) / 1000.0,
            }
        } else {
            PenSample {
                x: fp1616(e.event_x) / scale,
                y: fp1616(e.event_y) / scale,
                pressure: if d.down { 1.0 } else { 0.0 },
                tilt_x: 0.0,
                tilt_y: 0.0,
                rotation: 0.0,
                tangential: 0.0,
                phase,
                tool: Tool::Mouse,
                buttons: 0,
                time: f64::from(e.time) / 1000.0,
            }
        };
        // XInput2 has no proximity events, and XWayland sends nothing when
        // the pen leaves. So the pen counts as in range from its first event
        // until the mouse moves.
        if is_pen != self.device.in_proximity {
            self.device.in_proximity = is_pen;
            self.device_changed = true;
        }
        self.pending.push(sample);
    }

    /// Hand new samples to the app and wake it.
    fn publish(&mut self) {
        if self.pending.is_empty() && !self.device_changed {
            return;
        }
        let Ok(mut s) = self.shared.lock() else {
            return;
        };
        s.samples.append(&mut self.pending);
        if self.device_changed {
            s.device = self.device.clone();
            self.device_changed = false;
        }
        if let Some(wake) = &s.waker {
            wake();
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Motion,
    Press,
    Release,
}

fn fp3232(v: Fp3232) -> f64 {
    f64::from(v.integral) + f64::from(v.frac) / 4_294_967_296.0
}

fn fp1616(v: i32) -> f64 {
    f64::from(v) / 65536.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_point() {
        assert_eq!(fp1616(3 << 16 | 0x8000), 3.5);
        assert_eq!(fp1616(-(1 << 16)), -1.0);
        let half = Fp3232 {
            integral: -2,
            frac: 1 << 31,
        };
        assert_eq!(fp3232(half), -1.5);
    }

    #[test]
    fn axis_unit_clamps_to_range() {
        let a = Axis {
            number: 1,
            min: 0.0,
            max: 65535.0,
        };
        assert_eq!(a.unit(&[0.0, 65535.0]), 1.0);
        assert_eq!(a.unit(&[0.0, 70000.0]), 1.0);
        assert_eq!(a.unit(&[0.0]), 0.0);
    }
}
