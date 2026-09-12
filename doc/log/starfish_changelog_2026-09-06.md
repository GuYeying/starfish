# Starfish 更新日志 — 2026-09-06

> 接续 2026-09-05 批次（time 初版 / font 数据侧 / 渲染五缺口 / 特性系统 / 文本管线）。
> 本批主题：**架构定位收敛**（单窗口回归、空间化划归开发者、maths 采纳 glam）
> 与 **四项增量**（time SDL3 化、font kerning、audio 单声道优化、多窗口接口标注）。
> **补录**：audio 流式播放 / 组总线 / 录音批次（详见「🔊 audio — 流式播放与录音系统」区块，
> 该批内容量大且未及入 09-05 篇，按用户要求补录于此）。
>
> 状态：`cargo test` 38/38 通过，示例编译零错误，新增依赖 0。
> 注：上一份日志中 time 的「std Instant」描述已被本批 SDL3 方案取代——按日志惯例保留原文，以本篇为准。

---

## 🔊 audio — 流式播放与录音系统（补录）

### Added：流式播放（长音频内存恒定方案）

- **`MusicStream`**：SDL3 回调线程（消费端）+ 解码线程（生产者）+ SPSC 环形缓冲
  - 单调 head/tail 原子指针；`generation` 换代计数防 seek 撕裂读（写前换代 + 读后校验）
  - 环容量约 2 秒（2 的幂对齐），音频线程全程无锁、无分配
- **解码线程**：symphonia 逐包解码 → 跨包线性重采样（与 `SoundData::resample` 同款插值）→ 写入环；
  缓冲灌满即 park（condvar + 超时兜底），EOF 排空余量后自动退出
- **`MusicPlayer` 流式后端**：`music_load_file` / `music_queue_file`（排队预起解码线程，无缝切歌）/
  `music_seek`（generation flush）/ 淡变 / 排队——混音循环零改动
- **`SymphoniaDecoder`**：OGG / MP3 / FLAC / WAV 自动探测（纯 Rust）
- **`SoundData::from_file`** + `track.rs → sound_data.rs` 重命名（与内容对齐）
- 示例 `09_play_music_stream`（流式 BGM + 排队切歌 + 淡入淡出）

### Added：组总线音量

- `GroupHandle` → `group_volumes` 密集向量（索引 = group_id − 1，无哈希开销）
- `set_group_volume` / `group_volume`；增益链统一为一处：
  `master × sfx_bus × group × channel_volume × fade`（未知组增益 1.0）
- 2 个测试（含软限幅全链路衰减数学验证）

### Added：录音（AudioRecorder）

- `AudioRecorder`：SDL3 录音回调（生产者）+ SPSC 环（消费者为主线程）——
  缓冲写满**丢弃新数据 + `dropped` 计数**，音频线程永不阻塞
- `save_wav`：拉空缓冲 → 16-bit PCM 立体声 WAV 导出
- `pause` / `resume` / `clear` / `available` / `device_names`（FFI 设备枚举）
- 示例 `10_record_mic`（录音 N 秒 → recording.wav）

### 硬件验证修复
- draw_text 示例顶点缓冲缺失 `COPY_DST` 用途，导致 `Queue::write_buffer` 验证错误
  （`MeshBuilder` 顶点缓冲补 `COPY_DST`）

---

## 🕐 time — 时间源重写为 SDL3 封装

### Changed
- 时间源从 `std::time::Instant` 迁移至 **SDL3 timer**：
  - 高精度读数改用 `performance_counter / performance_frequency`
    （OS 高精度单调钟：Windows QPC / 类 Unix CLOCK_MONOTONIC，不受毫秒粒度限制）
  - 节流睡眠改用 `timer::delay`（粗睡）+ 末段自旋的混合策略（保留）
- **API 形态不变**：`Clock`（frame / raw_delta / delta / elapsed / total_f64 / fps / frame_count / set_scale）、
  `FixedTimestep`（死循环钳制）、`sleep_until`、`tick`（Fifo 节流叠加警告保留）
- 约束文档化：时间原点 = SDL 初始化时刻，**须在 SDL 初始化后创建 `Clock`**
  （base 工程内 SDL 总是最先初始化，自然满足）

### 决策记录
- 选型理由：base 的依赖底座就是 SDL3 + wgpu，时间随之走 SDL3 是依赖一致性；
  SDL timer 的独有能力（`add_timer` 回调定时器）属事件系统，留给 `pygame/` 兼容层
- 8 个测试全部迁移至 SDL 时间轴（缩放冻结 / 固定步长 / 死亡螺旋钳制 / 节流下限等）

---

## 🔤 font — kerning 字距 + 定制排版缝

### Added
- **kern 字距**：
  - `Font::kerning(l, r)`：解析 Apple/OpenType `kern` 表（水平子表，跳过状态机子表；
    GPOS-only 字体字距退化为 0，不影响正确性——文档注明）
  - `GlyphAtlas.kerning: HashMap<(char, char), f32>`：构建时全对查询
    （**≤512 字形**才查；全量 CJK 类大字符集 kern 收益趋零且查询量爆炸，自动跳过）
  - `layout_text[_with]` 在字形落位前施加 kern（`layout_text` 自动带表）
- **`layout_positioned(atlas, &[(char, 位置)], scale)`**：复杂文字系统的定制缝——
  阿拉伯/天城文等先经外部 shaper（如 harfbuzz）整形，把 `(字形, 绝对笔位置)`
  序列喂进来即可；引擎不做任何排版假设

### 决策记录
- **图集缓存：不内置**——增量/驻留属 runtime + lifetime 高度定制的策略，
  交由上层按场景自建（`build_atlas` 文档注明「字符集变更即整集重建」）

### Fixed
- kern 施加时机错误（原实现落在字形落位**之后**，被布局测试当场抓出并修正——
  正确语义：施加于「前一字形推进之后、当前字形落位之前」）
- 相机正交矩阵改为手写（glam `orthographic_rh` 已弃用），列主序 4 行注释自文档

---

## 🔊 audio — SoundData 单声道内存优化（流式批次的后续优化）

### Added
- `AudioChannels::Mono(Vec<f32>)` / `Stereo(Vec<StereoFrame>)` 双形态：
  - **单声道源只存一份**（原实现复制成 L=R 双份）——游戏 SFX 大多为单声道，
    该类资源内存减半；立体声源行为逐字节不变
  - `SoundData::read_into(cursor, out)`：全库唯一的形态分派点，
    单声道在读取时展开为 L=R——混音循环 / 流式 / 效果器链依旧只面对 `StereoFrame`
- `resample` 双分支各自保持形态（单声道重采样单路，立体声插值两路）
- 3 个新测试：单份存储与展开、立体声两路独立保留、重采样保形

### 决策记录
- 流式环形缓冲保持统一立体声（容量以秒计有界，翻倍代价可控）——与
  SoundData 的省内存策略刻意不同（`worker.rs` 注释注明）
- **不在引擎内置 3D 空间化**：`AudioEffect` 缝已闭环（效果器可在增益链前
  独立改写 L/R），声学模型（光追声学 / 材质层级 / HRTF）归开发者按场景自建

---

## 🪟 render — 多窗口接口重新定位

### Changed
- `RenderEntry::surface_from_context` / `async_surface_from_context`
  文档定位为 **「仅开放，不具备开箱即用能力」** 的桌面进阶接口：
  - 模块级 `//!` 总声明（多窗口非官方支持形态，移动/鸿蒙表面语义不在承诺内）
  - 函数级 `///` 警示横幅（桌面进阶用法；单窗口请用 `RenderEntry::new`）
- 官方支持形态回归 **单窗口**（`RenderEntry::new` 主路径一字未动）

---

## 🧹 架构清理

- 移除未接入编译的 `base/maths` 骨架目录（16 个文件全部为空壳/注释拷贝，
  含决策记录 `记录.txt`：**数学层直接采用 glam**——依赖已备、bytemuck 特性已开）
- 移除空占位 `base/runtimes`
- 模块树收敛为 9 个有效模块：render / audio / font / time / window /
  subsystem / resources / color / error
- 决策记录：`draw` 图形渲染管线由开发者后续自建；`sprite` 层不做
  （pygame 用户实际多自建组件，prefab 不可定制是原版痛点）

---

## 🧪 Web 路线 spike 结论（决策记录）

emscripten 目标实测（emsdk 3.x + emcc 6.0.9 + Ninja + SDL3 3.4.12 源码构建）：

- ✅ **SDL3 C 库 × emscripten 构建链可行**：cmake 工具链文件 + Ninja 生成器 +
  emsdk node（架构探测需要）配置后，SDL3 全子系统完成 Emscripten 平台配置
- ❌ **sdl3 Rust 封装存在上游编译 bug**（0.18.4 与最新 0.20.0 均未修复）：
  `raw_window_handle.rs:118` 的 `XlibWindowHandle::new(window as u64)` 在
  emscripten 目标下 u64→c_ulong 类型不匹配——**Web 渲染路线被上游阻塞**
- 环境要求已固化至项目根 `activate_wasm.bat`（CMake/Ninja/node +
  `CMAKE_TOOLCHAIN_FILE_wasm32_unknown_emscripten` + Ninja 生成器变量）

### 可选路线（按优先级）
1. 向 sdl3-rs 上游提 issue（修复点明确：u64→c_ulong 一行转换），等待新版后解锁
2. `[patch.crates-io]` 指向自维护 fork，一行补丁自修（需长期维护 fork）
3. Web 降级为「逻辑库」（gfx 生成器 / 字体光栅化 / 音频数据 wasm 可编译，
   渲染后端待上游修复后接入）

---

## 📊 状态

- 测试 **38/38**（含 audio 流式/录音/组总线批次用例——SPSC 环 5、流式端到端 2、
  录音 3、组总线 2、重采样 4、单声道展开 3 等，明细见各区块）
- 示例 8，编译零错误（含 09_play_music_stream / 10_record_mic）
- 新增依赖 0
