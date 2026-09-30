# Web 渲染不绿问题定位与修复（hello 案例）

> 2026-09-27。starfish-rewrite hello 测试案例：Web 端后台日志正常输出
> 但窗口"好像不是绿色"的完整定位过程。结论：**渲染管线无任何问题，
> 是清屏色传错了**——一行修复。本文价值在定位手法（如何用低成本证据
> 排除整类怀疑）与 Web 无头验证流程固化。

---

## 一、症状

- `web/hello.html` 打开后 wasm 加载正常、控制台周期打印
  `[hello] 帧 N`（帧循环在跑）；
- 但窗口显示的是**近黑的暗蓝色**，不是期望的纯绿。

症状的迷惑性：日志正常 → 容易怀疑"wgpu Web 后端没真正渲染 /
present 丢了帧 / WebGL2 兼容性"等渲染层问题。

## 二、定位过程

### 2.1 先看代码事实

`examples/hello.rs` 帧循环：

```rust
const CLEAR: Color = Color { r: 0.0, g: 1.0, b: 0.0, a: 1.0 }; // 纯绿
...
surface.begin_frame(wgpu::Color { r: 0.1, g: 0.1, b: 0.15, a: 1.0 }, 1.0);
```

`CLEAR` 绿色常量定义后**从未使用**，`begin_frame` 收到的是写死的
`(0.1, 0.1, 0.15)`。换算：0.1×255 ≈ 26、0.15×255 ≈ 38，即
`rgb(26, 26, 38)` 的暗蓝灰。

### 2.2 用截图像素收尾（证据闭环）

对照此前存档的无头截图（`web/hello_final.png` 等）：窗口颜色正是
`rgb(26, 26, 38)` 暗蓝灰——**与代码里的错误清屏色逐点吻合**。

这一步同时排除了所有渲染层怀疑：

| 怀疑项 | 排除依据 |
|---|---|
| wgpu web 后端没工作 | 窗口颜色 = begin_frame 传入值，说明 acquire → clear pass → submit → present 全链路在工作 |
| present 丢帧/黑屏 | 黑屏是 rgb(0,0,0)，实测是 rgb(26,26,38)，颜色有来源 |
| WebGL2 格式/交换链问题 | 同上，颜色正确上了屏 |

**方法论**：渲染问题先问"屏幕上的颜色是从哪条代码路径来的"，
颜色对得上代码 = 管线通，问题在传入值；颜色对不上 = 才查管线。

## 三、修复

```rust
surface.begin_frame(CLEAR, 1.0);
```

一行。修复后重建产物并重新截图，全屏纯绿。

## 四、验证流程固化（Web 无头截图）

静态渲染类验证用**虚拟时间 + 截图模式**（注意：时间类验证不能用，
见主 CLAUDE.md 坑位 7——虚拟时间会冻结 delta）：

```bash
cargo build --release --target wasm32-unknown-unknown --example hello
wasm-bindgen --out-dir web --target web \
  target/wasm32-unknown-unknown/release/examples/hello.wasm

python -m http.server 8877 --directory web          # 任意静态服务
"/c/Program Files (x86)/Microsoft/Edge/Application/msedge.exe" \
  --headless=new --enable-logging=stderr \
  --virtual-time-budget=6000 \
  --screenshot="D:\绝对路径\web\verify_green.png" \
  http://localhost:8877/hello.html
```

要点：

- `--screenshot` 一律**绝对路径**（相对路径会落到 Edge 版本目录）；
- 输出 PNG 直接目视或用 Python 裸解析像素核验；
- console 输出经 `--enable-logging=stderr` 收割（应用内诊断必须走
  `console_log`，wasm 上 `println!` 无处可去）。

## 五、测试状态

| 项 | 结果 |
|---|---|
| 修复后无头截图 | 纯绿 ✅（`web/verify_green.png` / `verify_final.png`） |
| 源码一致性 | 修复后用当前源码重新出包再验证，二次纯绿 ✅ |
| 桌面/Android 同一份 `app_body` | 清屏色三平台同源，web 修复即三平台修复 |
