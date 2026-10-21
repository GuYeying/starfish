# Starfish 更新日志 2026-09-22

> 本日主题：**Python 绑定层立项、五轮架构收敛与 M0~M2 落地**——双解释器
> 定向（RP 内嵌主运行时 + CPython launcher）、pygame 语义层独立 feature、
> Surface 三态统一架构、M0 spike 决策门放行、M1 桌面 hello 全链路、
> M2 行为回归 71 测试、M3 wasm 编译打通。
> 批次编号承接 09-20 日志（批次 9~16）。

---

## 批次 9：Python 绑定层立项交接——双解释器定向（用户决策）

> base 全模块交付 + API 面稳定 + architecture/ 文档就位后，用户判断进入
> Phase 4/5（pygame 风格层 + 绑定）。本批次为定向决策记录 + 里程碑规划；
> 实现未启动，下一会话按 M0 开工。上游契约：
> `reference/pygame绑定层API设计稿.md`（§二~§四、§六 继续有效）。

### 三条定向决策（用户拍板）

1. **主运行时 = RustPython 内嵌壳**：引擎 + 解释器同一份二进制，Web /
   Android / 桌面统一形态。设计稿 §5.2 路线 C 由"暂不选"升级为选定路线
   ——CPython 在 wasm 是 emscripten 工具链构建（与引擎 winit/wgpu 的
   unknown-unknown 二进制层永不相交，§5.2 已核实硬边界），Android 交叉
   编译 libpython 是工具链地狱；RustPython 纯 Rust 天然消解两者。
   Pyodide JS 桥（路线 A）降为 spike 失败回退项。
2. **CPython/PyO3 壳 = PyPI 尝鲜分发渠道**：不承担主运行时职责，仅需
   **行为与 RustPython 壳一致**（双壳一致性测试集执法，RustPython 行为
   为准）。标准 GIL 构建 wheel，不追 3.14t 特殊目标。
3. **GIL 单线程，free-threaded 不在支持范围**：绑定层全部调用落在主线程
   （GIL 模型天然如此），多线程归编译型语言（Rust）用户自理。base 线程
   契约保留为 Rust 层纪律（CLAUDE.md 措辞已同步），"wasm 也有稳定运行
   能力"由此保证——单线程是 RustPython 内嵌壳在 Web 上最稳的形态。

### 双运行时矩阵

| 运行时 | 平台 | 形态 | 分发 |
|---|---|---|---|
| **rustpython-vm 内嵌壳（主）** | 桌面 | 引擎+解释器+脚本单一原生二进制（开发/调试主循环也在此形态） | cargo 构建 |
| 同上 | Web | 同一壳的 wasm 单体 | xtask web 打包 |
| 同上 | Android | 同一壳的 cdylib（**零 libpython 交叉编译**） | xtask APK（probe 链路现成） |
| CPython + PyO3 壳（尝鲜） | Win/Linux/mac | 扩展模块 wheel | maturin → pip install |

### 架构不变量（§六 重申 + 新增受益）

- **业务逻辑只在核心绑定模块**（AppCore / Services 槽位 / manifest /
  启动门 / 资源惰性 / 生成器调度），两壳纯翻译——逻辑改一处双端同步。
- **Python 面零分叉**：pygame 层与用户脚本 = 解释器无关纯 Python。
  **双运行时兼容边界 = 纯 Python + RustPython stdlib 白名单**，C 扩展
  （numpy 等）明确避开（用户定案）——文档明示 + 一致性测试集执法。
- 9-08 设计稿之后新增的 base 能力直接受益：io `set_base_dir`（内嵌壳的
  脚本/资产装载统一走 io：web=URL、Android=私有目录）、permission 隐式
  （mixer 在 Android 自动弹权限）、debug::console_log（web 诊断通道）、
  `app_entry!`（PyApp 是"全库唯一 Application 实现"，入口直接复用）、
  probe 无头收割（一致性测试的判读基建）。

### 里程碑（内嵌壳优先，PyO3 垫后）

- **M0 · RustPython 嵌入 spike（决策门）**：rustpython-vm 最小嵌入，跑通
  定稿脚本三语言特性——生成器帧边界（run 逐帧 next()）/ `match` 解构
  事件 / `dataclass(slots)` + PEP 562 `__getattr__`；记录版本锁定。
  失败 → 回退路线 A（Pyodide 桥）。
- **M1 · 核心绑定模块 + 内嵌壳桌面 hello**：AppCore/manifest + display/
  event/time 最小片，桌面原生二进制驱动 .py 脚本（主运行时的开发主循环）。
- **M2 · 一致性测试集 CI 轨道**：金标准行为测试先立在内嵌壳上；PyO3 壳
  就位后升级为双壳对比。与 M1 同批起步。
- **M3 · 内嵌壳 → wasm**（xtask web + probe 无头判读）。
- **M4 · 内嵌壳 → Android**（xtask APK）。
- **M5 · PyO3 尝鲜壳**：maturin wheel 发 PyPI，行为对齐由 M2 测试集保证。
- **M6+ · 模块导出排期**：display/event/time/key → mixer（Sound/music，
  voice/resample 现成）→ font/gfx → draw/image → video/gamepad/net/dialog。

### 前置缺口（开工前清障）

1. M0 spike（上面已列——三语言特性支持度是定稿脚本可跑性的直接判据）。
2. PyO3 依赖隔离：`python` feature 门控，默认构建保持零 Python 依赖。
3. 构建工具统一**继续推迟**（用户决策）：Python 阶段才是 maturin/wasm/APK
   三形态的真实定型点，届时一并收拢（现打包链已证明代码无问题）。

### 待定项更新（设计稿 §七）

- §七.6 RustPython 版本锁定 → 提前到 M0 产出（主运行时，锁版本是硬要求）。
- §七.7 双壳一致性测试集 → M2，金标准先行。
- 其余待定项（run 签名/assets 粒度/事件缓存/Web 路径语义）维持原状。

### 设计稿详化（同日，§八~§十二 新增，§八 三轮收敛）

用户给出三条前提并要求详化：①双解释器对接均为原生接口；②starfish 适当
提供 pygame 启动基座类型（Python 不能实现 Rust trait）；③pygame 层必须
纯 Python（最低成本跨解释器）。拓扑经三轮收敛（多 crate → 单绑定 crate →
**不建新 crate,Python 以 feature 组件化插入 starfish 本体**）：

- **§八 最终拓扑**：单 crate 双 feature——`pyo3`（CPython 扩展壳,maturin
  wheel）+ `rpy`（rustpython 内嵌壳 + `pygame_entry!`）；绑定层住
  `src/pygame/`（cfg 门控），纯 Python 层住 `python/pygame/`（非 crate,
  include_str! 模块图内嵌,**免 freeze 工具链**）。crate-type 现成
  cdylib+rlib 即扩展所需;xtask **字面零改动**复用（Android = 现有
  `--features` 通道），唯一增量是 M3 的 web 打包子命令。结构性隔离退化为
  cfg 纪律,以 CI 依赖图卫生检查补位（`cargo tree` 断言默认特性图零
  Python 依赖）。PyPI 发行名避开 `pygame`（被占），发行名与 import 名分离。
- **§九 HookApp**：base 启动基座（钩子槽替代 trait 实现），PyApp 变薄为
  每壳一段装配；AudioEffect/Connection v1 标注 Rust-only。
- **§十 pygame Python 层**：包结构 + `_native` 双壳同名枢纽 + 分发形态
  （wheel 附带 / include_str! 内嵌）+ 用户脚本经 io 三平台装载。
- **§十一 原生面与 pyi 桩契约**：**双原生面一次注册**——面 A `pygame._native`
  （服务构造/引擎控制/事件批灌,pygame 层专用）+ 面 B `starfish`（base 直绑：
  Texture/Mesh/AudioMixer/StreamVoice/Video…,穿底用户;pygame 层自身也建于
  面 B 之上）。**.pyi 桩体系 = 接口的 Python 侧表达**：`starfish.pyi`（面 B）
  + `pygame/_native.pyi`(面 A,共享类型引 starfish 子模块) + pygame 层内联注解;
  **面 B 以 stub 包按模块分文件**——镜像 src/base/ 一比一
  (`starfish/render.pyi`、`starfish/audio.pyi`…,命名空间与 Rust 侧同构),
  Rust 模块 ↔ pyi 文件 ↔ 注册函数 ↔ M6 导出批次四者同键;
  桩 = 契约本体（手写为源、双壳向桩实现、一致性测试执法）,pyo3-stub-gen
  为 M5+ 漂移缩减选项。
- **§十二 一致性**：同源 + 测试集执法（RustPython 为准）+ M0 spike 判据
  扩充（pygame 层 stdlib 子集 + sys.modules 注入式装载）。

---

## 批次 10：WindowConfig 窗口特性 flags（绑定层前置 + README 既有宣称兑现）

> 绑定层 面 A 契约评审发现：`set_mode` 的 flags 通道需要 base 有窗口特性
> flag 集支撑；同时 README/架构文档早已宣称"全屏、无边框、窗口模式切换"
> 而运行时方法不全——本批次一次补齐。

- **WindowConfig 新增 6 flag**：`decorations`（无边框）/ `transparent`
  （**创建期一次性**——winit 各平台均不支持创建后切换；半透明 = 此开关 +
  应用侧 alpha 渲染）/ `always_on_top` / `fullscreen`（Borderless 当前
  显示器；Exclusive v2 再议）/ `visible`（隐藏启动）/ `cursor_visible`。
  `build_attrs` 应用（`with_decorations` / `with_transparent` /
  `with_window_level` / `with_fullscreen(Borderless)` / `with_visible`）。
- **光标可见性无 winit 属性期入口**：创建后立即应用一次（`resumed` 中,
  仅 false 时动）。
- **Window 运行时补 4 方法**：`set_decorations` / `set_fullscreen` /
  `set_maximized` / `set_minimized`——pygame display 运行时函数
  （toggle_fullscreen 等）的底层支撑到位。
- 平台口径（代码注释 + 设计稿）：装饰/透明/置顶 = 桌面主战场,Web（canvas
  模型）/Android（天然全屏）由 winit 后端降级,本层不分叉。
- **设计稿同步**：面 A `set_mode_request(w, h, flags=0)`；新增"窗口 flags
  映射"小节——pygame flag 常量（FULLSCREEN/NOFRAME/WINDOW_TRANSPARENT/
  WINDOW_ALWAYS_ON_TOP/WINDOW_HIDDEN…）→ base 字段翻译表,常量位值实现期
  自定只承诺名字与语义；鼠标类 flag 不走创建通道（运行时语义）；OPENGL
  显式报错（base 恒 wgpu）。

### 测试状态

- `cargo test --lib` **65 passed**（新增 WindowConfig 默认值契约 + builder
  链测试）；桌面/Android/wasm check 零 error。

### 补记（同日续）：经典逼近度收敛 + 动态 flag 补全

- **`init_finish()` 删除**（用户提案,设计稿 §二/§三/§4.2 同步）：封账并入
  `pygame.run()`——全量依赖校验错误仍在启动前报告,fail-fast 不减损；
  `pygame.init()` 回归经典全量语义（引擎按构造器链定序,调用顺序无关,
  `mixer.init(channels=N)` 显式覆盖保留）；**`while True:` 完全可用**
  （QUIT return 退出,漏处理 QUIT 由引擎 GeneratorExit 强制收口）。
  与经典 pygame 的差异清单从四条收敛到**两条机制必然**（`yield` 帧边界 +
  `pygame.run(game)`,引擎持循环的代价）。
- **`Window::set_cursor_grab`**（Confined,仍可见）：补全 pygame 鼠标 grab
  语义映射（与 `set_relative_mouse` 的 Locked+隐藏 FPS 模式互补）——
  面 A flags 映射表的鼠标行就此齐备。
- **动态 flag 结论**：窗口 flag 中除 `transparent`（创建期一次性,winit
  全平台限制）外全部支持运行时切换（`set_decorations`/`set_fullscreen`/
  `set_maximized`/`set_minimized`/`set_always_on_top`/`set_cursor_visible`/
  `set_relative_mouse`/`set_cursor_grab`/`set_visible`/`set_resizable`/
  `set_title`）——pygame display 运行时函数（toggle_fullscreen 等）底层
  支撑完整。
- **§8.5 pyd/so 导出与 feature 化架构的对接**（用户问：非传统 pyo3 项目
  架构能否出 pyd/so）：能。① crate-type cdylib 现成,`#[pymodule]` 在
  feature 门内,pyo3 折入 `extension-module`（不链 libpython 的扩展构建
  前提）；② 两条产出路径——本地裸 cargo 改名 pyd/so 即用（零工具链），
  PyPI 走根目录 pyproject.toml（maturin mixed-project 配置,对 cargo/xtask
  完全惰性），建议 abi3（一份 wheel 通吃）；③ CPython import 机制一个
  扩展文件一个导入名 → 编译扩展命名 `starfish`（面 B），面 A 在 CPython
  壳 = 纯 Python shim 再导出（rpy 壳无此约束,双面原生注册）；④
  `extension-module` 与 cargo test 已知互斥 → pyo3 验证走 maturin
  develop + 一致性测试集，默认特性图 cargo test 不受影响。

---

## 批次 11：CPython 壳退化为启动器（launcher）——PyO3 API 面删除（用户提案，设计稿 §8.6）

> 用户四修提案：真对接只做 RustPython;CPython 简化——不执行 pygame 代码，
> 进程还在，把脚本"映射"给内嵌 RP 执行，省去大量 PyO3 代码。采纳其内核，
> 否决其线程形态。

- **采纳**：CPython 侧只留**微型 bootstrap 扩展**（一个函数 `bootstrap(脚本)`），
  转交脚本给内嵌 RP 后**阻塞主线程让渡给 winit**；帧回调内驱动 RP——与 M1
  桌面内嵌壳同一份代码，入口仅差"谁调 bootstrap"。PyO3 面 A/B API 绑定实现
  **整体删除**（兼容边界"纯 Python + RP 白名单"下，同一份代码双解释器行为
  相同，CPython 执行 pygame 代码是冗余）。收益:PyO3 层实现删除、M2 双壳
  一致性简化为单运行时 + 启动器冒烟、面 B pyi 桩保留（契约/IDE 职责不变，
  实现方收敛为 rpy 注册）。
- **否决线程形态**（用户设想:CPython 线程 + RP 线程并存）:winit macOS
  强制主线程 → 引擎只能让渡进程主线程，RP 若去后台线程则每个 API 跨线程
  投递（视频 COM 套间/winit 非 Send/线程契约重写）——省 PyO3 层的代价是
  重写引擎线程模型，纯亏。**零线程**形态（主线程让渡 + 帧回调驱动 RP）
  拿到全部收益。
- **入口**:`python -m starfish game.py`（稳健）;备选首行接管技巧
  （`import starfish` shim 读 `__main__` 源码转交 + `os._exit`）——实现期定。
- **代价（诚实清单）**:RP 成为**硬依赖**，M0 spike 升格为单点决策门（原
  PyO3 全量壳回退路线不复存在）;CPython 入口 UX 变为 `-m` 形式。
- 设计稿:顶部修订标记 ④ + §8.6 新增;§六"双壳"读作"内嵌壳（唯一）+
  CPython 启动器（分发形态）"。
- **最终版整理**（用户定案"这就是最终形态"）:设计稿全文重写为干净现状
  陈述——九节结构（定位/生命周期/定稿脚本/绑定内部/工程拓扑/原生面与桩/
  里程碑/决策速查表/待定项），全部修订与三轮否决的推理链压缩为 §八 决策
  速查表（8 条,完整链在本文批次 9~11）。feature 定名调整:`pyo3` →
  `launcher`（bootstrap 特性,不再承担 API 面）。新增决策 #8:不支持
  CPython C 扩展生态为特性非缺陷——强制纯 Python + Rust 生态,维护量趋零。

---

## 批次 12：M0 spike 执行完毕——决策门放行（PASS 带已文档化限制）

> 里程碑 M0 实测完成,报告与 spike 代码在 `spike/m0/`(REPORT.md + 三个
> spike 工程)。锚点全绿清单见报告判定表。

- **版本锁定**：rustpython-vm **0.5.0**(match 支持为决定性理由,0.4 无)
  + freeze-stdlib + pylib 0.5;pyo3 0.29.2(launcher)。
- **六判据 + launcher 共存全验证**:生成器帧边界(严格交错)/ dataclass slots
  (✗,见下)/ match 解构(0.5 新支持)/ PEP 562 / stdlib 子集(部分)/
  sys.modules 注入 / 原生函数暴露 / **真 CPython 进程内 launcher 共存**(3.10
  + pyd 实测)。
- **发现与绕行**:①0.5.0 的 rustpython-stdlib 打包 bug(malachite 互斥,
  github 不可达无法走 git)→ 原生 stdlib 缺口(math/_opcode…),dataclasses
  不可用 → **pygame 层 Event 改普通类自实现**(对用户不可感);②pylib Lib
  缺 latin1.py → VM 启动序列 `import encodings` 预热即解;③RP Interpreter
  **非 Send** + 启动递归深 → **大栈线程(256MB)承载解释器**为嵌入标准实践,
  launcher 场景 CPython 主线程栈不够(实测静默退出);
  ④原生模块缺口记入兼容边界,后续逐个 shim 补。
- 设计稿:§九待定项 #6(版本锁定)已由 M0 产出;Event 形态修订随 M1 落。

---

## 批次 13：M1 完成——内嵌壳桌面 hello 全链路打通（绑定层第一次真实运行）

> 里程碑 M1（绑定核心 + 内嵌壳桌面 hello,设计稿 §七）实测完成。
> `cargo run --features rpy --example pygame_hello`:窗口打开、渐变清屏、
> 31 帧（验收阈值）后 StopIteration 自动退出,exit 0。

- **base::app::HookApp**（设计稿 §4.4 落地）：钩子槽应用——不实现 trait
  而以可替换闭包驱动引擎,为 Python 绑定层与闭包式 Rust 用户提供入口。
  桌面 run 本就无 `'static` 约束 → 钩子可捕获借用（如内嵌壳的 `&VirtualMachine`）。
- **WindowConfig::any_thread_event_loop**（新 flag）：允许在非主线程创建
  事件循环——rpy 大栈线程形态的必要件（winit 0.30 在 Windows 也 panic
  非主线程创建,实测）。Windows/Linux 经 `with_any_thread(true)` 绕过;
  macOS 系统级强制主线程,flag 无效（兼容边界）。
- **feature = "rpy"**（可选依赖 rustpython-vm 0.5.0 freeze-stdlib 锁版本）：
  `src/pygame/core.rs`（AppCore：init/set_mode 记账、事件缓冲、退出标记,
  thread_local 单线程访问）+ `src/pygame/shell_rpy.rs`（大栈线程 = 引擎 +
  RP 闭环:encodings 预热 → 注册 pygame 原生模块 → exec 脚本 → pygame.run
  阻塞进 HookApp 循环 → 帧回调逐帧 `__next__` 步进生成器 → 清屏 present）。
- **pygame 原生面 v1（临时面）**：init/set_mode(w,h)/set_title/run/
  event.get/draw.clear/flip/quit——扁平参数、字符串事件,纯 Python pygame
  层（设计稿 §九）落地后收敛为面 A 最小子集。
- **三个实测确认的关键行为**：
  1. 生成器对象**不可 `.call`**（静默 no-op,31k 帧空转的元凶）——必须
     `call_method("__next__")` 恢复（M0 判据 A 的形态）
  2. `game(ctx)` 生成器由 f_run 显式创建（`game.call((ctx_none,), vm)`）,
     M1 ctx 占位 None,真 ctx 门面随 M2+
  3. 桌面 run 末尾 `process::exit(0)`——"脚本执行完毕"日志在其前打印
- **回归**：`cargo test --lib` 65 passed;`cargo check --examples` 全过
  （pygame_hello 以 required-features=["rpy"] 注册,默认构建跳过）;
  默认特性图零 Python 依赖（rpy 关闭时不编译任何 RP 代码）。

---

## 批次 14：M2 完成——行为回归测试集（无头模式 + 6 项行为断言）

> 里程碑 M2(设计稿 §七)完成:行为回归测试集立在内嵌壳上,`cargo test
> --features rpy --lib` 一条命令全量执行(71 passed = 65 基线 + 6 行为)。

- **帧驱动与 winit 解耦**:`step_frame` 提取(引擎无关的帧步进——生成器
  `__next__` 恢复、StopIteration return 值捕获、异常捕获、帧计数),
  `EngineMode::{Real, Headless{max_frames}}` 分派——真实模式走 HookApp
  循环(不变),无头模式纯状态机模拟(无 winit/GPU,测试可并行)。
- **HeadlessReport(行为断言事实源)**:frames / frame_clears(逐帧清屏色
  时序,draw.clear 在无头模式追加)/ return_value(生成器 return 捕获)/
  last_error(脚本异常使用点报错)/ display_request(记账)/ results_repr
  (脚本 RESULTS repr——行为观测的标准出口)。
- **6 项行为测试**(脚本内嵌断言 + 报告断言):
  T1 生命周期(init/set_mode 记账 + 3 帧门 + return "ok")/
  T2 quit 事件注入首帧可见 / T3 esc 事件 /
  T4 清屏色时序(3 帧三色)/ T5 脚本异常捕获(boom 进 last_error)/
  T6 模块全局跨帧持久(31k 空转 bug 类的回归锚)+ return 值透传。
- **回归**:默认 `cargo test --lib` 65 passed(rpy 关闭,零 Python 依赖);
  `cargo check --examples` 全过;wasm/Android lib check 零 error。
  (构建注:并行链接 RP 大二进制曾触页面文件耗尽(os error 1455),
  环境资源问题非代码问题,降低并行度即过。)

---

## 批次 15：M2.5 架构优化——pygame 语义层独立成 feature（用户提案）

> 用户重审架构后定向:pygame 的方法和类型基于 Rust 实现于 src/pygame,
> 以 feature 形式存在;rustpython 内嵌为另一 feature;RP 的 python 模块
> 对接 starfish(base)与 pygame 两个映射模块;core 状态机保留;CPython
> 启动器映射保留。落地为三 feature 结构。

- **feature 重排**:
  - `pygame`(默认开):**纯 Rust 语义层**——Color/Rect(Phase 4 遗产纳入门控)
    + 绑定核心状态机(core.rs,RP-free)。Rust 用户不启用也能避开。
  - `rpy`(隐含 pygame):内嵌 RP 壳,`shell_rpy.rs` 迁为 `src/pygame/rpy/mod.rs`。
  - `launcher`(M5 占位):CPython bootstrap。
- **依赖方向**:`rpy → pygame → base`、`launcher → pygame → base`——pygame
  语义层不背解释器,两壳互不进对方产物。
- **base/color.rs 门控处理**:字节色桥接方法(from/to_byte_color)挂
  `cfg(feature = "pygame")`——base 本体不反向依赖 pygame 语义层。
- **回归**:默认 65 passed(rpy 关闭零 Python 依赖)/ rpy 71 passed(含
  M2 行为集)/ examples 全过 / wasm+Android lib check 零 error。
- 设计稿:§5.1 feature 表五修(含 base/color 门控说明)。
- **类型互通单向化(用户定案,同日)**:base **零 pygame 依赖**——此前挂在
  base/color.rs 的字节色桥接方法已迁入 pygame 层(Color::from_base/to_base
  + From 双向实现 + 往返/截断单测 2 个)。原则:pygame→base 直接转换;
  base→pygame 经 pygame 高层类型 from 接口;双向转换全部住 pygame 层。
  **entry 锁定**:base::app::run(纯 Rust 应用)与 pygame::run_script(pygame
  脚本)各自固定运行形态,类型互通不受限。base/color.rs 头注释同步更新。
- **回归**:rpy 73 passed(65 基线 + 6 行为 + 2 桥接)/ 默认 67 passed /
  base 反向引用 grep 归零。

---

## 批次 16：编译可行性终验——wasm 阻断（上游缺陷），Android 放行

> 里程碑 M3(wasm)开工即遇硬阻断;按"完成编译可行性后结束"收尾,
> M3 剩余项(入口/实测/xtask web)顺延,Android(M4)已具备开工条件。

- **wasm32-unknown-unknown:已打通**(用户下载 RustPython-main.zip 供源 →
  vendor/RustPython-main 本地化)。根因与解法:
  ① crates.io 0.5.0 对 wasm 编译损坏(`stdlib/_io` 无门控引用 wasm 缺失的
  crt_fd;os/ospath 挂 host_env 同病)——vendor main(0.6.0-dev)后自然修复;
  ② main 的 vm 默认特性含 host_env(os/ospath 的 crt_fd 依赖在
  unknown-unknown 不存在)→ rpy 依赖改 `default-features = false` +
  显式 `["compiler","wasmbind","gc","stdio","importlib","encodings",
  "freeze-stdlib"]`(vm 自带非 host_env 的 stdio 路径);
  ③ pylib 的 Lib 在 git zip 里是 symlink → 解压脚本按 S_IFLNK 识别并从
  根 Lib 实体化(10 条,含 vm/Lib 的 core_modules)。
  **代价与形态**:RP 版本从 crates.io 0.5.0 锁定改为 **vendor main 快照**
  (0.6.0-dev,本地目录依赖,版本升级 = 重下源码包);`vendor/` 需入 git
  或配 LFS(体积待评估,收尾时定)。
- **aarch64-linux-android:放行**。RP 依赖树里的 psm 构建脚本需要 NDK C
  工具链——补 `CC_aarch64_linux_android`/`AR_aarch64_linux_android` 指向
  NDK 30 clang 后 `cargo check --features rpy --target aarch64-linux-android`
  通过(1m11s)。M4 的 APK 流程经 cargo-ndk 自动注入这些变量,无额外工作。
- **feature 边界附带成果**:src/pygame 门控拆分后,默认特性图(rpy 关闭)
  在 wasm/Android 双目标均编译干净——Python 绑定层与引擎的隔离得到编译器
  级验证。

---

## 批次 17（2026-09-23）：TODO.md 清理批次——音频 API 审计九项全落地

> 接口审计的九项遗留一次清完(3 诚实性/能力项 + 3 破坏性整理 + 3 补齐),
> 全部带 Inner::mix 真实路径测试。默认测试 72 passed(rpy 73+行为集)。

- **诚实性修复**:`StreamVoice::push_interleaved` 在 closed 声部上返回 0
  (此前静默接受、数据永不被播——违反"实际接受"文档承诺)
- **能力补齐**:StreamVoice `pause()/resume()/is_paused()`(混音跳过,数据
  与推进不受影响)、`buffered_frames()`(ring 水位)、`set_pan()/pan()`
  (混音按 SFX 同款声像公式)、SFX `set_channel_muted`(通道 muted +
  混音跳过)
- **破坏性整理(v0.7 窗口)**:`channel_fade_in/out` → `fade_in/fade_out`
  (与 StreamVoice 词序对齐);`play_in_group` 参数序统一(声源在前);
  `output_sample_rate` 字段 → 方法(私有化,统一出口)
- **文档弥合**:stop 语义注释(回池复用 vs 流式销毁)、SFX 槽位索引有效期
  警告(写入 AudioMixer::stop 文档)
- 回归:默认 72 passed / examples 全过;详细测试断言见各新增 #[test]

---

## 批次 18（2026-09-23）：pygame 兼容层垂直切片——纯 Python 层首次全链路运行

> 兼容层第一块垂直切片落地:`python/pygame/__init__.py`(单文件纯 Python
> 兼容层)+ `pygame._native` 原生面 + 行为测试全绿。用户脚本从此以纯
> Python 写:`import pygame → init → set_mode → 两相位 game 生成器 → run`。

- **纯 Python 层(python/pygame/__init__.py)**:常量(QUIT/KEYDOWN/K_*)/
  Event 类(match 解构友好)/ init 阶梯 / display.set_mode+flip /
  event.get / draw.clear / run(生成器门)。
- **rpy/mod.rs 原生面**:poll_events 结构化二元组(经 Python 层包装为
  Event 对象)/ ticks 帧毫秒 / display_set_mode 元组接收。
- **两项实测结论**:① 生成器对象**不可 .call**(静默 no-op,31k 空转根因
  )——必须 `call_method("__next__")` 恢复;② sys.modules 注入需**模块包装
  **(裸 dict 属性访问失败,"has no attribute init")——vm.new_module 包
  dict 后注入解决。
- **回归**:rpy 78 passed(65 基线 + 6 行为 + 2 桥接 + 5 新特性),默认 72。

- **M3 剩余**：wasm 运行入口(spawn_local + io::read_text 脚本装载 +
  thread_local INTERP/GEN 槽)与浏览器实测(栈深行为观察)
- **M4**：Android 入口(app_entry 捕获 + 大栈线程引擎)与 xtask APK 打包
- **Tier 0 起步**：locals 重组 / math(glam) / version——绑定导出批次的起点
- base/audio 三小项(见主目录 TODO.md)：close-push 返回 0 / pause-resume /
  buffered_frames
