# Starfish / pygame-rs wgpu 渲染框架开发日志

-   项目方向：Rust + SDL3 + wgpu 的跨语言友好型渲染框架
-   目标：提供类似 pygame 的易用接口，同时保留 wgpu 底层能力
-   截止时间：2026-07-08

------------------------------------------------------------------------

# 一、早期验证阶段

## 0. 基础渲染验证

完成： - winit + wgpu 三角形测试 - SDL3 + wgpu 三角形测试

结论： - winit 存在侵入式事件循环问题，不适合作为核心封装层 - 转向 SDL3
作为窗口和系统抽象层

------------------------------------------------------------------------

# 二、框架初步设计阶段

## 1-8. 基础架构建立

完成： - 初步规划框架类型结构 - 对 wgpu 类型进行统一封装 - 使用 Config
简化初始化流程 - 提供 as_wgpu_ref / as_wgpu_arc 接口 -
优化命名规范并重新导出 wgpu 类型 - RenderContext 增加工厂式创建接口 -
App 默认初始化 video/audio/event 子系统 - 将 begin_frame、texture view
获取、present 移入 RenderContext

设计方向： - 对外隐藏复杂 wgpu 细节 - 保留原生 wgpu 获取能力 - 兼顾
Python / FFI 使用

------------------------------------------------------------------------

# 三、系统模块完善阶段

## 9-17. Subsystem 与资源体系

完成： - SDL subsystem 部分封装 - 移除低价值 resource 底层资源类型 -
audio 独立为大型模块

Audio 系统： - PlaybackCallback SDL 胶水层 - 1024 帧分片处理 -
限制单次计算避免 CPU 尖峰 - AudioUserCallback 统一上层接口 -
支持闭包、长音频播放器、混音器 - 支持 F32LE/F32BE - 双缓冲复用 -
音频回调无运行时堆分配

其他： - mipmap 基础生成 - SamplerConfig 补充 - RenderPipelineConfig /
SamplerConfig 与资源类型分离 - 移除 RenderContext 中 layout 请求 - 完成
LearnOpenGL 示例验证 - 优化 event 注册接口 - Settings 扩展 - 替换 unwrap
为错误处理机制

------------------------------------------------------------------------

# 四、Starfish 架构重构

## 18-20. 项目重新规划

Starfish 作为 pygame-rs 别名独立设计。

模块：

-   base
-   ext
-   pygame
-   resources
-   utils

base 分类：

-   gfx
-   core
-   font
-   graphics
-   image
-   maths
-   render
-   shapes
-   time
-   color

重大调整：

-   删除 App 高度集成模式
-   保持 pygame 与 SDL 系统独立
-   graphics 模块废除
-   顶点生成归入 mesh
-   shapes 移入 maths
-   render 提升为核心模块
-   starfish 独立 crate

------------------------------------------------------------------------

# 五、Render 核心设计阶段

## 21-24. GPU资源与材质系统

完成：

Shader： - 删除 ShaderModule 封装 - 使用原生 wgpu ShaderModule

Texture： - 添加 ViewKey - TextureDesc 枚举化重构 - mipmap 调整

Mesh： - 初步完成自动推导流程

资源结构调整： 删除： - bind_group_layout - bind_group -
pipeline_layout - render_pipeline - sampler - shader_module

原因： - 避免 Arc 多层嵌套 - 减少无意义包装

Material： - 完成自动 layout 推导 - mesh + pipeline + material
图片渲染测试成功

Uniform： - 完成 Uniform 数据打包 - 支持 alignment / size / stride -
支持 dirty 更新 - 支持 offset 修改 - UniformBlock 完善

------------------------------------------------------------------------

# 六、Lingyu 与 Starfish 合并

## 25-27. Render API 优化

完成：

模块： - rendercontext - texture - sampler - buffer

统一进入 render 模块。

调整： - UniformBinding 使用封装 Buffer - Mesh 顶点允许共享 - index
buffer 不共享 - RenderPass Descriptor 抽象

RenderPass： - Attachment 封装 - 自动检测合法性 - RenderPass 接受 Mesh /
Material / Pipeline

Material： - 移除 Entry + Vec 结构 - 改为直接保存： - uniform -
sampler - texture

Uniform： - 限制一个 Material 一个 Uniform

RenderContext： - GPU 不可变资源配置化 - Instance / Adapter / Device
配置封装 - Surface 配置封装 - 初始化接口优化

------------------------------------------------------------------------

# 七、BindGroup 与 Compute 支持

## 28-30. GPU接口稳定化

完成：

BindGroup： - Material 更名为 BindGroup - 支持计算着色器 - 支持默认
group0 - UniformLayout 增加 compute 默认接口

RenderPass： 新增： - draw_indirect - draw_indexed_indirect - viewport -
scissor - stencil reference - blend constant - instanced draw

Pipeline： - 修复多个 bind group 支持

Buffer： - StorageBuffer - UniformBuffer - 数据写入测试成功

Field 系统： - UniformValue 更名 FieldValue - 新增 FieldType - 自动获取
GPU size/alignment

Builder： - Builder 改为基于 Context 创建 - 隐藏 device 依赖

Mesh： - Semantic / Attribute 改为 Field 风格 - Index 类型细分 -
支持空间压缩

------------------------------------------------------------------------

# 八、当前阶段优化

## 31. Frame 提交与 Pipeline 定制，RenderContext解构

完成：

RenderSurface & RenderResourceAccess: RenderContext直接拆分为两个功能, 分别用于资源创建和主缓冲区处理

Buffer： - Storage / UniformBuffer 基于 RenderResourceAccess 创建

Depth： - 移除默认深度缓冲 Option - begin_frame 支持 depth_clear

Command： - RenderContext 增加 command buffer 中间缓存 - 延迟 submit -
present 时统一提交

Pipeline： - RenderPipelineBuilder 增加更多接口 - 支持 2D / 3D
默认管线 - 支持高度定制




------------------------------------------------------------------------

# 当前架构状态总结（2026-07-08）

核心设计：

    SDL3
     |
    RenderSurface & RenderResourceAccess
     |
     +-- Device / Queue / Surface
     |
     +-- Command Collection
     |
    Render Pipeline
     |
     +-- Pipeline
     +-- BindGroup
     +-- Mesh
     +-- Texture
     +-- Uniform / Storage Buffer
     |
    Renderer

设计原则：

1.  对外 Concrete API 优先
2.  避免公开 Trait
3.  支持 Python / FFI
4.  保留原生 wgpu 能力
5.  CPU Resource 与 GPU Resource 分离
6.  RenderResourceAccess 负责 GPU 生命周期
7.  Builder 负责构造复杂对象
8.  CommandBuffer 延迟提交





7.13 补全音频功能：
    添加audio 基本功能：
        channel
        group
        effect
        standard behavior
    