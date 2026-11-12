//! SDL2's GameController API as gamepad.rs's fallback on Linux, used when gilrs fails or finds no pad. DroidDeck
//! (Steam in proot on Android) fakes /dev/input/eventN with plain files and hooks open / read / ioctl / poll, but not
//! epoll: gilrs's epoll_ctl on a pad gets EPERM and Gilrs::new fails. SDL reads the same nodes without epoll.
//! SDL runs with no window (the pad events only), so it never takes the focus or the signals.

use super::gamepad::Pad;
use sdl2::controller::{Axis, Button, GameController};
use sdl2::event::Event;

/// SDL's buttons as padsin's virtual numbers (gamepad::VIRTUAL by position: SDL's A is the bottom face button, gilrs's
/// South). The triggers are axes in SDL, read in `state`.
const VIRTUAL: &[(Button, u8)] = &[
    (Button::B, 0),
    (Button::X, 1),
    (Button::Y, 2),
    (Button::A, 3),
    (Button::DPadRight, 4),
    (Button::DPadLeft, 5),
    (Button::DPadUp, 6),
    (Button::DPadDown, 7),
    (Button::LeftShoulder, 8),
    (Button::LeftStick, 10),
    (Button::RightShoulder, 11),
    (Button::RightStick, 13),
    (Button::Start, 14),
    (Button::Back, 15),
];

/// A trigger past this (0 ..= 1) is its button held, as gilrs's default press threshold.
const TRIGGER_PRESS: f32 = 0.75;

pub struct Sdl {
    _sdl: sdl2::Sdl,
    gc: sdl2::GameControllerSubsystem,
    pump: sdl2::EventPump,
    pads: Vec<GameController>,
    /// The instance id of the pad that plays: the last one touched.
    active: Option<u32>,
}

impl Sdl {
    pub fn new() -> Result<Sdl, String> {
        // No SIGINT / SIGTERM handlers of SDL's, and pad events while our (winit) window has the focus, SDL having none.
        sdl2::hint::set("SDL_NO_SIGNAL_HANDLERS", "1");
        sdl2::hint::set("SDL_JOYSTICK_ALLOW_BACKGROUND_EVENTS", "1");
        let sdl = sdl2::init()?;
        let gc = sdl.game_controller()?;
        let pump = sdl.event_pump()?;
        // The pads already there arrive as ControllerDeviceAdded on the first poll.
        Ok(Sdl { _sdl: sdl, gc, pump, pads: Vec::new(), active: None })
    }

    /// Takes SDL's events: pads coming and going, and the last one pressed.
    pub fn update(&mut self) {
        let events: Vec<Event> = self.pump.poll_iter().collect();
        for ev in events {
            match ev {
                Event::ControllerDeviceAdded { which, .. } => match self.gc.open(which) {
                    Ok(c) if !self.pads.iter().any(|p| p.instance_id() == c.instance_id()) => {
                        log::info!("gamepad (SDL) {} connected: {}", c.instance_id(), c.name());
                        self.active.get_or_insert(c.instance_id());
                        self.pads.push(c);
                    }
                    Ok(_) => {}
                    Err(e) => log::warn!("gamepad (SDL): opening pad {which}: {e}"),
                },
                Event::ControllerDeviceRemoved { which, .. } => {
                    self.pads.retain(|p| p.instance_id() != which);
                    if self.active == Some(which) {
                        self.active = self.pads.first().map(|p| p.instance_id());
                    }
                }
                Event::ControllerButtonDown { which, .. } => self.active = Some(which),
                _ => {}
            }
        }
    }

    /// The pad that plays, as gamepad.rs reads one.
    pub fn state(&self) -> Option<Pad> {
        let c = self.pads.iter().find(|p| Some(p.instance_id()) == self.active)?;
        let a = |x: Axis| c.axis(x) as f32 / 32767.0;
        let (lt, rt) = (a(Axis::TriggerLeft).max(0.0), a(Axis::TriggerRight).max(0.0));
        let mut buttons: Vec<u8> = VIRTUAL.iter().filter(|(b, _)| c.button(*b)).map(|(_, n)| *n).collect();
        buttons.extend([(lt, 9), (rt, 12)].into_iter().filter(|(v, _)| *v > TRIGGER_PRESS).map(|(_, n)| n));
        buttons.sort_unstable();
        // SDL's sticks are down positive; gilrs's (and XInput's) up.
        let s = |x: Axis| a(x).clamp(-1.0, 1.0);
        Some(Pad {
            buttons,
            axes: [s(Axis::LeftX), -s(Axis::LeftY), s(Axis::RightX), -s(Axis::RightY), lt.min(1.0), rt.min(1.0)],
        })
    }

    /// The connected pads' names, the one that plays first.
    pub fn names(&self) -> Vec<String> {
        let mut v: Vec<(bool, String)> =
            self.pads.iter().map(|p| (Some(p.instance_id()) != self.active, p.name())).collect();
        v.sort();
        v.into_iter().map(|p| p.1).collect()
    }
}
