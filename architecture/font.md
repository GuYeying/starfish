# font 架构（feature = "font"）

> 游戏内文本：自研扫描线光栅化（纯 Rust 零新增光栅依赖）→ 图集 →
> 标准 Mesh，任意管线直接绘制。

## 关键文件

| 文件 | 职责 |
|---|---|
| `font/mod.rs` | `Font`（ttf-parser 解析、`from_bytes/from_file`、度量、`rasterize_char`、`build_atlas`）+ 文本布局（`layout_text_local` / `layout_text_with` / `transform_vertices`） |
| `font/raster.rs` | 扫描线光栅化：非零环绕 + 4×4 超采样抗锯齿 |
| `font/pipeline.rs` | 文本渲染管线与图集绑定 |
| `font/text.wgsl` | 文本着色器 |

## 架构与数据流

```
ttf 字节 → Font(ttf-parser) → rasterize_char(扫描线+AA)
      → build_atlas(字符集 → shelf 打包 → RGBA8 图集上传 write_texture)
      → layout_text_* (kern 字距) → TextVertex(pos3+uv) → 标准 Mesh → 任意管线
```

- **图集热更新**：字符集变化重建图集（`write_texture` 局部更新）——
  probe_font 的 `DYN PASS rebuild x2` 验证动态文本路径。
- **pos3 顶点**：一套布局同时服务 2D（z=0）与 3D（世界坐标 + 深度变体
  管线）；`transform_vertices` 做矩阵变换。
- 非 ASCII 字符不在图集时以 `??` 替代显示（CJK 支持待立项，探针有此判读）。

## 公开 API 速览

`Font::from_bytes(data, size)` / `from_file`；度量 `size/ascent/descent/line_height`；
`build_atlas(chars)`；`layout_text_local(..)` → `Vec<TextVertex>`；
顶点布局 `Font::layout()`。

## 平台差异收敛点

零平台 cfg——纯 Rust 光栅化 + 标准 wgpu 纹理/管线。

## 设计纪律

图集 = 标准 `Mesh` + 标准纹理：**不设专属文本渲染路径**，文本就是普通几何
（复用引擎管线的深度/混合/MSAA 全部能力）。

## 测试锚点

probe_font `ATLAS PASS n=95` + `DYN PASS`（三平台）；CJK 过滤判读。

## 深入入口

`doc/log/` 字体批次（图集流水线 / kern / pos3 渐次落地记录）。
