# Starfish 更新日志 — 2026-09-05

本批围绕三个功能块：**time（时间系统）、font（字体系统）、render（渲染能力补充）**。
配套状态：`cargo test` 37/37 通过，全部示例编译零错误。

---

## 🕐 time — 时间系统（底层原语形态）

### Added
- `Clock`：帧时钟（度量 + 缩放）
  - `frame()` 每帧推进一次；`raw_delta()`（真实秒）/ `delta()`（缩放秒）分离
  - `elapsed() -> Duration` / `total_f64() -> f64`：启动以来总时长，f64 级精度不漂移
  - `set_scale()` 时间缩放：暂停（0）/ 慢动作（0.5），只影响逻辑时间
  - `fps()` EMA 平滑帧率、`frame_count()`
- `FixedTimestep`：固定步长累加器（确定性玩法更新），`max_steps` 死循环保护（默认 5，超出丢弃）
- `sleep_until()`：帧节流原语——混合策略（剩余 >2ms 走 OS 睡眠、末段自旋），规避 Windows ~15.6ms 定时器粒度超调
- `Clock::tick(fps)`：度量 + 节流的薄便利层，文档写明 **Fifo 呈现模式的节流叠加警告**

### 设计决策
- 时间源 = `std::time::Instant`（各平台即 QPC / CLOCK_MONOTONIC），**不启用 SDL timer 子系统**——时间独立于 SDL 生命周期，无头环境可测试
- **不对齐 pygame 语义**：返回秒（f32）而非毫秒；正典类型即 `std::time` 的 `Instant`/`Duration`；事件定时器（`set_timer` 类）留给将来 `pygame/` 兼容层
- 8 个单元测试（缩放冻结、固定步长、死亡螺旋钳制、节流下限等）

### Changed（示例层）
- 全部渲染示例统一接入 `Clock::tick(120)` 帧率控制
- `04_coord_system` / `05_storage_cube` 的手写 SDL timer 计时迁移至 `Clock`；`play_sound` 的裸 `thread::sleep(8ms)` 替换为混合节流

---

## 🔤 font — 字体系统（数据 + 渲染管线端到端）

### Added
- `Font`：`from_file` / `from_bytes`（ttf-parser 解析，自引用 SAFETY 处理，无新依赖）
- **自研字形光栅化** `font/raster.rs`：
  - 轮廓拍平（二次贝塞尔 8 段 / 三次 16 段细分）
  - **非零环绕**扫描线填充 + 每像素 4×4 超采样抗锯齿
  - 纯函数、零依赖、可独立测试（含"同心方块挖洞"非零环绕验证）
- `build_atlas()`：字符集光栅化 → shelf 打包 → 白字形 + alpha RGBA8 → GPU 上传
- `GlyphMetrics` / `GlyphAtlas`：UV、bearing、advance、ascent/descent/line_height
- `layout_text[_with]()`：文本 → 6 顶点/字形 的 `[x, y, u, v]`，支持 `\n`
- **`TextRenderer`** 文本渲染管线：
  - **实例持有，构建一次**（非单例、非逐次创建——管线类资源用法范式）
  - 内嵌通用 2D 着色器 `text.wgsl`（双 group 结构参考 hilbert-curve 教程：
    group0 场景相机 / group1 图集材质；像素空间正交投影，y 向下）
  - `set_camera` + `draw_text`（同帧多段文本独立顶点缓冲互不覆盖；
    图集 bind group 按 Arc 指针缓存，换图集免重建管线）
  - 白字形 + alpha 图集约定：管线乘以顶点色即可渲染任意颜色文字
- 示例 `examples/draw_text.rs`（120 FPS 帧控，多色多尺寸文本）

### 已知边界
- 未实现 kerning 字距调整（ttf-parser 有 kern 表接口，可后补）
- 图集无缓存（同字符集重复 build 会重复上传）
- 单字体文件，无多语言 fallback

### Fixed
- `draw_text` 顶点缓冲缺失 `COPY_DST` 用途导致 `Queue::write_buffer` 验证错误

---

## 🎨 render — 渲染能力补充

### Added：特性能力系统（许愿 → 掩码 → 暴露）
- `render/features.rs`：
  - 三层选型函数：`core()`（默认愿望 4 项）/ `recommended()`（高性价比 8 位）/ `special()`（特殊渲染场景 5 位）
  - **去掉清单**（6 位 + 回加条件）：MDIC、MULTIVIEW、16BIT_NORM、BGRA8UNORM_STORAGE、TIMESTAMP_INSIDE_×2
  - `resolve()` 掩码、`missing()` 缺差、`names()`、`report()` 启动报告
  - `recommended_limits()`：绑定数组配套上限（`max_binding_array_elements_per_shader_stage` 默认为 0，必须配此愿望）
- 边界声明：引擎只管「开/不开 + 暴露」，**不实现任何降级路径**；开发者以 `RenderContext::features()` 自查分支
- `RenderContext` 新增能力暴露：`features()` / `limits()` / `adapter_info()` / `device()` / `queue()` / `adapter()`

### Added：五个渲染 API 缺口补齐
- **绑定数组（bindless 地基）**：`BindGroupBuilder::texture_view_array` / `texture_array` / `storage_array`（layout count 自动推导，两遍构建避免借用冲突）
- **MSAA**：`SurfaceSettings::with_msaa(n)` + `RenderSurface` 全链路（MSAA 颜色纹理、深度采样数对齐、present 自动 resolve 到交换链、resize 重建）
- **查询**：`create_query_set` / `create_query_result_buffer` / `create_raw_buffer` / `write_timestamp` / `resolve_query_set` / `copy_buffer_to_buffer`（profiler 地基）
- **纹理局部更新**：`write_texture`（mip + origin + size + CPU 侧 layout；字体图集/视频帧/动态贴图）
- **多窗口**：`RenderEntry::surface_from_context` / `async_surface_from_context`（共享同一 instance/adapter/device/queue，不重复建 GPU 上下文）

### Changed
- `GpuSettings.required_features` → `desired_features`（默认 = `features::core()`）；
  `to_device` 语义改为「愿望掩码 + 上限钳制」——硬件不支持的位自动剥离，
  设备创建**永不因愿望失败**（wgpu 30 无 `optional_features`，掩码即等价实现）
- `RenderPipelineBuilder::wireframe` 补文档（需 `POLYGON_MODE_LINE`，默认愿望已含）

### Fixed
- **交换链自愈**：`begin_frame` 收到 `Outdated/Lost` 时自动重建配置与深度/MSAA 纹理并重试（最多 3 次），`Timeout/Occluded` 直接重试——**拖动窗口/最小化不再 panic**
- 修复 `Uniform`/`Sampler` 布局构建中的未使用变量告警

---

## 📊 批次总量

| 指标 | 批次前 | 批次后 |
|---|---|---|
| 单元测试 | 2 | **37** |
| 示例 | 5 | **8**（play_music_stream / record_mic / draw_text） |
| 新增依赖 | — | 0（全部基于现有依赖） |
