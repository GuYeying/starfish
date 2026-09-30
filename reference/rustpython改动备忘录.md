# RustPython vendored 改动备忘录

> 记录 pygame-rs 对 vendored RustPython 快照的全部源码/配置改动。
> 升级上游版本时按本清单重放或评估清理;回归命令见文末。
> 关联:`reference/vendor与crates切换指南.md` 例外登记;
> changelog 批次 22(2026-09-23)。
> **位置更新(2026-09-29)**:pygame-rs 已独立迁出至
> `D:\Projects\Rust\pygame-rs`(与 starfish 平行的 crate,starfish path
> 依赖改为 `../starfish`)——本清单继续有效,文内相对路径以 pygame-rs
> 为基准。

---

## 〇、快照信息

| 项 | 值 |
|---|---|
| 版本 | RustPython main 快照 0.6.0-dev(edition2024/rust1.95) |
| 来源 | 用户提供的 RustPython-main.zip(github main) |
| 位置 | `pygame-rs/src/rpy/dependencies/RustPython-main/` |
| 引用方式 | pygame-rs/Cargo.toml 的 path 依赖(vim + pylib),default-features = false |
| 启用特性 | vm: compiler,wasmbind,gc,stdio,importlib,encodings,freeze-stdlib(去 host_env) |

**改动总量:3 处,全部 android 限定**——桌面/网页编译路径走原版代码,
Python 语言语义(解释器/编译器/stdlib)零修改。

---

## 一、改动 ①:libffi 目标段剔除 android

- **文件**:`crates/host_env/Cargo.toml`
- **改动**:`[target.'cfg(all(any(target_os = "linux", target_os = "macos",
  target_os = "windows", target_os = "android"), not(any(musl, sgx))))'.dependencies]`
  的 any() 列表里删掉 `target_os = "android"`。

### 为什么

NDK 不提供 libffi。`rustpython-host_env` 是 vm 的**非可选** Cargo 依赖,
其 libffi 依赖按目标段无条件生效 → android 链接期
`ld.lld: error: unable to find library -lffi`。这是链接期错误,
`cargo check` 不暴露——平台编译矩阵因此漏检(批次 21/22 教训)。

---

## 二、改动 ②:ctypes 模块门控剔除 android

- **文件**:`crates/host_env/src/lib.rs`
- **改动**:`pub mod ctypes;` 前加 `#[cfg(not(target_os = "android"))]`
  (①的配套——模块代码引用 libffi,依赖被剔除后模块必须一并门控,
  否则编译失败)。

### ctypes 在各平台的真实可用性(重要,反直觉)

| 平台 | `import ctypes` | 说明 |
|---|---|---|
| 桌面(win/linux/mac) | **不可用** | vm 未启用 `host_env` 特性 → stdlib 的 `_ctypes` 原生模块不注册(stdlib/mod.rs 的 cfg 门),frozen 的 ctypes 纯 Python 包装层 import 即 ImportError |
| Web(wasm) | 不可用 | 同上;且浏览器无法 dlopen |
| **Android** | **不可用(与其它平台一致)** | 同上;补丁 ② 只是不让"死依赖 libffi"拖垮链接 |

**结论:改动 ①② 在功能上零损失**——我们的嵌入从第一天起就没启用
host_env/ctypes。补丁剪掉的是"即使功能不可用也拖着走的链接依赖"。
如果未来要用 ctypes:桌面 = 开 vm `host_env` 特性即可(libffi 桌面
可用);android = 需要 NDK libffi 方案(上游或自建),当前决策:不用,
需要 C 能力时走绑定面(Rust 原生函数)而非 FFI。

---

## 三、改动 ③:static_cell 的 android 专用分支

- **文件**:`crates/common/src/static_cell.rs`
- **改动**:追加 `#[cfg(all(not(feature = "threading"), feature = "std",
  target_os = "android"))] mod non_threading` 分支——StaticCell 从
  thread_local 缓存改为全局 `UnsafeCell<OnceCell<T>>` +
  `unsafe impl Sync`(约 130 行,含契约注释)。原实现保留给
  非 android(re-export 按 cfg 二选一)。

### 为什么

- bionic(android libc)上,rust 的每个 `thread_local!` 静态(含无析构的)
  占用**一个 pthread key**,全进程共享、上限 `PTHREAD_KEYS_MAX = 1024`。
- RP 的 static_cell 为**每个内建类型**生成一个 thread_local 静态
  (异常群 + 内建类群,数百个)——解释器创世纪(genesis)初始化类型群时
  把池吃穿 → `fatal runtime error: out of TLS keys, aborting`,启动即崩。
- 链接期 check 不暴露(纯运行期),平台矩阵因此漏检。

### 安全性论证(为什么不破坏解释器)

- 原实现用 thread_local 的目的:`&'static T` 需要 `T: Sync`,而 RP 对象
  是 Rc 系(`!Sync`),每线程各存一份绕开该约束。
- 我们的嵌入契约:**GIL 单线程 + 每进程一个解释器 + 恒在同一线程驱动**
  ——契约下"每线程一份"与"全局一份"可观察行为完全相同,unsafe Sync
  由契约兜底(契约写在修改处注释)。
- **语义红线**:桌面/网页保留原 thread_local 实现——每线程独立 genesis
  的隔离语义是行为测试集的依赖(全局化实测交叉失败 6/11 测试)。

---

## 四、升级上游版本的流程

1. 解压新快照到 `src/rpy/dependencies/RustPython-main/`(替换旧目录);
2. **重放三处补丁**(按本文一/二/三节的精确位置);
3. 跑回归:
   ```bash
   cd pygame-rs
   cargo test                                    # 11 passed
   cargo check --target wasm32-unknown-unknown  # wasm
   CC_aarch64_linux_android=<ndk>/…/aarch64-linux-android26-clang.cmd \
   AR_aarch64_linux_android=<ndk>/…/llvm-ar.exe \
   cargo check --target aarch64-linux-android   # android
   cargo run --example pygame_hello              # 桌面实跑
   ```
4. 若上游已自带等价修复 → 对应补丁不再重放,并更新切换指南例外登记。

### 上游化建议(长期消除维护)

①② 可提 PR:"gate ctypes/host_env libffi on android"(上游受益:
NDK 用户可直接构建);③ 可提 PR:"static_cell: avoid TLS on platforms
with small pthread key pools"。合入后本清单归零。
**完整评估与操作路径见 §七(2026-09-29 定稿)。**

---

## 六、魔改与架构的关系(winit 对比,2026-09-29)

三处补丁与架构选择**正交**——全是 Android 平台构建/运行层问题(链接
依赖、TLS 上限),与 winit/poll 无关,换任何架构一处都躲不开。

架构层面反而是"零魔改":

- **winit(控制反转)下嵌入 RP**:要么把 Python 帧塞进回调框架(控制
  反转泄漏进嵌入设计),要么解释器上线程 + 通道(大概率催生第四/五个
  补丁:线程安全包装、Send 桥);
- **poll(本项目)下嵌入 RP**:循环归壳,RP 原样嵌入,平台补丁 3 处 +
  **架构补丁 0 处**;
- 老方案(PyO3/CPython 启动器)则是整个不同的、大得多的集成面。

量化:**平台魔改 3 处(与架构无关)+ 架构魔改 0 处**——这组数字本身
就是 poll 重写价值的量化。

---

## 七、上游 PR 策略(2026-09-29 评估定稿)

| 补丁 | 上游价值 | 建议 |
|---|---|---|
| ①+② | **高**——NDK 嵌入者当前链接必败(硬阻断,且 check 不暴露) | **合并一个 PR**:"gate host_env libffi/ctypes on android"(几行 cfg,低成本)。先 rebase 最新 main 查重 |
| ③ | **bug 报告价值极高**——跑完整 genesis 的安卓嵌入者必撞 TLS 耗尽 | **先提 issue**(复现 + PTHREAD_KEYS_MAX 分析);PR 缓提——全局 cell 方案靠本项目单线程契约兜底,上游支持多线程,照搬不合上游;PR 需线程兼容重设计(如单 TLS 槽持注册表) |
| (附)host_env 默认特性破坏 wasm 构建 | 中——wasm32-unknown-unknown 嵌入者的第一堵墙 | 顺手提 issue(本项目以 `default-features=false` 绕过) |

操作路径:

1. rebase 最新 main,确认三点未修(快照 2026-09-22,上游推进快);
2. 按本文 §四回归命令跑通(android/wasm check + 桌面实跑);
3. **PR 形态 = "使能"改动**(①② 的 cfg 门控形态)——不照搬为自己契约
   做的裁剪(③ 的单线程假设尤其不可照搬,上游需要线程兼容设计);
4. 仓库归属:三处全在 RP 自有 workspace(host_env / common 均为 RP 自有
   crate),全部提 RustPython/RustPython 主仓,无第三方仓库牵涉。

一句话:**①② 是教科书级的 PR 题材;③ 是教科书级的 issue 题材**
(PR 需先做线程兼容设计)。

---

## 五、ctypes 不可用的功能边界(FAQ)

| 想做的事 | 影响 | 替代方案 |
|---|---|---|
| Python 里 `import ctypes` 调 .so/.dll | 不可用(所有平台,嵌入未启用) | 在绑定面加 Rust 原生函数(面 A/B) |
| 使用依赖 ctypes 的纯 Python 库 | 该库 import 即失败 | 找纯 Python 替代 / 绑定面实现 |
| 引擎能力(渲染/音频/输入/窗口) | **零影响**(全走 Rust 原生面) | — |
| 其它 stdlib 模块 | 不受牵连(ctypes 无被依赖方) | — |
