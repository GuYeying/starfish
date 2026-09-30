# RustPython 嵌入游戏库全案

> **定位**：完整案例记录——rpy 嵌入原理、跨平台原理、RustPython 魔改、
> Rust 接口绑定、踩坑与解法全集。2026-09-27~28 两日实战沉淀
> （批次十六~二十），全部内容均为实测，无推测性设计。
>
> 关联文档：`rustpython嵌入三端架构.md`（架构定稿 v1.0，本文的"设计
> 视角"浓缩版）、`../reference/安卓APK打包lib_name坑与真机诊断手册.md`
> （排障工具箱）、`../doc/log/starfish_changelog_2026-09-28.md`
> （批次十八~二十原始记录）。
>
> **结果先说**：同一份 Rust 宿主壳 + 同一份 Python 脚本，
> Windows（Vulkan）/ Android 真机（GLES/卓易通容器）/ Web（WebGL2）
> 三端同日全通，Python 逐帧驱动渲染，证据链完备。

---

## 一、目标与终态总览

**目标**：把 RustPython 解释器嵌入自研 starfish 引擎，让 Python 脚本
以 pygame 的心智模型驱动游戏循环——"复刻 pygame，但跑在 Rust 引擎上，
且跨 Windows/Android/Web 三平台"。

**终态用户代码**（`examples/binding_probe.py`，三平台一字不改）：

```python
import pygame

def game():
    screen = pygame.display.set_mode((800, 600))
    x = 370
    while True:
        for e in pygame.event.get():
            if e.type == pygame.QUIT:
                return
            if e.type == pygame.KEYDOWN and e.key == pygame.K_ESCAPE:
                return

        screen.fill((30, 30, 46))
        with screen.render() as r:
            r.rect((0, 200, 90), (x, 270, 60, 60))
            r.circle((255, 200, 0), (400, 300), 40)
            r.line((200, 60, 60), (x, 0), (x + 60, 600))
            r.polygon((90, 120, 220),
                      [(100, 100), (180, 80), (200, 160), (120, 170)])
        pygame.display.flip()

        x = 0 if x >= 730 else x + 4
        yield                      # ← 帧边界（生成器门，见 §3.4）


game = game()   # 壳负责实例化——脚本只定义（勿自调用，见 §6.3-5）
```

**证据链**（三端统一锚点，批次十八~二十实证）：`starting → set_mode OK
→ RP genesis 完成 → pygame shim 已安装 → generator created → entering
frame loop`，此后逐帧无异常。

---

## 二、rpy 嵌入原理

### 2.1 解释器装配（genesis）

```rust
pub fn new_interpreter() -> Interpreter {
    Interpreter::builder(Settings::default())
        .init_hook(|vm| { vm.recursion_limit.set(1000); })
        .add_frozen_modules(rustpython_pylib::FROZEN_STDLIB)
        .build()
}
```

三个关键点：

1. **`freeze-stdlib`**：整个 Python stdlib 冻结为字节码常量编进二进制
   ——没有外部 `Lib/` 目录依赖，三平台部署形态完全一致（Android apk 内、
   Web 单 wasm 文件、桌面单 exe）；
2. **`init_hook` 抬 recursion_limit**：debug 构建默认 256，frozen
   importlib 引导即撞线（RecursionError → "essential initialization
   failed"）；抬到 release 同款 1000。**但 debug 仍不可用**——debug 的
   巨型 native 栈帧触发栈守卫，不可配置修复 → **RP 相关构建恒 release**
   （铁律）；
3. **genesis 成本**：桌面 release 约 1~2s，真机数秒——嵌入一次、驻留
   全程，不用每脚本重建。

### 2.2 脚本执行链（RP 0.6 API 实测形态）

```rust
// 编译：源码 → 代码对象
let code = vm.compile(src, Mode::Exec, "main.py".to_owned())?;
// 执行：模块级代码跑一遍（def 定义 / 顶层语句）
vm.run_code_obj(code, scope.clone())?;
// 生成器：调用创建（函数体不执行，首帧由壳驱动）
let gen = game.call((), vm)?;
// 每帧步进：执行到下一个 yield 为止
vm.call_method(&gen, "__next__", ())?;
```

异常处理：`PyBaseException` **无 Display**——`format!("{e:?}")` 只有
类型名；必须 `exc.clone().into(): PyObjectRef` → `.repr(vm)` 取文本。

### 2.3 嵌入形态：谁持循环（本项目立项根因的解法）

| 形态 | 平台 | 机制 |
|---|---|---|
| **生成器门**（本案例三端验证） | 全平台 | Python 写 `def game(): while True: …; yield`，Rust 壳持循环，每帧 `__next__` 步进一帧；yield 即帧边界 |
| **while True 归 rpy**（批次十七，原生平台） | 桌面/Android | Python 模块级 `while True` 就是主循环——`fill/flip/quit_requested` 原生函数内部完成事件泵与上屏，SDL 原味 |

两种形态都成立的前提是 **poll 模型**（§3.1）。Web 因浏览器 rAF 强制
每帧让出，只能用生成器门——平台约束被 Rust 壳吸收，Python 语义不变。

---

## 三、跨平台原理：行为一致是如何达成的

### 3.1 引擎侧根基：poll 模型（循环归属调用方）

starfish-window（自研，替代 winit）：`Window::builder().build()` 之后
**调用方**在自己的循环里反复 `poll_event()` 拉事件、主动上屏。引擎不持
循环、无回调、无控制反转。

推论：**谁调用 poll/present，谁就是主循环**。嵌入场景下循环可以归
Rust 壳（生成器门），甚至归 Python 自己（while True 形态）——这正是
winit 控制反转架构做不到的（详见架构文档 §七对比）。

### 3.2 入口归一：`app_entry!` 宏（一行，三平台）

| 平台 | 宏展开 |
|---|---|
| 桌面 | `fn main()` → `block_on(run())`（pollster；Err → console_log + exit） |
| Android | `#[no_mangle] android_main(app)` → `android_init(app)` → `main()`（同桌面路径，同线程） |
| Web | `#[wasm_bindgen(start)]` → panic hook + `spawn_local(run())`（应用 future **永不完成**，驻留） |

驱动器经 `base::app` 再导出（`$crate` 化），用户 crate 零新增依赖；
唯一约束：web 构建需用户 crate 可见 wasm-bindgen（dev-deps 提供）。

### 3.3 帧拍：`next_frame().await`——壳里唯一的"平台感"

| 平台 | 语义 |
|---|---|
| 桌面/Android | no-op async（同步循环 + vsync 自然节流） |
| Web | rAF yield（**必须让出**——漏写 = 同步死循环冻结渲染进程） |

### 3.4 平台后端隐藏

starfish-window 三后端（Windows Win32 消息泵 / Web canvas+rAF /
Android android-activity 回调→队列反转）对外暴露同一套
`Window / poll_event / Event` 词汇。Android 的系统回调由后端内部
"回调写入、poll 排空"翻译成拉取式事件（含 Destroy/返回键→CloseRequested、
触摸→主手指合成鼠标等惯例翻译）。

### 3.5 一致性清单（怎么证明"一致"）

同一份 `binding_probe.rs` + 同一份 `binding_probe.py`，三平台：

- 同一条证据链（六标记，§一）逐环命中；
- 同样的帧行为（绿方块移动/四类图元，截图/目视实证）；
- 唯一的平台分岔被压进两处：宏展开（§3.2）与 `next_frame`（§3.3）。

---

## 四、RustPython 魔改全记录（vendor + 三补丁 + 接线）

### 4.1 vendor 方式

RP 源码快照（main 分支 0.6.0-dev，commit 23fcb8d9，2026-09-22 拉取）
整体解压进 `pygame-rs/src/rpy/dependencies/RustPython-main/`，Cargo 以
path 依赖直指其 `crates/vm` 与 `crates/pylib`。源码随仓库走——构建可
重现、补丁可审计。

### 4.2 三补丁（快照上重放，逐点验证上游未修）

| # | 补丁 | 缘由 |
|---|---|---|
| ① | **libffi 构建目标段剔除 android** | libffi 的 android 分支构建失败（依赖不存在/工具链不符）——RP 仅在启用 ctypes 时需要 libffi，而我们**不用 ctypes** |
| ② | **ctypes 门控** | ctypes 是 C ABI 能力，嵌入场景下由**绑定面 Rust 原生函数替代**（依赖双向规则：starfish 零 Python 资产，Python 需要的能力走 sf 面） |
| ③ | **static_cell android 复用上游自带 no_std 全局变体** | 比备忘录旧方案（自写 130 行）更简——新快照逐点验证后取简版 |

补丁细节与位置：`reference/rustpython改动备忘录.md`。

### 4.3 特性接线（Cargo.toml）

```toml
rustpython-vm = { path = ".../crates/vm", default-features = false, features = [
    "compiler", "wasmbind", "gc", "stdio", "importlib", "encodings", "freeze-stdlib",
] }
```

- `default-features = false`：**去 host_env**——wasm32-unknown-unknown
  上 crt_fd 依赖不存在（链接必败）；同时 ctypes 恒不用（见补丁②）；
- `freeze-stdlib`（vm + pylib 都要开）：stdlib 冻结（§2.1）；
- `wasmbind`：web 侧 wasm-bindgen 桥（非 wasm 目标自动惰性）。

### 4.4 实测运行时约束（铁律）

| 约束 | 缘由 |
|---|---|
| **恒 release 构建** | debug genesis 栈守卫必败（§2.1），不可配置修复 |
| 单线程 GIL 心智 | 壳内 `interp.enter` 串行进出，RP 值不跨线程传递（PyObjectRef 非 Send） |
| genesis 一次、驻留全程 | 成本秒级，不做每脚本重建 |

---

## 五、Rust 接口绑定实现

### 5.1 总体形态：双层结构（可维护性 ADR）

```
用户脚本 ──import pygame──▶ pygame_shim.py（Python 层：组装/默认值/转换）
                                 │ 调用
                                 ▼
                            sf.rs 扁平原生函数（Rust 层：sf_*，一行转发 starfish API）
                                 │
                                 ▼
                            starfish pygame 层（P1~P5 全量能力）
```

**可维护性 ADR**：加新 API 优先改 Python 侧（shim 真文件、可注释、可
转换），Rust 侧仅在需要新原语时扩容。性能优化后置（帧级调用，协议开销
可忽略；需要时再做参数编解码优化）。

### 5.2 原生函数签名惯例（RP 0.6 实测铁律）

```rust
fn sf_draw_rect(
    x: i32, y: i32, w: i32, h: i32,
    color: (i32, i32, i32, i32),   // 多值参数收敛为单 tuple（坑 1，§6.3）
    width: i32,
    vm: &VirtualMachine,           // ← &VirtualMachine 必须末位
) -> PyResult<()>
```

1. **`&VirtualMachine` 恒末位**，前面的参数经 FromArgs 自动绑定；
2. **数值严格**：f32 拒 int、i32 拒 float → **全参数定约 i32**（坐标/
   尺寸/颜色 0~255，pygame 原生习惯）；
3. **7 元组上限**：第 8+ 参数 → `PyNativeFnInternal` 不满足（编译期）；
4. **tuple FromArgs = 平铺消费连续位置参数**（非嵌套！）：形参
   `(i32,i32,i32,i32)` 要求 Python 侧传 4 个连续 int（`*rgba(color)`
   散开），传单个 tuple 报 `Expected type 'int' but 'tuple' found`；
5. **嵌套容器受限**：`Vec<(i32,i32)>` FromArgs/TryFromObject 均不支持
   → 点列扁平化 `[x,y,x,y,...]` 传 `Vec<i32>`，原生层 `chunks(2)` 组对；
6. 注册：`vm.new_function("sf_xxx", sf_xxx).into()` → `scope.globals
   .set_item(...)`。

### 5.3 pygame 包 shim（Python 侧组装）

`pygame_shim.py`（include_str 静态嵌入，无文件系统依赖）：

```python
import sys, types
pygame = types.ModuleType("pygame")

# 常量：同时写 globals（shim 内部用）与模块属性（pygame.QUIT 外部用）
for _name, _val in [("QUIT", "quit"), ("K_ESCAPE", 27), ...]:
    globals()[_name] = _val
    setattr(pygame, _name, _val)          # 坑：裸 global ≠ 模块属性

display = types.ModuleType("pygame.display")
display.set_mode = set_mode               # 校验+返回占位（真身由壳预初始化）
display.flip = lambda: sf_flip()
pygame.display = display
sys.modules["pygame.display"] = display   # import pygame.display 生效
sys.modules["pygame"] = pygame
```

宿主壳在**用户脚本之前** exec shim（同一 scope）→ 用户 `import pygame`
命中 sys.modules。**加新 API 优先改这里**（Python 侧转发）。

坑位：模块对象的常量必须 `setattr`（裸 global 对 `pygame.QUIT` 不可见）；
`set_mode` 是薄校验——GPU 装配 async（Web），真身由壳在脚本前 `set_mode`
完成。

### 5.4 with 会话绑定（渲染入口，单形态定约）

```python
with screen.render() as r:   # __enter__ → sf_session_begin
    r.rect(...)              # sf_draw_* 路由到活跃 RenderPass
                             # __exit__ → sf_session_end（finish 幂等打包）
```

Rust 侧（架构 §九.1 **单形态定约**：不提供 `pygame.draw.*(screen,…)`
兼容形态——双形态在 Surface/MRT 场景产生二义性）：

```rust
thread_local! {
    static SESSION: RefCell<Option<RenderPass<'static>>> = ...;
}   // get_screen() 是 &'static → RenderPass<'static> 可驻留 thread-local

fn sf_session_begin(_) { SESSION = Some(get_screen().render()); }
fn sf_session_end(_)   { SESSION.take() → finish()（幂等打包，Drop 兜底） }
fn sf_draw_rect(...)   { with_session(|t| draw::rect(t, …)) }  // 无会话=TypeError
```

### 5.5 事件流与诊断口

- **事件**：壳每帧 `pump()`（引擎事件 → 内部队列 + Quit/ESC 置位旗标）
  → Python `pygame.event.get()` → `sf_events()` 排空为
  `(tag, ...payload)` tuple 列表（`"quit"/"keydown"/…`）→ shim 包装成
  `Event` 对象（`.type/.key/.x`）；
- **诊断**：`devlog()` 四通道（println + console_log + /sdcard 文件 +
  TCP POST 到 PC 收集器）+ 计数器（session_begin/draw/session_end/
  frame #n）+ Python 侧 `sf_dlog("...")` 打点口。

---

## 六、踩坑与解法全集

> 按"哪一层"分组。★ = 差点带偏方向的重大坑。

### 6.1 打包/部署层（Android）

| 坑 | 症状 | 根因 | 解法 |
|---|---|---|---|
| ★ **lib_name 不匹配** | 秒退（200ms~2s），应用日志零输出 | 手动重打包只换 .so，manifest 的 `android.app.lib_name` 还是旧示例名 → NativeActivity dlopen 找不到库即崩——**应用代码从未执行**，此前一切"闪退/无日志"都是它的表症 | **aapt2 link 正规链**：模板 `__LIB_NAME__` 替换 → link 重造 base → 组装 dex+.so → zipalign → sign；模拟器 + `adb logcat -b crash` 取得决定性报错 |
| .so 压缩方式 | 55ms vs 273ms 死亡差异 | ZIP_STORED 的差异实为上述 crash 的噪音 | 定约 DEFLATED（与历史可跑包一致），但要知道它不是根因 |
| 存储权限缺失 | /sdcard 日志全部静默失败 | manifest 无 `WRITE_EXTERNAL_STORAGE`（且 dlog 的 `if let Ok` 吞错） | 补权限 + `requestLegacyExternalStorage`（卓易通按 legacy 授予，实证可写）；dlog 失败必须 println 报错 |
| 急速重启 abort | 刚退就重开 → 2.8s 死亡 | android-activity 0.6.1 胶水不支持"销毁中途重建"（上游缺陷） | 用户侧：等几秒再开；架构侧：宿主收到 CloseRequested/Suspended 立即终结进程 |

### 6.2 运行时层

| 坑 | 症状 | 根因 | 解法 |
|---|---|---|---|
| ★ **TCP 无超时阻塞 connect** | 应用 ANR（无响应冻结） | 诊断用 `TcpStream::connect` 网络不通时在主线程无限期阻塞 | `connect_timeout(300ms)` + 一次失败即拉黑（AtomicBool）；**诊断代码绝不可无超时阻塞主线程** |
| ★ **旋转 × 迟到 set_mode** | 只见清屏色、图元全缺（偶发） | 启动早期睡眠/阻塞把 set_mode 拖到横屏旋转（TerminateWindow/InitWindow 重建 ANativeWindow）之后 → 拿旧 window 配 surface → `Invalid surface` | **启动后立即 set_mode**；旋转交 `Resized` 事件——pygame 层接线 `handle_resized → get_screen().resize()`（批次二十，桌面 500×900 强改实测自愈）；启动路径禁止睡眠/长阻塞 |
| 模拟器 GLES 死穴 | 模拟器 `Surface::configure` 恒 `Invalid surface` | 环境层（GL 转译），新旧架构同败 | 逻辑层验证用模拟器（crash 取证价值极大），渲染验收只认真机 |
| debug genesis 栈溢出 | debug 构建 "essential initialization failed" | recursion_limit 256 + debug 巨型栈帧 | 恒 release（init_hook 抬 1000 只治一半） |
| Resized 无人处理 | 旋转后正交相机/交换链停留旧尺寸 | pygame 层事件翻译丢弃 Resized | 批次二十：`translate` 分支自动 `handle_resized → get_screen().resize()`（ADR-8：display 逻辑尺寸不跟随） |

### 6.3 绑定层（RP API，详见 §5.2）

1. 7 元组上限（颜色收敛单 tuple）；
2. tuple FromArgs 平铺语义（Python 侧 `*rgba` 散开传）；
3. `Vec<(i32,i32)>` 不支持（点列扁平化）；
4. 模块常量必须 `setattr` 到 ModuleType；
5. **生成器双重实例化**：脚本模块级 `game = game()` + 壳再 `call` →
   `'generator' object is not callable`——定约：**脚本只定义，壳实例化**；
6. FromArgs 数值严格（f32 拒 int/i32 拒 float → 全 i32 定约）；
7. PyBaseException 无 Display（repr 转文本）。

### 6.4 诊断层

| 坑 | 教训 |
|---|---|
| 日志通道全部静默失效时无从判断 | 建立**通道可信度排序**：hilog 进程寿命（唯一全时可用）→ 模拟器 logcat → 屏幕快照/阶段色 → TCP → 文件；交叉印证 |
| headless 截图黑屏误判为故障 | headless 把页面标 hidden → rAF 停摆；以 console 标记流为准 |
| Win32 FindWindow 找不到窗口 | 改 `Get-Process <名>.MainWindowHandle` 直取句柄 |
| 想当然的根因带偏方向 | lib_name 之前先后怀疑过 RP 链接/压缩方式/分区存储——**对照组**（pygame_hello 无 RP 能跑）+ **模拟器取证**才是破案手段；"应用代码从未执行"类结论要用证据链证明，不能靠排除法 |

---

## 七、排障方法论（可复用流程）

1. **通道可信度分级**：先建唯一完全可信的观测（hilog 进程寿命），
   再谈定位；
2. **睡眠标记法**：零日志条件下，在各阶段插 `sleep(6s)`，寿命读数 =
   到达阶段（本轮 M1 首行未达 → 直接证明死在 run() 之前）；
3. **对照组法**：疑似 X 引入的问题，找一个"除 X 外全同"的能跑样本
   （pygame_hello 无 RP）双向夹逼；
4. **降维取证**：真机盲区 → 模拟器（逻辑层 + 真 logcat）→ 桌面
   （全可见）——分层复现，每层拿到该层的最大信息量；
5. **视觉取证**：屏幕即日志（阶段色）+ snapshot_display 连拍 +
   桌面 Win32 强改窗口（resize 自愈验证）。

---

## 八、三端运行命令（终态）

```bash
# 桌面
cargo run --release -p pygame-rs --example binding_probe

# Android（cargo ndk → strip → aapt2 正规链 → 签名，详见诊断手册 §二）
# 产物：target/android-apk/binding_probe_android.apk

# Web
cargo build --release --target wasm32-unknown-unknown --example binding_probe -p pygame-rs
wasm-bindgen --out-dir web --target web pygame-rs/target/wasm32-unknown-unknown/release/examples/binding_probe.wasm
cd web && python -m http.server 8000   # http://localhost:8000/binding_probe.html
```

## 九、遗留与下一步

| 项 | 状态 |
|---|---|
| Surface 批次 v1.1（架构 §九：`depth_buffer` 整合创建 + `render_depth` 会话 + 资源句柄表 + `r.*` 扩展） | 待开工 |
| rp_main_loop 真机挂起 | 待排查（lib_name 正确、进程启动过；当前源码 set_mode 缺失需找回；同套 genesis 在 window_test 三平台通过 → 非 genesis 本身） |
| xtask `--dir` | 待开工（退役手动打包链） |
| 高级 GPU 功能扩展路径 | 已定约（架构 §九.4：compute 会话 / 会话新方法 / 材质变体 / 低阶逃生舱）——with 模型不锁 compute/实例化/体积云/光追/光探针 |
