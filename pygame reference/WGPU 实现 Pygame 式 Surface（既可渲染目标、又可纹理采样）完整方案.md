# WGPU 实现 Pygame 式 Surface（既可渲染目标、又可纹理采样）完整方案

## 核心概念对齐

Pygame `Surface` 本质：一块可绘制、可作为贴图贴到别的画布的像素缓冲区。
对应 WGPU 里的资源组合：

1. `Texture`：像素存储载体（RGBA 画布数据）

2. `TextureView`：**视图**，分两种用途：

    - RenderAttachment 视图：用于 `RenderPass` 绘制写入（作为渲染目标）

    - ShaderSampled 视图：用于着色器采样（作为贴图 /blit 源）

3. 同一张 `Texture` 可以生成两类不同用途的视图，实现「既能画上去、又能读出来贴别的地方」，完美对标 pygame Surface。

## 一、核心规则（WGPU 硬性限制，必须遵守）

1. **资源用途创建时固定**
创建 Texture 时必须同时声明两种用途：

    ```rust
    let usage = TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING;
    ```

    - `RENDER_ATTACHMENT`：允许作为渲染目标（画到这个 Surface）

    - `TEXTURE_BINDING`：允许作为纹理采样（把这个 Surface 贴到别的画布）
    缺任意一个，对应功能直接报错。

2. **读写不能同时进行，必须加内存屏障**
同一帧流程：

    1. 往 Surface A 绘制（写阶段）→ 结束当前 RenderPass

    2. 插入屏障 `encoder.insert_texture_barrier(&tex)`

    3. 在另一个 RenderPass 里采样 Surface A（读阶段）
    不插屏障会触发验证层报错、画面错乱。

3. 交换链主屏幕 Surface 特殊限制
窗口 SwapChain 的 Texture **不允许 ****`TEXTURE_BINDING`**，无法采样屏幕；
所以屏幕不能作为贴图 blit，想要截取屏幕画面，必须先渲染到**离屏 Surface**，再采样该离屏 Surface。

## 二、封装你的 Surface 结构体（适配之前 starfish/base/render）

```rust
use wgpu::{Texture, TextureView, TextureUsages, RenderPassColorAttachment, Operations, LoadOp, StoreOp};

pub struct Surface {
    width: u32,
    height: u32,
    // 底层像素存储
    tex: Texture,
    // 预缓存两种视图，避免每次新建开销
    render_view: TextureView,
    sample_view: TextureView,
    // 3D专用深度缓冲（可选）
    depth_tex: Option<Texture>,
    depth_view: Option<TextureView>,
}

impl Surface {
    /// 创建通用2D/3D离屏Surface（支持渲染+采样）
    pub fn new(
        device: &wgpu::Device,
        width: u32,
        height: u32,
        enable_3d: bool,
    ) -> Self {
        // 关键：同时开启渲染写入 + 纹理采样
        let tex_desc = wgpu::TextureDescriptor {
            label: Some("surface_color_tex"),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_SRC,
            view_formats: &[],
        };
        let tex = device.create_texture(&tex_desc);

        // 视图1：用于渲染写入（RenderPass附件）
        let render_view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        // 视图2：用于着色器采样/blit
        let sample_view = tex.create_view(&wgpu::TextureViewDescriptor::default());

        // 3D深度缓冲
        let (depth_tex, depth_view) = if enable_3d {
            let depth_desc = wgpu::TextureDescriptor {
                label: Some("surface_depth_tex"),
                size: tex_desc.size,
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Depth24PlusStencil8,
                usage: TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            };
            let dt = device.create_texture(&depth_desc);
            let dv = dt.create_view(&wgpu::TextureViewDescriptor::default());
            (Some(dt), Some(dv))
        } else {
            (None, None)
        };

        Self {
            width,
            height,
            tex,
            render_view,
            sample_view,
            depth_tex,
            depth_view,
        }
    }

    /// 获取用于渲染写入的视图（begin_render_pass 用）
    pub fn render_target_view(&self) -> &TextureView {
        &self.render_view
    }

    /// 获取用于采样/blit的纹理视图（画到别的Surface时使用）
    pub fn sample_texture_view(&self) -> &TextureView {
        &self.sample_view
    }

    /// 生成当前Surface的颜色附件，用于开启RenderPass绘制
    pub fn color_attachment(&self, clear_color: wgpu::Color) -> RenderPassColorAttachment {
        RenderPassColorAttachment {
            view: self.render_target_view(),
            resolve_target: None,
            ops: Operations {
                load: LoadOp::Clear(clear_color),
                store: StoreOp::Store,
            },
        }
    }

    /// 生成深度附件（3D专用）
    pub fn depth_stencil_attachment(&self) -> Option<wgpu::RenderPassDepthStencilAttachment> {
        self.depth_view.as_ref().map(|v| wgpu::RenderPassDepthStencilAttachment {
            view: v,
            depth_ops: Some(Operations { load: LoadOp::Clear(1.0), store: StoreOp::Store }),
            stencil_ops: Some(Operations { load: LoadOp::Clear(0), store: StoreOp::Store }),
        })
    }

    /// 插入屏障：写完后切换为可读采样状态
    pub fn barrier(&self, encoder: &mut wgpu::CommandEncoder) {
        encoder.insert_texture_barrier(&self.tex);
    }
}
```

## 三、完整渲染流程示例（对应你之前 `with surface as rp` 语法）

### 场景：surface1 绘制几何体 → 屏障 → surface2 采样 surface1 贴上去

```rust
// 全局单帧唯一encoder（你的架构规范）
let mut encoder = device.create_command_encoder(...);

// 1. 绘制 surface1（写入阶段）
{
    let color_att = surface1.color_attachment(wgpu::Color::BLACK);
    let depth_att = surface1.depth_stencil_attachment();
    let mut rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        color_attachments: &[Some(color_att)],
        depth_stencil_attachment: depth_att,
        ..Default::default()
    });
    rp.draw_3d_model(...); // 在surface1绘制3D物体
    rp.end();
}

// 2. 关键屏障：告知GPU，surface1写入完成，可以读取采样
surface1.barrier(&mut encoder);

// 3. 绘制 surface2，采样 surface1 作为贴图
{
    let color_att = surface2.color_attachment(wgpu::Color::BLACK);
    let mut rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        color_attachments: &[Some(color_att)],
        depth_stencil_attachment: None,
        ..Default::default()
    });
    // blit 逻辑：绑定 surface1.sample_texture_view() 到着色器贴图单元
    rp.blit_texture(surface1.sample_texture_view(), (0,0));
    rp.end();
}

// 提交指令
queue.submit([encoder.finish()]);
```

## 四、结合你之前 `ScopedRenderPass`（Drop 自动 end）封装

`Surface` 提供 `draw()` 方法返回带自动 end 的作用域 RP，内部自动取`render_target_view`；
外部 blit 时调用 `surface.sample_texture_view()`，分离读写视图，完全隔离两种用途。

```rust
impl Surface {
    pub fn draw<'a>(&'a self, encoder: &'a mut wgpu::CommandEncoder) -> ScopedRenderPass<'a> {
        let color_att = self.color_attachment(wgpu::Color::BLACK);
        let depth_att = self.depth_stencil_attachment();
        let rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            color_attachments: &[Some(color_att)],
            depth_stencil_attachment: depth_att,
            ..Default::default()
        });
        ScopedRenderPass::new(rp)
    }
}

// 使用（你想要的with语法）
let mut enc = ...;
{
    let rp = surface1.draw(&mut enc);
    rp.draw_rect(...);
} // 离开作用域自动 rp.end()

// 加屏障
surface1.barrier(&mut enc);

// 在surface2里采样surface1
{
    let rp = surface2.draw(&mut enc);
    rp.blit(surface1.sample_texture_view());
}
```

## 五、对标 Pygame Surface 行为差异总结

|功能|Pygame Surface|WGPU Surface 实现方案|
|---|---|---|
|绘制到自身|直接 `surface.fill()` / `draw.rect`|开启 RenderPass，使用 `render_target_view` 写入|
|作为贴图贴到其他画布|`target.blit(surf, pos)`|拿到 `sample_texture_view`，在另一个 RP 中采样，读写间插入屏障|
|屏幕主画布采样|可直接 blit 屏幕|交换链纹理禁止采样，必须先渲染到离屏 Surface 再采样|
|3D 深度支持|无原生深度|Surface 可选附带 depth 纹理，渲染时绑定深度附件|
|内存同步|CPU 自动同步|GPU 必须手动 `insert_texture_barrier` 同步读写状态|

## 六、性能优化点

1. 视图缓存：不要每次绘制 / 采样新建`TextureView`，构造 Surface 时预创建两个视图复用；

2. 屏障最小化：仅在「写完立刻要读」的两个 RenderPass 之间插入，无读写交替不用加；

3. 用途按需开启：纯 2D 静态贴图只开`TEXTURE_BINDING`，不用`RENDER_ATTACHMENT`节省资源；

4. 多重渲染目标 MRT：不属于通用 Surface，单独封装离线烘焙管线，不污染基础 Surface。

## 七、上层 Python 绑定适配

给 Python 层暴露两个接口，贴合 pygame 习惯：

1. `with surface as rp:` → 内部使用 render 视图，绘制当前 surface；

2. `rp.blit(target_surface, x, y)` → 内部取 target 的 sample 视图，自动在两次 RP 之间插入屏障（引擎底层封装，用户无感知）；
用户完全不用关心纹理视图、内存屏障，和原生 pygame 使用逻辑一致。

> （注：部分内容可能由 AI 生成）
