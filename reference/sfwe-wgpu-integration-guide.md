# starfish-we Integration Guide (for the wgpu rendering layer / interpreter hosts)

> Audience: the agent / developer integrating this library. This is the
> canonical entry point; it complements CLAUDE.md (repository engineering
> conventions) and "android绑定运行问题与解决方案.md" (the platform-runtime
> incident archive). Every "verified" claim below is tagged with its
> verification level: **real-hardware tested** / emulator-tested /
> compile-time (check).

中文版：[wgpu接入指南.md](wgpu接入指南.md)。

---

## 1. What this library is — and is not

**It is**: a cross-platform **single-window + events** library with an
SDL-style poll model — the caller owns the main loop and pulls events via
`poll_event()`; window operations are plain method calls. Built for the
"scripting interpreter drives its own tick" scenario (winit's inversion of
control conflicts with that).

**It is not**: a rendering library. It paints nothing — you attach wgpu;
the window entity reaches wgpu through raw-window-handle / raw handles.

Two iron rules run through the whole API:

1. **Single window**: at most one `Window` per process; a second `new`
   returns `WindowError::AlreadyExists`. Re-creating after drop works.
2. **Physical pixels**: every size/coordinate is a physical pixel (not CSS
   logical pixels, not scaled logical coordinates) — they line up directly
   with `wgpu::SurfaceConfiguration::width/height`.

---

## 2. Integration in three steps

### 2.1 Dependency and features

```toml
[dependencies]
starfish-we = { path = "…", features = ["wgpu"] }
```

| feature | meaning |
|---|---|
| `wgpu` | implements the raw-window-handle 0.6 traits; **wgpu itself is not a dependency** |
| `wgpu-auto` | implies `wgpu`, pulls in wgpu: [`AutoSurface`](…) owns the resize→configure cycle |
| `android-demo` | built-in Android demo host (logcat event loop + exit contract) |

Events are core — there is no "window without events" build shape.

### 2.2 Minimal window creation

```rust
use starfish_we::{Window, WindowError};

let mut window = Window::new("title", (800, 600), Default::default())?;   // shorthand
// Or the full-option builder (a new option = a new method; non-breaking):
let mut window = Window::builder()
    .title("demo")
    .size((800, 600))          // client area, physical pixels
    .resizable(true)           // default true
    .borderless(false)
    .min_size((400, 300))      // constrains user resizing
    .max_size((1920, 1080))
    .always_on_top(false)
    .build()?;
```

### 2.3 Main-loop skeleton (SDL style)

```rust
loop {
    while let Some(event) = window.poll_event() {   // non-blocking; pumps the platform source
        match event { /* see section 3 */ }
    }
    // ← advance your own tick here: interpreter bytecode / render a frame
}
```

**Never** pump the platform message loop yourself (Win32 PeekMessage /
NSApp / ALooper) — `poll_event` already drains it; pumping externally
steals events.

---

## 3. Event semantics — the full table

```rust
pub enum Event {
    CloseRequested,                 // user asked to close. End the loop, drop Window
    Resized { width: u32, height: u32 },   // client area, physical pixels → reconfigure surface
    FocusGained, FocusLost,
    KeyDown { key: KeyCode, modifiers: Modifiers, repeat: bool },
    KeyUp { key: KeyCode, modifiers: Modifiers },
    TextInput { text: String },     // final text (independent of KeyDown; no IME composition in v1)
    MouseMoved { x: f64, y: f64 },  // client-area origin, physical pixels
    MouseDown { button: MouseButton }, MouseUp { button: MouseButton },
    MouseWheel { delta_x: f64, delta_y: f64 },  // positive = up / right
    Touch { phase: TouchPhase, finger: u64, x: f64, y: f64 },  // touch devices only
    RedrawRequested,                // the platform asks for a redraw (wgpu: present here)
    Suspended, Resumed,             // mobile & Web only: surface invalid / restored
    User(UserEvent),                // host-injected custom event (see 3.5)
}
```

`KeyCode` follows the W3C `KeyboardEvent.code` naming (`KeyA` / `Digit1` /
`ArrowUp` / `Numpad0`… about 105 keys + `Other(native code)`); text always
travels through `TextInput`. `Modifiers { shift, ctrl, alt, meta }`
(meta = Windows logo / macOS Command).

**Platform production matrix** (variants a platform never produces simply
never appear on it):

| Event | Windows | macOS | Linux (Wayland) | iOS | Android | Web |
|---|---|---|---|---|---|---|
| CloseRequested | ✅ WM_CLOSE / Alt+F4 | ✅ close button | ✅ toplevel close_request / lost connection | ❌ (v0: no close concept) | ✅ Destroy / back key | ❌ (closing the page ends it) |
| Resized | ✅ | ✅ | ✅ configure / scale change | ✅ (initial only; inherently full-screen) | ✅ | ✅ |
| Focus | ✅ | ✅ | ✅ wl_keyboard enter/leave | ❌ (v0: not wired) | ✅ | ✅ |
| Keyboard / TextInput | ✅ | ✅ (NSEvent characters) | ✅ (evdev→KeyCode; xkb text; client-side repeat) | ❌ (v0: not wired) | ✅ (IME committed text) | ✅ (no IME composition) |
| Mouse | ✅ | ✅ | ✅ (logical × scale = physical) | ❌ (v0: touch delivery not wired) | synthesized from primary finger | ✅ |
| Touch | ❌ | ❌ | ❌ (touch protocols not wired) | ❌ (v0: delivery not wired) | ✅ | ✅ |
| RedrawRequested | ✅ WM_PAINT | ❌ (frame cadence = host tick) | ✅ one on the initial configure | ❌ (CADisplayLink drives the tick itself) | ✅ RedrawNeeded/InitWindow | ✅ rAF, coalesced per frame |
| Suspended / Resumed | ❌ | ✅ miniaturize / deminiaturize | ❌ | ❌ (v0: lifecycle not wired) | ✅ Pause·Stop·TerminateWindow / Resume·InitWindow | ✅ page hidden/visible |
| User (custom) | ✅ host-injected | ✅ host-injected | ✅ host-injected | ✅ host-injected (deterministic test channel) | ✅ host-injected | ✅ host-injected |

---

## 3.5 Custom events (`Event::User`) — a typed replacement for SDL's `RegisterEvents`

The standard path for the host (interpreter worker threads, task systems,
any code) to inject messages into the main loop. **Platform independent**:
the channel lives in the common layer; behavior is identical on all six
platforms.

```rust
use starfish_we::{Event, EventProxy, UserEvent};

// ① Take a proxy (Clone, Send + Sync) — hand it to any worker thread
let proxy: EventProxy = window.event_proxy();
std::thread::spawn(move || {
    proxy.send(1, "script finished".to_string()).unwrap();   // code is host-defined
    proxy.send(2, Timer { id: 7 }).unwrap();
});

// ② Receive in the main loop
while let Some(event) = window.poll_event() {
    match event {
        Event::User(user) => match user.code {
            1 => { let msg = user.downcast_ref::<String>().unwrap(); }
            2 => { let timer: std::sync::Arc<Timer> = user.downcast().unwrap(); }
            _ => {}
        },
        _ => {}
    }
}

// ③ Same-thread convenience (equivalent to proxy.send, ignoring errors)
window.push_event(3, 12345u64);
```

Semantics and caveats:

- `UserEvent { code: u32, payload }`: `code` is the host-defined category;
  the payload is `Any + Send + Sync` **Arc-shared** (`Event: Clone` shares
  the payload — broadcast semantics; `downcast_mut` / `downcast` need
  exclusive ownership to succeed).
- **Ordering guarantee**: a `send` is always visible on the next
  `poll_event`; `Event::User` dispatches before that round's platform
  events. Batched injections dispatch in order (platform events are
  delayed briefly, never lost).
- `EventProxyClosed`: after the `Window` is destroyed, `send` fails
  explicitly (a protection SDL lacks).
- The platform event loop is unaware of user events: they are never eaten
  or reordered by the platform layer; Android's `WINDOW_READY` lifecycle
  filter does not apply to them either.
- `Event` intentionally does not implement `PartialEq` (the `User` variant
  carries an incomparable dynamic payload). Match with `match`/`matches!`.

---

## 4. The wgpu integration contract (rendering-loop template)

```rust
use starfish_we::{Event, Window};

let mut window = Window::builder().title("app").size((1280, 720)).build()?;
let instance = wgpu::Instance::default();
let mut surface = Some(instance.create_surface(&window)?);   // zero unsafe
let mut size = window.size();
// configure(&device, &SurfaceConfiguration { width: size.0, height: size.1, … })

loop {
    while let Some(event) = window.poll_event() {
        match event {
            Event::Resized { width, height } => {
                size = (width, height);
                // surface.configure(...) with the new size (also
                // reconfigure when get_current_texture returns
                // Suboptimal/Outdated — see the wgpu docs)
            }
            Event::Suspended => { surface = None; }   // Android: the system reclaimed the surface
            Event::Resumed => { /* rebuild + configure with the current size */ }
            Event::CloseRequested => { return Ok(()); }
            _ => {}
        }
    }
    if let Some(surface) = &surface {
        // get_current_texture → render → present (RedrawRequested marks the pacing)
    }
    // The interpreter tick advances here
}
```

Executable reference: `tests/wgpu_integration.rs` (verified on real
Windows hardware: create_surface → configure → acquire a frame →
re-configure → acquire again, all zero-unsafe).

Per-platform surface sources:

| Platform | surface source | status |
|---|---|---|
| Windows | `create_surface(&window)` (HWND+HINSTANCE carried automatically) | **verified on real hardware** ✅ |
| macOS | same (NSWindow view + layer handles carried automatically) | traits implemented, wgpu path untested |
| iOS | same (UIView + layer handles) | traits implemented, untested |
| Android | same (ANativeWindow; valid only while `Resumed`) | traits implemented, device-side untested |
| Web | `window.canvas()` → wgpu's Web backend (needs a WebGPU-capable browser) | accessor ready, path not yet exercised |
| Linux | same (`wl_display` + `wl_surface` raw pointers; requires the wayland `system` backend) | traits implemented, wgpu path untested |

---

## 5. Per-platform driving principles (the part an agent must understand)

### 5.1 Windows (Win32, `windows-sys` FFI)

- **Mechanism**: the OS message queue natively supports polling.
  `poll_event` drains this thread's messages non-blockingly with
  `PeekMessageW(PM_REMOVE)` → `DispatchMessageW` → `wndproc` translates
  into a pending queue → one event is popped.
- The translation layer lives in `platform/windows/translate.rs`,
  compiled only under the `event` feature; messages returning `None` fall
  back to `DefWindowProcW` (Alt+F4, menu mnemonics, etc. keep their system
  defaults).
- The process declares Per-Monitor V2 DPI awareness → coordinates are
  always physical pixels.
- `WM_CLOSE` only reports `CloseRequested` and **does not destroy** the
  window; destruction happens when the `Window` is dropped
  (`DestroyWindow`). Continuing to poll after CloseRequested is safe.
- Mouse drags outside the window: `SetCapture` on the first button down,
  `ReleaseCapture` when all buttons are up.

### 5.2 Android (NativeActivity, `android-activity 0.6.1`)

- **Mechanism inversion**: the entry point and lifecycle are
  system-driven. The host integrates with one line:
  ```rust
  starfish_we::android_main!(my_app_main);   // or a hand-written android_main + android_init
  ```
  After that, the library translates `AndroidApp::poll_events(timeout=0)`
  + `input_events_iter()` callback streams **into a buffer queue** drained
  by `poll_event` — pure poll for the host.
- **Thread**: every call must happen on the `android_main` thread
  (android-activity asserts this in `poll_events`).
- **Lifecycle mapping**: `InitWindow→Resized+Resumed+Redraw`,
  `TerminateWindow→Suspended`, `Pause/Stop→Suspended`,
  `Destroy→CloseRequested`, `Back key→CloseRequested`.
- **Process reuse (three fixed pitfalls — do not regress)**:
  1. "Back" only destroys the Activity; **the process is cached**. On the
     next launch `android_main` runs again in the same process, on a new
     thread → `android_init` must update the app by overwrite (a
     `OnceLock` once silently ignored the second wiring, so every call
     landed on the previous run's destroyed app — a crash on relaunch via
     android-activity's thread assertion), and reset the event buffer /
     readiness flag / single-window slot.
  2. The old instance's late `Destroy` can cross-talk into the new
     instance's event stream → this used to cause "instant exit on
     relaunch" → now dropped by the `WINDOW_READY` gate (a Destroy before
     InitWindow is untrustworthy).
  3. **The glue's statics do not support same-process
     "destroy-mid-rebuild"** (unfixed upstream in 0.6.1): during
     old/new overlap its UI-thread callbacks abort.
     **Host exit contract: terminate the process immediately upon
     `CloseRequested` (or `Suspended`)** (the demo uses
     `libc::raise(SIGKILL)`; `exit(0)` leaves RenderThread SIGABRT exit
     noise). For an interpreter host (engine = process) the semantics are
     exactly equivalent.
- **Back-key trap**: the back KeyDown may be swallowed by the system (you
  only see `Suspended`) — never rely solely on the key event for exit
  decisions.
- **TextInput**: comes from the system IME's committed text
  (`InputEvent::TextEvent`).
- Packaging: the built-in demo host is the `android-demo` feature
  (`src/platform/android/entry.rs`). Building needs `ANDROID_NDK_ROOT`:
  `cargo apk build --features android-demo` (root package; APK metadata
  lives in the root Cargo.toml).

### 5.3 Web (wasm32-unknown-unknown, `wasm-bindgen` + `web-sys`)

- **Mechanism inversion**: the DOM is callback-driven. Listeners buffer
  events into a queue; `poll_event` drains it; the main loop's pacing is
  driven by the host (interpreter tick with `setTimeout` /
  `requestAnimationFrame`).
- **The window entity is a `<canvas>`**: looked up by id
  `starfish-we`, else the page's first canvas, else created filling
  the page. Rendering attaches via `window.canvas()` → wgpu's Web backend
  (needs a WebGPU-capable browser, Chrome/Edge 113+).
- Keys follow the W3C `KeyboardEvent.code` — same naming origin as
  `KeyCode`, a direct mapping.
- Coordinates = CSS coordinates × `devicePixelRatio` (physical pixels,
  same convention as desktop); on resize the library syncs the
  canvas.width/height attributes to the physical size.
- The `requestAnimationFrame` loop only sets a redraw flag (coalesced);
  `poll_event` yields at most one `RedrawRequested` per frame — treat it
  as the frame tick.
- **There is no `CloseRequested`**: closing the page ends everything.
  Switching tabs → `Suspended`/`Resumed`.
- Known edges: IME composition is not reported (final text only);
  surrogate-pair characters are dropped in v1; the wheel listener does
  not `preventDefault` (fine as long as the page itself doesn't scroll).

### 5.4 macOS (AppKit, `objc2` family)

- **Mechanism**: `poll_event` pumps available NSEvents non-blockingly
  with `nextEventMatchingMask` — input events (keys/text/mouse) are
  translated straight into the buffer; window-level events are
  `sendEvent`-dispatched to AppKit so the window delegate fires
  (close→`CloseRequested`, resize→`Resized`, focus,
  **miniaturize→`Suspended` / deminiaturize→`Resumed`**), then one event
  is popped. AppKit is main-thread only (enforced by a stored
  `MainThreadMarker`).
- Key codes are the Carbon virtual keycodes carried by `NSEvent keyCode`,
  mapped to `KeyCode` by the full table in `translate.rs`; `TextInput`
  comes from the NSEvent characters (no IME composition, same v1
  convention as Windows).
- **There is no `RedrawRequested`**: not wired on macOS; the frame
  cadence is the host tick (interpreter-driven), decoupled from
  AppKit's drawRect machinery.
- The app is activated at creation (otherwise a background process
  never gets keyboard focus).
- Verification: CI runs a WindowServer probe on GitHub's real Mac
  runners (real NSApplication + NSWindow creation + injected-event
  roundtrip) plus the full test suite; **the wgpu surface path is
  untested** (`HasWindowHandle` is platform-independent, expected to
  pass through).

### 5.5 iOS (UIKit, `objc2` family)

- **Entry (statement-form macro)** — one line inside `fn main`:
  ```rust
  fn main() {
      starfish_we::ios_main!(my_tick);   // hands the main thread to UIApplicationMain; never returns
  }
  fn my_tick(window: &starfish_we::Window) -> Option<i32> {
      // called each frame via CADisplayLink on the main thread: poll + render
      None // Some(code) exits the app with that status
  }
  ```
  The difference from `android_main!` is deliberate: the Android glue
  ABI requires a top-level `android_main` function; iOS has no such
  contract, so **a statement is the right shape** (a macro that expands
  to a `fn main` definition silently becomes a dead nested function
  when invoked inside another body — learned the hard way: type checks
  all green, zero runtime activity).
- **Mechanism inversion** (same device as Android): the delegate's
  `didFinishLaunching` creates the window (full-screen UIWindow + root
  view controller) and installs a CADisplayLink; each frame drives the
  host tick; `poll_event` drains the buffer queue with a zero-timeout
  runloop pump. On iOS the library calls `Window::new` internally —
  hosts do not call it themselves.
- **Bootstrap pitfalls (fixed in code, do not regress)**: ① objc2
  classes register lazily and the default runtime name embeds the
  module path + version — `UIApplicationMain` resolves the delegate by
  NAME, so force-register via `ClassType::class()` first and pin the
  name with `#[name]`; ② at the end of application launch the key
  window must have a `rootViewController` or UIKit raises
  `NSInternalInconsistencyException`.
- Coordinates = bounds × scale (physical pixels, same convention);
  iOS windows are inherently full-screen, borderless and non-resizable
  (platform facts; capability methods are fixed-value no-ops).
- **v0 boundaries**: events flow only through the injection channel
  (`ios_inject_test_event`, deterministic testing); real touch delivery
  (needs a responder subclass), lifecycle `Suspended`/`Resumed`, and
  keyboard/TextInput are not wired yet.
- Verification: CI runs the smoke inside an iPhone simulator
  (`simctl install+launch`), judged by the result-file protocol
  `STARTED→FRAME0→OK/FAIL/PANIC` (a panic hook covers the
  UIApplicationMain bootstrap; ObjC exceptions surface via
  `--stderr=`) — the injected-event roundtrip passes; real device
  pending.

### 5.6 Linux (**Wayland-only**, `smithay-client-toolkit` 0.21 + `wayland-client` 0.31)

- **X11 is dropped by design decision**: X11 sessions are not supported
  (under XWayland this is still a native Wayland client). Deps use the
  `system`+`dlopen` backends: real `wl_display*`/`wl_surface*` pointers
  for wgpu (the pure-Rust backend has no display pointer), libwayland
  loaded at runtime with no link-time dependency. Building needs
  `pkg-config` + `libxkbcommon-dev` (xkbcommon is a C link dep — the
  Linux target therefore can no longer be cross-checked from a Windows
  host; Linux compile verification lives in `linux.yml`).
- **sctk 0.21 wiring**: 0.21 merged all per-module `delegate_*!` macros
  into a single `delegate_dispatch2!` — the wiring is `delegate_registry!`
  + `impl ProvidesRegistryState` (`registry_handlers![OutputState,
  SeatState]`) + `delegate_dispatch2!`.
- **A window maps via an initial EMPTY commit**: the xdg toplevel commits
  with no buffer, the compositor replies with the first configure, then
  the shm backdrop is drawn (SlotPool, double-buffered) and presented —
  from then on the window is visible and `size()` is known. The init
  `roundtrip` is the library's only blocking point (same tier as Windows
  pumping messages in `new`).
- **Non-blocking six-step pump**: `flush` → `dispatch_pending` →
  `prepare_read`+`read` (WouldBlock = idle, digested to Ok(0) inside
  wayland-client) → `dispatch_pending` → client-side key-repeat synthesis
  → pop. **A lost connection / protocol error = exactly one
  `CloseRequested`** (hosts exit cleanly; same philosophy as the Android
  exit contract).
- **Keyboard**: sctk hands over RAW evdev codes (`translate.rs` maps them
  to W3C `KeyCode`; the +8 xkb offset is internal to sctk) and
  xkb-composed UTF-8 (compose included); compositors don't repeat keys —
  repeats are synthesized client-side from `RepeatInfo` (modifiers don't
  repeat).
- **Coordinates**: configure sizes are logical; physical = logical ×
  integer scale (fractional scaling ignored in v1); wheel uses value120
  first, discrete as fallback, vertical negated (wl positive = down, our
  convention positive = up).
- **Capability approximations** (documented): title / fullscreen /
  maximize / minimize real; resizable = the min=max-size idiom (xdg has
  no resizable request); borderless = xdg-decoration request + sctk-adwaita CSD (Mutter lacks
  xdg-decoration; the Adwaita frame draws titlebar/close/drag/resize
  edges, and yields to KWin-style server decorations);
  position / opacity / always-on-top / mouse grab / relative mouse /
  cursor hide = no-op (their protocols are follow-ups); `set_size` is
  advisory under tiling compositors.
- **Verification**: CI runs `wayland_smoke` under headless Weston
  (connect → configure → shm commit → event drain → single-window guard
  → drop/recreate); interactive input (typing / wheel / close) was
  verified on a real Wayland session (VMware Ubuntu VM, 2026-10-11).

---

## 6. Red lines (violating any of these is a bug)

1. All `Window` methods must be called on the creating thread (Win32
   thread affinity / AppKit main thread / android_main thread). `Sync`
   exists only to satisfy renderer bounds such as wgpu's
   `create_surface`; calling methods cross-thread violates the contract.
2. At most one `Window` per process; on Android the slot is counted per
   "run" (`android_init` resets it automatically).
3. Sizes/coordinates are physical pixels — feed them straight into
   `SurfaceConfiguration`.
4. Android: never use the surface outside `Resumed` (`raw_handle` reports
   unavailable); the render loop must handle `Suspended` (destroy the
   surface) / `Resumed` (rebuild).
5. Android: exit by terminating the process (see 5.2); do not expect the
   process to be reusable after returning from `android_main`.
6. Never pump the platform message loop yourself.
7. The event enum is `#[non_exhaustive]`: always leave a `_` arm in your
   matches.
8. Linux: all methods on the creating thread; a lost compositor
   connection / protocol error surfaces as **exactly one
   `CloseRequested`** — exit your loop on it and don't rely on further
   events.

---

## 7. Diagnostics quick reference

| Platform | What to look at | Command |
|---|---|---|
| Windows | window + events demo | `cargo run --example basic` |
| Windows | wgpu integration regression | `cargo test --features wgpu --test wgpu_integration` |
| Windows | full smoke suite | `cargo test` |
| macOS | CI smoke (real Mac runner: probe + full suite) | after push, `gh run watch` (the `macos` job of `.github/workflows/macos.yml`) |
| iOS | CI simulator smoke (window + injected roundtrip) | the `ios-sim` job of the same workflow; on failure read the result.txt terminal state + the `sim_stderr.txt` artifact |
| Linux | CI compile matrix + a real Wayland smoke under headless Weston | after push, `gh run watch` (the `linux` job of `.github/workflows/linux.yml`) |
| Linux | manual acceptance on a Wayland session | `cargo run --example basic`; if it stalls, `WAYLAND_DEBUG=1` traces the whole wire protocol |
| Android | event / lifecycle logs | `adb logcat -s starfish`; panics additionally under `RustStdoutStderr` |
| Android | build + install | `export ANDROID_NDK_ROOT=<ndk> && cargo apk build --features android-demo && adb install -r target/debug/apk/starfish-demo.apk` |
| Web | page overlay + console | `python -m http.server 8000 -d testweb` → http://localhost:8000 (`scripts/build_web.sh` repackages) |
| All platforms | compile matrix | `cargo check --target <t> --features wgpu`; **exception**: the Linux target cannot be cross-checked from a Windows host (xkbcommon is a C link dep) — its compile verification lives in linux.yml |

Incident methodology (from "android绑定运行问题与解决方案.md",
universally applicable): reproduce first and harvest the crash output
(logcat / tombstones); the crash site ≠ the root cause; a green platform
compile matrix ≠ the platform runs (pure runtime differences like TLS
keys or process reuse are invisible to check); anything the system hands
over asynchronously (surfaces, canvases, adapters) should only be taken
after the platform's own readiness signal.

---

## 8. Current status (2026-10-11 — read before integrating)

| Platform | Implementation | Verification level |
|---|---|---|
| Windows | ~95% (missing: IME composition, logical-pixel DPI API, fullscreen/icon/position, runtime resizable) | real-hardware tested + automated tests |
| macOS | ~90% (missing: RedrawRequested wiring, IME composition, fullscreen/icon/position, runtime resizable) | CI real-WindowServer probe + full test suite ✅; wgpu path untested |
| iOS | ~60% (v0: window + frame driver + injection channel; missing: real touch, lifecycle, keyboard/TextInput, real-device verification) | CI simulator smoke ✅ (injected-event roundtrip); real device pending |
| Android | ~90% (missing: GameActivity, gesture nav, soft-input control API, saveInstanceState) | emulator-tested (4 rapid relaunch cycles, 0 panics); real device pending |
| Web | ~85% (missing: IME, rwh Web registry, wheel preventDefault) | event layer manually verified ✅; the wgpu-WebGPU path not yet exercised |
| Linux | ~85% (Wayland-only; missing: touch, pointer constraints/relative, fractional scaling, IME preedit, xdg-activation) | real Wayland session verified ✅ (interactive input / close / CSD decorations end-to-end, 2026-10-11 VMware Ubuntu) + CI headless-Weston smoke ✅ |

Version facts: raw-window-handle **0.6** (`#[non_exhaustive]`; use the
`new()` constructors; `WindowsDisplayHandle` / `AndroidDisplayHandle` are
empty structs; the Android variant is named `AndroidNdk`); compatible
wgpu version **30** (`get_current_texture` returns the enum
`CurrentSurfaceTexture`, not a Result; `SurfaceConfiguration` gained the
required `color_space` field); wasm-bindgen pinned at **0.2.127** (must
match wasm-bindgen-cli exactly); Linux = sctk **0.21.1** (single-macro
`delegate_dispatch2!` wiring) + wayland-client **0.31** (`system`+`dlopen`,
required for wgpu interop) + xkbcommon **0.8** (C link dep: pkg-config
probes it at build time, so the Linux target can't be cross-checked from
a Windows host).
