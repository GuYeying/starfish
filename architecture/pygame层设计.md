# starfish pygame 层设计契约（定稿 v1.0）

> 2026-09-27 评审通过，**已定稿**。作为实施依据，与代码不一致时以
> 本文档修订版为准。
>
> 输入：`pygame模块架构分析.txt`（模块取舍与 Surface 收拢思路）、
> 《reference/Rust SDL3 + WGPU 复刻 Pygame 库推荐方案》（选型清单，
> SDL3 部分作废，轻量原则有效）。
>
> **v0.3 变更**：①依赖关系定约为双向规则（pygame 可直接使用 starfish
> 类型入接口；starfish 永不耦合 pygame）；②渲染管线收拢为**单一通用
> 管线**（图形学底层数据无 2D/3D 之分，camera 即 MVP）；③新增
> **通用数据桥**（吸取 surfarray 家族的底层互换类型经验，BufferProxy
> 定型）；④`get_screen()` 形态拍板（&'static + 内部 RefCell）。
>
> **v0.2 变更**：Surface 路线定 **GPU**（舍弃大部分 CPU 像素级处理）；
> Surface 封装 = RenderTarget + Texture；新增 **render 创新模块**；
> screen 与 Surface 本质区别解法定型；像素格式定 Rgba8Unorm。

---

## 一、定位与目标

1. **Rust 用户的 pygame**：接口形态 1:1（`display.set_mode /
   event.get / draw.rect / display.flip`），教程代码逐行可读。
2. **Python 绑定层的映射目标**：pygame-rs 的 Python `pygame` 包直接
   绑定本层，绑定层只做类型搬运（GIL 单线程 ≡ 引擎主线程契约）。
3. **render 创新模块**：为 pygame 设计的 2D/3D 通用渲染管线——
   与 `font`（文字）/`gfx`（几何）同辈的引擎能力模块；原版 pygame
   接口全部基于此实现。
4. **非目标**：非 pygame 源码移植，不追求 100% 函数覆盖；覆盖
   pygame 教程与典型 2D 原型核心面（对照表 §五）。

## 二、设计原则

1. **接口形态保真，执行 GPU 化**：v0.1 的"语义保真"修正为——API
   长得像 pygame（即时、逐调用），执行交给 GPU（命令式、批处理）；
   收拢方案见 ADR-5。
2. **轻量**：管线基于既有 `base::render` 核心与 `gfx` 家底，零新增
   第三方依赖。
3. **依赖关系定约（双向规则，硬性）**：
   - `pygame → starfish`：pygame 层允许**直接使用 starfish 类型入
     接口**（`base::render`/`ImageData`/`audio` 等直接出现在 pygame
     的签名、参数与返回值里），不做多余的转译壳；
   - `starfish ↛ pygame`：starfish（base/）**永不引用、永不感知
     pygame 类型**——base 的任何模块（含将来的新能力）都以
     font/gfx/audio 式独立立项，不通过 pygame 反哺；
   - 结论：starfish 保持独立干净（不跑 pygame 层时零耦合、可单独
     发布使用）；pygame 是 starfish 之上的兼容层 + 管线扩展层。
4. **运行循环归调用方**：延续 poll 模型，零控制反转。
5. **接口策略**（来自架构分析）：get 类/底层信息类接口留**默认空
   实现**保证不报错，另提供自有替代（如 wgpu 后端信息查询接口）。

## 三、核心架构决策（ADR）

### ADR-1：Surface 路线 = GPU（RenderTarget + Texture）✅ 已定

- **舍弃**：CPU 像素级处理大部分（get_at/set_at 即时像素读写、
  surfarray/pixelarray 的 CPU 直改语义）；
- Surface = GPU 纹理 + 渲染目标视图的封装；blit = 纹理 quad 绘制；
- 代价已接受：像素级接口要么舍弃、要么走 GPU 回读（慢路径，v2+
  评估，见 §五对照表标注）。

### ADR-2：Screen 与 Surface 的本质区别 —— `DrawTarget` 统一、背板不同

`set_mode` 返回的对象**不是 Surface**：它背板是交换链，Surface 背板
是纹理。本质差异与统一方式：

| | `Screen`（set_mode 产物） | `Surface`（纹理背板目标） |
|---|---|---|
| 背板 | swapchain（窗口呈现） | GPU 纹理 |
| 绘制 | ✅ fill/blit/draw（DrawTarget） | ✅ 同左 |
| 作 blit 源 | ❌（呈现缓冲不回读、不采样） | ✅ 纹理采样即 blit |
| 像素读（get_at） | ❌ 无意义（GPU 帧缓冲） | v1 ❌ / v2 readback 慢路径 |
| 生命周期 | 进程唯一（display 全局槽） | 用户自由创建 N 个 |
| 归属方法 | flip/present 属于它 | 可整只 blit 到别处 |

```rust
pub trait DrawTarget {                 // 绘制终点统一抽象
    // v1.1 修订：&self + 内部 RefCell（ADR-6 get_screen 形态拍板的
    // 连带结论，以 ADR-6 为准；实施见 P2，changelog 09-27 批次六）
    fn fill(&self, color: impl Into<Color>) -> Rect;
    fn blit(&self, src: &Surface, dest: impl Into<Coord>) -> Rect;
    fn size(&self) -> (u32, u32);
}
impl DrawTarget for Screen {}          // 交换链背板
impl DrawTarget for Surface {}         // 纹理背板
// draw::* 全部接受 &impl DrawTarget → 教程里 draw.rect(screen,..)
// 与 draw.rect(surface,..) 同一签名，形态保真
```

这同时回答架构分析提出的"多目标缓冲区"问题：每个 Surface 一个独立
渲染目标，DrawTarget 天然支持任意数量目标；MRT（单 pass 多输出）
属管线高级特性，按需后置。

### ADR-3：像素格式 = Rgba8Unorm（已定，最通用）

- 全后端（dx12/metal/vulkan/gles/webgl2/webgpu）保证支持；图像加载
  输出一致；混合语义直白；
- gamma 空间直出（pygame 无色彩管理，对齐其行为），v2 需要时再引入
  sRGB 视图；
- swapchain 格式由表面能力决定，管线直绘 swapchain 不做格式转译。

### ADR-4：render 创新模块（单一通用管线，camera 即 MVP）

**定位**：与 `font`/`gfx` 殊途同归的引擎能力模块——font 管文字、
gfx 管几何、render 管"pygame 的绘制世界"。位于 pygame 层底座
（`pygame/render`），按 base 风格设计，starfish 侧零感知（§二
依赖定约）。

**单一通用管线，不分 2D/3D**：图形学底层数据没有 2D/3D 之分——
一切绘制都是"顶点 + MVP 变换 + 材质"走同一条管线；2D 只是正交
MVP + 精灵 quad，3D 只是透视 MVP + mesh。因此**不设 Camera2d/
Camera3d 两套抽象**，camera 收拢为一个类型：

```rust
pub struct Camera { /* MVP 组装：view + projection */ }
impl Camera {
    pub fn ortho(...) -> Self;        // 2D 场景（像素坐标 → 裁剪空间）
    pub fn perspective(...) -> Self;  // 3D 场景（v2 内容）
    pub fn mvp(&self) -> Mat4;        // 唯一上传的变换量
}
```

这是"为 pygame 创建一个模块简化轮子量"的落点：精灵/图元/mesh 全部
只管提交几何，变换统一交给 `Camera::mvp()`，用户不手写矩阵。

```text
pygame API 面（display / draw / Surface.blit …）      ← 形态即 pygame
──────────────────────────────────────────────
pygame::render 单一通用管线（创新模块，本契约 §六）
  ├─ Texture          GPU 纹理 + 采样（from_rgba8 / from_image）
  ├─ RenderTarget     screen-bound / texture-bound 统一终点
  ├─ Batch            顶点批处理（纹理切换即 flush；quad 与 mesh 同路）
  ├─ Camera           MVP 组装（ortho/perspective 同型）
  ├─ Material         材质/管线抽象（"万能基础着色器"落地位）
  └─ DataBridge       通用数据桥（ADR-10：BufferProxy 定型）
──────────────────────────────────────────────
base::render 核心（RenderEntry / RenderSurface / 资源访问）——已有
```

- 实施仍分步（先 2D 内容后 3D 内容），但那是**排期**不是**架构**：
  管线从第一天起就是顶点+MVP+材质的统一形态，3D 内容零改造接入；
- `gfx` 家底复用：`shape2d/shape3d` 图元生成移植为 batch 几何来源。

### ADR-5：即时 API × 命令式执行的收拢（回应架构分析的思考）

pygame 即时、wgpu 命令式，收拢方案 = **即时语义 + 延迟提交**：

- `draw::rect(screen,..)` / `screen.blit(..)` 即时调用，内部**录制进
  该 DrawTarget 的 batch**；`flip()`（Screen）或显式/自动 flush 时
  统一提交 GPU——调用方全程无感，得到"每调用即生效"的 pygame 心智；
- txt 设想的 `with surface:` + Drop 魔术方法扔队列：Rust 侧等价物为
  **即时 API + 自动 flush**（纹理切换或目标切换即触发），无需用户写
  上下文；`Surface.flush()` 作为显式控制点保留；
- 多目标：每个 DrawTarget 独立 batch，互不串扰（见 ADR-2）。

> **v1.2 修订（2026-09-27 批次九，用户反馈驱动）**：补 **Rust 版 with
> 语法**——`screen.render()` / `surface.render()` 产出会话对象
> （`ScreenFrame`/`SurfaceFrame`），绘制指令经会话录入，`end()`/Drop
> 完成指令打包（编码挂入待提交队列，flip/flush 上屏；漏写 end 由 Drop
> 兜底——Python `__exit__` 的 Rust 对应物）。即时 API 保留为 pygame
> 兼容形态，两形态共用同一 batch；`DrawTarget` trait 文档显式声明
> "指令打包 + 延迟提交"语义。动机：显式打包边界让命令式执行对开发者
> 可见，避免即时长相引起误读。
>
> **v1.3 修订（2026-09-27 批次十五，用户提案）**：会话三类型统一为
> **`RenderPass<'a>`** 一型（Screen/Surface/MRT enum 分派）——与 base
> 的 `begin_render_pass → RenderPass → end()` 设计语言同构，Python
> `with` 的返回对象即此一型（一个 pyclass 通吃三形态）；顺带修复 MRT
> 会话 fill/blit 绕道各 Surface 自身 batch 导致的输出覆盖问题。

### ADR-6：display = 单窗口 + 模块级全局槽（v0.3 拍板）

`set_mode` 一次性建窗 + GPU 装配（`Window::builder` + `RenderEntry`）。
`get_screen()` 形态定案：**`&'static Screen` + Screen 内部 RefCell**
（绘制方法 `&self`）——教程代码顺（`let screen = display.get_screen();`
拿一次到处用），Python 绑定层零翻译；线程安全不设防（引擎主线程
契约 + debug_assert 兜底）。`flip()` 归 display（即 Screen 的
present）。

### ADR-7：event / key / mouse / locals

- event：`poll_event` → pygame 词汇 `Event` 枚举薄翻译，get/poll/clear
  对齐；fastevent 不做（激进处理：只留 event）；
- key/mouse：翻译时顺带维护状态表（引擎事件唯一事实源；不引入额外
  维护对象——回应架构分析的关注点）；
- **locals**：常量枢纽模块，按子模块组织（`locals::key::K_a`、
  `locals::mouse`），兼顾 pygame 兼容与可维护性；key 常量命名定
  `K_a` 风格（pygame 迁移优先，绑定层零翻译）。

### ADR-8：坐标系与缩放（保留 v0.1）

1 pygame px ≡ 1 surface 逻辑像素；web 上 canvas CSS 尺寸 = set_mode
尺寸（HiDPI 由引擎钉样式机制衔接，上采样显示 ≡ pygame 原生行为）；
窗口 resize 不改 display 尺寸（只 set_mode 定）。

### ADR-9：错误处理（保留 v0.1）

资源加载类 `Result`；契约违例 panic（经 `app_entry!` panic hook 上报）。

### ADR-10：通用数据桥（BufferProxy 定型，吸取 surfarray 家族经验）

surfarray/PixelArray/pixelcopy 这些模块本身不做，但它们的**底层设计
经验必须吸取**：pygame 用一族底层通用类型实现 Surface/像素数组/音频
样本/缓冲区之间的无缝互转，这是后续众多模块的公共地基。据此定型
pygame 层的通用数据桥：

```rust
/// 各类数据（CPU 字节 / GPU 缓冲 / 音频样本）的统一视图与搬运载体。
/// 本体不拥有数据：或包 CPU 内存，或持 GPU buffer 引用 + 布局描述。
pub struct BufferProxy { /* 格式 + 尺寸 + 数据来源(CPU/GPU) */ }
impl BufferProxy {
    pub fn from_bytes(layout: Layout, data: &[u8]) -> Self;
    pub fn as_bytes(&self) -> Result<&[u8], Error>;        // CPU 侧直读
    // 转换家族（无缝互转的核心）：
    //   Texture  <-> BufferProxy   (GPU 回读 = 慢路径，显式调用)
    //   ImageData <-> BufferProxy  (resources/image 直通)
    //   音频样本  <-> BufferProxy  (sndarray 的地基，v2)
}
```

- 定位：**底层通用类型先行**——BufferProxy v1 只随 render 管线定型
  类型与 Texture↔CPU 最小路径；sndarray 等转换端点 v2 逐个挂上；
- 这正是"模块可不做、地基必须对"的落点：后续任何"数据从 A 形态到
  B 形态"的需求都走同一座桥，不再各造各的轮子。

## 五、模块对照表（v0.2，含架构分析的取舍决策）

图例：✅ v1｜🔁 v2+｜❌ 不做｜↪ 直用/合并｜🚫 舍弃（GPU 路线代价）

| pygame 模块 | starfish::pygame | 范围 | 说明 |
|---|---|---|---|
| display | `display` | ✅ | 接口更新版：GL 属性类 → 空实现兜底 + wgpu 后端信息自有接口 |
| Surface | `surface::Surface` | ✅ | RenderTarget+Texture 封装；🚫 get_at/set_at；v2 readback 评估。**P2 已落地**（`pygame::render::Surface`，changelog 09-27 批次六；P3 归位 surface 模块面） |
| draw | `draw` | ✅ | 抗锯齿不暴露（采样设置属 render 管线配置，回应架构分析） |
| gfxdraw | ↪ render 管线 | ❌独立 | 其"draw 底层"定位由 render 模块承担（激进处理） |
| event | `event` | ✅ | fastevent 不做（合并进 event） |
| key / mouse | `key` / `mouse` | ✅ | 状态表 + locals 常量；touch 同期薄暴露（starfish-window 已有） |
| locals | `locals` | ✅ | 常量枢纽，子模块组织 |
| time | `time` | ✅ | Clock（tick/get_fps）/ delay |
| Rect | `Rect` | ✅ | 回收旧实现（git HEAD v0.9.2 全套） |
| Color | `Color` | ✅ | 回收旧实现 |
| image | `image` | ✅ | 基于 `resources/image` 封装（load/save → Surface）；**save 已落地**（GPU 回读慢路径 + `base::io` 跨平台写：原生落盘 / Web POST；09-27 批次十二） |
| font | `font` | ✅ P4 | ttf-parser CPU 光栅化字形 → DataBridge → Texture → Surface（资源制备，不违背 GPU 路线）；提至 P4 与 image 同批（评审决策：文字优先跑通便于 debug） |
| transform | `transform` | ✅ | flip_x/flip_y/scale/rotate/rotozoom——GPU 采样实现；**P5 提前落地**（09-27 批次十一）；v2 余 nearest 采样变体位 |
| mask | `mask` | ✅ | 构建=一次性 GPU 回读 alpha 阈值位图（save 回读链路复用）；检测=纯 CPU pygame 全套语义；**P5 落地**（批次十三）；视觉遮罩(stencil)=v2 随自定义材质 |
| sprite | `sprite` | ✅ | **Godot 式重设计落地**（不 1:1 兼容）：池化 Group + 世代 id + (z,y) 排序会话绘制 + AnimatedSprite2D 式帧动画 + 分组标签；❌场景树/信号/ECS（细案 v0.1 定案）；**批次十四落地** |
| math | `math` | ✅ | glam 薄封装（类型化别名直通 + 扩展 trait 补 pygame 方法缺口）：Vector2/3/4 + **Quaternion/Mat2/3/4**（starfish 扩展，pygame 无对应，3D 管线地基）；2026-09-27 落地（原 v2 计划提前，见 changelog 09-27 批次五） |
| sndarray | `sndarray` | ✅ | DataBridge 音频端点：SoundArray（交错 f32）↔ SoundData 双向；**P5 落地**（09-27 批次十三）；pygame int16 面留绑定位换算 |
| surfarray / PixelArray / pixelcopy | 🚫 | 舍弃 | 模块不做；**底层互换类型经验由 DataBridge 吸收**（ADR-10） |
| BufferProxy | `buffer_proxy` | ✅定型 | **通用数据桥**（ADR-10）：v1 定型类型 + Texture↔CPU 最小路径，后续端点 v2 逐个挂。**P2 已落地**（`pygame::render::BufferProxy` + `Texture::from_buffer`，批次六） |
| mixer / music | ↪ `base::audio` | ❌独立 | 激进处理：引擎音频直用，薄别名视需要 |
| joystick / controller | ↪ `gamepad` | ❌独立 | 激进处理：引擎 gamepad 直用（gilrs 设计可对齐 controller） |
| touch | `touch` | ✅薄 | Touch 事件直通（Event::Touch）+ **手指状态表落地**（09-27 批次十三：fingers/get_pos/get_count，按 id 组织多点触控） |
| version | `version` | ✅薄 | |
| tests | examples/探针 | ✅ | 接口测试 = 每批次探针 + 三平台验收 |
| scrap | ↪ 引擎新模块 | 🔁 | 跨平台剪贴板引擎级立项（v2+，独立批次） |
| cursors | ❌ | ❌ | 难度大适配差，舍弃优先 |
| camera / freetype / midi | ❌ | ❌ | |
| GUI | ❌ | ❌ | 远期独立立项 |

**引擎侧新增项**（架构分析"新增模块"核对）：dialog/gamepad/io/net
✅ 已在树；**scrap（剪贴板）为唯一缺口**，v2+ 立项。

## 六、render 模块契约（创新模块，签名级）

```rust
// ── 资源 ────────────────────────────────────────────────
pub struct Texture { /* GPU 纹理 + 采样器；Rgba8Unorm */ }
impl Texture {
    pub fn from_rgba8(size: (u32,u32), pixels: &[u8]) -> Self;
    pub fn from_image(image: &ImageData) -> Self;      // resources/image 直通
    pub fn size(&self) -> (u32, u32);
}

// ── 绘制终点（见 ADR-2）─────────────────────────────────
pub struct Screen { /* swapchain 背板，进程唯一；内部 RefCell */ }
pub struct Surface { /* 纹理背板：Texture + rt view */ }
pub trait DrawTarget { fn fill(..); fn blit(..); fn size(..); }

// ── 相机（唯一抽象：MVP 组装，ADR-4）────────────────────
pub struct Camera { /* view + projection */ }
impl Camera {
    pub fn ortho(left, right, bottom, top) -> Self;    // 2D（像素坐标）
    pub fn perspective(fov, aspect, near, far) -> Self;// 3D（v2 内容）
    pub fn mvp(&self) -> Mat4;                         // 唯一上传的变换量
}

// ── 批处理（即时 API 的执行体，ADR-5；quad 与 mesh 同路）─
// v1.1 修订：pub（ADR-4 "按 base 风格设计"自洽——验收探针在 examples/
// 独立 crate 需公开面；P3 draw::* 仍在其上包 pygame 词汇）
pub struct Batch { /* 顶点积累 + 纹理/材质状态 */ }
impl Batch { /* push_quad/push_quad_uv/push_geometry；encode 归目标层 */ }

// ── 材质 / 数据桥 ───────────────────────────────────────
pub struct Material { /* 着色器+混合状态；默认=带纹理 alpha 混合 */ }
pub struct BufferProxy { /* 通用数据桥，ADR-10 */ }
```

绘制调用路径（以 `draw::rect(screen, c, r, 0)` 为例）：
`draw::rect` → 图形转 quad（图元生成复用 `gfx/shape2d` 逻辑）→
`screen.batch.push(quad, material)` → flip 时 `batch.flush` →
base::render 提交。全路径零 CPU 逐像素。

## 七、核心 API 契约（更新签名）

```rust
// ── display ─────────────────────────────────────────────
pub fn set_mode(size: (u32, u32)) -> Result<(), Error>;   // 建窗+GPU 装配（进程一次）
pub fn get_screen() -> &'static Screen;                    // 绘制终点（未 set_mode 则 panic）
pub fn flip() -> ();                                       // batch flush + present
pub fn set_caption(title: &str) -> ();
pub fn quit() -> ();

// ── surface（GPU 版）────────────────────────────────────
impl Surface {
    pub fn new(size: (u32, u32)) -> Self;                  // 透明底渲染目标
    pub fn from_texture(texture: Texture) -> Self;
    pub fn texture(&self) -> &Texture;                     // blit 源的本体
    pub fn fill(&mut self, color: impl Into<Color>) -> Rect;
    pub fn blit(&mut self, src: &Surface, dest: impl Into<Coord>) -> Rect;
    pub fn get_size(&self) -> (u32, u32);
    pub fn flush(&mut self);                               // 显式提交点（通常无需调用）
    // 🚫 get_at/set_at/pixels —— GPU 路线舍弃；readback 为 v2 评估项
}

// ── draw（接受任何绘制终点；即时语义、batch 执行）────────
pub fn rect(t: &mut impl DrawTarget, color: impl Into<Color>, rect: Rect, width: u32) -> Rect;
pub fn line(t: &mut impl DrawTarget, ..); pub fn circle(t: &mut impl DrawTarget, ..);
pub fn ellipse(..); pub fn polygon(..); pub fn lines(..);

// event / key / mouse / time / image：同 v0.1 契约，不重复
```

用户最终形态（对照 pygame 教程逐行可读；`screen` 换成 `Surface`
代码同样成立）：

```rust
use starfish::pygame::{display, event, draw, key, time, Event, Rect};

starfish::app_entry!({
    display.set_mode((800, 600))?;
    let mut clock = time::Clock::new();
    let mut pos = Rect::new(370, 270, 60, 60);
    loop {
        for e in event::get() {
            if let Event::Quit = e { return; }
        }
        if key::get_pressed().left()  { pos.move_ip(-4, 0); }
        if key::get_pressed().right() { pos.move_ip(4, 0); }
        let screen = display.get_screen();
        screen.fill([30, 30, 46]);
        draw.rect(screen, [0, 200, 90], pos, 0);
        display.flip();
        clock.tick(60);
    }
});
```

## 八、目录规划（v0.2）

```
starfish-rewrite/src/pygame/
├── mod.rs           # 声明 + 顶层再导出（Rect/Color/Event/locals）
├── render/          # ★ 创新模块：单一通用管线（ADR-4，§六）
│   ├── mod.rs       #   模块面（Texture/RenderTarget/Camera/Material）
│   ├── texture.rs
│   ├── target.rs    #   Screen/Surface 背板 + DrawTarget
│   ├── batch.rs     #   批处理执行体（quad/mesh 同路）
│   ├── camera.rs    #   MVP 组装（ortho/perspective 同型）
│   ├── buffer_proxy.rs  # 通用数据桥（ADR-10）
│   └── shader/      #   着色器（"万能基础着色器"落地位）
├── display.rs       # 全局槽 + flip
├── surface.rs       # Surface API 面
├── draw.rs          # pygame draw 接口（图元→quad 翻译）
├── event.rs         # Event 翻译 + key/mouse 状态表
├── time.rs  image.rs  locals.rs  math.rs
├── color.rs  rect.rs    # 回收旧实现
└── touch.rs  version.rs # 薄模块
```

## 九、性能与执行模型

- **批处理**：纹理相同的连续绘制合并为一次 draw call；切换纹理 =
  flush 边界。典型场景（背景 + 十几精灵）每帧 1~5 次 draw call；
- **上传**：仅图片资源创建时上传（一次性）；flip 只 present，无每帧
  全屏拷贝（对比 v0.1 CPU 路线的根本优势）；
- **图元**：rect/line/circle 等 CPU 生成顶点（微秒级），GPU 光栅化；
  抗锯齿 = 采样器/MSAA 配置，API 不暴露。

## 十、实施批次（按"基本类型 → 底座 → API 面"丝滑顺序）

| 批次 | 内容 | 验收 |
|---|---|---|
| P1 基本类型 | 回收 Color/Rect + locals 常量骨架 + math 基础 | ✅ 完成（20/20 测试，math 于 09-27 批次五落地） |
| P2 render 底座 | Texture/RenderTarget(Screen+Surface)/Batch/Camera(MVP)/默认材质 + BufferProxy 定型（Texture↔CPU 最小路径）+ **font 提前**（用户指令，Surface 是 font.render 依赖） | ✅ **三平台验收通过**（09-27 批次六：桌面探针实跑 / web 无头截图+浏览器实测 / android 真机实测，渲染均正常） |
| P3 pygame API 面 | display/event/key/mouse/time + draw 基础 + **pygame hello**（§七示例） | ✅ **三平台验收通过**（09-27 批次七：桌面实跑 / web 无头截图 / android APK 出包+真机交互待用户；实现差异登记：set_mode async、循环帧拍 next_frame、Rust 路径 `::`——见 changelog 09-27 批次七） |
| P4 | image.load → Surface + blit 链路（**font.render 已随 P2 落地**） | ✅ **三平台验收通过**（09-27 批次八：桌面实跑 / web 无头截图双解码路+alpha 混合 / android APK 出包+真机待用户；save= v2 回读登记） |
| P5 | transform / mask / sndarray（DataBridge 端点） | 各自另立细案 |
| 并行立项 | scrap（引擎跨平台剪贴板） | 另立契约 |

## 十一、开放问题

**无——全部拍板**：`get_screen()` 形态（ADR-6）、渲染管线单一化
（ADR-4）、DataBridge 定型（ADR-10）、依赖双向规则（§二）、font 提至
P4（评审决策：文字优先跑通便于 debug）。契约定稿，进入实施。
