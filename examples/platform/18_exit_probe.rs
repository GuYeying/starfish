//! 18_exit_probe：退出/重进链路探针（**无渲染**——绕开模拟器 GLES 限制，
//! 专测 exit(0) → 进程死亡 → 二次启动 的完整链路）
//!
//! 行为：启动 3 秒后自动退出（ctx.exit）→ 引擎收进程。随后：
//! - 进程应死亡（pidof 为空）
//! - 再次 am start / 点图标：应正常二次启动
//! 若二次启动闪退，logcat 可见真实崩溃（模拟器有 logcat）。
//!
//! 运行：Android（任意 ABI，无需 GPU）：
//! `cargo xtask android 18_exit_probe --abi x86_64`
//! 桌面：`cargo run --example 18_exit_probe`

use starfish::base::app::{Application, Ctx, WindowConfig};
#[cfg(all(not(target_os = "android"), not(target_arch = "wasm32")))]
use starfish::base::app::run;
#[cfg(target_os = "android")]
use starfish::base::app::run_android;
use std::time::Instant;

struct ExitProbe {
    start: Option<Instant>,
}

impl Application for ExitProbe {
    fn frame(&mut self, ctx: &mut Ctx) {
        let t = *self.start.get_or_insert_with(Instant::now);
        if t.elapsed().as_secs_f32() >= 3.0 {
            println!("[18] 3s 到，自动退出（进程应收尾并死亡）");
            ctx.exit();
        }
    }
}

// ── Android 入口（cdylib）──
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
fn android_main(app: winit::platform::android::activity::AndroidApp) {
    unsafe { std::env::set_var("RUST_BACKTRACE", "1") };
    println!("[18] android_main 进入（进程 {}）", std::process::id());
    run_android(
        app,
        ExitProbe { start: None },
        WindowConfig::new("exit probe", 400, 300).with_fps_cap(30),
    );
}

// ── 桌面入口（bin）──
#[cfg(all(not(target_os = "android"), not(target_arch = "wasm32")))]
fn main() {
    run(ExitProbe { start: None }, WindowConfig::new("Exit Probe", 400, 300).with_fps_cap(30));
}
