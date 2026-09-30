# starfish 更新日志 2026-09-29

## 批次二十一：xtask 退役——统一打包脚本落地（pygame-rs 自持，全 crate 共用）

### 背景（用户决策）

xtask 仅支持根 Cargo.toml 示例注册表，pygame-rs（兄弟 crate）的示例
一直走手动五步链——**双打包路径并存**的代价在批次十八已付过学费
（lib_name 坑即手动链缺陷）。用户决策：**打包工具归 pygame-rs 自持**，
starfish 根示例共用同一脚本，xtask 退役。另：pygame-rs 将独立为
D:\Projects\Rust 下的平行 crate（pygame-rs / starfish / starfish-window
三分）——脚本因此按"无仓库根假设"设计。

### 落地

- **`pygame-rs/scripts/build_apk.py`**：五步链参数化固化
  （cargo ndk → llvm-strip（nm 自检 android_main）→ manifest 模板替换 +
  aapt2 link 重造 base（lib_name 对齐，**批次十八根因的永久哨兵自检**）
  → 组装（dex 探测 + .so DEFLATED）→ zipalign → apksigner 签名验证）。
  - 用法：`python pygame-rs/scripts/build_apk.py <示例名> [--dir <crate目录>]`
    ——默认自持 pygame-rs；根示例 `--dir .`；产物 `<crate>/target/android-apk/`；
  - **独立化友好**：无仓库根假设（模板搜索 crate 自有 android/ 优先、
    dex 从本 crate target 扫描、产物 crate 本地）——pygame-rs 迁出后开箱即用；
  - `pygame-rs/android/AndroidManifest.xml`：自持模板副本（含存储权限，
    独立后不依赖 starfish 目录）。
- **xtask 退役**：`xtask/` 目录删除、`.cargo/config.toml` 的 `cargo xtask`
  别名移除、CLAUDE.md（布局/常用命令/坑位 #6）同步。

### 验证

| 路径 | 结果 |
|---|---|
| pygame-rs（binding_probe，自持模板） | ✅ 五步全绿，lib_name 自检 ✓，产物 26MB |
| 根 crate（hello，--dir .） | ✅ 五步全绿，lib_name 自检 ✓，产物 2MB |
| 单元回归 | ✅ `cargo test --lib pygame::` 39/39（批次二十接线无回归） |

### 附带修复

- `build_apk.py`：Windows 控制台 GBK 编码崩溃（stdout 重配 utf-8）；
- 诊断 TCP 通道：connect 加 300ms 超时 + 一次失败拉黑（无超时阻塞
  主线程 = ANR，批次十九坑的脚本侧同步修复）。

### 下一步

Surface 批次 v1.1（架构 §九定约实现）→ rp_main_loop 挂起排查 →
pygame-rs 独立迁移（三 crate 平行布局，脚本已按独立形态就绪）。
