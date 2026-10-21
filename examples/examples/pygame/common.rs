//! pygame 模块运行测试共用框架（批次二十五）。
//!
//! 约定：每案例 = 纯逻辑/运行自检（`Checks::require` 登记，**晚登记合法**
//! ——时序类检查可在指定帧登记）+ 模块场景帧；状态呈现 = 顶部结果横幅
//! + 每自检项一格色块（绿=过/红=败，顺序=登记顺序，名称见各案例文件头
//! 注释）；240 帧后自动退出（Web 驻留不停让出）。
//!
//! 文本呈现依赖字体资产（无内嵌默认字体）——状态一律色块化，case 名称
//! 见各案例源码头注释。

#[cfg(target_arch = "wasm32")]
use starfish_we::next_frame;
use starfish_pygame::display;
use starfish_pygame::render::DrawTarget;

/// 自动退出的帧数（原生 60fps 节流 ≈ 10s 展示；Web 由 rAF 节流 ≈ 同量级）
pub const EXIT_FRAMES: u32 = 600;

/// 跨平台节流帧：原生 = follow-vsync（`Clock::tick(0)`，永不自睡——
/// 帧率 = 刷新率，由 flip 的 Fifo 回压节流；此前裸 sleep(16ms) 与
/// vsync 串联，Android 上 ~30fps）；Web = rAF 帧拍（浏览器节流）。
/// 每帧调用一次。
pub async fn paced_frame() {
    #[cfg(not(target_arch = "wasm32"))]
    PACE_CLOCK.with(|c| c.borrow_mut().tick(0));
    #[cfg(target_arch = "wasm32")]
    next_frame().await;
}

thread_local! {
    #[cfg(not(target_arch = "wasm32"))]
    static PACE_CLOCK: std::cell::RefCell<starfish_time::Clock> =
        std::cell::RefCell::new(starfish_time::Clock::new());
}

/// 诊断打点（println + console_log——三端可见，wasm 上即 F12 console）
pub fn log(msg: &str) {
    starfish_std::debug::console_log(msg);
    println!("{msg}");
}

/// 事件泵 + 退出判定（每帧一次：窗口 × / ESC 退出——pygame 惯例）。
/// 返回 true = 应用应退出。注意：本函数排空事件队列，调用方不要再
/// 自行 `event::get()`（会拿不到剩余事件——需要事件统计的案例自带泵）。
pub fn quit_requested() -> bool {
    use starfish_pygame::Event;
    for e in starfish_pygame::event::get() {
        match e {
            Event::Quit => return true,
            Event::KeyDown { key: starfish_we::KeyCode::Escape, .. } => return true,
            _ => {}
        }
    }
    false
}

/// 自检登记表（模块名 + 逐项 (名称, 是否通过)）
pub struct Checks {
    module: &'static str,
    items: Vec<(&'static str, bool)>,
}

impl Checks {
    pub fn new(module: &'static str) -> Self {
        starfish_std::debug::console_log(&format!("[{}] checks init", module));
        Self { module, items: Vec::new() }
    }

    /// 登记一项自检（晚登记合法——时序类检查在指定帧登记）
    pub fn require(&mut self, name: &'static str, ok: bool) {
        self.items.push((name, ok));
    }

    pub fn all_ok(&self) -> bool {
        self.items.iter().all(|(_, ok)| *ok)
    }

    /// (通过数, 总数)
    pub fn summary(&self) -> (usize, usize) {
        let total = self.items.len();
        let passed = self.items.iter().filter(|(_, ok)| *ok).count();
        (passed, total)
    }
}

/// 状态呈现：顶部结果横幅（全过=绿 / 有败=红）+ 每自检项一格色块。
/// 目标 = 任何 DrawTarget（Screen / `render()` 会话均可——会话内调用时
/// 状态条与场景同帧同批）。
pub fn draw_status<T: DrawTarget>(target: &T, checks: &Checks) {
    let (_, total) = checks.summary();
    let header = if total > 0 && checks.all_ok() {
        [0.0000, 0.8627, 0.3529, 1.0000]
    } else {
        [0.9020, 0.2353, 0.2353, 1.0000]
    };
    target.with_batch(|b| {
        b.push_quad(40.0, 24.0, 720.0, 26.0, header, None);
        let n = total.max(1) as f32;
        for (i, (_, ok)) in checks.items.iter().enumerate() {
            let c = if *ok {
                [0.0000, 0.8627, 0.3529, 1.0000]
            } else {
                [0.9020, 0.2353, 0.2353, 1.0000]
            };
            let x = 40.0 + i as f32 * (720.0 / n);
            b.push_quad(x, 62.0, 720.0 / n - 6.0, 14.0, c, None);
        }
    });
}

/// Web 驻留（应用 future 不得完成——防迟到微任务踩已 drop 的绑定闭包）；
/// 原生平台空实现（完成即退出，自动化友好）。
pub async fn reside() {
    #[cfg(target_arch = "wasm32")]
    loop {
        next_frame().await;
    }
}
