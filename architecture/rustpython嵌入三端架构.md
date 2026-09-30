# RustPython 嵌入三端架构（pygame-rs）

> 状态：**v1.0（2026-09-28 三平台实证定稿）**。关联：`pygame层设计.md`
> （v1.0 契约）、`reference/安卓APK打包lib_name坑与真机诊断手册.md`、
> `doc/log/starfish_changelog_2026-09-28.md`（批次十八，三端齐跑记录）。
>
> 本文回答四个问题：①这套嵌入架构是怎么分层的；②三平台差异是如何被
> 消化的；③它与 winit 侵入式循环架构的本质差异是什么；④立项时的核心
> 问题是否被解决。

---

## 一、分层与依赖（单向红线）

```
┌─────────────────────────────────────────────┐
│  用户 Python 脚本（game() 生成器 / while True）│  ← 游戏逻辑所在层
├─────────────────────────────────────────────┤
│  pygame-rs（独立 crate）                      │
│  ├─ rpy 壳：RP 解释器装配（vendor + 三补丁）   │
│  ├─ sf 绑定面：fill/rect/flip/quit_requested │  ← Python ↔ Rust 唯一通道
│  └─ examples：平台宿主壳（app_entry! 一行）    │
├─────────────────────────────────────────────┤
│  starfish（引擎）                             │
│  ├─ src/pygame/：GPU 路线 pygame 层（P1~P5）  │
│  ├─ src/base/：render/window/audio/...       │
│  └─ starfish-window/：自研窗口事件（poll 模型）│
├─────────────────────────────────────────────┤
│  平台：Windows / Web(wasm32) / Android        │
└─────────────────────────────────────────────┘
```

**依赖单向规则（硬性）**：`pygame-rs → starfish` 单向；starfish 永不
感知 pygame/RP 类型（零 Python 资产）。RP 以 vendor 方式收进
`pygame-rs/src/rpy/dependencies/`（0.6.0-dev snapshot + 三补丁：libffi
剔 android、ctypes 门控、static_cell 全局单元；`freeze-stdlib` 把 stdlib
冻结进二进制——无外部 Lib 目录依赖，三平台部署形态完全一致）。

## 二、核心机制：循环归属（poll 模型）——一切的前提

starfish-window 是 **SDL 风格 poll 模型**：`Window::builder().build()`
建窗后，**调用方**在自己的循环里反复 `poll_event()` 拉事件、主动上屏。
引擎不持循环、不注册回调、不做控制反转。

由此导出本项目最关键的自由度：**主循环归"调用 poll 的那个人"**。
嵌入式场景里，那个人可以是 Rust 宿主壳，也可以（在原生平台）直接是
Python 脚本——这正是脚本语言嵌入游戏引擎的历史性难点（见 §七）。

## 三、三平台驱动差异的消化

平台差异被压缩到**两处**，应用代码（Rust 壳 + Python 脚本）三端零分叉：

### 3.1 入口：`app_entry!` 宏（src/lib.rs）

```rust
starfish::app_entry!(run());   // 一行，三平台
```

| 平台 | 宏展开 |
|---|---|
| 桌面 | `fn main()` → `block_on(run())`（pollster；Err → console_log + exit） |
| Android | `android_main(app)` → `android_init(app)` → `main()`（同桌面路径） |
| Web | `#[wasm_bindgen(start)]` → panic hook + `spawn_local(run())`（应用 future 永不完成，驻留） |

用户 crate 零新增依赖（驱动器经 `base::app` 再导出 `$crate` 化）；唯一
约束：web 构建需用户 crate 可见 wasm-bindgen（dev-deps 提供）。

### 3.2 帧拍：`next_frame().await`

| 平台 | 语义 |
|---|---|
| 桌面/Android | no-op async（同步循环 + vsync 自然节流） |
| Web | rAF yield——**必须让出**，否则同步死循环冻结渲染进程 |

Rust 壳里唯一的"平台感"就是这个 await；Python 侧完全无感。

### 3.3 平台后端（starfish-window 内部，调用方不可见）

Windows（Win32 消息泵）/ Web（canvas + rAF 重绘合并 + visibilitychange）
/ Android（android-activity 回调 → "回调写入、poll 排空"队列反转）。
对外只有同一套 `Window / poll_event / Event` 词汇。

## 四、Python 参与主循环的两种形态

### 4.1 生成器门（本批次三端验证的形态；window_test）

```python
def game():
    x = 370
    while True:
        if quit_requested(): return
        fill(30, 30, 46, 255)
        rect(x, 270, 60, 60, 0, 200, 90, 255)
        x = 0 if x >= 730 else x + 4
        flip()
        yield            # ← 帧边界

game = game()
```

Rust 壳持循环：每帧 `pump()`（事件泵）→ `vm.call_method(gen, "__next__")`
（步进 Python 一帧）→ `next_frame().await`（帧拍）。**yield 即帧边界**
（pygbag 同款思路）。三平台通吃——Web 端 rAF 强制让出，生成器门是唯一
合法形态。

### 4.2 主循环归 rpy（批次十七形态；rp_main_loop，仅原生平台）

Python 模块级 `while True` 直接就是主循环：`fill/rect/flip` 在 Python 的
调用节奏里同步执行（SDL 原味），事件泵与上屏都发生在 Python 调用的原生
函数内部。引擎无循环可让——谁调用 poll/present 谁就是主循环，而那个人
是 Python。

**取舍**：Web 因 rAF 必须让出而只能用 4.1；原生平台两者皆可，4.2 更
"pygame 原味"。

### 4.3 为什么帧边界是 yield 而非 async/await（取舍记录）

RP 现状（实测）：`async def`/`await` 语法层可用（VM 有 coroutine 模块），
但 **asyncio 是纯 Python 冻结版**——缺 CPython 的 `_asyncio` C 加速器，
且依赖 selector/threading 等平台原语（wasm 上残缺）。对比：

| | 生成器 `yield` | 协程 `await` |
|---|---|---|
| 运行时依赖 | 生成器协议（`__next__`/`StopIteration`），RP 完整支持 | Future 协议 + 事件循环 + Task 调度（RP 残缺） |
| Rust 驱动 | `call_method(gen, "__next__")` 一行 | 需自写迷你 asyncio 桥（每个环节都是风险） |
| 帧边界语义 | 跑到暂停点停、下次续——**与 await 等价** | 同左（协程 `send` 与生成器 `__next__` 是同一跳板） |

结论：yield 是**零协议开销、零依赖的等价暂停原语**；Web 的帧界来自
rAF（与 Python 侧写法无关）；pygame 用户心智是同步 `while True`（生成器
体内可原样保留）。async 不被排除——等 RP 协程机制成熟或自实现极小
`frame()` awaitable，可作语法糖叠加（`await frame()` ≙ yield），仍跑在
同一个 Rust 跳板上。

## 五、绑定面：sf 原生模块（Python ↔ Rust 唯一通道）

`rpy::sf::register(vm, &scope)` 把扁平函数注入脚本全局（pygame 原味）：
`fill / rect / flip / quit_requested`，全部走 starfish 公共 API
（pygame::display / event）。RP 0.6 原生函数签名惯例（实测坑位）：

- `Fn(原生参数…, &VirtualMachine) -> PyResult`，**&VirtualMachine 恒末位**；
- FromArgs 严格数值：f32 拒 int、i32 拒 float → **sf 面全参数定约 i32**
  （坐标/尺寸/颜色 0~255，pygame 原生习惯）；
- 参数元组宏上限 7；PyBaseException 无 Display（`repr(vm)` 转文本）；
- `game.call((), vm)` 创建生成器、`vm.call_method(&gen, "__next__", ())`
  步进。

后续扩展 = 往 sf 面加函数（或模块化注册 display/event/draw 子模块），
Python 侧零感知平台。

## 六、三端验证记录（2026-09-28，批次十八）

统一证据链：六标记（starting → set_mode OK → RP genesis 完成 →
executing script → generator created → entering frame loop）+ 无脚本异常。

| 平台 | 证据通道 | 结果 |
|---|---|---|
| Windows | 进程存活 + 桌面运行 | ✅ |
| Android 真机（nova 15/卓易通） | TCP devlog 六标记 | ✅ |
| Web（Edge，wasm32） | console 六标记 + next_frame 帧拍 | ✅（批次十六 RP-on-wasm 闭包崩溃未复现，销项） |
| 模拟器（x86_64） | adb logcat | ✅ 逻辑层取证可用（GLES 死穴照旧，仅挡渲染） |

## 七、与 winit 侵入式循环的差异（核心论证）

### 7.1 两种架构

**winit（控制反转 / 侵入式）**：

```rust
EventLoop::run(|event, _, control_flow| { ... });   // winit 持循环
```

- `EventLoop::run` ** owns 主循环**，宿主以回调（ApplicationHandler）
  被动接收事件；控制流在 winit 手里，宿主代码活在 winit 的调用框架内；
- macOS 后端强制主线程独占；`run()` 不返回（`ControlFlow::Exit` 即终局）；
- 宿主若想"每帧执行一段自己的逻辑"，只能在 RedrawRequested 等回调的
  缝隙里插入——**帧节奏由 winit 定义**。

**starfish-window（SDL 风格 / poll）**：

```rust
let mut window = Window::builder().build()?;
loop {                                  // 调用方持循环
    while let Some(e) = window.poll_event() { ... }
    // 任意帧逻辑
    surface.begin_frame(..); surface.present();
    next_frame().await;
}
```

- 引擎退化为**普通对象的方法调用**；没有 run、没有回调、没有控制反转；
- 帧节奏、循环位置、退出时机全部归调用方。

### 7.2 为什么这对脚本嵌入是"根因级"差异

脚本语言（CPython/RustPython）的世界观是**同步阻塞**的：
`while True: handle_events(); draw(); flip()`。把它放进 winit 的回调
宇宙只有两条路，都是死路/窄路：

1. **Python 持循环 → 事件拿不到**：Python 的 while True 阻塞在解释器里，
   winit 的事件循环永远得不到调度（事件泵在 winit 手里）；
2. **Rust 持循环 → Python 只能当回调**：每帧从 winit 回调里"借一步"
   调解释器——嵌入层被迫做成长驻回调机器，Python 的循环语义被拆碎，
   pygame 的 `while True` 心智无法保留。

poll 架构下这个两难**直接消失**：事件泵是普通函数调用，谁在循环里调用
它，事件就归谁。于是：

- 原生平台：Python `while True` 里调用的 `flip()/quit_requested()` 内部
  完成事件泵与上屏——**Python 持有主循环**（4.2 形态，SDL 原味）；
- Web 平台：rAF 是浏览器强加的帧界，用生成器门（4.1）把 Python 的
  while True 切成帧片段——Python 语义保留，平台约束被 Rust 壳吸收。

### 7.3 结论：核心问题解决了吗？

**是。** 立项根因即"旧架构（winit）引擎持循环、无法把运行循环交给脚本
语言"。当前架构在三个层面给出了实证：

1. **机制层**：循环归属调用方（poll 模型）——脚本需要的一切（事件、
   渲染、时间、音频）都是循环内的普通调用，不存在"控制权上交"；
2. **实证层**：同一份 Rust 壳 + 同一份 Python 脚本，Windows / Android
   真机 / Web 三端同日跑通生成器门（批次十八）；生成器门里 Python 每帧
   的 `fill/rect/flip` 就是游戏逻辑本体；
3. **语义层**：pygame 的 `while True` 心智在两种形态下都成立——原生
   平台逐字保留（4.2），Web 平台以 yield 为帧界等价表达（4.1）。

剩余的边界（非架构缺陷，如实记录）：Web 端受 rAF 约束只能用生成器门；
Android 端 android-activity 胶水对"进程复用 + 急速重启"有上游缺陷
（starfish-window CLAUDE.md 已记）；RP genesis 需 release 构建（栈深）。

## 八、红线与约定（沿用 + 新增）

- **依赖单向**：pygame-rs → starfish 单向；starfish 零 Python 资产；
- **RP 构建恒 release**（debug genesis 栈溢出）；
- **Android 打包**：必须走 aapt2 link 正规链（lib_name 与示例名一致），
  见 `reference/安卓APK打包lib_name坑与真机诊断手册.md`；启动路径禁止
  睡眠/长阻塞（横屏旋转 × 迟到 set_mode → Invalid surface）；
- **日志锚点定约**：嵌入示例保留 starting/set_mode/genesis/script/
  generator/frame_loop 六标记（文件 /sdcard/Download/<名>_log.txt +
  TCP devlog 双通道），三端排障共用同一套锚点；
- **Web 形态定约**：生成器门；原生平台可选 while True 归 rpy。

## 九、绑定 API 形态定约（v1.1 Surface 批次预定）

1. **绘制 = 单一会话显式形态（定案，用户决策 2026-09-28）**：

   ```python
   with screen.render() as r:
       r.rect((0, 200, 90), (x, 270, 60, 60))
       r.circle((255, 200, 0), (400, 300), 40)
   ```

   **不提供** `pygame.draw.*(screen, …)` 兼容形态——双形态在 Surface/MRT
   场景产生二义性（那边会话有自己的 batch，传 screen 即错），单形态从根
   上消除"画到哪"的歧义。取舍：放弃"真实 pygame 教程代码直接贴"的兼容
   红利，换类型安全与单一语义（pygame 兼容保留在数据类型/事件/模块组织
   层面，绘制入口统一走会话）。
2. **Surface 创建 = color + 可选 depth 整合体**（对齐 Rust 层
   `Surface::with_depth`）：`pygame.Surface((w,h), depth_buffer=True)`。
   ⚠️ 命名地雷：真实 pygame 的 `depth` 参数 = 每像素位数（bpp），绑定层
   **禁用 `depth=` 表达深度缓冲**，用 `depth_buffer=`；`has_depth` 在
   创建时定格，`render_depth()` 由 native 校验。
3. **set_mode 默认带深度缓冲**（Depth24Plus 全屏一张，成本可忽略）——
   `screen.render_depth()` 开箱即用；省内存场景经 set_mode_ex 旗标关。
4. **会话模型不锁高级功能（扩展性定约）**：`with` 只是**光栅 pass 的
   命令录制窗口**（对应 wgpu RenderPass）；Compute Pass 在 GPU API 世界
   观里是与 RenderPass 平级的兄弟概念。高级功能沿四条路径进入，不改会话
   语义：
   - 新 pass 类型（如 `with gpu.compute() as c: c.dispatch(...)`）；
   - 会话新方法（实例渲染 `push_instances` 等——与批处理模型天然契合）；
   - 材质/管线变体（体积云/模拟光追 = 自定义 WGSL 全屏 pass 或 compute；
     光探针 = render-to-texture 会话 + compute 烘焙）；
   - **低阶逃生舱**：`gpu` 直通模块（建着色器/storage buffer/裸
     dispatch）供高级用户——高层 pygame 风格与底层直通分层互不污染。
   反模式：把高级功能塞进会话的 `__enter__/__exit__` 语义。会话恒为
   "命令录制窗口"；资源句柄（Shader/StorageBuffer/Surface）走 M1 后半
   的 RP 句柄表。
