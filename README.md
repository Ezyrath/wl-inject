# wl-inject

A tiny, persistent virtual pointer + keyboard for testing Wayland apps headlessly (no real display, no real input hardware touched).

## The problem this solves

Wayland's security model doesn't let a client inject input into another client's window — unlike X11, where `xdotool` could fake input trivially. Compositors that want to support this expose it explicitly via two wlroots protocol extensions:

- `zwlr_virtual_pointer_manager_v1` (mouse motion/buttons/scroll)
- `zwp_virtual_keyboard_manager_v1` (key presses)

The existing CLI tool for this, `wlrctl`, looked like the obvious choice — but every `wlrctl` invocation is a **one-shot connection**: it connects, creates a virtual device, sends a single action, and disconnects immediately. On Sway (and presumably other wlroots compositors), this means the virtual device is created and destroyed again within about a millisecond. The compositor's `wl_seat` briefly advertises the pointer/keyboard capability and then retracts it — too fast for a client (e.g. `winit`, which most Rust GUI/game apps use) to notice the capability appear, bind a `wl_pointer`/`wl_keyboard` object, and receive the event before the capability disappears again. In practice this means **input sent via `wlrctl` is silently dropped** — confirmed by diffing screenshots taken before/after several `wlrctl` calls: byte-identical, no matter what was sent.

`wl-inject` fixes this by opening the Wayland connection once and creating the virtual pointer and virtual keyboard **once at startup**, keeping them alive for as long as the process runs. Multiple actions sent over that single session all reliably reach the target application.

## Why keyboard needs a keymap, and pointer doesn't

The virtual keyboard protocol requires uploading a compiled XKB keymap (`.keymap()` request, an mmap'd blob) *before* any `.key()` event means anything to the compositor or the client reading it — otherwise key codes are meaningless numbers. `wl-inject` compiles a plain "us" `pc105` layout via `libxkbcommon` at startup and uploads it through a `memfd`. The pointer protocol has no such requirement, which is why getting mouse input working came first and was simpler.

## Building

```bash
nix build          # produces ./result/bin/wl-inject
# or, without nix:
cargo build --release
```

Needs `libwayland-client.so` and `libxkbcommon.so` at runtime (present on basically any Linux desktop already, since every Wayland compositor needs them). No system package beyond a normal Rust build environment is required if you already have those two libs — see `flake.nix` for the exact `buildInputs`/`nativeBuildInputs` used to build the package hermetically.

## Running

Point `WAYLAND_DISPLAY` at whatever compositor you want to inject into (a real session, or a headless one — see "Headless testing recipe" below), then feed it newline-separated commands on stdin:

```
move <dx> <dy>       relative pointer motion, in pixels
down [0|1|2]         press a mouse button and hold it (0=left/default, 1=right, 2=middle)
up [0|1|2]           release a mouse button
click [0|1|2]        press + release in one shot
scroll <dy> <dx>      scroll wheel/trackpad axis values (vertical, horizontal)
key <name> [0|1]     press (default / 1) or release (0) a key by name
sleep <ms>           pause between commands (useful for letting a drag register across frames)
quit                 close the connection and exit
```

Known key `<name>`s: `esc`/`escape`, `space`, `ctrl`/`leftctrl`/`control`, `rightctrl`, `shift`/`leftshift`, `rightshift`, `alt`/`leftalt`, `tab`, `enter`/`return`, `up`, `down`, `left`, `right`, `w`, `a`, `s`, `d`. Add more to the `evdev_keycode` match in `src/main.rs` as needed — they're plain Linux evdev codes from `input-event-codes.h`, not Bevy/winit `KeyCode` names.

### Driving it from a shell script

Because the whole point is a **long-lived** process, and most shells (including a fresh Claude Code `Bash` tool call) don't persist file descriptors between separate invocations, the simplest way to send it commands over time is a FIFO kept open by a throwaway background holder process — otherwise the moment a single `echo cmd > fifo` closes, the reader sees EOF and `wl-inject` exits:

```bash
mkfifo /tmp/wl-inject.fifo
nohup bash -c "exec 3>/tmp/wl-inject.fifo; sleep 999999" >/dev/null 2>&1 & disown
nohup env WAYLAND_DISPLAY=wayland-1 ./result/bin/wl-inject < /tmp/wl-inject.fifo > /tmp/wl-inject.log 2>&1 & disown

echo "move 640 360" > /tmp/wl-inject.fifo
echo "scroll -30 0" > /tmp/wl-inject.fifo     # e.g. zoom in a few notches
echo "down 0"       > /tmp/wl-inject.fifo
echo "move 100 0"   > /tmp/wl-inject.fifo     # drag
echo "up 0"         > /tmp/wl-inject.fifo
echo "key esc"      > /tmp/wl-inject.fifo     # press+hold... (send "key esc 0" after to release)
echo "quit"         > /tmp/wl-inject.fifo
```

## Headless testing recipe (the reason this exists)

Verified end-to-end against a real Bevy/wgpu app (Vulkan renderer, AMD radv driver) on NixOS. Two things that *don't* work, tried first:

- **Xvfb (X11)**: Vulkan's X11 WSI presentation requires the `DRI3` X extension, which Xvfb doesn't implement — the app panics the moment it tries to create a swapchain (`Fallback system failed to choose present mode`). Forcing a software/GL fallback avoids the crash but means you're no longer testing the graphics backend real users get.
- **weston `--backend=headless-backend.so`**: no real GPU-backed allocator behind its headless output, so Vulkan surface creation fails a different way (`ERROR_SURFACE_LOST_KHR`).

**What works: headless Sway.** wlroots' headless backend still allocates buffers through the real DRM render node (`Created GBM allocator with backend drm` in its debug log), so Vulkan gets a genuine GPU-backed swapchain even with no monitor attached. It also never loads a `libinput` backend in this configuration at all (no real evdev/seat device-open ever happens), so there is no path by which it can touch your actual keyboard/mouse.

```bash
echo "output HEADLESS-1 resolution 1280x720" > /tmp/sway-headless.conf
WLR_BACKENDS=headless WLR_LIBINPUT_NO_DEVICES=1 WAYLAND_DISPLAY=wayland-1 sway -c /tmp/sway-headless.conf &
# sway may ignore the requested socket name and pick the next free wayland-N slot —
# check `ls $XDG_RUNTIME_DIR/wayland-*` rather than assuming it took wayland-1.

env -u DISPLAY WAYLAND_DISPLAY=wayland-1 /path/to/your/app &   # unset DISPLAY or winit may still try X11

grim -o HEADLESS-1 -t png shot.png   # or just `grim shot.png` with one output
# ... drive input via wl-inject as shown above, screenshot again, diff ...
```
