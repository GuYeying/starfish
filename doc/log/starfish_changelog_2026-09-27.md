# starfish-rewrite 更新日志 2026-09-27

## 批次索引（当日总览）

| 批次 | 内容 | 状态 |
|---|---|---|
| 一 | hello 三平台里程碑（web 纯绿 / Android 闪退修复） | ✅ |
| 二 | `app_entry!` 宏统一入口（三入口归一） | ✅ |
| 三 | web canvas 尺寸正反馈爆炸修复 | ✅ |
| 四 | pygame 契约定稿 v1.0 + P1（Color/Rect/locals） | ✅ |
| 五 | math 模块（Vector2/3/4 + Quat/Mat，P1 收官） | ✅ |
| 六 | **P2 render 底座 + font**（三平台打包验收 + headless 坑位） | ✅ |
| 七 | **P3 API 面**（display/event/key/mouse/time/draw/version）+ pygame hello | ✅ |
| 八 | **P4 image**（load/load_from_bytes → Surface）+ 精灵 demo | ✅ |
| 九 | 渲染会话（Rust 版 with 语法，ADR-5 v1.2） | ✅ |
| 十 | color+depth RenderTarget + MRT 会话 | ✅ |
| 十一 | API 完善包（display 旗标/set_clip/arc/粗描边/transform） | ✅ |
| 十二 | image.save（GPU 回读 + base::io 跨平台） | ✅ |
| 十三 | mask / sndarray / touch（sprite 细案出） | ✅ |
| 十四 | sprite 模块落地（Godot 式重设计） | ✅ |
| 十五 | RenderPass 三类型统一 + MRT fill/blit 覆盖修复 + 实测反馈修复 | ✅ |

当日收官状态：**契约对照表除 scrap（挂起）外全部 ✅**；pygame 单元测试
39/39；探针三件套（probe/hello/image_demo）桌面 + web + Android APK
全链路验证。

---

## 批次：hello 三平台里程碑——web 纯绿验证通过，Android 闪退根因修复

### 设计背景

hello（`examples/hello.rs`，同一份源码注册 `hello`/`hello_android` 双 target）
是 rewrite 架构（starfish-window 自研 poll 模型）的首个跨平台测试案例，
目标 = 三平台渲染纯绿窗口。此前状态：web 后台日志正常但窗口不是绿色；
Android 点击图标立即闪退。

### 定位与修复

**① web 不绿——清屏色用错（非渲染问题）**

- 无头截图证实渲染管线完全正常：窗口输出的是 `begin_frame` 传入的
  `(0.1, 0.1, 0.15)` 暗蓝灰，与截图像素逐点吻合。
- 根因：`CLEAR` 纯绿常量定义后未使用，帧循环里写死了暗色。
- 修复：`surface.begin_frame(CLEAR, 1.0)`。

**② Android 闪退——cdylib 未导出 `android_main`（启动即 abort）**

- `llvm-nm` 对比：包内旧 `libhello_android.so` 动态符号表中
  `android_main` 为 `U`（未定义，胶水 import 无人实现）；
  NativeActivity 的 `ANativeActivity_onCreate` 启动时 `dlsym("android_main")`
  失败 → 进程 abort → 闪退。
- 修复：hello.rs 增加 Android 入口
  `#[unsafe(no_mangle)] fn android_main(app: AndroidApp)`：
  `android_init(app)`（装配 starfish-window 回调）→ `block_on(app_body())`。
  日志经 android-activity 胶水的 stdout/stderr→logcat 重定向
  （`adb logcat -s RustStdoutStderr`）。
- 重建后动态符号表 `android_main` 变为 `T`（已导出）。

**③ Android 第二层问题——surface configure `Invalid surface`（模拟器独有）**

闪退修复后应用可启动、GPU 初始化成功，但 `Surface::configure` 报
`Invalid surface`（wgpu 30.0.1 GLES 后端）。对照实验：

- `-gpu swiftshader_indirect` 与 `-gpu host`（Radeon 780M 直通）两种
  GPU 模式**同样失败** → 非 SwiftShader 特有；
- 强制 Vulkan 后端 → `RequestAdapter` 失败（模拟器无 Vulkan 适配器）；
- **旧 winit 架构的 probe_gfx APK 在同一模拟器报一模一样的错误**
  （同一 wgpu 30.0.1 位置）→ 判定为环境层问题（wgpu GLES × 模拟器
  GL 转译层），非 rewrite 回归，与
  `starfish-window/android绑定运行问题与解决方案.md` 的既有结论一致
  （"真机 GPU/EGL 环境不同，惰性装配后实测通过"）。

### 关键保证

- 三平台同一份 `app_body`；平台差异只剩入口函数（桌面 `main` /
  Android `android_main` / web `spawn_local`）。
- Android 打包链路固化：`cargo ndk -t <abi> -P 26`（注意 `-P` 大写）
  → `llvm-strip`（注意 strip 后查符号要 `nm -D` 看动态表，静态表已被删）
  → 替换 APK 内 `lib/<abi>/libhello_android.so` → `zipalign 4`
  → `apksigner`（debug.keystore）。
- `begin_frame` 对 surface 未就绪已自愈（catch_unwind + 3 次重试跳帧），
  Android 启动期 ANativeWindow 抖动不 panic。

### 测试状态（2026-09-27）

| 平台 | 结果 |
|---|---|
| Windows 桌面 | `cargo check --example hello` 通过（历次实跑正常） |
| Web（wasm32） | **纯绿截图验证通过**（`web/verify_green.png` / `verify_final.png`，Edge headless） |
| Android x86_64（模拟器） | 闪退已修复（进程存活、GPU 初始化成功）；渲染卡在模拟器 GLES 层 `Invalid surface`（旧架构同环境同样失败，环境层问题） |
| Android arm64（真机） | **✅ 真机验收通过（当日）：纯绿窗口正常渲染**。同时确证 `Invalid surface` 为模拟器独有环境层问题（真机 GLES/EGL 路径工作正常） |

---

## 批次二：`app_entry!` 宏统一入口落地（hello 接入，源码三入口归一）

### 设计背景

hello.rs 此前三平台各写一份入口模板（桌面 `main`/Android `android_main`/
web `spawn_local`），`src/lib.rs` 里已有 `app_entry!` 宏雏形但示例未接入。
统一性分析确认宏已覆盖 80%，剩 4 个缺口：错误出口缺失、裸 `pollster::`
路径不可移植（下游 crate 无此依赖）、Android 无 `fn main`（bin 目标在
android target 编不过）、web panic hook 未覆盖手写路径。

### 设计方案

1. **新增 `src/base/app.rs` 驱动器垫片**：`block_on`（pollster 再导出，
   非 wasm）与 `spawn_local`（wasm-bindgen-futures 再导出）——宏体路径
   全部 `$crate::base::app::…`，用户 crate 零新增依赖。
2. **宏升级为双形态**（`$body:block` 规则在前，`{…}` 优先匹配）：
   - 块形态：语句块，错误走 expect/panic（web 经 panic hook 落控制台）；
   - 表达式形态 `app_entry!(app_body())`：`Output = Result<(), E: Display>`，
     驱动器统一 `console_log` 错误后 `exit(0)`。
3. **Android 改桥接**（旧引擎 `set_android_app` 方案同构）：
   `android_main → android_init(app) → main()`，`fn main` 全平台存在，
   桌面/Android 共用同一条驱动路径。
4. hello.rs 删三入口模板 → `starfish::app_entry!(app_body());` 一行，
   顺带清理未用 import。

### 关键保证

- 宏内零裸外部 crate 路径（pygame-rs 等下游可直接使用；唯一约束：web
  构建需用户 crate 可见 `wasm-bindgen`，examples 由 dev-deps 提供）。
- web panic hook 统一安装；桌面/Android 退出统一 `process::exit(0)`
  收音频线程（真机重验收时关注 exit 噪音，android-demo 文档记录过
  exit(0) 在有存活线程时的 SIGABRT 案例，必要时换 SIGKILL）。

### 测试状态（批次二）

| 平台 | 结果 |
|---|---|
| 桌面 | check 零 warning；release 实跑冒烟通过（帧计数至 720，循环正常） |
| Web | `__starfish_app_entry` 导出确认；最终产物无头截图纯绿 ✅ |
| Android x86_64 模拟器 | 宏路径启动链完整（android_main→main→GPU 初始化），终点为已定性的 GLES 环境死穴，与宏改造前一致 ✅ |
| Android arm64 真机 | APK 已重建（`android_main` 导出确认），**待真机重验收**（入口重构 + exit 策略变化） |

---

### 坑位记录（批次二补）：模拟器跑 arm64 包 = berberis 翻译层伪影

x86_64 镜像装 arm64 APK 会经 `libndk_translation`（berberis）二进制翻译
运行——本次冒烟在翻译层炸出 `jni: Expected an exception after
ExceptionCheck` 断言，**与代码无关**（换 x86_64 原生包同流程正常）。
教训：**模拟器冒烟必须用与镜像 ABI 一致的原生包**，翻译层结果不可作为
回归依据；arm64 包只对真机负责。

---

## 批次三：web canvas 尺寸正反馈爆炸修复（DevTools 停靠 / 窗口缩放）

> 详见 `reference/web画布尺寸正反馈爆炸定位与修复.md`（完整机理、复现
> 手法、canvas HiDPI 契约与 headless 注意事项）。

### 症状

打开 DevTools（停靠改变视口）或缩放窗口后，web 渲染逐渐失效：控制台刷
`Texture size (70380×52806) exceeded maximum texture size (8192)`，
depth_texture → view → swapchain → CommandBuffer 逐级 Invalid，每帧
clear pass 全部报废（"too many warnings"），画面死掉但帧循环还在跑。

### 根因（starfish-window web 后端）

canvas **无 CSS 尺寸时布局尺寸 = width/height 属性**，而
`sync_canvas_size` 只做"属性 = client 尺寸 × dpr"：

```
属性 800 → client 800 → 写属性 800×1.25 → 布局跟随属性变 1000
→ 下一轮 client 1000 → 写属性 1250 → …（每次 resize 事件 ×dpr）
```

DevTools 停靠/浏览器缩放/拖窗口都会连发 resize 事件 → 指数爆炸。
探针实测（dpr=1.25，每 300ms 派发一次 resize）：1250 → 1563 → … →
14 轮 22763×17073，与用户截图的 70380×52806 同源。

### 修复（starfish-window/src/platform/web/mod.rs）

1. **`sync_canvas_size` 钉住内联 CSS 尺寸**（仅当内联样式未定宽高时）：
   先把 `style.width/height` 写成当前 CSS 像素，再写属性 = CSS×dpr。
   布局与属性解耦后循环消除；内联已有尺寸（如自建画布的 100vw/100vh）
   不覆盖，保留响应式布局语义。属性只管渲染密度，样式只管布局尺寸
   ——canvas HiDPI 的标准形态。
2. **resize 监听去重**：`sync_canvas_size` 返回属性是否变化，未变
   （DevTools 停靠常触发同尺寸 resize）不再上报 `Resized`，避免应用侧
   无效 surface 重建。
3. **`set_size` 双重缩放修复**：原实现写属性后走 sync 又乘一次 dpr；
   现按物理像素语义直接写"属性 = 物理，样式 = 物理/dpr"。

### 验证（resize_probe.html 探针页，保留作回归）

| 项 | 修复前 | 修复后 |
|---|---|---|
| 探针页 12+ 次 resize（dpr=1.25） | ×1.25 指数增长至 22763×17073 | **恒定 1000×750**（800×600×1.25） |
| 纹理超限报错 | 逐级 Invalid 全链报废 | 零报错 |
| 截图 | 渲染死 | 纯绿（`verify_resize_fix.png`） |
| 常规页面 dpr=1.5 回归 | — | 纯绿零报错（`verify_dpr150.png`） |

---
## 批次四：pygame 层契约定稿 v1.0 + P1 基本类型落地

### 契约定稿（architecture/pygame层设计.md v0.1→v1.0）

评审三轮收敛，关键决策落档：①Surface 路线定 **GPU**（RenderTarget+
Texture，舍弃 CPU 像素级处理）；②新增 **render 创新模块**——为 pygame
设计的 2D/3D **单一通用管线**（图形学底层数据无 2D/3D 之分，camera 即
MVP，`Camera::ortho/perspective/mvp` 一个类型简化轮子量）；③Screen 与
Surface 背板不同、终点统一（`DrawTarget` trait）；④**通用数据桥
BufferProxy 定型**（吸取 surfarray 家族底层互换类型经验，模块不做地基
要做）；⑤**依赖双向规则**（硬性）：pygame 可直用 starfish 类型入接口，
starfish 永不耦合 pygame；⑥像素格式 Rgba8Unorm；⑦即时 API × batch 延迟
提交（即时语义、命令式执行的收拢）；⑧font 提至 P4（文字优先跑通便于
debug）。输入：`pygame模块架构分析.txt`（模块取舍/激进处理清单/接口
策略全部落档）。

### P1 落地（基本类型，零平台依赖）

- 回收旧仓库 `src/pygame/{color,rect}.rs`（git HEAD v0.9.2）→
  `starfish-rewrite/src/pygame/`，适配：`crate::base::color` 路径、
  `from_name` 未初始化改返回 None（原 panic）；
- Color 补契约所需转换家族：`From<[u8;3]/[u8;4]/(u8..)/(i32..)/u32>`（u32
  支持 0xRRGGBB 与 0xRRGGBBAA，RGB 补 alpha=255）；
- 新增 `locals` 常量枢纽：`K_a..K_KP_ENTER` 全集（类型化常量 =
  `KeyCode`，宏生成）+ mouse 按钮常量；
- lib.rs 注册 `pub mod pygame;`。

### 测试状态（批次四）

| 项 | 结果 |
|---|---|
| pygame 单元测试（locals×2 / rect×1 / color×4） | 7/7 ✅ |
| wasm32 lib check | ✅ |
| 全量 lib 测试 | base::video 5 个真样本文件依赖测试失败——存量环境问题（缺样本媒体），与本批次无关（P1 纯新增模块，与 video 零交集） |

---
## 批次五：pygame math 模块落地（P1 收官：Vector/Quaternion/Mat 薄封装）

### 背景

契约 §十批次表 P1 含"math 基础"，此前未落地（§五对照表却标 v2，口径
两处不一致）——本批次落地后口径归一：**math = ✅ v1**。上游输入：
`reference/pygame绑定层API设计稿.md`（Vector2/Vector3 + Quaternion/Mat4，
3D 优先）、`reference/pygame兼容层模块架构.md` Tier 0（math = glam 直绑）。

### 方案：类型化别名薄封装（与 locals 同思路）

- **类型本体直通 glam**：`Vector2/3/4 = Vec2/3/4`、`Quaternion = Quat`、
  `Mat2/3/4 = Mat2/3/4`——零成本、bytemuck Pod 保持（P2 Camera::mvp 的
  Mat4 uniform 直传即用）；pygame 命名落别名与扩展 trait；
- **扩展 trait 只补 glam 缺口**（glam 已有的 length/dot/cross/lerp/
  distance/reflect 直用不重复）：
  - `Vector2Ext`：rotate_degrees / rotate_rad（±_ip 变体）、
    scale_to_length、normalize_ip、distance_to(_squared_to)、
    cross（=perp_dot）、angle_to_deg（带符号度数夹角）、
    as_polar / from_polar、slerp（最短弧角度插值 + 长度线性插值；
    反向平行 panic 对齐 pygame ValueError）；
  - `Vector3Ext`：rotate / rotate_rad（轴角，Rodrigues 数值同 pygame）、
    distance_to(_squared_to)、normalize_ip；
  - `QuaternionExt`：度数制便捷（from_axis_angle_deg / from_euler_deg /
    to_axis_angle_deg）；
- 模块级标量函数对齐 pygame.math 2.1.3+：clamp / lerp / inv_lerp /
  remap / smoothstep / smootherstep；
- **Quat/Mat 为 starfish 扩展面**（pygame 无对应，"3D 优先"决策）：
  接口即 glam 原生，P2 Camera/Material 直接可用；
- Python 专属面不做：elementwise 代理（Rust 运算符天然逐分量）、
  enable_swizzling（属性膨胀换语法糖）；
- 错误策略（ADR-9）：零向量 scale/normalize、反向平行 slerp、零轴
  rotate → panic（对齐 pygame 抛 ValueError 的契约违例语义）。

### 坑位记录

- **glam 0.33 `Vec2` 已有固有 `rotate(self, rhs: Vec2)`（复数乘语义）与
  `angle_to`（弧度制）**——固有方法优先遮蔽 trait 同名方法（不报错、
  静默换语义，测试直接暴露），让名为 `rotate_degrees` / `angle_to_deg`；
- `Quat::from_euler` 自 0.33 收 `EulerRot` 枚举首参（非三浮点）。

### 测试状态（批次五）

| 项 | 结果 |
|---|---|
| pygame 单元测试（math×9 + color×4 + rect×1 + locals×2） | **16/16 ✅** |
| wasm32 lib check | ✅ |
| 全量 lib 测试 | base::video 5 个真样本依赖测试失败——存量环境问题，与本批次无关 |

---
## 批次六：P2 render 底座 + font 模块落地（pygame 层主战役）

### 范围

契约 §十 **P2 全量**（Texture/RenderTarget/Batch/Camera/Material/BufferProxy）
+ **P4 的 font 提前**（用户指令；render 的 Surface 是 font.render 的依赖，
同批顺理成章）。验收探针 `examples/pygame_probe.rs`（纹理 quad + 图元 +
文字上屏 + 离屏面，180 帧自动退出可无头判读），双注册
`pygame_probe_android`（真机验收待执行）。

### 方案落位（pygame/render/ 六文件 + shader/）

- **单一通用管线**（ADR-4/ADR-5）：`shader/sprite.wgsl` 一套着色器走天下
  ——顶点 pos3+uv2+color4（统一顶点布局，与 base::font::TextVertex 同构），
  片元 = `textureSample × 顶点色`（调制语义）。三路同管线：纹理精灵
  （白色顶点色透传）/ 纯色图元（Gpu 1×1 白纹理）/ 文字图集（白色字形 ×
  目标色，base::font 图集直挂）；
- **Texture**（ADR-1/ADR-3）：Rgba8Unorm（gamma 直出对齐 pygame）；用途
  三位一体 `COPY_DST | TEXTURE_BINDING | RENDER_ATTACHMENT`——上传、blit
  采样、离屏绘制一纹全通，`Surface::from_texture` 零拷贝回环。
  ⚠️ **直建 wgpu 纹理**（不经 base::render::Texture）：base `TextureUsage`
  枚举拆成 Sampled（有 COPY_DST 无 RENDER_ATTACHMENT）/ RenderTarget（反之）
  两态，凑不齐"上传+采样+渲染目标"三位；绑定走 `texture_view` 裸视图口
  （视频帧同款先例）；
- **DrawTarget/Screen/Surface**（ADR-2/ADR-6）：绘制方法 **`&self`**（ADR-6
  v0.3 拍板的 get_screen 形态，内部 RefCell——ADR-2 签名的 &mut 以 ADR-6
  为准修正）；Screen = 交换链背板进程唯一（内持 base RenderSurface，
  begin_frame 透明清屏 + present 合并提交），Surface = 纹理背板透明底
  自由创建；blit 源自动 flush（ADR-5 目标切换即触发的落点）；
- **Batch**：纹理切换即段边界（同纹理合并一次 draw call）；填充/线双流
  （topology 变体），一条 pass 走完；顶点缓冲按需增长常驻（write_buffer
  就地更新，v1 非环形复用——P3 优化位）；图元几何 = `base::gfx::shape2d`
  家族直入（GeometryKind 声明拓扑）；
- **Camera**（ADR-4）：ortho（像素 y 向下，bottom=h/top=0 翻转）/
  perspective（度数 fov，右手系）同型，`mvp()` 唯一上传量；glam 0.33 起
  `orthographic_rh` 废弃 → `glam::camera::rh::proj::directx`（= wgpu
  0..1 深度约定）；
- **Material**：默认带纹理 alpha 混合；管线变体矩阵（颜色格式 × MSAA ×
  拓扑）惰性建+缓存；格式维两面 = 交换链格式（Screen 直绘）与离屏
  Rgba8Unorm（ADR-3 不转译）；
- **BufferProxy**（ADR-10）：类型定型（Layout/PixelFormat/字节校验）+
  Texture↔CPU 最小路径（`Texture::from_buffer` 上传向）；GPU 回读 v2；
- **font**（P4 提前）：base 扫描线光栅化 → CPU 合成 RGBA → Surface
  （资源制备路线，不违背 GPU 路线）；字形位图缓存；差异文档化：antialias
  恒 AA / 透明背景 / 无 kern（对齐 pygame.font.Font 默认行为）。

### 关键保证

- **GPU 槽 = thread_local**（ADR-6 display 全局槽的前置形态）：wasm 的
  wgpu 类型非 Send/Sync（Rc 底座），静态 OnceLock 过不了 Sync 界
  （首轮实现实测编译失败）——TLS 是主线程契约的类型级表达，顺带把
  越线程访问挡在编译期；web spawn_local / Android main 同线程成立；
- Screen `present` 对"begin 未就绪无帧"免疫（对齐 base 跳帧哲学）；
  首个 fill 特例消除：begin 恒透明清屏，fill 一律覆盖 quad。

### 坑位记录

1. **glam 0.33 `Vec2` 固有 `rotate`/`angle_to` 遮蔽 trait 同名方法**
   （math 批次遗留补充：`Mat4::orthographic_rh/_zo` 已废弃，新面 =
   `glam::camera::rh::proj::directx::*`）；
2. **base MeshBuilder::build 带 println 调试输出**——批处理热路径不可用，
   顶点缓冲改直建 `create_buffer_init`（batch.rs），管线布局模板仍用
   MeshBuilder（一次性）；
3. `base::render::RenderPipelineBuilder::color_targets` 返回 `()` 断链——
   链式收尾时须 `let mut b` 后语句式调用。

### 测试状态（批次六）

| 项 | 结果 |
|---|---|
| pygame 单元测试（+camera×2/buffer_proxy×1/batch 逻辑经 target 层/font×1） | **20/20 ✅** |
| 桌面探针 `cargo run --example pygame_probe` | ✅ 180 帧正常退出，零 wgpu 验证错误 |
| wasm32 lib + example check | ✅ |
| web 无头截图 / Android 真机 | ⏳ 探针双注册就绪，待验收执行 |
| 全量 lib 测试 | base::video 5 个真样本依赖测试失败——存量环境问题，与本批次无关 |

### 契约偏差（登记，v1.1 修订输入）

- `Batch` 契约标 `pub(crate)`——实作为 `pub`：验收探针在 examples/
  独立 crate 需公开面；ADR-4 "按 base 风格设计"的自洽结论，P3 draw::*
  仍在其上包 pygame 词汇；
- `DrawTarget::fill/blit` 契约 ADR-2 标 `&mut self`——实作 `&self`（内部
  RefCell）：ADR-6（更晚拍板）的 get_screen 形态 + §七示例代码要求，
  以 ADR-6 为准。

---
### 打包与平台验收（批次六补）

- **字体内嵌**：探针字体改 `include_bytes!` + `Font::from_bytes`（47KB）——
  web 无 std::fs、Android APK 无相对路径，三平台零 IO；
- **自动退出 180→600 帧**（≈10 秒）：真机开 App 有启动耗时，留足观看余量；
- **web 打包**：release wasm（4.7MB）+ wasm-bindgen 胶水 +
  `web/pygame_probe.html`（hello.html 同款加载范式）；
- **Android 打包**：`cargo xtask android pygame_probe`（NDK r30）→
  `target/android-apk/pygame_probe_android.apk`（3.1MB，debug 签名，
  `nm -D` 确认 `android_main`/`ANativeActivity_onCreate` 双 T 导出）；
- **web 验收 ✅（本机 headless 自验）**：Edge headless 截图全场景正确
  ——棋盘精灵 alpha 混合 / 离屏面 blit / 纯色 quad / 圆+描边双流 /
  文字上屏全部到位（`web/probe_final.png`）。

### 坑位记录（批次六补二）：headless 截图的 `--disable-gpu` 死穴

Edge headless 加 `--disable-gpu` → WebGPU `requestAdapter` 恒报
"No available adapters" → 应用 panic（hello 同败，对照实验定位）。
**GPU 相关的 headless 验证严禁 `--disable-gpu`**；虚拟时间预算对帧计数
型探针友好（20000 预算覆盖 600 帧 rAF），场景静态则冻结 delta 无影响。
### 验收反馈修复（批次六补三）：web 退出后 console 闭包报错

**现象**（用户真机/浏览器实测反馈）：两平台渲染正常、600 帧退出日志
干净打出，但 web 在退出**之后** console 抛 `Uncaught Error: closure
invoked recursively or after being dropped`。

**定位**：胶水 `real` = wasm-bindgen 闭包引用计数包装，被调面是零参
`FnMut()` 绑定（spawn_local 任务/回调链）——`app_body` 在异步宿主里
完成后，队列中迟到的 rAF/微任务踩到已 drop 的绑定。无害（渲染早已
完成、日志先于报错），但污染控制台。

**修复**：探针 web/Android 路径**驻留不返回**（`std::future::pending`
悬挂）——应用 future 永不完成，迟到回调无从踩空；画面保持，Android
由用户划掉收尾。桌面保留 `return Ok(())` 干净退出（block_on+exit(0)，
无头判读依赖）。复验：web console 零报错（仅剩 powerPreference 无害
提示），截图场景正常。

**判定语义（登记）**：探针"运行一段时间后退出/驻留"均为设计行为——
600 帧 ≈ 10 秒验收窗口；渲染正常 + 退出日志打出 = 验收通过。
---
## 批次七：P3 pygame API 面落地 + pygame hello 三平台验收

### 范围（契约 §七 / ADR-6 / ADR-7）

`src/pygame/` 新增七模块：

- **display**：全局槽（thread_local：Window 本体 + `Box::leak` 的
  `&'static Screen`）。`set_mode` **async**（WebGPU 无阻塞模型——契约
  示例的一处 `.await` 差异，绑定层 Python 侧无此问题）/ 进程一次
  （重复调用 panic）/ get_screen / flip / set_caption / quit / get_init；
- **event**：`Event` 枚举薄翻译（Quit/Resized/KeyDown{key,repeat}/
  MouseMotion/MouseButtonDown{button,x,y}/Wheel/Touch/Suspend/Resume/
  Other 兜底；MouseButton `#[non_exhaustive]` 需 `_` 臂——坑位）；
  get/poll/clear 对齐 pygame（fastevent 不做）；翻译时顺带喂 key/mouse
  状态表（ADR-7 唯一事实源）；
- **key / mouse**：thread_local 状态表 + `get_pressed()` 视图（方向族
  便捷方法）/ `get_pos` / `get_pressed()` 三元组（1=左 2=中 3=右）；
- **time**：`Clock` = base::time::Clock 薄包装（tick 返回**秒**，pygame
  ms 留给绑定层换算）；`delay` Web 上 no-op（无阻塞模型，文档标注）；
- **draw**：rect/line/lines/circle/ellipse/polygon——
  `fn(t: &impl DrawTarget, color, …, width) -> Rect`，Screen 与 Surface
  同签名；图元几何复用 `base::gfx::shape2d`；**v1 描边恒 1px**
  （width>0，粗描边 P4 优化位）；返回包围 Rect；
- **version**：ver/vernum/SDL 版本垫底 (2,0,20)。

配套：`DrawTarget` trait 补 `with_batch`（draw::* 执行通道）；Screen/
Surface 补 **fill/blit 固有方法**（教程代码免 trait 导入）。

### pygame hello（examples/pygame_hello.rs，契约 §七 教程对照）

三平台同一份代码；与 pygame 教程的**三处差异**（全部平台执行模型所致，
已文档化）：①`set_mode` 一处 `.await`；②循环末尾 `next_frame().await`
帧拍（Web rAF 让出——**漏写即同步死循环冻结渲染进程**，headless 报
"Abnormal renderer termination"，实测踩坑后修复）；③`exit()` →
`return Ok(())`。另：Fifo 呈现回压已节流，示例用 `tick(0)`（time 模块
「回压叠加警告」——同档 tick(60) 会叠加降帧 ~30fps）。

### 实现期坑位

1. **Rust 模块路径 `::` ≠ Python `.`**——契约 §七 示例的
   `display.set_mode` 是 Python 风味伪代码，Rust 侧 = `display::set_mode`
   （真 1:1 在 Python 绑定层）；
2. app_entry 宏内联 async block 的 `Ok(())` 无法推断 E——改具名
   `async fn run() -> Result<(), Box<dyn Error>>` + `app_entry!(run())`；
3. DrawTarget trait 方法 `screen.fill()` 需 trait 在作用域——固有方法
   补齐让教程形态免导入。

### 测试状态（批次七）

| 项 | 结果 |
|---|---|
| pygame 单元测试（+event×2/key×1/mouse×1/time×1/version×1） | **26/26 ✅** |
| 桌面 pygame_hello 实跑 | ✅ 6 秒无崩溃（timeout 截停） |
| web 无头截图 | ✅ 深蓝底 + 中央绿色方块，构图与契约 §七 一致（`web/hello_p3.png`） |
| Android APK | ✅ `target/android-apk/pygame_hello_android.apk`（真机交互验收待用户：←/→ 移动方块） |
| wasm32 lib check | ✅ |
---
## 批次八：P4 image 模块落地 + 带文字的精灵 demo 三平台验收

### 范围（契约 §五 image 行 / §十 P4："image.load → Surface + blit 链路"）

`src/pygame/image.rs`：

- `load(path)`：文件路径（桌面文件系统）；`load_from_bytes(bytes)`：
  **全平台口**（Web 无文件系统 / Android 资产通道 v2 前的内嵌字节方案，
  demo 同款 include_bytes!）；`get_extended()` 垫底恒 true；
- 解码 = `image` crate（PNG/JPEG/BMP/GIF/WebP），经 `ImageData::Rgba8`
  （CPU 统一图像格式，契约"基于 resources/image 封装"）→
  `Texture::from_image` → `Surface::from_texture`——GPU 路线全链；
- **save 不随 P4 提供**（登记）：Surface 内容驻留显存，保存 = 纹理回读
  慢路径（ADR-2 既有分期 v2）；Android AssetManager 资产桥 v2。

### 带文字的精灵 demo（examples/pygame_image_demo.rs）

PNG RGBA（awesomeface 476×476）+ JPEG RGB（container 512×512）两条
解码路 → Surface → blit（同源二次 blit 验 alpha 混合）+ draw 线 +
font 文字叠加。600 帧桌面退出 / web-Android 驻留（probe 同约定）。

### 测试状态（批次八）

| 项 | 结果 |
|---|---|
| pygame 单元测试（+image×3：PNG 解码/垃圾字节报错/垫底） | **29/29 ✅** |
| 桌面 demo 实跑 | ✅ 6 秒无崩溃 |
| web 无头截图 | ✅ 双图 blit + alpha 混合 + 文字全到位（`web/image_demo_p4.png`） |
| Android APK | ✅ `target/android-apk/pygame_image_demo_android.apk`（真机待用户） |
| wasm32 lib check | ✅ |
---
## 批次九：Rust 版 with 语法——渲染会话（用户设计反馈落地）

### 背景（用户反馈）

即时长相的 `fill/blit/draw.rect` 会让开发者误以为"即时上屏"，而执行
实际是命令式打包（ADR-5 延迟提交）。Python 侧有 `with screen: ...
自动 end` 的既定形态（兼容层模块架构 §3.1 + base RenderPass::end 按
值消费的预留设计），Rust 侧应同构：**显式创建会话 → 画 → end() 打包**。

### 落地（ADR-5 v1.2 修订）

- `screen.render()` / `surface.render()` → `ScreenFrame`/`SurfaceFrame`
  会话对象；创建即开始打包（Screen 会话顺带 ensure_begun——交换链
  纹理在 with 开始时 acquire，对齐 `with` 语义）；
- `end(self)` 按值消费 = 显式打包；**Drop 兜底同路径**（漏写 end 也
  安全——Python `__exit__` 对应物）；
- 会话全量实现 `DrawTarget`——`draw::rect(&f, ...)` 等 draw::* 直接吃
  会话，fill/blit/with_batch 委托目标；
- 即时 API 保留为 pygame 兼容形态，两形态共用同一 batch（可混用，
  提交序 = 调用序）；
- `DrawTarget` trait 文档显式声明"指令打包 + 延迟提交"语义。

### 实测抓到的两个真 bug（headless 截图逐帧对照暴露）

1. **同帧多次 pack 的顶点池 write 覆盖**：`write_buffer` 挂下一次
   submit 开头执行——会话 end 与 present 各 pack 一次时，后一次的
   write 先于统一提交落位，前一批已编码命令读到被覆盖的顶点
   （症状：会话内容全被顶成 text 位置的杂色条）。
   **修复**：顶点数据内联一次性 `create_buffer_init` buffer（每
   encode 独立，天然免疫覆盖）；VertexPool 池化删除，P3 优化位 =
   append-only arena + slice 偏移。
2. **直接提交破坏 clear→draw 顺序**：修复 1 时曾改为 pack 直接
   `queue.submit`——base 清屏 pass 还挂在 RenderSurface.pending 里，
   画反而先于清屏执行（症状：全黑）。回退为 pending 统一提交
   （`submit_single`），保序由单 submit 队列天然保证。

### 验证（批次九）

| 项 | 结果 |
|---|---|
| 探针改双形态（会话为主 + 即时混用） | ✅ web 无头截图与原验收字节一致（`web/probe_session.png`） |
| pygame 单元测试 | 29/29 ✅ |
| 桌面探针实跑 | ✅ |
| Android 三 APK 重打包（含修复） | ✅ probe / hello / image_demo |
---
## 批次十：color+depth RenderTarget + MRT 会话（用户三项需求落地）

### 需求（用户指令）

①会话语法保证未来对接 Python `with`；②支持 color+depth 的
rendertarget；③有能力则支持多渲染目标。

### 落地

**① Python with 对接保证（设计硬化）**：
- 会话补 `finish(&self)` **幂等打包口**——Python `__exit__` 的绑定形态
  （句柄持有下可重复调用；`end(self)` 仍为 Rust 习惯糖，Drop 兜底）；
- `Screen` 为 `&'static`（Box::leak）——绑定层会话句柄零生命周期问题；
- 映射定约（写入会话文档 + 契约）：`with screen:` → render()/finish；
  `with surface(depth=True):` → render_depth()；
  `with pygame.targets([a,b]):` → render_targets(&[&a,&b])——兼容层
  模块架构 §3.1 预言的三形态全部落位。

**② color+depth RenderTarget**：
- `Surface::with_depth(size)`（Depth24Plus 深度纹理 + RENDER_ATTACHMENT）；
- `render_depth()` 会话（Screen 用 base 深度缓冲[每帧 begin 清 1.0，
  pass 恒 Load]；Surface 每 pack 自清）——深度测试管线变体
  （Standard + 写入，Less）加入材质变体矩阵；
- `set_camera(&Camera)`：会话/目标级相机替换（透视/位姿，深度会话与
  3D 内容用）；相机 uniform 每打包写入不变；
- 顶点 z 透传（shader 无改）——2D 会话照旧画家算法，深度会话按 z。

**③ MRT**：
- `render_targets(&[&a, &b])` → `MrtFrame`（N≥2 同尺寸 Surface 一次
  pass 同写）；`sprite_mrt.wgsl` 双输出着色器（v1 镜像双写；异构输出
  随 v2 自定义材质）；管线缓存键扩维（双格式 × 采样 × 拓扑）；
- 编码层泛化：`EncodeCtx.colors: Vec<PassColor>` + `depth: Option<PassDepth>`
  ——单/多目标、有无深度统一一条编码路径；
- v1 边界（登记）：MRT 无深度、无 blit/draw::*（自定义材质 v2）；
  深度会话的 fill 会写深度 z=0（后续 z>0 内容被遮——文档标注）。

### 验证（批次十）

| 项 | 结果 |
|---|---|
| 探针升级四形态（会话+深度+MRT+即时混用） | ✅ web 无头截图：深度面板"红先绿后、重叠红赢"（与画家序相反＝深度生效）；MRT 双面板镜像一致（`web/probe_batch10.png`） |
| pygame 单元测试 | 29/29 ✅ |
| 桌面探针实跑 | ✅ |
| Android probe APK 重打包 | ✅ |
---
## 批次十一：API 完善包（display 旗标 / clip / arc / 粗描边 / transform）

### 范围（教程核心面补齐，P5 transform 提前）

- **display 旗标**：`locals::display` 旗标组（RESIZABLE/FULLSCREEN/NOFRAME/
  HIDDEN/ALWAYS_ON_TOP/OPENGL，位值 = 引擎 WindowFlags 位直通）+
  `set_mode_ex(size, flags)`（映射 `WindowBuilder::flags`；未登记位警告
  忽略[垫底]，OPENGL panic[引擎恒 wgpu，垫底表显式报错]）；`set_mode`
  = flags 0 的特例；
- **Surface.set_clip / get_clip**（Screen 同）：scissor 裁剪（面级状态，
  编码时整 pass 生效；`scissor_from` 钳入目标界，纯函数可测）；
- **draw.arc**：椭圆弧（对齐 pygame 用**弧度**；折线分段 4~256）；
- **draw.rect 粗描边**：width>1 = 四条实心边带（真粗边；厚度向内，
  钳半边长）——1px 细边仍走线流；
- **transform 模块（P5 提前）**：`flip_x/flip_y/scale/rotate/rotozoom`
  ——GPU 采样实现（顶点变换 + 线性采样，契约定稿路线），产物 = 新
  Surface（源不变）；`Batch::push_quad_corners` 四角 quad 原语；
  rotate 包围盒纯函数可测（ceil 带容差——90° 的 cos 在 f32 下 6e-9
  非 0，直接 ceil 虚增 1px）；
- **Color 补 f32 归一化转换**：`From<[f32;4]>` / `(f32,f32,f32,f32)`
  （0..1 钳制，引擎/glam 惯例色）；
- **pygame::surface** 再导出别名（Surface 宿主在 render——会话与
  DrawTarget 所在地）。

### 坑位记录

**headless 首跑黑屏 = 编译竞态（非代码 bug）**：批次十一首跑 web 全黑
（循环活着、零报错、桌面同码正常）——二分排查 arc/transform 均无辜，
复跑即愈。机理：首跑真实 GPU 管线编译（新深度/MRT/线管线族）跑赢了
虚拟时间预算，截图抢在帧 0 前完成。**判定法：黑屏先复跑一次再排查**；
`--enable-logging=stderr` 收 console 是 web 侧唯一可见的诊断通道
（wgpu on_uncaptured_error 走 eprintln，web 上不可见——改进位：
并发往 console_log）。

### 测试状态（批次十一）

| 项 | 结果 |
|---|---|
| pygame 单元测试（+transform 包围盒×1） | **30/30 ✅** |
| web 无头截图 | ✅ arc + transform + 深度 + MRT + clip 同帧全渲染（`web/tmp6.png`，终版 = `web/probe_final.png`） |
| 桌面探针 | ✅ |
| Android probe APK | ✅ 重打包 |
---
## 批次十二：image.save 落地（GPU 回读 + base::io 跨平台）

### 范围（用户指令：save + 统一走 starfish io 模块）

`src/pygame/image.rs`：

- **`save_async(surface, path)`**（全平台）：GPU 回读 → `image` crate
  编码（扩展名推断 png/jpg/bmp/tga…，无扩展名默认 PNG；JPEG 自动
  RGBA→RGB）→ `base::io::write`（原生落盘 / Web POST 到端点落盘）；
- **`save(surface, path)`**：桌面同步壳（`block_on(save_async)`）；
  **Web 不提供**（无阻塞模型，同步等 GPU 回读 + fetch 会冻结页面）；
- **`load_async(path)`**：全平台加载（`base::io::read`——原生 fs /
  Web fetch GET）；sync `load` 保留（桌面）；
- 回读实现：`copy_texture_to_buffer`（bytes_per_row **256 对齐**，去行
  填充还原紧凑 RGBA）→ `map_async`（桌面 `poll(PollType::Wait{..})`
  阻塞驱动 / Web `poll(PollType::Poll)` + `next_frame().await` rAF 让出
  重试）→ `get_mapped_range`（wgpu 30 起可失败，需 map_err）；
- `ImageError` 扩维：`Backend(io::IoError)`（base::io 后端错）/
  `Readback(String)`（回读错）。

### 坑位记录（两处，均实测抓到）

1. **回读纹理缺 `COPY_SRC`**：copy_texture_to_buffer 要求源含 COPY_SRC
   ——pygame 纹理原三位一体（COPY_DST|TEXTURE_BINDING|RENDER_ATTACHMENT）
   漏位 → 整个 encoder 被丢弃 → 回读全零且 save "成功"（静默数据损坏，
   最危险的一类）。**修复：pygame 纹理用途四位一体（+COPY_SRC）**，
   save 回读与未来截图/序列帧同享。
2. **wgpu 30 poll API**：`Maintain` 已更名 `PollType`，且
   `PollType::Wait` 是结构体变体（`{submission_index, timeout}`）；
   `get_mapped_range` 返回 Result。

另：**save 前自动 `flush()`**（对齐 blit 自动 flush 语义——未打包的
绘制先落纹理再回读，demo 首测抓到"只存出底色"）。

### 验证（批次十二）

| 项 | 结果 |
|---|---|
| image_demo 帧合成 → save → saved_demo.png | ✅ 320×180 RGBA 内容正确（face/container/text/blit 布局与合成一致） |
| 验证错误计数 | 0（修复前 2 条 COPY_SRC 缺失横幅） |
| pygame 单元测试（+bpr 对齐/去行填充×1） | **31/31 ✅** |
| wasm32 lib + example check（demo 的 save 块 cfg 裁剪） | ✅ |
---
## 批次十三：P5 尾巴落地——mask / sndarray / touch（sprite 出细案）

### 用户方向（2026-09-27 定案）

- mask：先调研业界做法再落；**sndarray**：直接桥 base SoundData；
- **sprite：参照 Godot 概念大改重设计**（原版少人用，不 1:1 兼容）；
- scrap 不急（UI 底层未立）；**touch 具备条件，落地**。

### mask：业界调研 → 定案

| 引擎 | 像素级掩码 | 视觉遮罩 |
|---|---|---|
| pygame | CPU 位图（Mask，本源） | — |
| Godot | **BitMap**（贴图阈值→位图；点击/贴图遮罩） | clip_children / Light2D / 材质 |
| Unity | ❌ 无内建（物理=Collider2D 形状） | SpriteMask（stencil） |
| LÖVE/Cocos/Defold | ❌（物理=形状） | stencil/clip |

**结论**：像素级碰撞检测按帧走 GPU 回读不现实——业界通行 =
**装载期一次性生成位图 + 运行期纯 CPU 比对**；视觉遮罩是另一族概念
（stencil 管线特性）。本层落位：`mask::Mask`（构建 = 一次性 GPU 回读
[复用 save 回读链路] + alpha 阈值位图；检测 = 纯 CPU pygame 全套语义
——overlap/overlap_area/overlap_mask/count/invert/scale/to_surface）；
**视觉遮罩（stencil）登记 v2 随自定义材质**。

### 落地

- **`mask`**：`Mask::new/from_surface_async(全平台)/from_surface(桌面壳)/
  size/get_at/set_at/count/fill/invert/overlap/overlap_area/overlap_mask/
  scale/to_surface`；
- **`sndarray`**：`SoundArray`（交错 f32 + 格式描述）↔ SoundData 双向
  （get_array/make_sound；f32 与混音器同源零转码；声道/长度校验）；
  AudioChannels/StereoFrame 补 base::audio 根导出（通用 API 完善）；
- **`touch`**：手指状态表（id 组织，事件翻译时维护——与 key/mouse 同款）；
  `fingers()/get_count()/get_pos(id)`；桌面恒空表。

### 验证（批次十三）

| 项 | 结果 |
|---|---|
| pygame 单元测试（+mask×2/sndarray×2/touch×1） | **36/36 ✅** |
| wasm32 lib check | ✅ |

### sprite 细案（批次十四实施依据）

`reference/pygame sprite 的 Godot 式重设计细案.md`——采纳 Godot 骨架
（Node2D/Sprite2D/AnimatedSprite2D/z_index/Y-sort/分组标签），剔除
场景树/信号/回调（违 poll 哲学）；Rust 形态 = 池化 Group + 世代 id +
会话绘制（按 z,y 排序入 batch）；碰撞 = Rect/Mask 组合留给用户。
---
## 批次十四：sprite 模块落地（Godot 式重设计）

### 范围（细案实施：reference/pygame sprite 的 Godot 式重设计细案.md）

`src/pygame/sprite.rs`：

- **`Sprite`**（builder 构造，数据全量 owned——Texture 廉价克隆句柄）：
  `new(&Surface)` / `with_frames(&[&Surface], fps)`（AnimatedSprite2D
  对位）/ at / scale·scale_xy / rotated / flip_x·flip_y / alpha /
  centered / offset / z / tag / visible；
- **`Group`**：池化容器（`Vec<Option<Slot>>` + **世代 id**——remove 后
  旧句柄自动失效）+ 分组标签查询（`with_tag`）+ `tick(delta)` 动画推进
  （纯函数 `advance_frame`）+ **`draw(&impl DrawTarget)`**——按
  (z, y, 插入序) 排序逐精灵 `push_quad_corners` 入 batch（旋转/缩放/
  翻转/alpha 乘制全走仿射四角，与 transform 同一条 GPU 采样路径）；
- 排序逻辑抽纯函数 `draw_order`/`order_positions`（零 GPU 单测观察口），
  动画推进 `advance_frame` 纯函数——单测不触 GPU（P1 口径）；
- `Batch::push_quad_corners`（任意四角 quad 原语，transform 共用）。

### 设计边界（细案定案）

❌场景树父子（v2 按需）❌信号系统 ❌_process 回调（违 poll 哲学——
用户循环里 tick/查询）❌ECS（手写池足够）。碰撞 = `rect(id)`/`Mask`
组合留给用户（sprite 管组织，Rect/Mask 管判定）。

### 验证（批次十四）

| 项 | 结果 |
|---|---|
| pygame 单元测试（+sprite×3：世代 id/排序/分组+动画） | **39/39 ✅** |
| web 无头截图 | ✅ 敌人群移动 + z 排序重叠正确 + 翻转玩家棋盘（`web/probe_batch14.png`） |
| 桌面 / Android probe | ✅ 实跑 / 重打包 |

### 坑位记录

- **`gen` 是 edition 2024 保留字**（closure 参数名不可用）；
- Surface 含 RefCell 状态不可 Clone——Sprite 改持 Texture（视觉内容
  语义本就正确：精灵不需要渲染目标视图）；
- sprite 单测禁 GPU（P1 口径）：排序/动画抽纯函数 + cfg(test) dummy
  构造器，GPU 路径由探针覆盖。
---
## 批次十五：会话三类型统一为 RenderPass（用户提案）+ MRT fill/blit 覆盖修复

### 提案（用户）："直接暴露出 render_pass，就像 starfish 那样——优雅，
同时方便以后 with 返回对象。"

### 落地

- **`ScreenFrame`/`SurfaceFrame`/`MrtFrame` 三类型删除**，统一为
  **`RenderPass<'a>`**（enum 分派 Screen/Surface/MRT）——与 base 的
  `begin_render_pass → RenderPass → end()` 设计语言同构；Python `with`
  的返回对象即此一型（**一个 pyclass 通吃三形态**，绑定层零分叉）；
- `render()/render_depth()/render_targets()` 签名不变（返回类型统一）；
- `end(self)` / `finish(&self)`（幂等，__exit__ 形态）/ Drop 兜底 /
  set_camera / DrawTarget 全量——draw::* 直接吃 RenderPass。

### 顺带修复：MRT 会话 fill/blit 输出覆盖（真 bug，回读取证）

**症状**：RenderPass 统一重构后，MRT 面板只剩底色、face 消失
（桌面回读取证确定性复现，web 同样）。

**根因**：Mrt 分支的 fill/blit 错误地循环调用了各 Surface 自身的
fill/blit——紫色 fill 落进 mrt_a 自身 batch；后续 `screen.blit(&mrt_a)`
的自动 flush 把这份 batch 提交，**覆盖了 MRT pass 先画好的 face**。

**修复**：Mrt 分支的 fill/blit 全部走会话自身 batch（一次 pass 真双写）；
各 Surface 自身 batch 不再被污染。桌面回读确认 face 回归。

### 验证（批次十五）

| 项 | 结果 |
|---|---|
| pygame 单元测试 | 39/39 ✅ |
| web 无头截图（探针全要素） | ✅ `web/probe_final.png`（MRT face 回归） |
| wasm32 三 example 构建 | ✅ |
| Android 三 APK 重打包 | ✅ |
### 用户实测反馈修复（批次十五补）

1. **web 控制台 POST 501**：批次十四的 MRT 排查诊断代码
   （`save_async(mrt_a_debug.png)`）忘删——web 上走 fetch POST，
   python http.server 不支持 → 501。已删（诊断完成使命）。
2. **Android 定时退出语义澄清**：批次十五的"驻留"此前误套 Android
   （log 说"正常退出"实际驻留）——现 Android **到点真实退出**
   （app_entry exit(0)；logcat 或有退出噪音，见批次二记录），
   仅 Web 驻留（闭包报错规避）。日志文案按平台区分
   （"正常退出" / "Web 驻留"）。
### 示例统一会话形态（批次十五补二，用户提案）

三示例源码的绘制写法**统一为 RenderPass 会话形态**（此前混有即时直调 /
render() 会话 / render_targets 三种长相）：

- 探针：离屏产物会话（深度/MRT）前置 → 主会话全场景合成（fill/blit/
  draw::*/with_batch/group.draw 全走会话）→ present；
- hello：`{ let f = get_screen().render(); fill/draw; end(); }` + flip；
- image_demo：主会话 + save 用的合成面同样会话化（end 即提交，save 的
  flush 变 no-op 兜底）。

即时直调 API（fill/blit/draw 不套会话）保留为兼容面不再出现在示例中。
验证：39/39 测试、桌面双例到点退出、web 截图全要素（统一后渲染无损）。
---
## 批次十六：RustPython 嵌入 M0 落地（pygame-rs 创建 + 三补丁重放 + 三目标验证）

### 背景（用户启动 Python 绑定层前置）

用户提供 `RustPython-main.zip`（main 快照 0.6.0-dev，commit 23fcb8d9，
2026-09-22）+ 两份既有文档（`rustpython改动备忘录.md` 三补丁 /
`rustpython上游贡献PR准备.md` 上游化草稿）。**分析结论：三处补丁与
窗口架构切换零关联（全是 RP 内部平台修复），全部仍需重放**——新快照
逐点验证 ①libffi 目标段（L73 含 android）②ctypes 无门控（L9）
③static_cell 每类型 thread_local（L119）均未修。

### 落地

- 创建独立 crate `pygame-rs/`（非 workspace 成员，starfish 本体零
  Python 资产的物理隔离红线保持）；
- vendor 快照解压至 `pygame-rs/src/rpy/dependencies/RustPython-main/`
  （pylib 的 build.rs 上游已自带 Windows-zip Lib 文本文件/symlink 双
  兼容解析——旧路由的"材料化"步骤零工作量）；
- **三补丁重放**（对照备忘录精确位置）：
  - ① host_env/Cargo.toml libffi 目标段剔除 android；
  - ② host_env/src/lib.rs ctypes 加 `#[cfg(not(target_os = "android"))]`；
  - ③ static_cell：**简化落地**——发现上游自带的 `no_std` 变体（全局
    OnceCell + unsafe impl Sync）与备忘录本地修法同构，直接扩展其 cfg
    让 android 复用（免写 130 行新模块），桌面/网页保留原 thread_local
    实现（行为测试依赖每线程独立 genesis）；
- 依赖接线（切换指南 §二）：`default-features=false` 去 host_env +
  [compiler, wasmbind, gc, stdio, importlib, encodings, freeze-stdlib]；
- M0 冒烟（`examples/rp_hello.rs` + `rpy::run_smoke`）：创世纪 → 编译
  → 执行，覆盖 print / match 解构 / 生成器 / 冻结 stdlib(json) 导入。

### 坑位记录

1. **debug 构建 genesis 必败**：recursion_limit debug 默认 256 + debug
   巨型 native 栈帧触发栈守卫——frozen importlib 引导 RecursionError
   → "essential initialization failed"。**RP 嵌入的冒烟/测试用
   release**；init_hook 可调 `vm.recursion_limit`（pub 字段）但 debug
   的 native 栈帧尺寸问题不可配置修复；
2. **wgpu 30 poll API**（批次十二同源补充）：`PollType::Wait{..}` 结构体
   变体 / `get_mapped_range` 可失败——image.save 已适配；
3. RP 0.6 嵌入 API：`Interpreter::builder(Settings).add_frozen_modules(
   rustpython_pylib::FROZEN_STDLIB).build()` + `vm.compile(src,
   compiler::Mode::Exec, path)` + `vm.run_code_obj`；PyBaseException 无
   Display（用 `{e:?}`）。

### 验证（批次十六）

| 项 | 结果 |
|---|---|
| 桌面 M0 冒烟（release） | ✅ `hello from RustPython` / match / 生成器 / json 冻结导入 |
| wasm32-unknown-unknown check | ✅ |
| aarch64-linux-android check（三补丁生效） | ✅（NDK r30 环境变量注入） |
| pygame 单元测试（starfish 侧回归） | 39/39 ✅ |
---
## 批次十六补二：M1 先行切片——Python 脚本驱动 starfish 窗口（rpy 主循环窗口测试）

### 落地（pygame-rs）

- **`rpy::sf` 原生模块**（第一组绑定面）：`sf_fill(r,g,b,a)` /
  `sf_rect(x,y,w,h,r,g,b,a)` / `sf_flip()` / `sf_quit_requested()`——
  全走 starfish 公共 API（display/event/draw），注入脚本全局；
- **`examples/window_test.rs`**（生成器门）：Rust 侧窗口装配 + 事件泵 +
  **每帧 `__next__` 步进 Python 生成器** + next_frame 帧拍；Python 侧
  `game()` 生成器 = 帧逻辑（填充/画方块/翻转/移动）。

### 坑位记录（RP 0.6 嵌入 API 实测）

1. **原生函数签名惯例**：`Fn(原生参数…, &VirtualMachine) -> PyResult`
   ——**&VirtualMachine 必须末位**；FromArgs 自动绑定 i32/f32/元组；
2. **f32/f64 FromArgs 严格 float**：Python int 不可绑定 float 参数
   （Expected 'float' but 'int'）——脚本颜色传浮点字面量，或绑定层
   自建 Numeric FromArgs（后续）；
3. 参数宏上限 7 元组（第 8+ 个参数触发 PyNativeFnInternal 不满足）；
4. PyBaseException 无 Display——异常文本走 `exc.clone().into()`
   → PyObjectRef::repr(vm)。

### 验证（批次十六补二）

| 项 | 结果 |
|---|---|
| 桌面 window_test（release） | ✅ Python 驱动移动方块持续渲染（timeout 截停验证存活） |
| Android window_test APK | ✅ `target/android-apk/window_test_android.apk` |
| starfish 侧回归 | 39/39 ✅ |
### M1 先行切片：window_test 主循环窗口测试（批次十六补三）

**形态**：Python 生成器门 + starfish 窗口——`examples/window_test.rs`
（pygame-rs），Rust 侧窗口装配/事件泵/每帧 `__next__` 步进 Python 生成
器 + next_frame 帧拍；Python 侧 `game()` 生成器 = 帧逻辑（填充/方块移
动/翻转），经注入的 `sf_*` 原生函数驱动 starfish 渲染。

**RP 0.6 原生函数 API 坑位（全部实测）**：
1. 签名 = `Fn(原生参数…, &VirtualMachine) -> PyResult`——**&VirtualMachine
   必须末位**（rp 宏按 FromArgs 自动绑定 i32/f32/元组）；
2. f32/f64 的 FromArgs **严格 float**：Python int 不可绑定（Expected
   'float' but 'int'）——脚本颜色传浮点字面量；
3. 参数元组宏上限 7（第 8+ 参数 → PyNativeFnInternal 不满足）；
4. `gen` 是 edition 2024 保留字；PyBaseException 无 Display——异常文本
   = `exc.clone().into(): PyObjectRef` → `.repr(vm)`；
5. 0.6 调用约定：`game.call((), vm)` 创建生成器、
   `vm.call_method(&gen, "__next__", ())` 步进。

**验证**：桌面 release 运行——Python 驱动移动方块持续渲染（8 秒存活截
停验证）。**Android APK 待办**：xtask 只读根 Cargo.toml，pygame-rs 的
示例不在解析范围——需 xtask 增加 `--dir` 兄弟 crate 支持（下一批）。
---
## 批次十七：主循环归 rpy——Python `while True` 即主循环（无 yield）

### 背景（用户定案）

"主循环可以直接交给 rpy，不用 yield。"——重写立项根因（旧架构引擎持循
环、无法交给脚本）被 poll 架构消灭后，此诉求在原生平台完全成立：引擎
不持循环，谁调用 poll/present 谁就是主循环。

### 落地（examples/rp_main_loop.rs）

- Rust 侧**零循环零帧拍**：set_mode → RP 装配 → `run_code_obj` 执行
  脚本 → 脚本返回 = 进程退出；
- Python 模块级 `while True` 即主循环：`sf.quit_requested()` /
  `sf.fill(...)` / `sf.rect(...)` / `sf.flip()`——事件泵与上屏都在
  Python 的调用节奏里（SDL 模型原味，vsync 自然节流）；
- `types.SimpleNamespace` 打包 sf 原生面为 `sf` 命名空间。

### 平台边界（定约）

| 平台 | 主循环形态 |
|---|---|
| 桌面 / Android（原生） | **同步 while True，主循环归 rpy**（本例） |
| Web | 浏览器 rAF 强制每帧让出——同步死循环冻结页面，仍用生成器门（window_test 形态） |

### 坑位记录（RP 0.6 FromArgs 严格数值）

- f32/f64 参数 **严格 float**（Python int 不可绑定，Expected 'float'
  but 'int'）；i32 严格 int（不接受 float）——sf 面全部参数定约 i32
  （坐标/尺寸/颜色 0~255，pygame 原生习惯）；
- `gen` 是 edition 2024 保留字；参数元组宏上限 7（超限拆参数）。

### 验证（批次十七）

| 项 | 结果 |
|---|---|
| 桌面 rp_main_loop（release） | ✅ 8 秒存活、零 PyException（Python 驱动移动方块） |
| Android rp_main_loop APK | ⏳ 桌面已验证；APK 需 xtask `--dir` 兄弟 crate 支持（pygame-rs 的 [[example]] 不在根 Cargo.toml 解析范围），或走批次一手动链 |
### window_test Web 状态（批次十六补三续）：RP-on-wasm 闭包问题（待专项）

window_test 打包上 web 后：页面黑屏 + 控制台两条 **Uncaught Error:
closure invoked recursively or after being dropped**（window_test.js:206，
wasm-bindgen 闭包 shim）——发生在 RP genesis/bootstrap 阶段（next_frame
未到达、无 Python 异常文本）。同页对照：probe（无 RP）正常渲染。

判定：RP-on-wasm 的嵌入形态问题（疑似 genesis 期间 wasm-bindgen 闭包
生命周期与 spawn_local/rAF 交互），**需专项排查批次**，非本批示例逻辑
问题。native 三例不受影响。
