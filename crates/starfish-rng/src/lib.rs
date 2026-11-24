//! starfish-rng：确定性随机数层（**stdlite 定约**——游戏标准库能力：
//! 跨平台同 seed 同序列——联机同步 / 存档回放 / 程序化生成的硬前提；
//! wasm 下 JS 随机不可播种 = 标准库路线死路，Rust 自持算法唯一正道）。
//!
//! 算法 = PCG32（状态 16B 可序列化、无预热、统计质量高于 MT19937）。
//!
//! 结构定约：**算法全在 `impl Pcg32` 方法**（模块级函数 = 默认流包装，
//! PyRandom 实例流同款复用）。

use starfish_debug::log_debug;

/// PCG32 状态（seed 流实例——pygame.random.Random(seed) 载体）
pub struct Pcg32 {
    state: u64,
    inc: u64,
}

impl Pcg32 {
    /// 播种（seed/seq 对——pcg32_initseq 惯例；seed=0 合法）
    pub fn new(seed: u64, seq: u64) -> Self {
        let mut rng = Pcg32 {
            state: 0,
            inc: ((seq << 1) | 1) | 1,
        };
        // 两次 next 暖机 + 注入 seed（标准初始化序）
        let _ = rng.next_u32();
        rng.state = rng.state.wrapping_add(seed);
        let _ = rng.next_u32();
        rng
    }

    /// 原始 u32 输出（PCG-XSH-RR 64/32）
    pub fn next_u32(&mut self) -> u32 {
        let old = self.state;
        self.state = old
            .wrapping_mul(6364136223846793005)
            .wrapping_add(self.inc | 1);
        let xorshifted = (((old >> 18) ^ old) >> 27) as u32;
        let rot = (old >> 59) as u32;
        xorshifted.rotate_right(rot)
    }

    /// [0, 1) 浮点（24 位尾数——f32 精度对齐）
    pub fn next_f32(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 / 16777216.0
    }

    /// 下一 u64（两次 u32 拼——bulk/混洗用）
    pub fn next_u64(&mut self) -> u64 {
        ((self.next_u32() as u64) << 32) | (self.next_u32() as u64)
    }

    /// [a, b] 整数（含两端）——无偏有界（Lemire 拒绝法，模偏差消除）
    pub fn rand_bounded(&mut self, span: u64) -> u64 {
        let threshold = span.wrapping_neg() % span;
        loop {
            let v = self.next_u64();
            if v >= threshold {
                return v % span;
            }
        }
    }

    /// [a, b] 浮点
    pub fn uniform(&mut self, a: f32, b: f32) -> f32 {
        a + self.next_f32() * (b - a)
    }

    /// [a, b] 整数（含两端——CPython randint 语义）
    pub fn randint(&mut self, a: i64, b: i64) -> i64 {
        if b <= a {
            return a;
        }
        let span = (b - a + 1) as u64;
        a + (self.rand_bounded(span) as i64)
    }

    /// 高斯分布（Box-Muller——瞄准散布/粒子）
    pub fn gaussian(&mut self, mu: f32, sigma: f32) -> f32 {
        // u1 避 0（ln(0) = -inf）
        let u1 = (self.next_f32() + 1e-9).min(1.0);
        let u2 = self.next_f32();
        (u1.ln() * -2.0).sqrt() * (u2 * std::f32::consts::TAU).cos() * sigma + mu
    }

    /// 均匀 2D 单位方向 → (x, y)
    pub fn unit_vec2(&mut self) -> (f32, f32) {
        let theta = self.next_f32() * std::f32::consts::TAU;
        (theta.cos(), theta.sin())
    }

    /// 均匀 3D 单位方向 → (x, y, z)（球面均匀——z 均匀 + 反三角，
    /// **不是**两次高斯归一化的常见错误）
    pub fn unit_vec3(&mut self) -> (f32, f32, f32) {
        let cz = self.next_f32() * 2.0 - 1.0; // z ∈ [-1, 1] 均匀
        let sz = (1.0 - cz * cz).max(0.0).sqrt();
        let theta = self.next_f32() * std::f32::consts::TAU;
        (sz * theta.cos(), sz * theta.sin(), cz)
    }

    /// 圆内均匀点 → (x, y)（sqrt 反变换——**不是** (r·cosθ) 的中心
    /// 聚集常见错误）
    pub fn in_circle(&mut self, r: f32) -> (f32, f32) {
        let rr = r * self.next_f32().sqrt();
        let theta = self.next_f32() * std::f32::consts::TAU;
        (rr * theta.cos(), rr * theta.sin())
    }

    /// 球内均匀点 → (x, y, z)（cbrt 反变换——体积均匀）
    pub fn in_sphere(&mut self, r: f32) -> (f32, f32, f32) {
        let rr = r * self.next_f32().cbrt();
        let (ux, uy, uz) = self.unit_vec3();
        (rr * ux, rr * uy, rr * uz)
    }

    /// 批量 [0,1) 列表（**批量口**——一次 py 边界穿越 n 个）
    pub fn rand_list(&mut self, n: usize) -> Vec<f32> {
        (0..n).map(|_| self.next_f32()).collect()
    }

    /// Fisher-Yates 混洗序（索引回传——py 侧组装，免对象编组）
    pub fn shuffle_order(&mut self, len: usize) -> Vec<usize> {
        let mut order: Vec<usize> = (0..len).collect();
        for i in (1..len).rev() {
            let j = self.rand_bounded((i + 1) as u64) as usize;
            order.swap(i, j);
        }
        order
    }
}

// ── 全局默认流（pygame.random 模块级函数驱动；Random(seed) 独立流
// 不经此——多系统子流互不踩）──
use std::cell::RefCell;
thread_local! {
    static DEFAULT_RNG: RefCell<Pcg32> =
        RefCell::new(Pcg32::new(0x853C49E6748FEA9B, 0xDA3E39CB94B95BDB));
}

/// 模块级：重播种（默认流）
pub fn seed(n: u64) {
    DEFAULT_RNG.with(|dflt| *dflt.borrow_mut() = Pcg32::new(n, 0xDA3E39CB94B95BDB));
    log_debug!(&format!("[rng] seed({n})"));
}

/// 模块级：[0, 1) 浮点
pub fn random() -> f32 {
    DEFAULT_RNG.with(|dflt| dflt.borrow_mut().next_f32())
}

/// 模块级：[a, b] 浮点
pub fn uniform(a: f32, b: f32) -> f32 {
    DEFAULT_RNG.with(|dflt| dflt.borrow_mut().uniform(a, b))
}

/// 模块级：[a, b] 整数（含两端）
pub fn randint(a: i64, b: i64) -> i64 {
    DEFAULT_RNG.with(|dflt| dflt.borrow_mut().randint(a, b))
}

/// 模块级：序列取一（越界 = 首元素兜底——调用方自证非空）
pub fn choice_len(len: usize) -> usize {
    if len == 0 {
        return 0;
    }
    DEFAULT_RNG.with(|r| r.borrow_mut().rand_bounded(len as u64)) as usize
}

/// 模块级：高斯分布
pub fn gaussian(mu: f32, sigma: f32) -> f32 {
    DEFAULT_RNG.with(|r| r.borrow_mut().gaussian(mu, sigma))
}

/// 模块级：均匀 2D 单位方向
pub fn unit_vec2() -> (f32, f32) {
    DEFAULT_RNG.with(|r| r.borrow_mut().unit_vec2())
}

/// 模块级：均匀 3D 单位方向
pub fn unit_vec3() -> (f32, f32, f32) {
    DEFAULT_RNG.with(|r| r.borrow_mut().unit_vec3())
}

/// 模块级：圆内均匀点
pub fn in_circle(r: f32) -> (f32, f32) {
    DEFAULT_RNG.with(|dflt| dflt.borrow_mut().in_circle(r))
}

/// 模块级：球内均匀点
pub fn in_sphere(r: f32) -> (f32, f32, f32) {
    DEFAULT_RNG.with(|dflt| dflt.borrow_mut().in_sphere(r))
}

/// 模块级：批量 [0,1) 列表
pub fn rand_list(n: usize) -> Vec<f32> {
    DEFAULT_RNG.with(|r| r.borrow_mut().rand_list(n))
}

/// 模块级：Fisher-Yates 混洗序
pub fn shuffle_order(len: usize) -> Vec<usize> {
    DEFAULT_RNG.with(|r| r.borrow_mut().shuffle_order(len))
}
