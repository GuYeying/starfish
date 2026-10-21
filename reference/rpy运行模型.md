# rpy 统一运行模型(详解)

> 面向:想理解 pygame-rs"引擎怎么跑 Python 脚本"的开发者。
> 关联:设计稿 §五(拓扑)/§六(原生面);`reference/android绑定运行问题
> 与解决方案.md`(平台坑位与栈数据);changelog 批次 22/23。

---

## 一、一句话模型

> **引擎(starfish)负责开窗口和画图,RustPython 负责执行你的 Python
> 脚本,两者被"焊接"在同一条线程上;你的脚本每画完一帧就 `yield`
> 暂停,引擎画完这一帧再唤醒你——如此循环,直到脚本 return 或窗口关闭。**

三个角色:

| 角色 | 是谁 | 类比 |
|---|---|---|
| 脚本(game.py) | 你的游戏逻辑,`yield` 一帧一停 | 演员演完一幕就下台等信号 |
| 解释器(RustPython) | 在 Rust 里执行 Python 代码的虚拟机 | 同声传译 |
| 引擎(starfish/winit/wgpu) | 窗口、事件、每帧渲染 | 剧场:开灯、报幕、每秒 60 次喊"下一幕" |

---

## 二、运转全程(桌面 10 步,android/web 只换三个"接头")

```text
① cargo run → main() → run_script("...py")
② 读脚本文件 → run_source → 起一条 16 MiB 栈的"引擎线程"
③ 引擎线程:创建 Python 解释器 → 放进 INTERP_SLOT(储物柜)
④ 预热 encodings → 把 "pygame" 模块注入 sys.modules(import 有答案了)
⑤ 执行脚本(作为 __main__):
     pygame.init()         → 记账(initialized = true)
     display.set_mode(...) → 记账 (640, 480)
     def game(ctx): ...    → 只是定义,不执行
⑥ 最后一行 pygame.run(game) → 原生函数 f_run_start:
     - 调 game(None) → 得到"生成器"(剧本到手,还没开演)
     - 生成器放进 GAME_SLOT(第二个储物柜)
     - 组装 HookApp{on_start, on_event, on_frame} 三个回调
     - 调 starfish 的 run() → winit 事件循环启动 → 【阻塞在这】
⑦ winit 每帧喊 on_frame,三拍:
     ensure_gpu(GPU 没装就装,装过跳过)
     step(唤醒生成器:从上个 yield 跑到下个 yield——
          期间脚本读事件、调 draw.clear 设置清屏色)
     present(读 AppCore 清屏色,真正画一帧)
⑧ 脚本就这样"跑一段→停→被唤醒→再跑一段",每秒约 60 次
⑨ 退出:ESC → 脚本 return → 生成器结束(StopIteration)→ 引擎退出
   (窗口 × 同样兜底,不依赖脚本)
⑩ process::exit(0) 收账
```

### 三平台只有三个"接头"不同

| 接头 | 桌面 | Android | Web |
|---|---|---|---|
| ① 脚本从哪来 | 读磁盘 | `include_str!` 烧进 APK | HTTP fetch 下载 |
| ② 跑在哪条线程 | 自起 16 MiB 引擎线程 | android_main 胶水线程(2 MiB) | 浏览器唯一线程 |
| ③ GPU 何时能装 | 首帧就绪,立即装 | 等系统挂窗口(首个 Resized) | adapter 异步应答 |

---

## 三、如何收拢的(之前 vs 现在)

收拢前,三个平台各有一套"每帧干什么"的闭包——同样逻辑写三遍,
android 的表面 bug 就是从分叉里长出来的。收拢的关键洞察:

> **web 必须用"槽"(thread_local)才能让回调找到解释器——那桌面和
> android 为什么不也用槽?**

于是把 web 的槽模式推广为全平台统一做法:

- 收拢前:桌面回调借用入口函数的局部变量、web 走槽、android 又一种
  ——三个 `on_frame` 变体;
- 收拢后:解释器和生成器**建好就放进槽**,三平台回调从同一个槽取
  ——**一个 `on_frame` 闭包走天下**;
- 顺带删除:`wasm_init`(并入 `engine_main`)、256MB 迷信值(换成 T9
  实测背书的 16 MiB 常量)、三种 GPU 装配写法(收进 `ensure_gpu_lazy`)。

**为什么这么设计(取舍的"因为")**:

1. **为什么解释器和引擎在同一条线程**:RustPython 解释器是"不可搬运"
   的(Rust 术语 `!Send`)——在哪个线程创建就必须在哪个线程使用。所以
   不是"解释器去适应引擎",而是"引擎循环搬到解释器所在的线程"。
2. **为什么用槽而不是 Mutex/全局变量**:槽 = "属于当前线程的储物柜",
   单线程访问不需要锁(Mutex 对单线程是无意义开销),还绕开了"对象不能
   跨线程"的类型限制。
3. **为什么脚本要写 `yield`**:经典 pygame 是"你的代码持循环"
   (`while True`),我们的引擎持循环。折中:你的函数写 `yield`
   (="这帧我完了"),引擎每帧喊你一次。这就是与经典 pygame 唯二的
   机制差异之一(`yield` + `run`)。
4. **为什么 GPU 装配是惰性的**:android 的窗口表面是系统**异步**塞过来
   的(SurfaceCreated),Web 的 GPU 适配器是**异步**应答的——凡是系统
   异步给的东西,一律"等就绪事件再拿,没就绪就跳过本帧",绝不在启动
   那一刻假设它已就绪。

---

## 四、目录拓扑(批次 24 定案)

```text
pygame-rs/
├── Cargo.toml                # rustpython path 依赖指向 src/rpy/dependencies/
├── pygame/                   # 绑定① 接口面(.pyi only,零 .py——批次 26 全原生化)
│   ├── __init__.pyi          #   包根契约:常量/Event/init 阶梯/生成器门(run)
│   ├── display/event/draw/time.pyi  #   子模块契约
│   └── _native.pyi           #   (已删除:原生面即公开面)
├── starfish/                 # 绑定② 纯引擎原生接口(M6+,占位桩)
│   └── __init__.pyi
├── src/
│   ├── pygame/               # Rust 语义层(状态机/Color/Rect)
│   └── rpy/                  # 壳:具体绑定
│       ├── mod.rs            #   引擎主程/槽/帧管线/原生函数
│       ├── bindings.rs       #   原生绑定注册(常量/Event/函数/子模块构建)
│       ├── event.rs          #   PyEvent 原生类(动态堆类型)
│       └── dependencies/
│           └── RustPython-main/  # 第三方解释器源码(路径依赖)
└── examples/                 # 三平台 hello + stack_probe 栈实验
```

- `pygame/`、`starfish/` 顶层包 = 脚本 `import` 的对象(绑定);
- `src/rpy/` = 壳(如何绑定),`bindings.rs` = 通用绑定注册表;
- `src/rpy/dependencies/` = 第三方(解释器源码);
- M5 wheel 打包时,顶层 `pygame/`、`starfish/` 即包目录直接来源。

---

## 五、模块清单(统一模型涉及的全部构件)

### `src/rpy/mod.rs`(壳)

| 构件 | 类别 | 作用 |
|---|---|---|
| `INTERP_SLOT` / `GAME_SLOT` | thread_local 槽 | 解释器槽 / 生成器槽——回调与入口之间的储物柜 |
| `GPU` | thread_local 槽 | GPU 三件套槽(装配后才有) |
| `struct Gpu` | struct | RenderContext + 资源访问 + 表面 |
| `pub run_script(path)` | fn 入口① | 桌面读盘 / web fetch |
| `pub run_source(src)` | fn 入口② | 内嵌源码;含线程策略 cfg(桌面 spawn 16 MiB / android 直跑 / web 直调) |
| `const ENGINE_STACK_BYTES` | 常量 | 桌面引擎线程栈 16 MiB(T9 数据) |
| `fn engine_main(source)` | fn 核心 | 三平台同一:建解释器入槽 → 预热 → 注册 → 执行 |
| `fn exec_user_script(vm, src)` | fn | 编译脚本为 `__main__` 并运行 |
| `fn f_run_start(game, vm)` | fn 原生面 | `pygame.run` 真身:造生成器入槽 + 组装 HookApp + 启动循环 |
| `fn ensure_gpu_lazy(window)` | fn(原生) | 惰性 GPU 装配(android 带 Resized 闸门 + catch_unwind) |
| `fn assemble_gpu(window)` | fn(原生) | RenderEntry 装配 → GPU 槽 |
| `fn step_via_slots()` | fn | 从槽步进生成器一帧(全平台同构) |
| `fn present_frame()` | fn | 清屏 + present(GPU 槽空则跳过) |
| `f_init / f_get_init / f_quit / f_run_start / f_set_title / f_flip / f_draw_clear / f_ticks` | fn 原生面(在 mod.rs) | 注册给 Python 的函数本体(bindings.rs 引用) |
| `f_display_set_mode / f_event_get` | fn 原生面(在 bindings.rs) | set_mode 收元组+隐式 init;event.get 直产 Event 实例 |
| `fn run_headless` + tests | 测试 | 无头模式 + 8 个行为测试 |
| `pub exec_probe` | 实验钩子 | T9 栈实验用(当前线程 + 可配 recursion_limit) |

### `src/rpy/bindings.rs`(原生绑定注册)

| 构件 | 作用 |
|---|---|
| `pub fn register(vm)` | 原生绑定注册:常量/Event 类/函数/子模块/挂 sys.modules(全原生,零 .py) |

### `src/rpy/event.rs`(批次 26)

| 构件 | 作用 |
|---|---|
| `#[pyclass] PyEvent` | Event 原生类:动态堆类型 + pygetset(type/key)+ Representable(repr) |
| `make_event_class(vm)` | `type("Event", (), {__init__/__repr__: Rust 函数})` 运行期建类 |
| `make_event(vm, class, typ, key)` | 空参创建实例 + set_attr 填属性 |

### `src/pygame/core.rs`(语义层状态机)

| 构件 | 作用 |
|---|---|
| `struct AppCore` + `thread_local APP` | 记账本:init/set_mode/标题/清屏色/事件缓冲/帧数/resized_seen |
| `fn with_app(f)` | 打开记账本 |
| `fn push_window_event(e)` | 引擎事件 → Python 事件入缓冲 |
| `fn pygame_code(KeyCode)` | 引擎键码 → pygame `K_*` 码 |
| `EngineMode` / `PyEvent` | 模式(真实/无头)与结构化事件 |

### 顶层 `pygame/`(绑定①)与 `starfish/`(绑定②)

| 文件 | 作用 |
|---|---|
| `pygame/__init__.pyi` + display/event/draw/time.pyi | 接口契约桩(IDE/文档;无 .py 实现) |
| `starfish/__init__.pyi` | M6+ 占位 |

### 依赖的 starfish 引擎侧构件(既有,未改)

`base::app::run`(三平台事件循环)/`HookApp` 三回调/`WindowConfig`
(canvas 接管等)/`RenderEntry`(GPU 装配)/`RenderSurface`(begin_frame
/present)/`io::read_text`(web fetch)/`debug::console_log` 与 panic 钩子。

---

## 六、一帧的生命周期(最细粒度)

```text
winit: "该画帧了"
 → on_frame:
    ① GPU 槽空?→ 尝试装配(android 看闸门;装不上跳过绘制)
    ② 唤醒生成器:
         脚本从上个 yield 继续
         → event.get()  → 原生 event.get → Rust 排空缓冲直产 Event 实例
         → draw.clear() → 只是记账(改 AppCore.clear_color)
         → flip()       → no-op(真上屏在 Rust 侧)
         → yield        → 脚本暂停,控制权回 Rust
    ③ 读清屏色 → wgpu 清屏 → present 上屏
 → winit 等下一个 vsync(~16.7ms 后重复)
```

退出路径:生成器 return(StopIteration)或窗口关闭 → `ctx.exit()` →
循环结束 → 桌面/android `process::exit(0)`;web 循环与页面同寿命。

---

## 七、递归/栈语义(T9 实测,数字背书)

- 每 Python 帧 native 栈成本 ≈ **400 B**(release;debug 更重);
- RP 有 native 栈感知守卫:SP 接近栈底+余量(64 KiB)→ 干净抛
  `RecursionError`,**绝无段错误**;
- 可用深度 ≈ (栈 − 64 KiB)/400 B:2 MiB → ~5000 层;16 MiB → ~4 万层;
- android 胶水线程 2 MiB + 默认 limit 1000 → 安全余量约 5 倍;
  更深需求 → 调 `sys.setrecursionlimit` 或引擎线程加大栈。
