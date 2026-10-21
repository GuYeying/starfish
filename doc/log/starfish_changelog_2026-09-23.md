# Starfish 更新日志 2026-09-23

## 批次 19（2026-09-23）：Python 绑定抽离——pygame-rs 独立 crate（拓扑反转）

> 撤销批次 9 §八"不建新 crate"决策:starfish 内全部 pygame/Python 绑定资产
> （`src/pygame` 语义层 / `src/rpy` 内嵌 RustPython 壳 / `python/pygame`
> 纯 Python 兼容层 / 示例 / vendored RustPython）整体抽离为仓库根下独立
> crate `pygame-rs/`。starfish 回归零 Python 资产的纯引擎。

### 设计背景

- 批次 9~18 以"单 crate + feature 组件化"承载绑定层(`pygame`/`rpy`
  feature + cfg 纪律 + 依赖图卫生检查)。实际演进暴露三笔持续成本:
  ① rustpython 与 vendor/ 资产只有绑定层使用,却挂在 starfish 的
  Cargo.toml;② feature 门控需要 cfg 纪律与 CI 检查执法;③ 引擎本体与
  绑定层的发布节奏被锁死在同一 crate。
- 物理抽离后隔离不再需要执法(资产不在源码树内),starfish 默认特性图
  天然零 Python 依赖。
- 抽离前 `pygame-rs/` 已有一份当天新建的脚手架(GameBuilder/engine/registrar
  声明,不可编译、引用幻影模块)——按"迁入代码为准"决策废弃,以 starfish
  内已验证的 M1/M2 实现为迁移主体。

### 设计方案

- **迁移映射**(全部为移动,不改逻辑):`src/pygame/{mod,core,color,rect}.rs`
  → `pygame-rs/src/pygame/`;`src/rpy/{mod,py_src}.rs` → `pygame-rs/src/rpy/`;
  `python/pygame/__init__.py` → `pygame-rs/python/pygame/`;
  `examples/pygame_hello*` → `pygame-rs/examples/`;`vendor/` →
  `pygame-rs/vendor/`。相对布局保持 → `py_src.rs` 的
  `include_str!("../../python/pygame/__init__.py")` 路径不变。
- **构建关系**:独立 crate(根 Cargo.toml 保持单包,非 workspace)——
  自带 lockfile/target/.gitignore;`starfish = { path = ".." }` 单向依赖;
  pygame-rs 无 features(整 crate 即绑定层全量编译),`rpy` 模块沿用
  `#[cfg(not(target_arch = "wasm32"))]` 门控(M3 wasm 入口落 pygame-rs 时调整)。
- **代码改动仅两类**:① `crate::base::` → `starfish::base::`(core.rs 1 处 /
  color.rs 7 处 / rpy/mod.rs 9 处);② 拆 feature 门控(子模块 cfg、测试
  `cfg(all(test, feature = "rpy"))` → `cfg(test)`)。
- **依赖合并实例**:pygame-rs 声明 `wgpu = { version = "30",
  default-features = false }`(`RenderSurface::begin_frame` 直收
  `wgpu::Color` 字面量,base 的 `to_wgpu` 是 pub(crate) 拿不到)与
  `serde`/`serde_json`/`once_cell`(THECOLORS JSON 装载)——同版本需求与
  starfish 合并为同一 crate 实例,特性并集不受扰动。
- **构建期发现并修复**:示例脚本两处过期调用(`set_mode(640, 480)` 双 int
  → 元组;`e == "quit"` 字符串比较 → `Event.type` 判定)——批次 18 重做
  兼容层后脚本未跟跑;兼容层补 `set_title = set_caption` 别名(pygame
  窗口标题旧名)。

### 关键保证

- **starfish 零 Python 资产**:源码树无 pygame/rpy/python 路径;Cargo.toml
  无 rustpython 依赖与 pygame/rpy/launcher 特性——隔离从 cfg 纪律升级为
  物理隔离。
- **依赖单向**:`pygame-rs → starfish`,crate 内 `rpy → pygame →
  starfish::base`;base 零 pygame 反向依赖的既有纪律不变(HookApp/
  `with_any_thread_event_loop` 留在 base 作通用基建)。
- **兼容层单一源自包含**:`python/pygame/__init__.py` 随壳迁入 pygame-rs,
  include_str! 资产与 Rust 侧绑定代码一体。
- **回归**:pygame-rs `cargo test` **11 passed**(8 M2 行为 + 2 color 桥 +
  1 rect);`cargo run --example pygame_hello` 桌面链路实测(wgpu 装配 →
  640x480 窗口 → 120 帧循环 → exit 0);starfish `cargo test` **69 passed**
  (原 72 基线 − 随 pygame 层迁走的 3 个测试,总数守恒)、
  `cargo check --no-default-features` 与 `cargo check --examples` 均过。
- **已知既有行为(非本次引入)**:`base::app::run` 收尾 `std::process::exit`
  (app.rs:559-566)截断 rpy 侧"脚本执行完毕"收尾日志——脚本异常路径
  (exit 1)不受影响,行为测试覆盖。

### 文档同步

- `reference/pygame绑定层API设计稿.md` §五 六修:独立 crate 拓扑 + 新落位树;
  `reference/vendor与crates切换指南.md`:vendor 归属 pygame-rs/vendor/,
  验证锚点去 feature 化。
- CLAUDE.md 双层 API 节、README 架构树同步;后续里程碑 M3(wasm 入口)/
  M4(Android)/M5(launcher)/M6(模块导出)归属 pygame-rs,口径不变。

---

## 批次 20（2026-09-23）：pygame-rs 内部重排——src/rpy/dependencies/ 注入源码区 + 双绑定架构定案

> pygame-rs 内部布局重排,目标是开发者一眼看明白:Rust 代码在哪、绑定在哪、
> 如何绑定。同批定案**双绑定模块**架构(Python 侧两种使用形式)。

### 设计背景

- 抽离后(批次 19)的布局是 `src/{rpy,pygame}` + `python/pygame/` +
  `vendor/` 三处散放——纯 Python 绑定资产、第三方解释器源码与 Rust 代码
  的归属关系不直观。
- 用户新架构决策:后续还要绑定 starfish 底层原生接口,Python 侧存在两种
  使用形式,绑定模块会有两个。

### 设计方案

- **双绑定模块定案**:① `pygame` 绑定 = 状态机形式(`pygame.run` 生成器
  门,现有兼容层);② `starfish` 绑定 = 纯引擎原生接口形式(无内置状态机,
  M6+ 落地)。层级规则:**pygame 模式下可用 starfish;纯 starfish 模式下
  不推荐再用 pygame 类型**(对应设计稿 §六 面 A/面 B 的进一步明确)。
- **目录拓扑(用户定案 `src/rpy/dependencies/`)**:壳要注入解释器的绑定包
  与第三方源码统一挂在 rpy 模块下——
  `src/rpy/dependencies/{pygame/,starfish/,RustPython-main/}`;`src/` 保持
  纯 Rust。pyi 契约桩延后至 M6 原生面拆分批次(starfish 先放占位
  `__init__.pyi` 标归属)。
- 移动:`python/pygame/__init__.py` → `src/rpy/dependencies/pygame/`;
  `vendor/RustPython-main` → `src/rpy/dependencies/RustPython-main`。
  引用重指:`py_src.rs` include 路径、`Cargo.toml` 两行 rustpython path、
  `lib.rs`/`py_src.rs` 文档注释。
- 实施纠错:vendor 整体 mv 时产生
  `RustPython-main/RustPython-main/` 双层嵌套(目录名带版本后缀的典型
  陷阱),cargo path 解析失败暴露后拍平。

### 关键保证

- **一眼可读**:`src/` = Rust 代码;`src/rpy/` = 壳(如何绑定);
  `src/rpy/dependencies/` = 绑定包(pygame 现有、starfish M6+)+ 第三方
  (RustPython-main)。
- **回归**:pygame-rs `cargo test` 11 passed 不变(含 include_str! 新路径
  下的兼容层全链路行为测试)。
- M5 wheel 打包时 `dependencies/{pygame,starfish}` 即包目录的直接来源;
  真实 .pyi 契约桩(`pygame/__init__.pyi`、`_native.pyi`、starfish 分模块
  镜像)随 M6 建。

---

## 批次 21（2026-09-23）：pygame-rs 全平台验证矩阵(编译级)——win 实跑 + wasm/android 编译 PASS

> 重排后的架构首次全平台摸底:能实跑的实跑,能编译的编译,受阻的记录
> 受阻原因。结论:**代码侧零改动需求**,Web/Android 运行能力差的是 M3/M4
> 入口(既定里程碑),Linux/macOS 差的是本机交叉环境。

### 验证矩阵(cargo check --target <triple>,于 pygame-rs;范围 = Windows/Web/Android,Linux/macOS 用户决策暂缓)

| 平台 | 编译 | 运行 | 说明 |
|---|---|---|---|
| Windows x64(native) | PASS | **PASS** | 11 测试 + pygame_hello 实跑(窗口/渐变/ESC) |
| Web wasm32-unknown-unknown | **PASS** | M3 未建 | rpy 在 wasm cfg 关闭;rustpython(wasmbind) 参与编译——批次 16 结论复验成立 |
| Android aarch64(API 26) | **PASS** | M4 未建 | rpy **参与编译**;NDK clang 环境变量手设(CC/AR 指向 NDK 30 包装器)——cargo-ndk 只在 xtask 管线内代设 |
| Linux x86_64 | 暂缓 | 未测 | 用户决策(09-23):暂不考虑。受阻点 = gstreamer 系统包需 Linux 实机,非代码问题 |
| macOS aarch64 | 暂缓 | 未测 | 用户决策(09-23):暂不考虑。受阻点 = psm 需 apple 交叉 CC(osxcross),iOS 同;非代码问题 |

### 关键保证与后续

- **pygame-rs 源码全平台可编译性成立**(本机可验证范围内零代码错误);
  差距全部是里程碑(M3 wasm 入口/M4 Android 入口)或环境(交叉工具链/
  系统包),不是架构问题。
- Android 直查手法入库:手设 `CC_aarch64_linux_android=<ndk>/…/aarch64-
  linux-android26-clang.cmd` + `AR_…=llvm-ar.exe` 即可绕过 cargo-ndk 做
  裸 cargo check(M3/M4 落地期间的高频验证手段)。
- 下一步候选:M3 wasm 入口(spawn_local + thread_local INTERP/GEN 槽)→
  浏览器实测;M4 Android 入口(app_entry 捕获 + xtask APK)。

---

## 批次 22（2026-09-23）：M3+M4 落地——pygame-rs Web/Android 运行入口,三平台可测

> Web/Android 从"编译通过"到"有运行入口、可交付实测"。M3 = wasm 入口
> (fetch 装载 + thread_local 槽);M4 = Android 入口(内嵌脚本 + xtask
> --dir 出包)。产出:`pygame-rs/web/`(浏览器测)+ `target/android-apk/
> pygame_hello_android.apk`(29.4MB,签名通过)。

### 设计方案

- **M3 wasm 生命周期(核心)**:`base::app::run` 在 web 上 spawn_local 后
  立即返回且要求 `Application + 'static` → HookApp 三闭包必须零捕获。
  解释器/生成器分两槽:`WASM_GAME`(exec 期间 f_run_start 短借写入)、
  `WASM_INTERP`(exec 完成、run 返回后才装入,与 on_frame 借用不交叠)。
  GPU 装配 web 为 async(`RenderEntry::async_new` + spawn_local,Window
  可克隆),槽就位前 present_frame 自行跳过;清屏+present 提炼
  `fn present_frame()` 两平台共享。`WindowConfig` wasm 段加
  `.with_web_canvas_id("canvas")` 接管页面画布。
- **入口拆分**:`run_script(path)`(native fs 读 / wasm io fetch)→
  `run_source(source)`(native 大栈线程 engine_main / wasm wasm_init);
  `run_source` 供 Android 内嵌脚本(`include_str!`)。rpy 模块门控
  `#[cfg(not(wasm32))]` 撤除(条件编译收敛到"谁来驱动"的边界)。
- **示例三平台一体**:examples/pygame_hello.rs 同源注册两条
  [[example]](桌面 bin + `pygame_hello_android` cdylib),cfg 收敛在
  android_main / wasm start 两个入口函数。
- **M4 xtask `--dir <path>`**:main() 启动处 `set_current_dir` 一处生效
  (管线全程 cwd 相对路径),其余零改动即可服务兄弟 crate;
  `cargo xtask android pygame_hello --dir pygame-rs`。
- **vendor 局部修改(用户决策"剔除 C 依赖,不用 ctypes")**:android 链接
  缺 `-lffi`(NDK 无 libffi;libffi 来自 rustpython-host_env 的 target 段
  非 optional 依赖,此前 check 不链接故未暴露)。两处对称最小编辑:
  `host_env/Cargo.toml` libffi 目标段 any() 剔除 android + `lib.rs`
  `pub mod ctypes` 加 `#[cfg(not(android))]`(内部标注批次号)。
  桌面 Windows/Linux/macOS 的 ctypes 不受影响;vm 的 _ctypes 模块本就
  被 feature "host_env" 门控(未启用)。
- **已测锚点**:web headless 控制台四连(init/set_mode/run 启动/脚本执行
  完毕·槽续驱动)+ 截图像素实锤(canvas (28,26,43) = 渐变清屏色 vs 页面
  背景 (13,15,20));桌面 11 测试 + 6s 存活回归;wasm/android check 复验。

### 关键保证

- **三平台同一份应用代码**(示例零平台分支,差异收敛在入口 cfg);
  引擎循环三平台同构(start 门 GPU 装配 / event 事件翻译 + Resized 自愈 /
  frame 生成器步进 + present)。
- **已知边界**:web 上脚本结束 → 循环退出 → starfish web 尾部
  unreachable!(app.rs:550-554,页面同寿命语义,渲染/交互不受影响);
  wasm 无大栈线程,RP 递归深度受 wasm 栈限制(hello 浅调用无碍,深递归
  脚本的栈深行为留待后续观察)。

### 测试交付物

- Web:`cd pygame-rs/web && python -m http.server 8000` →
  `http://localhost:8000/pygame_hello.html`(出包命令见 html 头注释)。
- Android:`pygame-rs/target/android-apk/pygame_hello_android.apk` 直接
  安装;或接设备后 `cargo xtask android pygame_hello --dir pygame-rs`
  自动安装启动。退出 = ESC 触屏不可用 → 杀进程/窗口关闭(返回键)。

---

## 批次 22·续（2026-09-23）：android 闪退三连修——TLS/惰性装配/Resized 闸门

> 首版真机闪退,经 x86_64 模拟器复现+logcat 定位,修掉两层问题;第三层
> (模拟器 GL 表面)经对照实验定性为引擎侧既有问题,真机待验。

### 修复一:TLS 键耗尽(真机闪退根因)

- 症状:`fatal runtime error: out of TLS keys, aborting`,崩在 RP genesis
  (`Context::init_genesis` → `ExceptionZoo::init`)。
- 根因:android bionic 的 pthread key 池上限 1024;rustpython-common 的
  `static_cell::non_threading` 把**每个内建类型**做成一个 thread_local!
  静态,每个吃一个 pthread key,genesis 初始化类型群时即耗尽。链接期
  check 不暴露(仅运行期),平台编译矩阵因此漏检。
- 修复(残余侵入最小方案):**大栈线程仅在桌面使用**——android 直接在
  android_main 线程跑 `engine_main`(与 app_entry 惯例同构,顺带修好
  JNI 线程亲和),仍不够,最终在 `static_cell.rs` 加 android 专用分支:
  StaticCell 改全局 `UnsafeCell` + `unsafe Sync`(嵌入契约 GIL 单线程 +
  每进程单解释器,per-thread 缓存与全局缓存语义等价)。
- **语义红线**:桌面/网页保持原 thread_local 实现——每线程独立 genesis
  的隔离语义是行为测试集(并行测试)的依赖,全局化会使 11 测试中的 6 个
  交叉失败(实测踩过)。

### 修复二:android GPU 惰性装配

- 症状:TLS 修复后第二层崩——wgpu `Surface::configure` 校验 panic
  ("Invalid surface"),ANativeWindow 就绪晚于 start 门(SurfaceCreated
  异步),`about_to_wait` 的即时装配必踩空;且 panic 发生在 pollster 为
  Send future 起的线程上,catch_unwind 拦不住,block_on 挂死。
- 修复:on_start 不装配;以**首个 Resized 事件**为表面就绪闸门
  (AppCore 加 `resized_seen`,on_event Resized 置位),on_frame 每 15 帧
  重试装配,成功后走正常帧路径。与 web 的异步装配同构。

### 对照实验与遗留

- **模拟器对照实验**:`cargo xtask android probe_gfx --abi x86_64` 在同
  一模拟器上同样炸在 `Invalid surface`、同一条 about_to_wait →
  RenderEntry::new 路径——**starfish 引擎在模拟器 GL(SwiftShader 翻译层)
+ wgpu GLES 组合下的表面问题,非 pygame-rs 抽离引入**。真机 GPU/EGL 环
  境不同,惰性装配后表现待真机验证。
- 引擎侧遗留(记入后续工作):android 表面生命周期(start 门早于
  SurfaceCreated)的通用解法应在 base 层做——如 start 门延迟到首个
  Resized/表面就绪回调,probe 家族同步受益。

---

## 批次 23（2026-09-23）：rpy 统一运行模型——三平台一条 exec 路径、一个帧管线

> 把 M1~M4 期间分叉出的三种驱动形态收拢为**一个运行模型**:一条 exec
> 路径、一个帧管线、三个入口差异点(装载源/引擎线程/GPU 装配同步性,
> 全部为 OS 给定差异)。桌面/android/web 三平台回归全绿。

### 统一模型

- **解释器/生成器槽全平台化**:`WASM_INTERP/WASM_GAME` →
  `INTERP_SLOT/GAME_SLOT`(无 cfg)。解释器建好即入槽,帧回调经槽
  零捕获驱动('static)——web 的槽模式推广为全平台,native 的
  "闭包借用局部 vm/game_cell" 分叉删除。
- **engine_main 三平台同一函数**:`wasm_init` 并入,原
  `#[cfg(not(wasm32))]` 门控撤除。桌面/android:exec 内阻塞进引擎
  循环;web:run 立即返回,循环由 rAF 经槽续驱动。
- **统一帧管线(单一 on_frame 闭包)**:`ensure_gpu → step → present`。
  ensure_gpu 惰性:桌面首帧即成功;android 以首个 Resized 为闸门
  (resized_seen + 每 15 帧重试 + catch_unwind 吞未就绪期 wgpu 校验
  panic);web 在 start 门异步发起。三种装配姿势收敛为一个入口。
- **线程策略(唯一保留的实现差异,OS 给定)**:桌面 = 专用引擎线程
  `ENGINE_STACK_BYTES = 16 MiB`;android = 入口胶水线程(平台给定
  2 MiB);web = 单线程。T9 数据背书:每 Python 帧 native ≈ 400 B,
  16 MiB ≈ 4 万层、2 MiB ≈ 5000 层,默认 limit 1000 均有充足余量;
  256MB 迷信值退役。
- run_script/run_source 装载分派不变(fs / 内嵌 / fetch)。

### 回归与交付

- 桌面:11 测试全绿 + hello 实跑(新锚点"GPU 装配完成"出现);
- web:统一模型重建 + headless 冒烟五锚点(含 GPU 装配完成);
- android:x86_64 模拟器结构性验证(三锚点 + 重试闸门 + 进程存活;
  表面仍为模拟器 GL 遗留问题,probe_gfx 对照已定性)+ arm64 真机包
  重建交付(29.3MB,签名通过)。
- 栈数据/诊断手法沉淀:`examples/stack_probe.rs` +
  `rpy::exec_probe` 钩子保留;`reference/android绑定运行问题与解决
  方案.md` 附录 T9 数据表。

---

## 批次 24（2026-09-23）：T9 深递归实验 + 目录拓扑三版——绑定包提顶层、pyi 桩落地

> 两件事:① 用 stack_probe 在三平台量出深递归栈的真实边界,回答
> "android 2 MiB 够不够、能不能调大";② 按用户检查后的目录意见完成
> 第三版拓扑——绑定包提顶层(py+pyi),dependencies 只留解释器源码。

### T9 深递归实验(数据详见解决方案文档附录)

- 钩子:`rpy::exec_probe`(当前线程执行,recursion_limit 可配——它是
  `VirtualMachine` 运行期 Cell,release 默认 1000/debug 256,
  `sys.setrecursionlimit` 同源);探针 `examples/stack_probe.rs`
  (栈 MB / 递归深度 / limit 三参数)。
- 关键数字(release):每 Python 帧 native ≈ **400 B**;可用深度 ≈
  (栈 − 64 KiB 余量)/400 B。2 MiB → ~5000 层(✓ 实测);16 MiB →
  ~4 万层(✓);32 MiB → ~8 万层(✓);64 MiB 冲击百万层 → 干净
  RecursionError,**全矩阵零真爆栈**(native 栈感知守卫,psm 读 SP vs
  软限)。
- **android 判定**:胶水线程 2 MiB + 默认 limit 1000(≈400 KB native)
  ——安全余量约 5 倍,**无需调大**;超 1000 层 → RecursionError,与
  CPython 同语义。调大路径(如需):入口自 spawn 显式栈引擎线程
  (待验证 JNI attach),胶水线程本身无旋钮。
- 方法论:深度 N 需 limit > N+基线帧(差一误判踩两次);空消息异常
  必须报类型名(err_str 已改"类名: 消息")。

### 目录拓扑三版(用户定案)

```text
pygame-rs/
├── pygame/            # 绑定① python 模块(py + pyi 桩)
│   ├── __init__.py / __init__.pyi / _native.pyi
├── starfish/          # 绑定②(M6+,占位 __init__.pyi)
├── src/rpy/           # 壳:具体绑定(mod.rs / py_src.rs)
│   └── dependencies/RustPython-main/   # 第三方解释器源码
└── examples/
```

- 变化:绑定包(py)从 `src/rpy/dependencies/` 提到 **crate 顶层**,
  pyi 契约桩同步落地(`pygame/__init__.pyi` 模块面 +
  `_native.pyi` 面 A 契约);dependencies/ 只剩解释器源码。
- 引用重指:py_src.rs include 路径(../../pygame/)、lib.rs/文档。
- M5 wheel 打包时顶层 pygame/、starfish/ 即包目录直接来源。

### 回归

- 桌面 11 测试全绿(含新 include 路径);wasm/android 编译 0 error;
- web 包与 arm64 APK 按新拓扑重建交付。

---

## 批次 25（2026-09-23）：兼容层解构——绑定子模块 + bindings.rs 通用注册表

> py_src.rs(单文件模块图 + 单文件兼容层)按"各自绑定模块"解构:
> 兼容层拆为 display/event/draw/time 绑定子模块(实现 + pyi 各自成桩),
> py_src.rs 改名 **bindings.rs**(通用绑定注册表,只留表项——正是
> "通用绑定入口"的归宿)。

- **兼容层解构**:`pygame/__init__.py` 只留常量/Event/init 阶梯/生成器
  门;display.py(set_mode 隐式 init)/event.py(Event 构造)/draw.py/
  time.py 各自成模块;pyi 桩同步(display/event/draw/time 各自带桩,
  `__init__.pyi` 去掉包装类)。
- **装载器新序(CPython 同序)**:原生面 → 包模块注册进 sys.modules
  (**先注册后执行**)→ 子模块逐个 exec+注册+挂载到包命名空间 →
  包根源码 exec。子模块 `import pygame` 命中已注册包,跨模块引用
  (Event/init)在调用期解析;`_native` 逐 dict 预注入,零 import 契约
  不变。
- **bindings.rs**:`PY_PACKAGE_ROOT`(包根,最后执行)+ `PY_SUBMODULES`
  (子模块,表序执行)两张表——新增绑定 = 加表项,装载器零改动;
  未来 starfish 绑定包 = 再加一对表。
- 回归:11 测试全绿(t2/t3/t4 直击解构后 event/draw 的跨模块路径)+
  桌面 hello 实跑 4 锚点;wasm/android 包按新结构重建交付。

---

## 批次 26（2026-09-23）：pygame 绑定全原生化——.py 层退役，.pyi-only 接口面

> .py 转发层(一行 Python 调一个原生函数)整体吸收进 Rust 原生面:
> **pygame 模块本身即原生面**,`_native` 桥消失;目录里 pygame/ 只剩
> .pyi 接口桩。脚本侧行为不变(import pygame → 全部 API 同形)。

- **Event 原生类**:动态堆类型方案——`type("Event", (), {__init__:
  Rust 函数, __repr__: Rust 函数})` 运行期建类(静态类型机制需
  'static Context,不可用);实例由壳 `PyRef::new_ref` + `set_attr`
  填充 type/key/pos;repr 走 `Representable` 槽;类对象存 OnceLock?
  否——static_cell 幂等查复用(RP per-thread cell,天然并行测试安全)。
- **bindings.rs 重写为原生注册**:常量(宏批量 set_attr)/Event 类/
  顶层函数(init 幂等/get_init/quit/run)/四个原生子模块
  (display:set_mode 收元组+隐式 init;event:get 直产 Event 实例;
  draw/time 透传)。子模块=原生模块注册进 sys.modules + 挂包命名
  空间;名字一律 `vm.ctx.new_str` 物化后传引用(AsPyStr 只收
  'static str / Py<PyStr>)。
- **.py 层退役**:pygame/{__init__,display,event,draw,time}.py 五个
  文件删除;`_native.pyi` 删除(原生面即公开面);t7 测试删除(HDR
  往返已由 color.rs base_bridge_tests 覆盖)。
- **RP API 实录(踩坑记录)**:new_module 返回 PyRef<PyModule>(
  .into() 转 PyObjectRef);dict.get_item 返回 PyObjectRef(非
  Option/Result);set_attr 名字参数走 AsPyStr(只收 'static str 或
  Py<PyStr> 引用,非 'static &str 须先 new_str);new_function 的
  __init__ 不自动补 self(RP 非 CPython 描述符协议)——故 Event
  走"空参创建+事后填属性"。
- 回归:10 测试全绿(t7 删除)+ 桌面 hello 4 锚点;wasm/arm64 包重建。

### 附录:最终落位——绑定实现下沉 dependencies/<包名>/

- **BindingModule 入口标准(bindings.rs)**:`name()` + `register(vm)`
  两方法 + `BINDINGS` 静态表 + `register_all` 总入口——新增绑定包 =
  dependencies/ 加目录 + 实现 trait + 表里加一项,壳零改动。
- **pygame 绑定实现整体下沉 `src/rpy/dependencies/pygame/`**:注册逻辑
  + 常量/Event 类/全部 f_* 原生函数 + Event 类构建,一个文件收拢;
  mod.rs(壳)只留引擎服务(槽/帧管线/GPU 槽/gpu_install·gpu_resize
  助手,均 pub(crate))。
- 顶层 `pygame/` 与 `dependencies/pygame/` 的分工:前者 = .pyi 接口桩
  (给脚本开发者),后者 = Rust 绑定实现(给绑定维护者)。

---

## 批次 27（2026-09-23）：.py 层删除落地——pygame/ 目录收敛为纯 .pyi 接口面

> 批次 26 收尾时 .py 文件删除步骤在编译迭代中遗漏(文件仍在盘上)。
> 本批目录检查发现后补删,并完成接口面最终确认。

- **删除**:`pygame/{__init__,display,event,draw,time}.py` 五个 .py 实现
  文件 + `_native.pyi`(原生面即公开面,桥不存在故桩也不需要)。
- **删除前核验**:grep 全源码确认零引用(bindings.rs 已全原生注册,
  include_str! 不再指向任何 .py)。
- **lib.rs 文档修正**:过时的 "py_src/include_str! 兼容层嵌入" 表述
  更新为全原生化表述(常量/Event/函数/子模块由 Rust 构建注册,
  `pygame/*.pyi` 仅为接口契约桩)。
- **回归**:10 测试全绿(删除后重跑确认,行为零变化——运行时本就不
  加载这些文件)。

### 最终形态(接口面)

```text
pygame/
├── __init__.pyi    # 常量 / Event / init 阶梯 / run
├── display.pyi     # set_mode / set_caption / set_title / flip
├── event.pyi       # get()
├── draw.pyi        # clear()
└── time.pyi        # get_ticks()
```

对脚本开发者:`pygame/*.pyi` 即全部接口文档(IDE 补全/类型检查),
实现细节全在 Rust(`src/rpy/bindings.rs`)。已交付的 web 包与 APK
不受影响(删除的是运行时本就不引用的死文件)。
