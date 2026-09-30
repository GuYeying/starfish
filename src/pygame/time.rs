//! pygame.time 对应物（契约：Clock（tick/get_fps）/ delay）
//!
//! [`Clock`] = `base::time::Clock` 的 pygame 词汇薄包装（引擎时钟已内建
//! web 桥 / EMA 帧率 / 缩放）。

use crate::base::time as engine;

/// 帧时钟（pygame `time.Clock`）
#[derive(Default)]
pub struct Clock {
    inner: engine::Clock,
}

impl Clock {
    pub fn new() -> Self {
        Self {
            inner: engine::Clock::new(),
        }
    }

    /// 推进一帧并节流到目标帧率，返回本帧真实间隔（**秒**；pygame 返回
    /// 毫秒——Python 绑定层换算）。
    ///
    /// ⚠️ **回压叠加警告**（承自引擎时钟文档）：Fifo 呈现模式下 `flip`
    /// 已在 vsync 上节流——同档再 `tick(60)` 会睡眠与回压各等一个
    /// vsync（实际 ~30fps 且平白加延迟）。Fifo 下跟帧用 `tick(0)`；
    /// 只在「低于刷新率」或 Immediate/Mailbox 呈现时才传非零目标。
    pub fn tick(&mut self, fps: u32) -> f32 {
        self.inner.tick(fps)
    }

    /// EMA 平滑帧率（pygame `get_fps`）
    pub fn get_fps(&self) -> f32 {
        self.inner.fps()
    }

    /// 本帧真实间隔（秒，未节流；引擎 raw_delta 直通）
    pub fn raw_delta(&self) -> f32 {
        self.inner.raw_delta()
    }
}

/// 延时（pygame `time.delay`/`wait`）。
///
/// ⚠️ Web 上是 **no-op**（无阻塞模型——调用方持循环，阻塞主线程等于
/// 冻结页面）；Web 的节流手段 = `Clock::tick` / rAF 帧拍。
pub fn delay(ms: u64) {
    #[cfg(not(target_arch = "wasm32"))]
    std::thread::sleep(std::time::Duration::from_millis(ms));
    #[cfg(target_arch = "wasm32")]
    let _ = ms;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_tick_returns_elapsed() {
        let mut clock = Clock::new();
        std::thread::sleep(std::time::Duration::from_millis(5));
        let dt = clock.tick(0); // 0 = 不节流（避免测试真睡眠）
        assert!(dt > 0.0, "tick 应返回正间隔");
        assert!(clock.get_fps() > 0.0);
    }
}
