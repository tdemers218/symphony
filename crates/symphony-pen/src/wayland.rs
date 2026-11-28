//! Wayland backend: the `tablet-v2` protocol for pens, plus `wl_pointer`
//! for the mouse, so the app gets one sample stream like on macOS.
//!
//! The window toolkit owns the Wayland connection. We share that same
//! `wl_display` (a second connection would never get events for the app's
//! surface), but use our own event queue, read on a background thread.
//! The compositor sends tablet events instead of pointer events while a pen
//! is in proximity, so the toolkit never wakes up for them: `set_waker`
//! lets the app repaint as soon as samples arrive.
//!
//! The pen's cursor is the app's job too: tablet-v2 shows none until the
//! client sets one. `set_cursor` gives each pen over our window the shape
//! the UI asks for, through the cursor-shape protocol. A compositor
//! without that protocol keeps its own default.

use std::collections::HashMap;
use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use rustix::event::{poll, PollFd, PollFlags, Timespec};
use wayland_client::backend::{Backend as WlBackend, ObjectId};
use wayland_client::protocol::wl_pointer::{self, WlPointer};
use wayland_client::protocol::wl_registry::{self, WlRegistry};
use wayland_client::protocol::wl_seat::{self, WlSeat};
use wayland_client::protocol::wl_surface::WlSurface;
use wayland_client::{
    event_created_child, Connection, Dispatch, EventQueue, Proxy, QueueHandle, WEnum,
};
use wayland_protocols::wp::cursor_shape::v1::client::{
    wp_cursor_shape_device_v1::{self, Shape, WpCursorShapeDeviceV1},
    wp_cursor_shape_manager_v1::{self, WpCursorShapeManagerV1},
};
use wayland_protocols::wp::tablet::zv2::client::{
    zwp_tablet_manager_v2::{self, ZwpTabletManagerV2},
    zwp_tablet_pad_group_v2::{self, ZwpTabletPadGroupV2},
    zwp_tablet_pad_ring_v2::{self, ZwpTabletPadRingV2},
    zwp_tablet_pad_strip_v2::{self, ZwpTabletPadStripV2},
    zwp_tablet_pad_v2::{self, ZwpTabletPadV2},
    zwp_tablet_seat_v2::{self, ZwpTabletSeatV2},
    zwp_tablet_tool_v2::{self, ZwpTabletToolV2},
    zwp_tablet_v2::{self, ZwpTabletV2},
};

use crate::{Cursor, DeviceInfo, PenError, PenSample, Phase, Tool};

// Linux input event codes (linux/input-event-codes.h).
const BTN_LEFT: u32 = 0x110;
const BTN_STYLUS: u32 = 0x14b;
const BTN_STYLUS2: u32 = 0x14c;
const BTN_STYLUS3: u32 = 0x149;

// Button bits, matching AppKit's buttonMask on macOS.
const TIP: u32 = 1;
const LOWER: u32 = 2;
const UPPER: u32 = 4;
const THIRD: u32 = 8;

type Waker = Box<dyn Fn() + Send + Sync>;

#[derive(Default)]
struct Shared {
    samples: Vec<PenSample>,
    device: DeviceInfo,
    waker: Option<Waker>,
    /// What the UI last asked for.
    cursor: Cursor,
    /// Pens over our window that can take a cursor shape.
    near: Vec<Near>,
}

/// A pen over our window, with the serial of its proximity event, which
/// every cursor request must carry.
struct Near {
    tool: ZwpTabletToolV2,
    shape: WpCursorShapeDeviceV1,
    serial: u32,
}

impl Near {
    fn show(&self, cursor: Cursor) {
        match shape_of(cursor) {
            Some(shape) => self.shape.set_shape(self.serial, shape),
            None => self.tool.set_cursor(self.serial, None, 0, 0),
        }
    }
}

/// The protocol's shape for `cursor`; `None` hides it.
fn shape_of(cursor: Cursor) -> Option<Shape> {
    Some(match cursor {
        Cursor::Hidden => return None,
        Cursor::Default => Shape::Default,
        Cursor::ContextMenu => Shape::ContextMenu,
        Cursor::Help => Shape::Help,
        Cursor::Pointer => Shape::Pointer,
        Cursor::Progress => Shape::Progress,
        Cursor::Wait => Shape::Wait,
        Cursor::Cell => Shape::Cell,
        Cursor::Crosshair => Shape::Crosshair,
        Cursor::Text => Shape::Text,
        Cursor::VerticalText => Shape::VerticalText,
        Cursor::Alias => Shape::Alias,
        Cursor::Copy => Shape::Copy,
        Cursor::Move => Shape::Move,
        Cursor::NoDrop => Shape::NoDrop,
        Cursor::NotAllowed => Shape::NotAllowed,
        Cursor::Grab => Shape::Grab,
        Cursor::Grabbing => Shape::Grabbing,
        Cursor::AllScroll => Shape::AllScroll,
        Cursor::ResizeEw => Shape::EwResize,
        Cursor::ResizeNs => Shape::NsResize,
        Cursor::ResizeNesw => Shape::NeswResize,
        Cursor::ResizeNwse => Shape::NwseResize,
        Cursor::ResizeE => Shape::EResize,
        Cursor::ResizeN => Shape::NResize,
        Cursor::ResizeNe => Shape::NeResize,
        Cursor::ResizeNw => Shape::NwResize,
        Cursor::ResizeS => Shape::SResize,
        Cursor::ResizeSe => Shape::SeResize,
        Cursor::ResizeSw => Shape::SwResize,
        Cursor::ResizeW => Shape::WResize,
        Cursor::ResizeCol => Shape::ColResize,
        Cursor::ResizeRow => Shape::RowResize,
        Cursor::ZoomIn => Shape::ZoomIn,
        Cursor::ZoomOut => Shape::ZoomOut,
    })
}

pub struct Backend {
    conn: Connection,
    shared: Arc<Mutex<Shared>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Backend {
    /// `display` is the toolkit's `wl_display`, `surface` the window's
    /// `wl_surface`. Both must outlive the backend.
    pub fn attach(display: NonNull<c_void>, surface: NonNull<c_void>) -> Result<Self, PenError> {
        // SAFETY: raw-window-handle guarantees a live wl_display, and the
        // toolkit keeps it open for the window's lifetime.
        let backend = unsafe { WlBackend::from_foreign_display(display.as_ptr().cast()) };
        let conn = Connection::from_backend(backend);
        // SAFETY: raw-window-handle guarantees a live wl_surface proxy.
        let surface_id =
            unsafe { ObjectId::from_ptr(WlSurface::interface(), surface.as_ptr().cast()) }
                .map_err(|_| PenError::NoNativeView)?;

        let mut queue: EventQueue<State> = conn.new_event_queue();
        let qh = queue.handle();
        conn.display().get_registry(&qh, ());

        let shared = Arc::new(Mutex::new(Shared::default()));
        let mut state = State::new(surface_id.as_ptr() as usize, shared.clone());
        // Learn the globals now, so a compositor without tablet support or
        // a broken connection is reported to the caller.
        queue
            .roundtrip(&mut state)
            .map_err(|_| PenError::MonitorFailed)?;
        // Seat capabilities and tablets arrive in reply to our binds.
        queue
            .roundtrip(&mut state)
            .map_err(|_| PenError::MonitorFailed)?;

        let stop = Arc::new(AtomicBool::new(false));
        let quit = stop.clone();
        let thread = std::thread::Builder::new()
            .name("symphony-pen-wayland".into())
            .spawn(move || run(queue, state, &quit))
            .map_err(|_| PenError::MonitorFailed)?;

        Ok(Backend {
            conn,
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

    /// Called by the app each frame. Requests go out only on a change, or
    /// from the reader thread when a pen comes near.
    pub fn set_cursor(&self, cursor: Cursor) {
        let Ok(mut s) = self.shared.lock() else {
            return;
        };
        if s.cursor == cursor {
            return;
        }
        s.cursor = cursor;
        for n in &s.near {
            n.show(cursor);
        }
        drop(s);
        let _ = self.conn.flush();
    }

    pub fn set_waker(&self, waker: Waker) {
        if let Ok(mut s) = self.shared.lock() {
            s.waker = Some(waker);
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

/// Read and dispatch our queue until asked to stop. Each wait is short so
/// a stop request is seen within 100 ms.
fn run(mut queue: EventQueue<State>, mut state: State, stop: &AtomicBool) {
    let timeout = Timespec::try_from(Duration::from_millis(100)).expect("timeout fits");
    while !stop.load(Ordering::Relaxed) {
        if queue.dispatch_pending(&mut state).is_err() {
            break;
        }
        state.publish();
        if queue.flush().is_err() {
            break;
        }
        let Some(guard) = queue.prepare_read() else {
            // Events are already queued; dispatch them first.
            continue;
        };
        let ready = {
            let fd = guard.connection_fd();
            let mut fds = [PollFd::new(&fd, PollFlags::IN)];
            matches!(poll(&mut fds, Some(&timeout)), Ok(n) if n > 0)
        };
        if ready {
            // A failed read means the connection is gone; the toolkit sees
            // the same error and handles it.
            if guard.read().is_err() {
                break;
            }
        }
        // Dropping the guard without reading cancels the read.
    }
    state.release();
    let _ = queue.flush();
    // The registry has no destructor, so the queue must outlive any event
    // the compositor might still send it. The process is usually exiting.
    std::mem::forget(queue);
}

/// What one tablet tool has reported since its last `frame` event.
struct ToolState {
    kind: Tool,
    /// In proximity over our window's surface.
    ours: bool,
    /// A position arrived since proximity in; frames before it have none.
    has_pos: bool,
    down: bool,
    went_down: bool,
    went_up: bool,
    x: f64,
    y: f64,
    pressure: f32,
    tilt_x: f32,
    tilt_y: f32,
    rotation: f32,
    tangential: f32,
    buttons: u32,
    serial: u64,
    capabilities: u64,
    /// Sets this pen's cursor, when the compositor has cursor-shape.
    shape: Option<WpCursorShapeDeviceV1>,
}

impl Default for ToolState {
    fn default() -> Self {
        ToolState {
            kind: Tool::Pen,
            ours: false,
            has_pos: false,
            down: false,
            went_down: false,
            went_up: false,
            x: 0.0,
            y: 0.0,
            pressure: 0.0,
            tilt_x: 0.0,
            tilt_y: 0.0,
            rotation: 0.0,
            tangential: 0.0,
            buttons: 0,
            serial: 0,
            capabilities: 0,
            shape: None,
        }
    }
}

#[derive(Default)]
struct PointerState {
    ours: bool,
    down: bool,
    x: f64,
    y: f64,
}

struct State {
    surface: usize,
    shared: Arc<Mutex<Shared>>,
    pending: Vec<PenSample>,
    device: DeviceInfo,
    device_changed: bool,
    manager: Option<ZwpTabletManagerV2>,
    cursor_shapes: Option<WpCursorShapeManagerV1>,
    seats: Vec<WlSeat>,
    tablet_seats: Vec<ZwpTabletSeatV2>,
    pointers: Vec<WlPointer>,
    tools: HashMap<ObjectId, ToolState>,
    tablets: HashMap<ObjectId, (u64, u64)>,
    pointer: PointerState,
}

impl State {
    fn new(surface: usize, shared: Arc<Mutex<Shared>>) -> Self {
        State {
            surface,
            shared,
            pending: Vec::new(),
            device: DeviceInfo::default(),
            device_changed: false,
            manager: None,
            cursor_shapes: None,
            seats: Vec::new(),
            tablet_seats: Vec::new(),
            pointers: Vec::new(),
            tools: HashMap::new(),
            tablets: HashMap::new(),
            pointer: PointerState::default(),
        }
    }

    fn is_ours(&self, surface: &WlSurface) -> bool {
        surface.id().as_ptr() as usize == self.surface
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

    fn forget_near(&self, tool: &ZwpTabletToolV2) {
        if let Ok(mut s) = self.shared.lock() {
            s.near.retain(|n| n.tool != *tool);
        }
    }

    fn pair_seats(&mut self, qh: &QueueHandle<State>) {
        let Some(manager) = &self.manager else {
            return;
        };
        while self.tablet_seats.len() < self.seats.len() {
            let seat = &self.seats[self.tablet_seats.len()];
            self.tablet_seats
                .push(manager.get_tablet_seat(seat, qh, ()));
        }
    }

    /// Destroy everything we created, so the compositor stops sending to a
    /// queue nobody reads.
    fn release(&mut self) {
        if let Ok(mut s) = self.shared.lock() {
            s.near.clear();
        }
        for t in self.tools.values_mut() {
            if let Some(d) = t.shape.take() {
                d.destroy();
            }
        }
        if let Some(m) = self.cursor_shapes.take() {
            m.destroy();
        }
        for p in self.pointers.drain(..) {
            if p.version() >= 3 {
                p.release();
            }
        }
        for s in self.tablet_seats.drain(..) {
            s.destroy();
        }
        if let Some(m) = self.manager.take() {
            m.destroy();
        }
        for s in self.seats.drain(..) {
            if s.version() >= 5 {
                s.release();
            }
        }
    }
}

impl Dispatch<WlRegistry, ()> for State {
    fn event(
        state: &mut Self,
        registry: &WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = event
        {
            match interface.as_str() {
                "wl_seat" => {
                    state
                        .seats
                        .push(registry.bind(name, version.min(5), qh, ()));
                    state.pair_seats(qh);
                }
                // Version 1 is all we read; version 2 only adds pad dials.
                "zwp_tablet_manager_v2" => {
                    state.manager = Some(registry.bind(name, 1, qh, ()));
                    state.pair_seats(qh);
                }
                // Version 1 has the tablet tool request.
                "wp_cursor_shape_manager_v1" => {
                    state.cursor_shapes = Some(registry.bind(name, 1, qh, ()));
                }
                _ => {}
            }
        }
    }
}

impl Dispatch<WlSeat, ()> for State {
    fn event(
        state: &mut Self,
        seat: &WlSeat,
        event: wl_seat::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_seat::Event::Capabilities {
            capabilities: WEnum::Value(caps),
        } = event
        {
            if caps.contains(wl_seat::Capability::Pointer) && state.pointers.is_empty() {
                state.pointers.push(seat.get_pointer(qh, ()));
            }
        }
    }
}

impl Dispatch<WlPointer, ()> for State {
    fn event(
        state: &mut Self,
        _: &WlPointer,
        event: wl_pointer::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let (phase, time) = match event {
            wl_pointer::Event::Enter {
                surface,
                surface_x,
                surface_y,
                ..
            } => {
                state.pointer.ours = state.is_ours(&surface);
                state.pointer.x = surface_x;
                state.pointer.y = surface_y;
                return;
            }
            wl_pointer::Event::Leave { .. } => {
                state.pointer.ours = false;
                return;
            }
            wl_pointer::Event::Motion {
                time,
                surface_x,
                surface_y,
            } => {
                state.pointer.x = surface_x;
                state.pointer.y = surface_y;
                let phase = if state.pointer.down {
                    Phase::Move
                } else {
                    Phase::Hover
                };
                (phase, time)
            }
            wl_pointer::Event::Button {
                time,
                button,
                state: WEnum::Value(pressed),
                ..
            } if button == BTN_LEFT => {
                let down = pressed == wl_pointer::ButtonState::Pressed;
                // A release after the pointer left still ends the stroke.
                if !down && !state.pointer.down {
                    return;
                }
                state.pointer.down = down;
                (if down { Phase::Down } else { Phase::Up }, time)
            }
            _ => return,
        };
        if !state.pointer.ours && phase != Phase::Up {
            return;
        }
        state.pending.push(PenSample {
            x: state.pointer.x,
            y: state.pointer.y,
            pressure: if state.pointer.down { 1.0 } else { 0.0 },
            tilt_x: 0.0,
            tilt_y: 0.0,
            rotation: 0.0,
            tangential: 0.0,
            phase,
            tool: Tool::Mouse,
            buttons: 0,
            time: f64::from(time) / 1000.0,
        });
    }
}

impl Dispatch<ZwpTabletManagerV2, ()> for State {
    fn event(
        _: &mut Self,
        _: &ZwpTabletManagerV2,
        _: zwp_tablet_manager_v2::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwpTabletSeatV2, ()> for State {
    fn event(
        state: &mut Self,
        _: &ZwpTabletSeatV2,
        event: zwp_tablet_seat_v2::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let zwp_tablet_seat_v2::Event::ToolAdded { id } = event {
            let shape = state
                .cursor_shapes
                .as_ref()
                .map(|m| m.get_tablet_tool_v2(&id, qh, ()));
            state.tools.insert(
                id.id(),
                ToolState {
                    shape,
                    ..ToolState::default()
                },
            );
        }
    }

    event_created_child!(State, ZwpTabletSeatV2, [
        zwp_tablet_seat_v2::EVT_TABLET_ADDED_OPCODE => (ZwpTabletV2, ()),
        zwp_tablet_seat_v2::EVT_TOOL_ADDED_OPCODE => (ZwpTabletToolV2, ()),
        zwp_tablet_seat_v2::EVT_PAD_ADDED_OPCODE => (ZwpTabletPadV2, ()),
    ]);
}

impl Dispatch<ZwpTabletV2, ()> for State {
    fn event(
        state: &mut Self,
        tablet: &ZwpTabletV2,
        event: zwp_tablet_v2::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            zwp_tablet_v2::Event::Id { vid, pid } => {
                state
                    .tablets
                    .insert(tablet.id(), (u64::from(vid), u64::from(pid)));
            }
            zwp_tablet_v2::Event::Removed => {
                state.tablets.remove(&tablet.id());
                tablet.destroy();
            }
            _ => {}
        }
    }
}

impl Dispatch<ZwpTabletToolV2, ()> for State {
    fn event(
        state: &mut Self,
        tool: &ZwpTabletToolV2,
        event: zwp_tablet_tool_v2::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use zwp_tablet_tool_v2::Event as E;

        if let E::Removed = event {
            state.forget_near(tool);
            if let Some(d) = state.tools.remove(&tool.id()).and_then(|t| t.shape) {
                d.destroy();
            }
            tool.destroy();
            return;
        }
        if let E::ProximityOut = event {
            state.forget_near(tool);
        }
        if let E::ProximityIn {
            serial,
            tablet,
            surface,
        } = &event
        {
            let ours = state.is_ours(surface);
            let shape = state.tools.get(&tool.id()).and_then(|t| t.shape.clone());
            if let (true, Some(shape), Ok(mut s)) = (ours, shape, state.shared.lock()) {
                let near = Near {
                    tool: tool.clone(),
                    shape,
                    serial: *serial,
                };
                near.show(s.cursor);
                s.near.retain(|n| n.tool != *tool);
                s.near.push(near);
            }
            let (vendor_id, tablet_id) = state.tablets.get(&tablet.id()).copied().unwrap_or((0, 0));
            if let Some(t) = state.tools.get_mut(&tool.id()) {
                t.ours = ours;
                if ours {
                    state.device = DeviceInfo {
                        vendor_id,
                        tablet_id,
                        unique_id: t.serial,
                        capability_mask: t.capabilities,
                        in_proximity: true,
                    };
                    state.device_changed = true;
                }
            }
            return;
        }

        let Some(t) = state.tools.get_mut(&tool.id()) else {
            return;
        };
        match event {
            E::Type {
                tool_type: WEnum::Value(kind),
            } => {
                t.kind = if kind == zwp_tablet_tool_v2::Type::Eraser {
                    Tool::Eraser
                } else {
                    Tool::Pen
                };
            }
            E::HardwareSerial {
                hardware_serial_hi,
                hardware_serial_lo,
            } => {
                t.serial = (u64::from(hardware_serial_hi) << 32) | u64::from(hardware_serial_lo);
            }
            E::Capability {
                capability: WEnum::Value(cap),
            } => {
                t.capabilities |= 1 << (cap as u32);
            }
            E::ProximityOut => {
                if t.ours {
                    state.device.in_proximity = false;
                    state.device_changed = true;
                }
                t.ours = false;
                t.has_pos = false;
                t.down = false;
            }
            E::Down { .. } => {
                t.down = true;
                t.went_down = true;
                t.buttons |= TIP;
            }
            E::Up => {
                t.down = false;
                t.went_up = true;
                t.buttons &= !TIP;
            }
            E::Motion { x, y } => {
                t.x = x;
                t.y = y;
                t.has_pos = true;
            }
            E::Pressure { pressure } => t.pressure = pressure as f32 / 65535.0,
            // Degrees, positive when the top of the pen leans toward +x or
            // +y (screen-down), which is already our convention. ±90°
            // maps to ±1.
            E::Tilt { tilt_x, tilt_y } => {
                t.tilt_x = (tilt_x / 90.0).clamp(-1.0, 1.0) as f32;
                t.tilt_y = (tilt_y / 90.0).clamp(-1.0, 1.0) as f32;
            }
            E::Rotation { degrees } => t.rotation = degrees as f32,
            E::Slider { position } => t.tangential = position as f32 / 65535.0,
            E::Button {
                button,
                state: WEnum::Value(pressed),
                ..
            } => {
                let bit = match button {
                    BTN_STYLUS => LOWER,
                    BTN_STYLUS2 => UPPER,
                    BTN_STYLUS3 => THIRD,
                    _ => 0,
                };
                if pressed == zwp_tablet_tool_v2::ButtonState::Pressed {
                    t.buttons |= bit;
                } else {
                    t.buttons &= !bit;
                }
            }
            E::Frame { time } => {
                // A quick tap can put down and up in one frame; send both
                // so the stroke still ends.
                let phases: &[Phase] = match (t.went_down, t.went_up) {
                    (true, true) => &[Phase::Down, Phase::Up],
                    (true, false) => &[Phase::Down],
                    (false, true) => &[Phase::Up],
                    (false, false) if t.down => &[Phase::Move],
                    (false, false) => &[Phase::Hover],
                };
                t.went_down = false;
                t.went_up = false;
                for &phase in phases {
                    let up = phase == Phase::Up;
                    // The pen can leave our window mid-stroke; still end it.
                    if !up && !(t.ours && t.has_pos) {
                        continue;
                    }
                    state.pending.push(PenSample {
                        x: t.x,
                        y: t.y,
                        pressure: if up { 0.0 } else { t.pressure },
                        tilt_x: t.tilt_x,
                        tilt_y: t.tilt_y,
                        rotation: t.rotation,
                        tangential: t.tangential,
                        phase,
                        tool: t.kind,
                        // A tap's up already cleared the tip bit.
                        buttons: if phase == Phase::Down {
                            t.buttons | TIP
                        } else {
                            t.buttons
                        },
                        time: f64::from(time) / 1000.0,
                    });
                }
            }
            _ => {}
        }
    }
}

// Pads (express keys, rings, strips) aren't used yet, but their objects
// still arrive and need handlers.
impl Dispatch<ZwpTabletPadV2, ()> for State {
    fn event(
        _: &mut Self,
        pad: &ZwpTabletPadV2,
        event: zwp_tablet_pad_v2::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let zwp_tablet_pad_v2::Event::Removed = event {
            pad.destroy();
        }
    }

    event_created_child!(State, ZwpTabletPadV2, [
        zwp_tablet_pad_v2::EVT_GROUP_OPCODE => (ZwpTabletPadGroupV2, ()),
    ]);
}

impl Dispatch<ZwpTabletPadGroupV2, ()> for State {
    fn event(
        _: &mut Self,
        _: &ZwpTabletPadGroupV2,
        _: zwp_tablet_pad_group_v2::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }

    event_created_child!(State, ZwpTabletPadGroupV2, [
        zwp_tablet_pad_group_v2::EVT_RING_OPCODE => (ZwpTabletPadRingV2, ()),
        zwp_tablet_pad_group_v2::EVT_STRIP_OPCODE => (ZwpTabletPadStripV2, ()),
    ]);
}

impl Dispatch<ZwpTabletPadRingV2, ()> for State {
    fn event(
        _: &mut Self,
        _: &ZwpTabletPadRingV2,
        _: zwp_tablet_pad_ring_v2::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwpTabletPadStripV2, ()> for State {
    fn event(
        _: &mut Self,
        _: &ZwpTabletPadStripV2,
        _: zwp_tablet_pad_strip_v2::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WpCursorShapeManagerV1, ()> for State {
    fn event(
        _: &mut Self,
        _: &WpCursorShapeManagerV1,
        _: wp_cursor_shape_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WpCursorShapeDeviceV1, ()> for State {
    fn event(
        _: &mut Self,
        _: &WpCursorShapeDeviceV1,
        _: wp_cursor_shape_device_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}
