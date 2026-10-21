//! pygame 预设特效库（AudioEffect 的参数化预设 + 统一参数面）
//!
//! 分层（effect 体系）：
//! - **预设层（本模块）**：Rust 内置特效（Gain/Echo/Distortion/LowPass）
//!   + [`Effect`] 句柄（Clone = 同实例别名，`set` 实时生效）；
//! - **挂载层**：`pygame::mixer::Channel::add_effect(effect)`——串联在
//!   声道数据通路（read_frames → effects → mix）；
//! - **参数面**：[`EffectParam`] 统一字符串键控（`set(key, value)`）——
//!   绑定层一次实现通吃全部预设；Rust 侧各预设保留类型化 setter。
//!
//! # 自定义 effect 的边界（RT 线程铁律）
//!
//! `AudioEffect::process` 在音频回调线程按缓冲调用——**Python 代码
//! 永远不进音频线程**。自定义 DSP 的正确形态：
//! - L1 内置预设参数化（本模块，句柄实时调参）；
//! - L2 数据面自定义（sndarray 生成/变换波形 → make_sound）；
//! - L3 参数自动化（v2：Python 帧循环喂关键帧，Rust 特效按帧推进）。

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use starfish_audio::common::{AudioEffect, AudioError, StereoFrame};

/// f32 ↔ 原子位存储（RT 线程无锁参数读）
fn store_f32(a: &AtomicU32, v: f32) {
    a.store(v.to_bits(), Ordering::Relaxed);
}

/// f32 ↔ 原子位存储（RT 线程无锁参数读）
fn load_f32(a: &AtomicU32) -> f32 {
    f32::from_bits(a.load(Ordering::Relaxed))
}

/// 预设特效的统一参数面（字符串键控——绑定层一次实现通吃全部预设）
///
/// 每个预设实现：`set_param` 按 key 调参（未知键返回 false）、
/// `param_keys` 声明可调键集（绑定层枚举/校验用）。机制选型：
/// 字符串键 + f32 值（跨语言绑定零类型映射负担，运行时校验兜住
/// 拼写错误）；Rust 侧各预设同时保留类型化 setter。
pub trait EffectParam: AudioEffect {
    /// 按 key 调参；未知键返回 false
    fn set_param(&mut self, key: &str, value: f32) -> bool;

    /// 可调参数键集（声明式——绑定层可枚举）
    fn param_keys(&self) -> &'static [&'static str];
}

// ── 内置特效（AudioEffect 内置实现 + 句柄）──────────────────────
// 契约（base::audio::common）：process 在音频回调线程按缓冲调用——
// 实现内禁分配/禁阻塞。参数走 Arc<AtomicU32>（f32 bits）+ 声道侧
// Box 克隆共享同一状态：句柄 set 实时生效，RT 线程无锁读。


/// 增益（音量乘数 0.0..4.0）
#[derive(Clone)]
pub struct Gain {
    gain: Arc<AtomicU32>,
}

impl Gain {
    pub fn new(gain: f32) -> Self {
        let g = Self {
            gain: Arc::new(AtomicU32::new(0)),
        };
        g.set(gain);
        g
    }

    /// set(gain)：乘数（越界钳制 0.0..4.0）
    pub fn set(&self, gain: f32) {
        store_f32(&self.gain, gain.clamp(0.0, 4.0));
    }
}

impl AudioEffect for Gain {
    fn name(&self) -> &str {
        "gain"
    }

    fn process(&mut self, frames: &mut [StereoFrame]) {
        let g = load_f32(&self.gain);
        for f in frames {
            f.left *= g;
            f.right *= g;
        }
    }
}

/// 回声（延迟线 + 反馈；采样率构造定格，延迟按毫秒设定）
#[derive(Clone)]
pub struct Echo {
    rate: u32,
    delay_ms: Arc<AtomicU32>,
    feedback: Arc<AtomicU32>,
    /// 延迟环（仅音频线程触达；Clone 深拷贝——尾巴不跨克隆共享）
    buf: std::cell::RefCell<std::collections::VecDeque<StereoFrame>>,
}

impl Echo {
    /// new(sample_rate)：设备采样率（mixer::output_sample_rate()）
    pub fn new(sample_rate: u32) -> Self {
        Self {
            rate: sample_rate.max(1),
            delay_ms: Arc::new(AtomicU32::new(250)),
            feedback: Arc::new(AtomicU32::new(0)),
            buf: std::cell::RefCell::new(std::collections::VecDeque::new()),
        }
    }

    /// set_delay_ms(ms)：延迟毫秒（1..2000）
    pub fn set_delay_ms(&self, ms: f32) {
        store_f32(&self.delay_ms, ms.clamp(1.0, 2000.0));
    }

    /// set_feedback(fb)：反馈系数（0.0..0.95，防自激）
    pub fn set_feedback(&self, fb: f32) {
        store_f32(&self.feedback, fb.clamp(0.0, 0.95));
    }

    fn delay_frames(&self) -> usize {
        (load_f32(&self.delay_ms) * self.rate as f32 / 1000.0) as usize
    }
}

impl AudioEffect for Echo {
    fn name(&self) -> &str {
        "echo"
    }

    fn process(&mut self, frames: &mut [StereoFrame]) {
        let fb = load_f32(&self.feedback);
        let delay = self.delay_frames().max(1);
        let mut buf = self.buf.borrow_mut();
        // 环长恒等于延迟帧数：不足补零、超出截断——pop_front 读到的
        // 恒为 delay 帧前写入的回灌
        while buf.len() < delay {
            buf.push_front(StereoFrame { left: 0.0, right: 0.0 });
        }
        while buf.len() > delay {
            buf.pop_front();
        }
        for f in frames {
            let wet = buf
                .pop_front()
                .unwrap_or(StereoFrame { left: 0.0, right: 0.0 });
            let out_l = (f.left + wet.left * fb).clamp(-1.0, 1.0);
            let out_r = (f.right + wet.right * fb).clamp(-1.0, 1.0);
            buf.push_back(StereoFrame {
                left: out_l,
                right: out_r,
            });
            f.left = out_l;
            f.right = out_r;
        }
    }
}

/// 失真（tanh 软削波；drive 1.0 = 原样）
#[derive(Clone)]
pub struct Distortion {
    drive: Arc<AtomicU32>,
}

impl Distortion {
    pub fn new(drive: f32) -> Self {
        let d = Self {
            drive: Arc::new(AtomicU32::new(0)),
        };
        d.set(drive);
        d
    }

    /// set(drive)：驱动强度（1.0..10.0）
    pub fn set(&self, drive: f32) {
        store_f32(&self.drive, drive.clamp(1.0, 10.0));
    }
}

impl AudioEffect for Distortion {
    fn name(&self) -> &str {
        "distortion"
    }

    fn process(&mut self, frames: &mut [StereoFrame]) {
        let d = load_f32(&self.drive);
        for f in frames {
            f.left = (f.left * d).tanh();
            f.right = (f.right * d).tanh();
        }
    }
}

/// 低通（一阶；采样率构造定格，截止频率可实时调）
#[derive(Clone)]
pub struct LowPass {
    rate: u32,
    coeff: Arc<AtomicU32>,
    /// 滤波状态（L/R 各一；仅音频线程触达）
    state: std::cell::Cell<(f32, f32)>,
}

impl LowPass {
    /// new(sample_rate, cutoff_hz)
    pub fn new(sample_rate: u32, cutoff_hz: f32) -> Self {
        let s = Self {
            rate: sample_rate.max(1),
            coeff: Arc::new(AtomicU32::new(0)),
            state: std::cell::Cell::new((0.0, 0.0)),
        };
        s.set_cutoff(cutoff_hz);
        s
    }

    /// set_cutoff(hz)：截止频率（20..=rate/2，越界钳制）
    pub fn set_cutoff(&self, hz: f32) {
        let hz = hz.clamp(20.0, self.rate as f32 / 2.0);
        let a = 1.0 - (-std::f32::consts::TAU * hz / self.rate as f32).exp();
        store_f32(&self.coeff, a);
    }
}

impl AudioEffect for LowPass {
    fn name(&self) -> &str {
        "lowpass"
    }

    fn process(&mut self, frames: &mut [StereoFrame]) {
        let a = load_f32(&self.coeff);
        let (mut l, mut r) = self.state.get();
        for f in frames {
            l += a * (f.left - l);
            r += a * (f.right - r);
            f.left = l;
            f.right = r;
        }
        self.state.set((l, r));
    }
}

/// 特效句柄（Clone = 同实例别名，set 实时生效——参数走共享原子，
/// 通道侧 Box 克隆同读一状态）
#[derive(Clone)]
pub struct Effect {
    inner: EffectInner,
}

#[derive(Clone)]
enum EffectInner {
    Gain(Gain),
    Echo(Echo),
    Distortion(Distortion),
    LowPass(LowPass),
}

impl Effect {
    /// Effect(kind, sample_rate=48000)：工厂
    /// （kind = "gain" / "echo" / "distortion" / "lowpass"）
    pub fn new(kind: &str, sample_rate: u32) -> Result<Self, AudioError> {
        let inner = match kind {
            "gain" => EffectInner::Gain(Gain::new(1.0)),
            "echo" => EffectInner::Echo(Echo::new(sample_rate)),
            "distortion" => EffectInner::Distortion(Distortion::new(2.0)),
            "lowpass" => EffectInner::LowPass(LowPass::new(sample_rate, 4000.0)),
            _ => {
                return Err(AudioError::custom(format!(
                    "未知特效 '{kind}'（可用：gain / echo / distortion / lowpass）"
                )))
            }
        };
        Ok(Self { inner })
    }

    /// set(key, value)：实时调参（键随特效类型；未知键 → Err）
    pub fn set(&self, key: &str, value: f32) -> Result<(), AudioError> {
        let unknown = || {
            AudioError::custom(format!("特效参数未知：{key}"))
        };
        match &self.inner {
            EffectInner::Gain(g) => match key {
                "gain" => Ok(g.set(value)),
                _ => Err(unknown()),
            },
            EffectInner::Echo(e) => match key {
                "delay_ms" => Ok(e.set_delay_ms(value)),
                "feedback" => Ok(e.set_feedback(value)),
                _ => Err(unknown()),
            },
            EffectInner::Distortion(d) => match key {
                "drive" => Ok(d.set(value)),
                _ => Err(unknown()),
            },
            EffectInner::LowPass(l) => match key {
                "cutoff" => Ok(l.set_cutoff(value)),
                _ => Err(unknown()),
            },
        }
    }

    /// 入通道用：装箱为 AudioEffect（克隆共享同一参数状态）
    pub(crate) fn boxed(&self) -> Box<dyn AudioEffect> {
        match &self.inner {
            EffectInner::Gain(g) => Box::new(g.clone()),
            EffectInner::Echo(e) => Box::new(e.clone()),
            EffectInner::Distortion(d) => Box::new(d.clone()),
            EffectInner::LowPass(l) => Box::new(l.clone()),
        }
    }
}




impl EffectParam for Gain {
    fn set_param(&mut self, key: &str, value: f32) -> bool {
        match key {
            "gain" => {
                self.set(value);
                true
            }
            _ => false,
        }
    }

    fn param_keys(&self) -> &'static [&'static str] {
        &["gain"]
    }
}

impl EffectParam for Echo {
    fn set_param(&mut self, key: &str, value: f32) -> bool {
        match key {
            "delay_ms" => {
                self.set_delay_ms(value);
                true
            }
            "feedback" => {
                self.set_feedback(value);
                true
            }
            _ => false,
        }
    }

    fn param_keys(&self) -> &'static [&'static str] {
        &["delay_ms", "feedback"]
    }
}

impl EffectParam for Distortion {
    fn set_param(&mut self, key: &str, value: f32) -> bool {
        match key {
            "drive" => {
                self.set(value);
                true
            }
            _ => false,
        }
    }

    fn param_keys(&self) -> &'static [&'static str] {
        &["drive"]
    }
}

impl EffectParam for LowPass {
    fn set_param(&mut self, key: &str, value: f32) -> bool {
        match key {
            "cutoff" => {
                self.set_cutoff(value);
                true
            }
            _ => false,
        }
    }

    fn param_keys(&self) -> &'static [&'static str] {
        &["cutoff"]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn echo_effect_delays_wet() {
        // 冲激 → 湿声恰好在延迟帧数后出现（10ms @48k = 480 帧）
        let mut echo = Echo::new(48000);
        echo.set_delay_ms(10.0);
        echo.set_feedback(0.5);
        let mut frames = vec![StereoFrame { left: 0.0, right: 0.0 }; 1000];
        frames[0] = StereoFrame { left: 1.0, right: 1.0 };
        echo.process(&mut frames);
        assert!((frames[0].left - 1.0).abs() < 1e-4, "冲激直通");
        assert_eq!(frames[100].left, 0.0, "延迟期内无湿声");
        assert!(
            (frames[480].left - 0.5).abs() < 1e-4,
            "第 480 帧出现 0.5 反馈回声，实际 {}",
            frames[480].left
        );
    }

    #[test]
    fn gain_and_distortion_shapes() {
        let mut g = Gain::new(0.5);
        let mut frames = [StereoFrame { left: 1.0, right: -1.0 }; 4];
        g.process(&mut frames);
        assert!((frames[0].left - 0.5).abs() < 1e-5);

        let mut d = Distortion::new(8.0);
        let mut f2 = [StereoFrame { left: 0.5, right: -0.5 }; 2];
        d.process(&mut f2);
        assert!(f2[0].left > 0.99, "tanh(4) 软削波到 ±1");
        assert!(f2[0].right < -0.99);
    }

    #[test]
    fn lowpass_smooths_step() {
        let mut lp = LowPass::new(48000, 500.0);
        let mut frames = [StereoFrame { left: 1.0, right: 1.0 }; 200];
        lp.process(&mut frames);
        assert!(frames[0].left < 0.2, "首帧被平滑");
        assert!(frames[199].left > 0.9, "稳态逼近输入");
    }

    #[test]
    fn effect_factory_and_params() {
        let fx = Effect::new("echo", 48000).unwrap();
        assert!(fx.set("delay_ms", 100.0).is_ok());
        assert!(fx.set("feedback", 0.6).is_ok());
        assert!(fx.set("unknown", 1.0).is_err());
        assert!(Effect::new("nope", 48000).is_err());
    }

    #[test]
    fn param_keys_declared() {
        assert_eq!(Echo::new(48000).param_keys(), &["delay_ms", "feedback"]);
        assert_eq!(Gain::new(1.0).param_keys(), &["gain"]);
    }
}
