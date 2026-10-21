# starfish/base 模块修正梳理文档（区分 mixer 音频混音 \+ 图层合成重命名）

## 修正说明

原目录里 `mixer` 实际对应 Pygame `pygame.mixer` 音频混音模块，**不是画面图层混合**；画面合成 / 图层合并单独重命名新模块避免混淆。

## 完整修正后目录结构

```Plain Text
starfish/base
├─ mod.rs          # 统一导出所有基础类型、对外入口
├─ core/           # 全局上下文、生命周期、基础Trait、资源句柄、错误定义
├─ maths/          # 2D&3D通用数学：向量、矩阵、四元数、相机、包围盒、几何运算
├─ color.rs        # 统一Color类型、色彩空间转换、THECOLORS颜色名映射(JSON软加载)
├─ time/           # 帧时间、deltaTime、计时器、帧率统计、时间工具
├─ buffer/         # GPU缓冲统一封装：顶点/索引/统一/存储缓冲、内存映射
├─ texture/        # 纹理底层封装、纹理视图、纹理用途、采样器、资源复用池
├─ render/         # 核心渲染层：Surface、ScopedRenderPass、CommandEncoder、ComputePass、渲染附件、屏障管理
├─ shapes/         # 基础预制几何体网格：矩形、圆、立方体、球体、平面；输出顶点缓冲
├─ image/          # 图片文件加载、像素解码、CPU像素读写、贴图转Texture工具
├─ font/           # 字体解析、字符图集生成、文字顶点生成、文本渲染基础
├─ gfx/            # 基础渲染管线封装：2D精灵管线、3D基础模型管线、计算管线缓存
├─ graphics/       # 绘图基础API封装：draw_rect/draw_sprite/draw_model/blit 上层绘图接口
├─ composite/      # 【新增】画面图层合成、后处理、多层Surface混合、混合模式（原计划画面mixer更名）
├─ mixer/          # 音频混音模块（对应pygame.mixer）
└─ music/          # 废弃合并进 mixer，删除该目录
```

## 模块职责拆分（重点修正 mixer /composite/music）

### 1\. mixer（音频混音，对标 pygame\.mixer）

定位：底层音频基础，2D/3D 游戏通用音效、背景音乐管理

1. 音频设备初始化、全局音频上下文；

2. Sound 短音效：加载、播放、暂停、循环、音量；

3. Music 长背景音乐流：加载文件、播放 / 停止 / 淡入淡出、轨道控制；

4. 声道管理、全局音量、音效分组；

5. 音频格式解码封装（wav/ogg 等基础格式）；

> 原独立 `music` 目录职责全部合并到 `mixer`，删除冗余 music 文件夹，统一音频入口。
> 
> 

### 2\. composite（画面图层合成，替代原画面 mixer 概念）

定位：渲染配套图层合并工具，依赖 render/texture，纯图形无音频逻辑

1. 多 Surface 按 Z 序批量合并渲染；

2. Alpha / 加法 / 乘法等混合状态封装；

3. 全屏基础后处理：灰度、亮度、模糊、色调；

4. 临时帧缓冲池复用，减少显存重复分配；

5. 屏幕多图层批量输出辅助工具；

> 完全和音频 mixer 隔离，命名无歧义，新人不会混淆音视频模块。
> 
> 

## 其余模块简要职责（不变，仅精简）

### core

全局帧上下文、资源句柄、Drop 自动释放规范、全局 wgpu 设备单例、统一错误。

### maths

唯一 2D/3D 通用数学库，Vec/Mat/Quat、正交 / 透视相机、包围盒、坐标变换。

### \[color\.rs\]\(color\.rs\)

对标 pygame\.Color，RGBA 色彩运算、lerp、预乘 alpha、JSON 动态加载 THECOLORS 颜色表。

### time

帧间隔、计时器、帧率统计，全引擎统一时间源。

### buffer

各类 GPU 缓冲底层封装，顶点 / 索引 / Uniform 缓冲分配复用。

### texture

纹理创建、双用途视图（渲染目标 \+ 采样贴图）、采样器、图片转纹理。

### render

核心底层渲染：Surface、带自动 end 的 ScopedRenderPass、指令编码器、纹理读写屏障、计算通道。

### shapes

基础几何网格生成（矩形、球体、立方体等），产出可复用顶点缓冲。

### image

CPU 端图片加载、像素读写、颜色转换，对接 texture 生成贴图。

### font

字体解析、生成文字图集、文本顶点生成，供 graphics 绘制文字。

### gfx

预编译基础渲染管线缓存：2D 精灵、纯色矩形、3D 模型、Compute 像素着色管线。

### graphics

上层统一绘图 API，封装 draw/blit 接口，对外提供贴近 pygame 的简易绘图函数。

## 依赖分层规范（无循环依赖）

1. 无 GPU / 音频纯工具层：maths、color、time

2. 全局核心层：core（依赖纯工具层）

3. GPU 资源层：buffer、texture、shapes、image、font（依赖 core、maths、color）

4. 渲染底层与管线：render、gfx（依赖资源层）

5. 上层绘图与合成：graphics、composite（依赖 render、texture）

6. 音频独立分支：mixer（仅依赖 core、time，不依赖任何图形模块）

## 优势

1. 命名无歧义：`mixer` = 音频混音，`composite` = 画面图层合成，完全对应 pygame 原生模块概念；

2. 消除冗余：废弃单独 music 目录，音频逻辑统一收纳在 mixer；

3. 分层清晰：图形、音频两条支线完全解耦，互不依赖；

4. 兼容 2D/3D：所有基础模块不区分 2D/3D，一套底层支撑两种渲染场景。

> （注：部分内容可能由 AI 生成）
