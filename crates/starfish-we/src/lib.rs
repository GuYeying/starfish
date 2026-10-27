//! starfish-we: a cross-platform **window + events** crate (SDL-style poll
//! model). Single-window by default; the `multi-window` feature lifts the
//! invariant on whitelisted platforms (compile-time gate, see below).
//! starfish-weï¼è·¨å¹³å°**çªå£ + äºä»¶** crateï¼SDL é£æ ¼ poll æ¨¡åï¼ã
//! é»è®¤åçªå£ï¼`multi-window` feature å¨ç½ååå¹³å°è§£é¤åçªçº¦æ
//! ï¼ç¼è¯ææå³ï¼è§ä¸ï¼ã
//!
//! Built to replace winit where the inversion of control hurts: the caller
//! (e.g. a scripting-language interpreter) owns the main loop and pulls
//! events, instead of handing control flow to event-loop callbacks.
//! ç¨äºæ¿æ¢ winitï¼è°ç¨èï¼èæ¬è¯­è¨è§£éå¨ï¼æ¥æä¸»å¾ªç¯ï¼ä¸»å¨è½®è¯¢äºä»¶ï¼
//! èä¸æ¯ææ§å¶æµäº¤ç»äºä»¶å¾ªç¯çåè°ã
//!
//! ```ignore
//! use crate::{Event, Window, WindowFlags};
//!
//! let mut window = Window::new("demo", (800, 600), WindowFlags::default())?;
//! loop {
//!     while let Some(event) = window.poll_event() {
//!         if let Event::CloseRequested = event {
//!             return Ok(());
//!         }
//!         // ... handle the rest / å¤çå¶ä½äºä»¶
//!     }
//!     // The interpreter advances its own tick here (bytecode / scheduling).
//!     // è§£éå¨å¨è¿éæ¨è¿èªå·±ç tickï¼æ§è¡å­èç  / è°åº¦ï¼
//! }
//! ```
//!
//! # Layering and features / åå±ä¸ feature
//!
//! - **Base layer (default)**: window + events with zero third-party
//!   dependencies; [`Window::raw_handle`] exposes raw platform handles so
//!   any rendering backend can be bridged via unsafe.
//!   **åºç¡å±ï¼é»è®¤ï¼**ï¼çªå£ + äºä»¶ï¼é¶ç¬¬ä¸æ¹ä¾èµï¼[`Window::raw_handle`]
//!   æ´é²è£¸å¹³å°å¥æï¼å¯ç¨ unsafe æ¡¥æ¥ä»»ä½æ¸²æåç«¯ã
//! - **`wgpu` feature**: implements the raw-window-handle traits so
//!   `instance.create_surface(&window)` works with zero unsafe; wgpu itself
//!   is deliberately not a dependency.
//!   **`wgpu` feature**ï¼å®ç° raw-window-handle traitï¼
//!   `instance.create_surface(&window)` é¶ unsafe ç´éï¼ä¸å¼å¥ wgpu æ¬ä½ã
//!
//! | feature | meaning / å«ä¹ |
//! |---|---|
//! | `wgpu` | raw-window-handle interop / raw-window-handle å¯¹æ¥ |
//! | `android-demo` | built-in Android demo host / åç½® Android æ¼ç¤ºå®¿ä¸» |
//!
//! Architecture: each target platform is one parallel module under
//! [`platform`] (horizontal cfg); all implement the same `WindowInner`
//! interface while the public API stays platform-independent.
//! æ¶æï¼[`platform`] ä¸åç®æ å¹³å°ä¸ä¸ªå¹³è¡æ¨¡åï¼æ¨ªå cfgï¼ï¼
//! åèªå®ç°åä¸å¥ `WindowInner` æ¥å£ï¼å¬å± API å¹³å°æ å³ã
//!
//! # Multi-window / å¤çªå£ï¼feature `multi-window`ï¼
//!
//! Opens the single-window invariant: any number of `Window`s, each with
//! its own event queue. The event topology differs from SDL on purpose â
//! SDL keeps one global queue + `windowID` routing; here an event belongs
//! to the `Window` it is polled from (no ID indirection, type-safe).
//! Pump discipline: poll **every** window every frame â an un-polled
//! window's events stay queued in its own pending buffer (never lost,
//! but its UI stalls). Keyboard goes to the focused window (per-platform
//! focus model, same as SDL).
//! è§£é¤åçªçº¦æï¼ä»»æä¸ª `Window`ï¼æ¯çªç¬ç«äºä»¶éåãäºä»¶ææææåºå«äº
//! SDLââSDL åä¸å¨å±éå + `windowID` è·¯ç±ï¼æ¬åºäºä»¶å½å±**è¢« poll ç
//! `Window`**ï¼æ  ID åæ¥ï¼ç±»åå®å¨ï¼ãæ³µçºªå¾ï¼**æ¯å¸§è½®è¯¢æ¯ä¸ªçªå£**
//! ï¼æ¼ poll ççªå£äºä»¶ç§¯åå¨èªå·± pending éââä¸ä¸¢å¤±ï¼ä½è¯¥çªçé¢åæï¼ã
//! é®çè¿ç¦ç¹çªï¼åå¹³å°ç¦ç¹æ¨¡åï¼SDL åæ¬¾ï¼ã
//!
//! ```ignore
//! for w in &windows {
//!     while let Some(event) = w.poll_event() { /* å½å± = w */ }
//! }
//! ```
//!
//! Platform support is enforced at compile time: behavioural differences
//! are too large for a silent runtime degradation (single-Activity /
//! single-canvas semantics), so an unsupported target fails the build via
//! `compile_error!` below. Whitelist today: **Windows** (the Win32 message
//! pump is naturally per-window). Light up Linux/macOS by extending the
//! whitelist after per-platform verification.
//! å¹³å°æ¯æç¼è¯ææå³ï¼è¡ä¸ºå·®å¼è¿å¤§ï¼å Activity / å canvas è¯­ä¹ï¼ï¼
//! ä¸åè¿è¡æéé»éçº§ââç½ååå¤ç´æ¥ç¼è¯å¤±è´¥ï¼ä¸æ¹ `compile_error!`ï¼ã
//! å½åç½ååï¼**Windows**ï¼Win32 æ¶æ¯æ³µå¤©ç¶ per-windowï¼ã
//! Linux/macOS éå¹³å°å®æµéè¿åæ©ç½ååå³å¯ã

#![deny(missing_docs)]

// å¤çªå£å¹³å°ç½ååï¼ç¼è¯ææå³ï¼è¯­ä¹ä¸æ©å±æ¹å¼è§ä¸æ¹æ¨¡åææ¡£
// "Multi-window" èï¼ãæ°å¹³å°éä¸ªå®æµéè¿ååç½ååè¿½å  target_osã
#[cfg(all(feature = "multi-window", not(target_os = "windows")))]
compile_error!(
    "starfish-we: feature \"multi-window\" ä¸æ¯æå½åç®æ å¹³å°ï¼ç½ååï¼windowsï¼ã\
     å¤çªå£è¡ä¸ºå¹³å°å·®å¼è¿å¤§ï¼å¼æä¸åè¿è¡æéçº§ââè¯·å³é­è¯¥ feature ææ´æ¢ç®æ å¹³å°ã"
);

mod error;
mod event;
pub mod flags;
mod handle;
mod platform;

// 嵌入式宿主门面（2026-10-26）：外部 Surface 注入/事件注入/解除——嵌入式运行时（ pygame-rs-dev 的 Android 売）的小暴露面
#[cfg(target_os = "android")]
pub mod embedded;
// 平台后端（控不变）
#[cfg(feature = "we-wgpu")]
mod rwh_impl;
#[cfg(feature = "wgpu-auto")]
mod wgpu_auto;
mod window;

pub use error::WindowError;
pub use event::{Event, KeyCode, Modifiers, MouseButton, TouchPhase, UserEvent};
pub use flags::WindowFlags;
pub use handle::RawWindowHandle;
#[cfg(feature = "wgpu-auto")]
pub use wgpu_auto::{AutoSurface, AutoSurfaceError};
pub use window::Window;
pub use window::{EventProxy, EventProxyClosed};

/// (Android builds only) Call from the host's `android_main` to wire up
/// the system callbacks.
/// ï¼ä» Android æå»ºï¼å®¿ä¸»å¨ `android_main` ä¸­è°ç¨ï¼è£éç³»ç»åè°ã
///
/// Platform-specific API by explicit exception: it only exists on Android
/// builds.
/// å¹³å°ç¹æ API çæ¾å¼ä¾å¤ï¼åªå­å¨äº Android æå»ºä¸­ã
#[cfg(target_os = "android")]
pub use platform::android::android_init;

/// (Android builds only) The system app handle â the parameter type of
/// [`android_init`], needed by the host's `android_main` signature.
/// ï¼ä» Android æå»ºï¼ç³»ç»åºç¨å¥æââ[`android_init`] çåæ°ç±»åï¼
/// å®¿ä¸» `android_main` ç­¾åæéã
#[cfg(target_os = "android")]
pub use android_activity::AndroidApp;

/// (iOS builds only) Test-only event injection: feeds a synthetic event
/// into the same buffer the OS path uses. The OS delivery (touches /
/// keyboard) is implemented; this stays for deterministic event testing
/// on the simulator.
/// ï¼ä» iOS æå»ºï¼æµè¯ç¨äºä»¶æ³¨å¥ï¼å°åæäºä»¶åå¥ä¸ OS è·¯å¾ç¸åçç¼å²ã
/// OS æéï¼è§¦æ¸ / é®çï¼å·²å®ç°ï¼æ¬ééä¿çç¨äºæ¨¡æå¨ä¸çç¡®å®æ§äºä»¶
/// æµè¯ã
#[cfg(target_os = "ios")]
#[doc(hidden)]
pub use platform::ios::inject_test_event as ios_inject_test_event;

/// (iOS builds only) Host entry: install the tick and hand the main
/// thread to UIApplicationMain (never returns).
/// ï¼ä» iOS æå»ºï¼å®¿ä¸»å¥å£ï¼å®è£ tick å¹¶æä¸»çº¿ç¨äº¤ç» UIApplicationMain
/// ï¼æ­£å¸¸æåµä¸æ°¸ä¸è¿åï¼ã
#[cfg(target_os = "ios")]
pub use platform::ios::entry::ios_start;

/// (iOS builds only) Host entry macro â a statement form, called inside
/// `main`: hands the main thread to UIApplicationMain (never returns).
/// Unlike `android_main!` (which must define `android_main` at module
/// top level for the Android glue ABI), iOS has no such ABI contract â
/// the entry is a plain Rust `main`, so a statement is the right shape.
/// Defining a `fn main` here instead would silently become a dead nested
/// function when invoked inside another function body.
/// ï¼ä» iOS æå»ºï¼å®¿ä¸»å¥å£å®ââ**è¯­å¥å¼**ï¼å¨ `main` ä½åè°ç¨ï¼æä¸»
/// çº¿ç¨äº¤ç» UIApplicationMainï¼æ°¸ä¸è¿åï¼ãä¸ `android_main!` ä¸å
/// ï¼Android glue ç ABI è¦æ±å¨æ¨¡åé¡¶å±å®ä¹ `android_main`ï¼ï¼iOS æ²¡æ
/// è¿ç§ ABI çº¦å®ââå¥å£å°±æ¯æ®é Rust `main`ï¼è¯­å¥ææ¯æ­£ç¡®å½¢ç¶ãè¥å¨
/// æ­¤å®ä¹ `fn main`ï¼è¢«æ¾è¿å¦ä¸å½æ°ä½æ¶ä¼éé»åææ°¸ä¸æ§è¡çåµå¥å½æ°ã
///
/// ```ignore
/// fn main() {
///     crate::ios_main!(my_tick); // never returns / æ°¸ä¸è¿å
/// }
///
/// fn my_tick(window: &crate::Window) -> Option<i32> {
///     // Per-frame: poll events + render + interpreter step.
///     // æ¯å¸§ï¼poll äºä»¶ + æ¸²æ + è§£éå¨æ­¥è¿ã
///     None // Some(code) exits the app / Some(code) éåºåºç¨
/// }
/// ```
#[cfg(target_os = "ios")]
#[macro_export]
macro_rules! ios_main {
    ($tick:path) => {
        $crate::ios_start($tick)
    };
}

/// Wait for the next frame â the per-frame yield primitive of the poll
/// model. Call it unconditionally at the end of the application loop body.
///
/// ç­å¾ä¸ä¸å¸§ââpoll æ¨¡åç**éå¸§è®©åºåè¯­**ï¼å¨åºç¨å¾ªç¯ä½æ«å°¾æ æ¡ä»¶è°ç¨ã
///
/// - **Desktop / Android**: no-op async. The frame cadence is naturally
///   bounded by vsync'd present (or `Clock`-style throttling).
///   æ¡é¢/Androidï¼no-op asyncââå¸§èå¥ç± vsync presentï¼æ Clock ç±»èæµï¼
///   èªç¶æ¿æã
/// - **Web**: the `requestAnimationFrame` promise bridge. The browser
///   requires the loop to yield every frame â a tight `loop {}` without
///   this call freezes the page's renderer. **This primitive is what makes
///   the poll model complete on the web**; without it a consumer would have
///   to hand-write the wasm-bindgen rAF bridge (non-trivial, easy to get
///   wrong).
///   Webï¼`requestAnimationFrame` ç Promise æ¡¥ãæµè§å¨å¼ºå¶å¾ªç¯éå¸§è®©åº
///   ââæ²¡ææ¬è°ç¨ï¼ç´§å¯ `loop {}` ä¼å»ç»é¡µé¢æ¸²æè¿ç¨ã**æ¬åè¯­æ¯ poll
///   æ¨¡åå¨ Web ä¸æç«çä¸å**ï¼æ²¡æå®ï¼æ¶è´¹èå°±å¾æå wasm-bindgen ç
///   rAF æ¡¥ï¼éå¹³å¡ä¸æéï¼ã
///
/// ```ignore
/// let mut window = Window::new(...)?;
/// loop {
///     while let Some(event) = window.poll_event() { /* ... */ }
///     // draw ...
///     crate::next_frame().await;   // â unconditional, all platforms
/// }
/// ```
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub async fn next_frame() {}

/// Web implementation: the rAF promise bridgeï¼Web å®ç° = rAF ç Promise
/// æ¡¥ï¼è¯­ä¹ä¸æ³¨æäºé¡¹è§ä¸æ¹æ¡é¢å­ªçåæ¯çææ¡£ï¼ã
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
pub async fn next_frame() {
    let promise = js_sys::Promise::new(
        &mut |resolve: js_sys::Function, _reject: js_sys::Function| {
            web_sys::window()
                .expect("web: no global window / æ å¨å± window")
                .request_animation_frame(&resolve)
                .expect("requestAnimationFrame failed / rAF è°ç¨å¤±è´¥");
        },
    );
    let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
}