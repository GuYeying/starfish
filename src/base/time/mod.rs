//! 时间模块：度量 / 缩放 / 固定步长 / 节流——四个正交原语
//!
//! 时间源直接基于 **`std::time::Instant`**（OS 高精度单调钟：Windows QPC /
//! 类 Unix CLOCK_MONOTONIC），不依赖任何平台库——时间模块因此对窗口/音频
//! 后端完全中立，随渲染层同进退。
//!
//! - 高精度读数：`Instant`，不受毫秒粒度限制
//! - 节流睡眠：`thread::sleep`（粗粒度）+ 末段自旋补齐的混合策略，
//!   规避 OS 定时器粒度超调
//! - 时间原点：进程内首次调用 [`now()`] 的时刻（懒初始化固定原点），
//!   无需任何先行初始化；`wasm32-unknown-unknown` 上无阻塞睡眠，
//!   [`sleep_until`] 为 no-op（节流权归浏览器 rAF）
//!
//! 与 pygame.time 的关系：本模块是底层原语，**不**对齐其毫秒语义与
//! 事件定时器（`add_timer` 对应物属事件系统范畴），那些留给 `pygame/` 兼容层。

use std::time::Duration;

/// 高精度单调时钟读数（自进程内固定原点起的时长）
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
fn now() -> Duration {
    // Web（unknown-unknown）：std Instant 不可用，走 performance.now()
    //（毫秒 f64，页面相对时间，单调性满足帧计量需求）
    let ms = web_sys::window()
        .expect("Web 环境无全局 window")
        .performance()
        .expect("Web 环境无 performance")
        .now();
    Duration::from_secs_f64(ms / 1000.0)
}

/// 高精度单调时钟读数（自进程内固定原点起的时长）
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
fn now() -> Duration {
    use std::sync::OnceLock;
    use std::time::Instant;
    // 懒初始化固定原点：保证返回值非负、单调，且与首次调用时机解耦
    static ORIGIN: OnceLock<Instant> = OnceLock::new();
    ORIGIN.get_or_init(Instant::now).elapsed()
}

/// 帧时钟：度量与缩放
///
/// ```ignore
/// let mut clock = Clock::new();
/// let mut fixed = FixedTimestep::new(60.0);
/// loop {
///     clock.frame();                              // 每帧恰好一次
///     for step in fixed.update(clock.delta()) {   // 确定性玩法步
///         physics(step);
///     }
///     render(clock.raw_delta());                  // 渲染/相机用真实时间
///     sleep_until(clock.next_frame_deadline(60.0)); // 可选节流
/// }
/// ```
pub struct Clock {
    start: Duration,
    last: Duration,
    raw_delta: Duration,
    fps: f32,
    scale: f32,
    frames: u64,
}

impl Clock {
    pub fn new() -> Self {
        let now = now();
        Self {
            start: now,
            last: now,
            raw_delta: Duration::ZERO,
            fps: 0.0,
            scale: 1.0,
            frames: 0,
        }
    }

    /// 推进一帧。**每帧必须调用恰好一次**，之后所有查询反映本帧状态
    pub fn frame(&mut self) {
        let now = now();
        self.raw_delta = now.saturating_sub(self.last);
        self.last = now;
        self.frames += 1;

        let secs = self.raw_delta.as_secs_f32();
        let fps = if secs > 0.0 { 1.0 / secs } else { 0.0 };
        // EMA 平滑（新值权重 0.1）
        self.fps = if self.fps == 0.0 { fps } else { self.fps * 0.9 + fps * 0.1 };
    }

    // ── 度量 ──

    /// 启动以来的真实总时长（Duration，f64 级精度——累计计时的正典来源）
    pub fn elapsed(&self) -> Duration {
        now().saturating_sub(self.start)
    }

    /// 启动以来的总秒数（f64 便利版）
    pub fn total_f64(&self) -> f64 {
        self.elapsed().as_secs_f64()
    }

    /// 本帧真实间隔（秒，未缩放）——UI / 网络 / 相机平滑用
    pub fn raw_delta(&self) -> f32 {
        self.raw_delta.as_secs_f32()
    }

    /// 本帧缩放后间隔（秒）——游戏逻辑用；暂停时为 0
    pub fn delta(&self) -> f32 {
        self.raw_delta.as_secs_f32() * self.scale
    }

    /// EMA 平滑帧率
    pub fn fps(&self) -> f32 {
        self.fps
    }

    /// 已推进的帧数
    pub fn frame_count(&self) -> u64 {
        self.frames
    }

    // ── 时间缩放 ──

    /// 设置时间缩放：1.0 正常 / 0.0 暂停 / 0.5 慢动作（负值钳为 0）
    ///
    /// 只影响 [`delta()`](Self::delta)；[`raw_delta()`](Self::raw_delta)
    /// 与 [`elapsed()`](Self::elapsed) 永远走真实时间。
    pub fn set_scale(&mut self, scale: f32) {
        self.scale = scale.max(0.0);
    }

    /// 当前时间缩放
    pub fn scale(&self) -> f32 {
        self.scale
    }

    // ── 便利层 ──

    /// 便利组合：推进一帧 + 节流到目标帧率，返回本帧真实间隔（秒）
    ///
    /// `target_fps` 传 0 表示不限帧。需要缩放时间请改用
    /// `frame()` + [`delta()`](Self::delta)。
    ///
    /// # 节流叠加警告
    ///
    /// wgpu 的 Fifo 呈现模式自带渲染回压（`get_current_texture` 阻塞在刷新率上）。
    /// **不要把两者的目标叠在同一档**：60Hz 屏 + Fifo 下再 `tick(60)`，
    /// 睡眠与回压各等一个 vsync，实际 ~30fps 且平白加延迟。
    /// 组合建议：Fifo 下跟随刷新率用 `tick(0)`；只在「低于刷新率」或
    /// `Immediate`/`Mailbox`（回压不生效）时才用非零目标节流。
    pub fn tick(&mut self, target_fps: u32) -> f32 {
        if target_fps > 0 {
            let min_frame = Duration::from_secs_f32(1.0 / target_fps as f32);
            sleep_until(self.last + min_frame);
        }
        self.frame();
        self.raw_delta()
    }
}

impl Default for Clock {
    fn default() -> Self {
        Self::new()
    }
}

/// 帧节流原语：睡眠至目标时刻（本模块时间轴；已过则立即返回）
///
/// 混合策略：剩余 >2ms 时走 `thread::sleep`（主动少睡 1ms），末段自旋补齐——
/// 规避 OS 定时器粒度（Windows ~15.6ms）造成的帧率超调，精度可达亚毫秒。
/// 代价是每帧最多 ~2ms 的自旋占用，可接受即用。
///
/// `wasm32-unknown-unknown` 上为 no-op：Web 无阻塞睡眠，帧节奏由浏览器
/// rAF 驱动（见 Step 4 的 web 主循环），`deadline` 仅作标记。
pub fn sleep_until(deadline: Duration) {
    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    {
        let _ = deadline; // 浏览器 rAF 负责节流，不阻塞主线程
    }
    #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
    loop {
        let now = now();
        if now >= deadline {
            return;
        }
        let remaining = deadline - now;
        if remaining > Duration::from_millis(2) {
            std::thread::sleep(remaining - Duration::from_millis(1));
        } else {
            std::hint::spin_loop();
        }
    }
}

/// 固定步长累加器：确定性玩法更新（物理 / 回放 / 联机模拟）
///
/// 可变 `delta` 用于渲染插值，固定 `step` 用于模拟——两者分离是
/// 实时引擎的标准做法。
///
/// ```ignore
/// let mut fixed = FixedTimestep::new(60.0);
/// loop {
///     clock.frame();
///     for step in fixed.update(clock.delta()) {
///         physics(step);        // step 恒为 1/60，与帧率无关
///     }
///     render();
/// }
/// ```
pub struct FixedTimestep {
    step: f32,
    accumulator: f32,
    max_steps: u32,
}

impl FixedTimestep {
    /// 以固定频率创建（Hz，非正值回落 60Hz）
    pub fn new(hz: f32) -> Self {
        let hz = if hz > 0.0 { hz } else { 60.0 };
        Self {
            step: 1.0 / hz,
            accumulator: 0.0,
            max_steps: 5,
        }
    }

    /// 死循环保护：单帧最多补的步数（默认 5；超出部分丢弃）
    pub fn with_max_steps(mut self, max: u32) -> Self {
        self.max_steps = max.max(1);
        self
    }

    /// 固定步长（秒）
    pub fn step(&self) -> f32 {
        self.step
    }

    /// 消耗本帧 delta，返回本帧应执行的固定步序列（0..N 个，每个恒为 `step`）
    ///
    /// 帧耗时超过 `max_steps × step` 时，多余时间被丢弃（防死亡螺旋）。
    pub fn update(&mut self, delta: f32) -> impl Iterator<Item = f32> + '_ {
        self.accumulator += delta.max(0.0);
        let steps = ((self.accumulator / self.step).floor() as u32).min(self.max_steps);
        self.accumulator -= steps as f32 * self.step;
        (0..steps).map(move |_| self.step)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_measures_raw_delta() {
        let mut clock = Clock::new();
        clock.frame();
        thread_sleep(4);
        clock.frame();
        assert!(clock.raw_delta() >= 0.003, "raw = {}", clock.raw_delta());
        assert!((clock.delta() - clock.raw_delta()).abs() < 1e-6, "scale=1 时两者一致");
        assert_eq!(clock.frame_count(), 2);
    }

    #[test]
    fn elapsed_grows_f64_precision() {
        let clock = Clock::new();
        thread_sleep(3);
        assert!(clock.elapsed() >= Duration::from_millis(3));
        assert!(clock.total_f64() > 0.0);
    }

    #[test]
    fn scale_zero_freezes_logic_time() {
        let mut clock = Clock::new();
        clock.frame();
        thread_sleep(4);
        clock.set_scale(0.0);
        clock.frame();
        assert_eq!(clock.delta(), 0.0, "暂停时逻辑时间为 0");
        assert!(clock.raw_delta() > 0.0, "真实时间照常流动");
    }

    #[test]
    fn scale_half_halves_delta() {
        let mut clock = Clock::new();
        clock.frame();
        thread_sleep(20);
        clock.set_scale(0.5);
        clock.frame();
        let raw = clock.raw_delta();
        assert!((clock.delta() - raw * 0.5).abs() < 1e-6);
    }

    #[test]
    fn tick_paces_to_target() {
        let mut clock = Clock::new();
        clock.tick(0);
        let start = now();
        let dt = clock.tick(30); // 33ms > OS 定时器粒度，结果稳定
        assert!((now() - start).as_secs_f32() >= 0.03 * 0.8);
        assert!(dt >= 0.03 * 0.8, "dt = {dt}");
    }

    #[test]
    fn sleep_until_past_is_immediate() {
        let start = now();
        sleep_until(start);
        assert!(now() - start < Duration::from_millis(5));
    }

    fn thread_sleep(ms: u64) {
        std::thread::sleep(Duration::from_millis(ms));
    }
}
