# starfish 更新日志 2026-10-21 —— workspace 化 + 按 feature 拆分 crate（架构重构批次）

## 背景

- 本仓库为 2026-10-21 新建的 git 仓库（引擎自旧仓库 starfish-rewrite 整体迁入，
  位于 crates/starfish）。根 Cargo.toml 是损坏的 stub（无 targets 的
  `[package]` + members 指向不存在的目录），cargo 向上搜索导致全仓库任何
  cargo 命令失败。
- 目标（用户定稿）：① 根目录改可用 workspace；② examples 提为 workspace
  根下独立成员 crate；③ 按 starfish 的 feature 依赖关系把模块 crate 化放进
  crates/；④ 不做门面 facade——`app_entry!` 宏并入核心 crate
  （定名 starfish-std；包名 "std" 与标准库冲突会令全工作区 `use std::` 报
  E0659，不可行）。

## 方案（落地形态）

- **根 manifest**：纯虚拟 `[workspace]`，members = ["crates/*", "examples"]；
  `[workspace.package]` 统一 version 0.9.25 / edition 2024 / rust-version 1.85；
  `[workspace.dependencies]` 收拢全部共享依赖（版本单源）。
- **14 个引擎 crate**（crates/）：
  starfish-std（error/debug/app + app_entry! 宏）、starfish-we（窗口/事件，
  feature we-wgpu/wgpu-auto）、starfish-time、starfish-permission、
  starfish-io、starfish-net、starfish-dialog、starfish-gamepad、
  starfish-render（wgpu 封装 + resources）、starfish-gfx、starfish-font、
  starfish-audio（feature dialog=录音保存，默认开）、starfish-video（yuv 并入）、
  starfish-pygame（兼容层；base 的 color 模块并入为内部 base_color）。
- **examples**：workspace 根下独立 crate（examples/examples/ 源码 +
  [[example]] 注册原样搬运，含 _android 双注册）；resources/ 运行期资产随迁
  （examples/resources），android/ 清单模板 + jniLibs/ + web/（wasm 产物与
  回归探针）随迁 examples/。
- **依赖 DAG**（严格单向）：std ← we ← render ← {gfx,font}；
  std ← {time,permission,io}；permission ← {net,dialog}；dialog ←(可选) audio；
  {std,audio} ← video；全部 → pygame → examples。
- **项目文档归位**：doc/、architecture/、reference/、pygame reference/、
  assets/、README、LICENSE 移至仓库根；拆分前单 crate 工作指南归档为
  doc/CLAUDE-engine.md；开发脚本移至 scripts/。

## 拆分中实测修正的既有问题

1. **pollster 双版本歧义（E0464，基线编译即暴露）**：旧 manifest 同时声明
   pollster 1.0（可选，wgpu-auto 启用）与 0.4（非 wasm 目标），两个不兼容
   版本并存令 `use pollster::` 无法解析——workspace 统一 0.4 单版本根治。
2. **`log` optional 从未被启用**：dialog.rs / record.rs 无条件使用 `log::`
   但旧 manifest 中 log 为 optional 且无任何 feature 引用——dialog/audio
   crate 中改为非可选。
3. **app_entry! 宏 Android 分支潜伏 bug**：`$crate::crate::base::we::AndroidApp`
   双重 crate 段（Android 上必炸）——迁移时修正为 `$crate::we::AndroidApp`
   （std 经 feature "we" 再导出 starfish-we）。
4. **死依赖移除**：gltf、libc 在 src 中零引用，不再声明。
5. **隐藏依赖显式化**（原 feature 图未声明、实测 grep 出的边）：render→resources、
   audio→dialog（record 录音保存，改为可选 feature）、video→audio（音轨泵）、
   video→yuv、net/dialog→permission——crate 化后全部成为显式依赖边。
6. 陈旧路径修正：examples 中 `starfish::base::window`（不存在）→ `starfish_we`、
   `crate::base::we`（example 内无效）→ `starfish_we`。

## 裁剪 UX 变化

无门面元 feature——按需依赖 sub-crate 即达裁剪：轻量逻辑层 =
starfish-std+time+io；带窗渲染 = +we(wgpu-auto)+render；完整体验 =
starfish-pygame。保留 feature：starfish-we{we-wgpu,wgpu-auto}、
starfish-std{we}、starfish-audio{dialog}。

## 环境备注

- 本仓库 `.cargo/config.toml` 走 rsproxy 镜像（本机直连 crates.io 不通），
  仅影响本仓库。
- crates/starfish 空壳目录被系统进程占用暂无法删除，已在根 manifest
  `exclude` 中排除；目录可删后该行可移除。

## 测试状态

- `cargo check --workspace --all-targets`（桌面 Windows）：**全绿**（14 crate
  + examples 全部目标，含 `_android` 双注册）。
- `cargo test --workspace`（桌面）：**107 通过 / 0 意外失败**；仅
  starfish-video 5 个真样本依赖测试失败——缺
  resources/videos/sample-5s.mp4（原仓库即缺的存量，样本补入后自愈）。
- 拆分中测试配套修正：font 测试字体路径改 `CARGO_MANIFEST_DIR` 拼接
  （字体资产已随迁 examples/resources）；colors.json 改由 starfish-pygame
  编译期内嵌（`COLORS_JSON` 常量，三平台零 IO 语义不变）；audio 增
  `#[doc(hidden)]` 测试面（`StreamVoice::new_for_test` 等，供 video 音轨泵
  跨 crate 测试直构，内部环实现保持 pub(crate)）。
- wasm32 / Android / Linux 目标：`cargo check --workspace --all-targets
  --target wasm32-unknown-unknown` **全绿**（VS BuildTools 装完后经
  scripts/vs-cargo.bat 验证；注意本机须在 VS 开发者环境跑链接类操作，
  裸 bash 下 link.exe 解析会退化到 Git 自带 GNU link）。拆分中 wasm 侧
  补的依赖边：permission→std（ensure_async 诊断打点）、gamepad→web-sys、
  render→web-sys（render_entry canvas 直传）；video 真样本测试模块加
  `not(wasm32)` 门控（原 `#[cfg(test)]` 在 wasm 目标误入编译）。Linux
  交叉检查 Windows 主机不可行（CI linux.yml 承担）；Android 出包脚本随
  pygame-rs。
- **运行验证**：`cargo run -p examples --example pygame_test_rect` 实跑
  **600 帧完成、自检 8/8 全绿**、干净退出——拆分后运行链路（窗口/渲染/
  自检套件/资产）端到端可用。
