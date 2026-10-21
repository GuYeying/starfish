# time 架构

> 零平台依赖的时间：`std::time::Instant` 固定原点（进程内首次调用），
> Web 节流权归浏览器 rAF。

## 关键文件

| 文件 | 职责 |
|---|---|
| `time/mod.rs` | `Clock`（帧计时）/ `FixedTimestep`（确定性步进）/ `sleep_until`（混合节流） |

## 架构与数据流

- **`Clock`**：`frame()` 逐帧刷新 → `raw_delta()`（真实帧间隔）/ `delta()`
  （× `set_scale` 缩放，暂停=0、慢动作=0.5）分离；`total_f64` f64 累计防漂移；
  `fps()` EMA 平滑；`frame_count()`。
- **`FixedTimestep::new(hz)`**：`tick(target_fps)` 返回本帧应执行的固定步数
  （追帧上限 = 死循环保护）——确定性玩法更新与渲染帧率解耦。
- **`sleep_until(deadline)`**：睡眠 + 末段自旋（亚毫秒精度）——桌面 fps cap
  的节流原语；**wasm 上 no-op**（Web 帧节奏由 rAF 决定，自设睡眠无意义）。

## 公开 API 速览

`Clock::{new, frame, raw_delta, delta, set_scale, scale, total_f64, elapsed,
fps, frame_count}`；`FixedTimestep::{new, tick}`；`sleep_until(Duration)`。

## 平台差异收敛点

仅 `sleep_until` 一处 wasm no-op。**wasm 上 `std::time::Instant` 不可用**
（无单调时钟入口）——库内 Web 计时走 `performance.now()`（web-sys），
见 `base/app` 与诊断模块；本模块 API 面保持全平台一致。

## 设计纪律

`delta()` 永远走 `set_scale` 缩放——玩法代码统一用 `delta()`，回放/调试
才用 `raw_delta()`；两者混用会破坏暂停/慢动作语义。

## 测试锚点

lib 纯逻辑测试（Clock 缩放/累计、FixedTimestep 步进与死循环保护）。
注意：无头虚拟时间模式会冻结 delta（CLAUDE.md Web 坑位 7）——时间相关
验证走存活模式。

## 深入入口

`doc/log/` 时间批次。
