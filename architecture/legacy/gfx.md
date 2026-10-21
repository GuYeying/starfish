# gfx 架构（feature = "gfx"）

> 几何绘制：16 种 2D/3D 形状生成器 + 填充/描边双管线；形状参数即物理
> 碰撞原语。自管几何的项目可整体剔除本模块。

## 关键文件

| 文件 | 职责 |
|---|---|
| `gfx/geometry/shape2d.rs` | rect(填充/描边)、circle、ellipse、line/polyline、regular_polygon、polygon（耳切三角化，凹多边形支持）、capsule 2D |
| `gfx/geometry/shape3d.rs` | cube、sphere、plane、cylinder、cone、capsule 3D |
| `gfx/geometry/tri.rs` | 耳切三角化 |
| `gfx/vertex.rs` | `ShapeVertex` |
| `gfx/pipeline.rs` | 填充（TriangleList）/ 描边（LineList）双管线，2D/3D 变体 |
| `gfx/shape.wgsl` | 形状着色器 |
| `gfx/mod.rs` | `Geometry`（含 `transformed(&Mat4)` 矩阵变换） |

## 架构与数据流

```
shape 函数(px 参数 + color) → Geometry → shape_mesh(&Geometry) → 标准 Mesh
    → fill_pipeline_2d/3d 或 outline 管线 → 普通 RenderPass 绘制
```

- **形状参数 = 物理碰撞原语**：AABB / OBB / Sphere / Capsule / Cylinder /
  Plane——渲染形状的构造参数就是将来物理层的形状描述。
- 叠层绘制：3D 场景 + 2D overlay 双管线同帧（probe_gfx 的 `depth + overlay`）。

## 公开 API 速览

`shape2d::{rect, rect_outline, circle, circle_outline, ellipse, line, polyline,
regular_polygon, polygon, capsule}`；`shape3d::{cube, sphere, plane, cylinder,
cone, capsule}`；`Geometry::transformed(&Mat4)`；`pipeline::{fill_pipeline_2d,
fill_pipeline_3d, outline_*}`（导出面见 `gfx/mod.rs`）。

## 平台差异收敛点

零平台 cfg——纯几何生成 + 标准管线。

## 设计纪律

产出一律是标准 `Mesh`：gfx 不设专属绘制路径，与字体/视频纹理同享引擎
管线能力（probe_gfx 升级即验证：overlay 自建双管线绘 4 形状）。

## 测试锚点

probe_gfx `BUILD PASS`（3d scene + 8 2d shapes）+ `DRAW PASS depth + overlay`。

## 深入入口

`doc/log/` 几何批次。
