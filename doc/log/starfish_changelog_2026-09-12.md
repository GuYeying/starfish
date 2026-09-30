# Starfish 更新日志 2026-09-12

> 批次 1–7（示例修复 / 色度偏移 / 六平台硬解 / 模块重构 / Web+Android 后端 /
> feature 裁剪 / 手柄）见 `starfish_changelog_2026-09-11.md`。今日为文档与状态同步日。

## 批次 8：文档体系同步（README 现代化 + 进度活文档 + 跨会话记忆）

### 设计背景

库经历 SDL3 退役、视频六平台硬解、feature 裁剪、手柄接入四大变化后，
README 仍停留在 SDL3 时代（徽章/技术栈/项目结构含已删除的 subsystem/，
无视频/手柄/特性/循环模型描述）；且缺少一份"当前状态总览"视角的活文档
（按日 changelog 是历史视角，翻阅成本高）。

### 设计方案

- **README 全面现代化**：SDL3 → winit + cpal（含退役声明）；特性区新增
  视频六平台硬解 / 手柄 / 特性裁剪 / 引擎持循环模型；项目结构更新
  （subsystem 移除、video/gamepad/web/app 就位）；快速开始补
  `--features` 运行方式与特性化依赖引用示例；示例表补 11–14；技术栈
  补 winit/cpal/gilrs/mp4 与各平台硬解框架；路线图补 Phase2.5 迁移节点
  与设备接口路线
- **新建 `doc/starfish_开发进度记录.md` 活文档**：当前能力矩阵 / 特性矩阵 /
  批次索引（1–7）/ 实机验证清单（勾选框）/ 设备接口路线（摄像头→GPS→
  Android 桥）/ 技术债与已知边界——与按日 changelog 互补（历史 vs 现状）
- **跨会话记忆沉淀**：项目状态与用户决策偏好（硬解唯一、默认全包含、
  盲写+自测模式、按平台命名）写入持久记忆，后续会话免重新探查

### 关键保证

- 文档三件套分工明确：README（对外门面）/ 进度记录（现状+待办）/
  changelog（历史归档）——三者各司其职，引用闭环
- README 与 CLAUDE.md 结论一致性核对过（特性矩阵、命令、平台矩阵）

### 测试状态

- 纯文档变更，无代码影响；`cargo check --lib` / 测试基线不变
  （50 passed / 特性矩阵全绿，见 09-11 批次 6/7）

## 批次 9：示例目录化整理 + README 截图更新

### 设计背景

示例增至 14 个后平铺杂乱；新增多窗口与视频播放截图后 README 截图区待更新。
两个三角形示例（02 原生 / 11 Web）确认**保留各自独立不合并**（`web_entry!`
宏虽支持跨目标单文件，02 的双端化留待后续），Web 三角形新增专用截图。

### 设计方案

**示例分类目录**（编号前缀保留 = 学习路线不变；名称经显式 `[[example]]`
段映射保持稳定——`--example 02_triangles` 等所有既有命令照旧）：

```
examples/
├── basics/    01_hello_world  02_triangles          # 入门与循环模型
├── render/    03_texture  04_coord_system  05_storage_cube
├── draw/      06_draw_gfx  07_draw_text
├── audio/     08_play_sound  09_play_music_stream  10_record_mic
├── platform/  11_web_triangles  12_multi_window    # 平台能力
└── media/     13_video_decode  14_gamepad          # 媒体与设备
```

- 子目录内文件不参与 cargo 自动发现，全部 14 个示例改显式 `[[example]]`
  段（name 不变 + path 指向分类路径 + required-features 保留）
- `include_str!` 相对路径随目录层级修正（11/13 两处 `../` → `../../`）；
  其余示例的资源读取均为 CWD 相对（项目根运行），不受影响

**README 截图更新**：老三角形截图（02，7 月旧图）替换为 Web 三角形新截图
（11）；追加 12_multi_window 与 13_video_decode 两张新截图。

### 关键保证

- 示例名与全部文档/命令引用零变化；特性门控（06/07/13/14）行为不变
- 目录即分类文档：`examples/<类>/<编号_名称>.rs` 一眼可辨示例主题域

### 测试状态

- `cargo check --examples`（默认特性）与 `--features video` 全量：零 error
- `cargo run --example` 列表确认 14 个示例名称全部保持

## 批次 10：README 路线图重写（基于批次 1–9 实际记录）

### 设计方案

路线图从"计划式 Phase 平铺列表"重构为四段式：

- **✅ 已完成**（7 项里程碑带完成日期）：Phase 1/2、SDL3→winit+cpal 迁移、
  视频六平台硬解、feature 裁剪、手柄、文档体系
- **🚧 进行中**：四路后端实机验证清单、video v2 体验优化（GPU 采样转换/
  Web 流式/音轨注入）
- **📋 下一步**：Phase 3 接口文档（已起步）→ Phase 4 pygame 风格 API →
  设备接口第二批 → Phase 5 PyO3 → Phase 6/7/8
- **⛔ 阻塞项**：引擎安卓引导立项、统一权限模型（显性化自日志"已知边界"）

### 说明

- 与 `doc/starfish_开发进度记录.md` 互为镜像（README 精简版 / 进度文档详尽版）
