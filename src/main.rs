//! Persistent virtual pointer + keyboard for headless Wayland compositor testing.
//!
//! One-shot CLI injectors (e.g. `wlrctl`) connect, send a single action, and
//! disconnect — on wlroots compositors this creates and immediately destroys
//! the virtual input device, and the resulting `wl_seat` capability flicker
//! is too fast for a client like winit to react to. This tool instead opens
//! one connection, creates the virtual devices once, and keeps them alive for
//! the whole session, reading simple commands from stdin.

use std::io::{BufRead, Write as _};
use std::os::fd::AsFd;

use wayland_client::protocol::wl_seat::WlSeat;
use wayland_client::{Connection, Dispatch, QueueHandle};
use wayland_protocols_misc::zwp_virtual_keyboard_v1::client::zwp_virtual_keyboard_manager_v1::ZwpVirtualKeyboardManagerV1;
use wayland_protocols_misc::zwp_virtual_keyboard_v1::client::zwp_virtual_keyboard_v1::ZwpVirtualKeyboardV1;
use wayland_protocols_wlr::virtual_pointer::v1::client::zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1;
use wayland_protocols_wlr::virtual_pointer::v1::client::zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1;

struct State {
    seat: Option<WlSeat>,
    pointer_manager: Option<ZwlrVirtualPointerManagerV1>,
    keyboard_manager: Option<ZwpVirtualKeyboardManagerV1>,
}

impl Dispatch<wayland_client::protocol::wl_registry::WlRegistry, ()> for State {
    fn event(
        state: &mut Self,
        registry: &wayland_client::protocol::wl_registry::WlRegistry,
        event: wayland_client::protocol::wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wayland_client::protocol::wl_registry::Event::Global { name, interface, .. } = event {
            match interface.as_str() {
                "wl_seat" => {
                    state.seat = Some(registry.bind::<WlSeat, _, _>(name, 1, qh, ()));
                }
                "zwlr_virtual_pointer_manager_v1" => {
                    state.pointer_manager =
                        Some(registry.bind::<ZwlrVirtualPointerManagerV1, _, _>(name, 1, qh, ()));
                }
                "zwp_virtual_keyboard_manager_v1" => {
                    state.keyboard_manager =
                        Some(registry.bind::<ZwpVirtualKeyboardManagerV1, _, _>(name, 1, qh, ()));
                }
                _ => {}
            }
        }
    }
}

impl Dispatch<WlSeat, ()> for State {
    fn event(_: &mut Self, _: &WlSeat, _: wayland_client::protocol::wl_seat::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

impl Dispatch<ZwlrVirtualPointerManagerV1, ()> for State {
    fn event(_: &mut Self, _: &ZwlrVirtualPointerManagerV1, _: wayland_protocols_wlr::virtual_pointer::v1::client::zwlr_virtual_pointer_manager_v1::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

impl Dispatch<ZwlrVirtualPointerV1, ()> for State {
    fn event(_: &mut Self, _: &ZwlrVirtualPointerV1, _: wayland_protocols_wlr::virtual_pointer::v1::client::zwlr_virtual_pointer_v1::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

impl Dispatch<ZwpVirtualKeyboardManagerV1, ()> for State {
    fn event(_: &mut Self, _: &ZwpVirtualKeyboardManagerV1, _: wayland_protocols_misc::zwp_virtual_keyboard_v1::client::zwp_virtual_keyboard_manager_v1::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

impl Dispatch<ZwpVirtualKeyboardV1, ()> for State {
    fn event(_: &mut Self, _: &ZwpVirtualKeyboardV1, _: wayland_protocols_misc::zwp_virtual_keyboard_v1::client::zwp_virtual_keyboard_v1::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

fn now_ms() -> u32 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u32
}

/// Compiles a plain "us" XKB keymap and uploads it via a memfd — required
/// once before a `zwp_virtual_keyboard_v1` will forward any `key` event; the
/// compositor has no other way to know what a given evdev keycode means.
fn upload_keymap(keyboard: &ZwpVirtualKeyboardV1) {
    let context = xkbcommon::xkb::Context::new(xkbcommon::xkb::CONTEXT_NO_FLAGS);
    let keymap = xkbcommon::xkb::Keymap::new_from_names(
        &context,
        "",
        "pc105",
        "us",
        "",
        None,
        xkbcommon::xkb::KEYMAP_COMPILE_NO_FLAGS,
    )
    .expect("failed to compile a plain 'us' xkb keymap");
    let mut keymap_str = keymap.get_as_string(xkbcommon::xkb::KEYMAP_FORMAT_TEXT_V1);
    keymap_str.push('\0');
    let bytes = keymap_str.as_bytes();

    let fd = rustix::fs::memfd_create("wl-inject-keymap", rustix::fs::MemfdFlags::empty())
        .expect("memfd_create failed");
    {
        let mut file = std::fs::File::from(fd.try_clone().unwrap());
        file.write_all(bytes).expect("failed to write keymap into memfd");
    }

    keyboard.keymap(wayland_client::protocol::wl_keyboard::KeymapFormat::XkbV1.into(), fd.as_fd(), bytes.len() as u32);
}

/// Linux evdev keycodes (`input-event-codes.h`) — the virtual-keyboard
/// protocol's `key` request uses these directly, not XKB keycodes (XKB
/// keycode == evdev + 8) and not Bevy's `KeyCode`.
fn evdev_keycode(name: &str) -> Option<u32> {
    Some(match name {
        "esc" | "escape" => 1,
        "1" => 2,
        "2" => 3,
        "3" => 4,
        "4" => 5,
        "5" => 6,
        "6" => 7,
        "7" => 8,
        "8" => 9,
        "9" => 10,
        "0" => 11,
        "minus" | "-" => 12,
        "equal" | "=" => 13,
        "backspace" => 14,
        "tab" => 15,
        "q" => 16,
        "w" => 17,
        "e" => 18,
        "r" => 19,
        "t" => 20,
        "y" => 21,
        "u" => 22,
        "i" => 23,
        "o" => 24,
        "p" => 25,
        "leftbrace" | "[" => 26,
        "rightbrace" | "]" => 27,
        "enter" | "return" => 28,
        "ctrl" | "leftctrl" | "control" => 29,
        "a" => 30,
        "s" => 31,
        "d" => 32,
        "f" => 33,
        "g" => 34,
        "h" => 35,
        "j" => 36,
        "k" => 37,
        "l" => 38,
        "semicolon" | ";" => 39,
        "apostrophe" | "'" => 40,
        "grave" | "`" => 41,
        "shift" | "leftshift" => 42,
        "backslash" | "\\" => 43,
        "z" => 44,
        "x" => 45,
        "c" => 46,
        "v" => 47,
        "b" => 48,
        "n" => 49,
        "m" => 50,
        "comma" | "," => 51,
        "dot" | "." => 52,
        "slash" | "/" => 53,
        "rightshift" => 54,
        "kpasterisk" => 55,
        "alt" | "leftalt" => 56,
        "space" => 57,
        "capslock" => 58,
        "f1" => 59,
        "f2" => 60,
        "f3" => 61,
        "f4" => 62,
        "f5" => 63,
        "f6" => 64,
        "f7" => 65,
        "f8" => 66,
        "f9" => 67,
        "f10" => 68,
        "numlock" => 69,
        "scrolllock" => 70,
        "kp7" => 71,
        "kp8" => 72,
        "kp9" => 73,
        "kpminus" => 74,
        "kp4" => 75,
        "kp5" => 76,
        "kp6" => 77,
        "kpplus" => 78,
        "kp1" => 79,
        "kp2" => 80,
        "kp3" => 81,
        "kp0" => 82,
        "kpdot" => 83,
        "f11" => 87,
        "f12" => 88,
        "kpenter" => 96,
        "rightctrl" => 97,
        "kpslash" => 98,
        "rightalt" => 100,
        "home" => 102,
        "up" => 103,
        "pageup" => 104,
        "left" => 105,
        "right" => 106,
        "end" => 107,
        "down" => 108,
        "pagedown" => 109,
        "insert" => 110,
        "delete" => 111,
        _ => return None,
    })
}

fn main() {
    let conn = Connection::connect_to_env().expect("connect to WAYLAND_DISPLAY");
    let display = conn.display();
    let mut event_queue = conn.new_event_queue();
    let qh = event_queue.handle();
    display.get_registry(&qh, ());

    let mut state = State { seat: None, pointer_manager: None, keyboard_manager: None };
    event_queue.roundtrip(&mut state).unwrap();
    event_queue.roundtrip(&mut state).unwrap();

    let seat = state.seat.clone().expect("no wl_seat found");

    let pointer_manager = state.pointer_manager.clone().expect("no zwlr_virtual_pointer_manager_v1 (compositor doesn't support it)");
    let pointer = pointer_manager.create_virtual_pointer(Some(&seat), &qh, ());

    let keyboard_manager = state.keyboard_manager.clone().expect("no zwp_virtual_keyboard_manager_v1 (compositor doesn't support it)");
    let keyboard = keyboard_manager.create_virtual_keyboard(&seat, &qh, ());
    upload_keymap(&keyboard);

    event_queue.roundtrip(&mut state).unwrap();

    eprintln!("wl-inject: persistent virtual pointer + keyboard ready, reading commands from stdin");
    eprintln!("commands: move <dx> <dy> | down/up/click [0|1|2] | scroll <dy> <dx> | key <name> <0|1> | sleep <ms> | quit");

    let stdin = std::io::stdin();
    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        let parts: Vec<&str> = line.trim().split_whitespace().collect();
        if parts.is_empty() {
            continue;
        }
        match parts[0] {
            "move" => {
                let dx: f64 = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0.0);
                let dy: f64 = parts.get(2).and_then(|s| s.parse().ok()).unwrap_or(0.0);
                pointer.motion(now_ms(), dx, dy);
                pointer.frame();
            }
            "click" => {
                let btn = mouse_button(parts.get(1).copied());
                pointer.button(now_ms(), btn, wayland_client::protocol::wl_pointer::ButtonState::Pressed);
                pointer.frame();
                pointer.button(now_ms(), btn, wayland_client::protocol::wl_pointer::ButtonState::Released);
                pointer.frame();
            }
            "down" => {
                let btn = mouse_button(parts.get(1).copied());
                pointer.button(now_ms(), btn, wayland_client::protocol::wl_pointer::ButtonState::Pressed);
                pointer.frame();
            }
            "up" => {
                let btn = mouse_button(parts.get(1).copied());
                pointer.button(now_ms(), btn, wayland_client::protocol::wl_pointer::ButtonState::Released);
                pointer.frame();
            }
            "scroll" => {
                let dy: f64 = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0.0);
                let dx: f64 = parts.get(2).and_then(|s| s.parse().ok()).unwrap_or(0.0);
                if dy != 0.0 {
                    pointer.axis(now_ms(), wayland_client::protocol::wl_pointer::Axis::VerticalScroll, dy);
                }
                if dx != 0.0 {
                    pointer.axis(now_ms(), wayland_client::protocol::wl_pointer::Axis::HorizontalScroll, dx);
                }
                pointer.frame();
            }
            "key" => {
                let Some(name) = parts.get(1) else {
                    eprintln!("key: missing key name");
                    continue;
                };
                let Some(code) = evdev_keycode(name) else {
                    eprintln!("key: unknown key name '{name}'");
                    continue;
                };
                match parts.get(2).copied() {
                    Some("0") => keyboard.key(now_ms(), code, wayland_client::protocol::wl_keyboard::KeyState::Released.into()),
                    _ => keyboard.key(now_ms(), code, wayland_client::protocol::wl_keyboard::KeyState::Pressed.into()),
                }
            }
            "sleep" => {
                let ms: u64 = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(100);
                std::thread::sleep(std::time::Duration::from_millis(ms));
            }
            "quit" => break,
            other => {
                eprintln!("unknown command: {other}");
            }
        }
        conn.flush().unwrap();
        event_queue.roundtrip(&mut state).unwrap();
    }
}

fn mouse_button(arg: Option<&str>) -> u32 {
    match arg {
        Some("1") => 0x111, // BTN_RIGHT
        Some("2") => 0x112, // BTN_MIDDLE
        _ => 0x110,         // BTN_LEFT
    }
}
