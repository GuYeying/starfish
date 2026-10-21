# vendor 与 crates.io 切换指南

> 回答两个问题：**为什么当前这么设计**（vendor 本地源而非 crates.io 拉取）、
> **以后上游修复了怎么切回**（删除什么、改什么、如何验证）。
> 零魔改承诺：装载脚本只做"解压原样 + symlink 材料化"，RP 源码零修改——
> 切回 crates.io 无任何源码残留需要清理。

---

## 一、为什么当前不能直接从 crates.io 拉取

crates.io 上的 rustpython-vm/pylib **0.5.0 存在两个已实测的上游发布缺陷**：

| 缺陷 | 影响 | 触发条件 |
|---|---|---|
| **malachite 互斥** | 原生构建失败（rustpython-stdlib 17 个编译错误） | 任何启用 stdlib 的原生目标——即我们的主用法 |
| **wasm `_io`/`os` 编译损坏** | wasm32-unknown-unknown 编译失败（crt_fd 缺失，55 错误） | wasm 目标（M3 需要） |

- 两个缺陷都源于 **0.5.0 发布时 workspace 依赖锁未对齐**（malachite 版本
  vm 要 0.12 / stdlib 锁 ^0.9.1）与 wasm 门控未完成——上游 git main 已修，
  但修复**未发布到 crates.io**。
- 结论：**"传统 cargo 从 crates 下拉"拉到的就是坏的**——这不是我们不用
  crates.io，是 crates.io 上还没有可用的版本。
- 我们的解法：用户提供 RustPython-main.zip（github main 快照）→
  **pygame-rs/src/rpy/dependencies/RustPython-main/** 本地化 + symlink
  材料化（见 `tools/prepare_vendor.py`，M3 起生效）。

## 二、当前接线（切换的基准状态）

> 2026-09-23 起 Python 绑定抽离为独立 crate：接线在 **pygame-rs/Cargo.toml**，
> RP 源码目录 = **pygame-rs/src/rpy/dependencies/RustPython-main/**（与绑定
> 包同挂 rpy/dependencies/ 下）；starfish 本体零 rustpython 资产。

```toml
# pygame-rs/Cargo.toml
[dependencies]
rustpython-vm = { path = "src/rpy/dependencies/RustPython-main/crates/vm",
    default-features = false,
    features = ["compiler", "wasmbind", "gc", "stdio", "importlib",
                "encodings", "freeze-stdlib"] }
rustpython-pylib = { path = "src/rpy/dependencies/RustPython-main/crates/pylib",
    features = ["freeze-stdlib"] }
```

- **default-features = false 去 host_env**：host_env 引入 ospath/os 的
  crt_fd/fileutils 依赖——在 wasm 与部分目标上不存在（wasm 必需去，
  见 §九.6 与批次 16）。
- **零魔改**：vendor 源码零修改；wasm 修复全在 features 声明侧；装载
  脚本只做解压 + symlink 材料化（zip 无法表达 symlink,材料化 = 复制
  真实目录到 symlink 位,内容与 git checkout 一致）。
  **例外登记（2026-09-23 批次 22,共三处）**：
  ① `crates/host_env/Cargo.toml` libffi 目标段 any() 剔除
  `target_os = "android"`；
  ② `crates/host_env/src/lib.rs` `pub mod ctypes` 加
  `#[cfg(not(target_os = "android"))]`——①② 缘由：NDK 无 libffi,android
  链接 `-lffi` 失败,用户决策剔除 ctypes 而非引入 C 依赖；
  ③ `crates/common/src/static_cell.rs` 追加 android 专用 non_threading
  分支（StaticCell 全局 UnsafeCell,零 pthread key）——bionic 键池 1024
  被 genesis 类型群耗尽；桌面/网页保留原实现（并行测试依赖每线程独立
  genesis）。切回 crates.io 上游时按本清单清理这三处即可。
  **上游化**：①② 拟以 feature 门控形态提 PR、③ 拟提 issue——
  草稿与准备清单见 `rustpython上游贡献PR准备.md`。

## 三、切回 crates.io（上游发布修复版后）

前提：crates.io 出现含 wasm 修复的 rustpython-vm 版本（≥0.5.1 或 0.6.0）。

### 步骤

1. **改依赖**（两行，在 pygame-rs/Cargo.toml）：删 path 键、指 crates.io
   版本、保留特性表：

```toml
rustpython-vm = { version = "0.6", default-features = false,
    features = ["compiler", "wasmbind", "gc", "stdio", "importlib",
                "encodings", "freeze-stdlib"] }
rustpython-pylib = { version = "0.6", features = ["freeze-stdlib"] }
```

2. **删 pygame-rs/src/rpy/dependencies/RustPython-main/ 目录与装载脚本**
   （`tools/prepare_vendor.py`，若已建）。
3. **验证**（切回检查清单，`cd pygame-rs` 后执行）：
   - [ ] `cargo test`：11 passed（M2 行为集全绿）
   - [ ] `cargo check --target wasm32-unknown-unknown`
   - [ ] pygame_hello wasm 冒烟（浏览器锚点）

### 回滚

切回过程中任一步失败 → 恢复 vendor 形态（git revert 本次的
pygame-rs/Cargo.toml 改动即可，vendor/ 目录与脚本不受影响）。

## 四、为什么当前这么设计（决策记录摘要）

| 问题 | 决策 | 理由 |
|---|---|---|
| 解释器从哪来 | vendor main 源码（用户供 zip） | crates.io 0.5.0 双缺陷（wasm 编译损坏 + malachite 互斥）实测确认;git main 在本环境不可达（github 连接超时）——vendor 是唯一可行路径 |
| 为什么不用 crates.io 0.5.0 凑合 | 双缺陷实测 | wasm 编译失败阻塞 M3;malachite 互斥阻塞一切启用 stdlib 的原生构建 |
| 为什么 zip+脚本而非提交解压树 | 仓库体积（18MB vs 124MB）+ 脚本可读可审计（零魔改可验证） | 解压树无法自证"未被修改" |
| 为什么不 etc/crates.io patch | patch 无法修改 crate 内部 cfg | wasm 缺陷在 RP 源码内部,只能换源码 |

- 决策推理链完整历史：`doc/log/starfish_changelog_2026-09-20.md` 批次 9~16。

## 五、验证锚点速查（切回/升级后必跑，`cd pygame-rs` 后执行）

```bash
cargo test                               # 11 passed（8 M2 行为 + 2 桥接 + 1 rect）
cargo check --target wasm32-unknown-unknown
cargo check --target aarch64-linux-android
cargo run --example pygame_hello         # 桌面链路冒烟
```
