# Starfish 更新日志 2026-09-17

> 本日主题：**Android 全链路打通与实机验证**。从"能编译"到"卓易通（鸿蒙 NEXT）
> 实机渲染正确 + 返回退出 + 一键构建工具"，期间修复 2 个桌面固有 bug、
> 3 个引擎级 Android bug，并沉淀 xtask 构建工具与两份环境定论。

## 批次 12：Android 全链路打通——构建管线 + NativeActivity 模板 + 实机部署

### 设计背景

video 六平台承诺中的 Android 路此前只有类型级验证（盲写）。用户目标：
**在鸿蒙 NEXT 手机（卓易通容器）上真机验证**。前置事实：
- cargo-ndk 4.x 不再把 bin/example 自动转 cdylib（只拷贝 target 本身
  是 cdylib 的产物），而 NativeActivity 需要可 dlopen 的共享库
- 项目此前无任何 APK 打包设施（无 gradle 模板、无签名流程）

### 设计方案

**① NativeActivity 无 Java 模板**（`android/AndroidManifest.xml`）

- winit 走 `android-native-activity`：APK 用系统自带 `android.app.NativeActivity`，
  dlopen `lib/__LIB_NAME__.so` 并调 `android_main`——**零 Java/Kotlin 代码**
- 三要素：`hasCode`、`meta-data android.app.lib_name`（= 示例名）、
  `exported="true"`（Android 12+ 强制）；`launchMode="singleInstance"`
  （每进程仅一次 winit EventLoop，重复实例必 RecreationAttempt）
- `minSdk 26`：cpal 的 AAudio 硬性下限；`extractNativeLibs="true"` +
  .so 压缩入包 → 免 zipalign 页对齐

**② 示例同源双注册约定**

- 同一源文件注册两条 `[[example]]`：桌面 bin + `*_android` cdylib
  （`crate-type = ["cdylib"]` 不可省；bin 不能与 cdylib 混用，cargo 拒绝
  混合声明——故拆两条，平台入口经 cfg 分家）
- 文件骨架：`#[cfg(target_os = "android")] mod entry` 内
  `#[unsafe(no_mangle)] fn android_main(app: AndroidApp)`（edition 2024
  必须 `unsafe(no_mangle)`）→ `run_android(...)`；桌面 `fn main()` → `run(...)`
- 资源装载 cfg 分家：桌面读 `resources/` 相对路径；Android
  `include_str!/include_bytes!` 内嵌（渲染/字体直用；音频/视频落盘应用
  私有目录 `AndroidApp::internal_data_path()` 再按路径加载——
  `SoundData`/`VideoModule` 仅有文件构造）

**③ 打包管线**（`scripts/android_run_example.sh` + 后续 xtask）

cargo ndk（`--platform 26`，**大写 `-P`**）→ aapt2 link → 内嵌
`lib/arm64-v8a/*.so` → apksigner（Windows 是 `.bat`，按 uname 分派）→
adb 部署。产物 `target/android-apk/*.apk`。

### 关键保证

- **API 26 是硬性下限**（`libaaudio` 仅 API 26+ 提供；默认 21 报
  `unable to find library -laaudio`）
- `-o` 目录会追加一层 ABI 子目录：给 gradle/脚本用应写 `-o ./jniLibs`
- Android 专属编译错误桌面查不出：`cargo check --target
  aarch64-linux-android --lib` 提前捕获

### 测试状态

- arm64 APK 于 **卓易通（鸿蒙 NEXT）实机跑通**：三角形渲染、
  返回退出、二次进入正常
- 模拟器（Android 17 / sdk_gphone16k_x86_64）两项环境定论（非引擎缺陷）：
  - arm64 + Berberis 转译：jni `find_class` 异常状态丢失 → 启动 panic
  - x86_64 原生：模拟器 GLES 透传 `eglCreateWindowSurface: BadAlloc`
  - 结论：模拟器栈不可用，实机为准

---

## 批次 13：实机回测修复批——两个桌面固有 bug + 三个引擎级 Android bug

### 设计背景

实机回测暴露五类问题：纹理单面涂抹（桌面+手机同现）、旋转失效
（桌面）、dialog/录音失败、返回退出后二次进入闪退、横竖屏不可控。
逐一定位后确认：**全部可修，其中两个是桌面端一直存在的固有 bug**。

### 设计方案与修复清单

**① 引擎：表面 view_formats 能力掩码**（`settings.rs to_wgpu`）

- Unorm↔Srgb 表面视图重解释需 `DownlevelFlags::SURFACE_VIEW_FORMATS`——
  **GLES/WebGL 与 Android Vulkan 均不支持**（wgpu 源码明注），configure
  直接 panic。改为按 `adapter.get_downlevel_capabilities()` 运行时掩码
  （Web 恒置空的既定行为保留）。此前任何 Android 真机都会命中。

**② 引擎：Android 启动门追加句柄探测**（`app.rs`）

- winit 在 Android 的 `inner_size()` 返回**显示器尺寸**（恒非零），
  "等首个有效尺寸"的门形同虚设；ANativeWindow 经 onNativeWindowCreated
  异步送达，过早建 GPU 表面 → `configure: Invalid surface`。
  启动门在 Android 上追加 `HasWindowHandle::window_handle().is_ok()`
  探测（winit 未就绪时返回 `Err(Unavailable)`），不设兜底超时。

**③ 引擎：RenderPass 绘制路由**（`render_pass.rs`）

- `set_mesh` 记录索引数，`draw()` 自动路由 `draw_indexed`——wgpu 原生
  `draw()` **无视已绑定的索引缓冲**，索引网格用 `set_mesh + draw` 会按
  顶点数线性绘制，跨面拼接的三角形呈"单像素拉伸涂抹"（04 实机截图实锤）。
  `draw_mesh` 复用 `set_mesh + draw`，三者语义统一。

**④ 示例数据：04 立方体 Left 面第 4 顶点 UV 笔误**

- `(1.0,1.0)` 应为 `(0.0,1.0)`（对照 05 同表定位）——该面第二三角形在
  UV 空间退化成一条对角线，整面只采样纹理一条斜线。桌面/手机同现。

**⑤ 示例：04 旋转角累积**

- 原代码把 `ctx.delta()`（帧间隔）直接当旋转角、无累积 → 视觉静止。
  加 `rot` 字段每帧 `+= delta`。

**⑥ 返回退出 + 进程语义**（`app.rs`）

- NativeActivity 输入队列接管按键后，系统默认"返回 = 结束 Activity"失效
  → `translate()` 把 `NamedKey::BrowserBack` 翻成 `CloseRequested`（Android）
- `run_android` 事件循环结束后显式 `process::exit(0)`：Android 保留缓存
  进程，winit 每进程仅一次 EventLoop——图标再进的第二个 activity 实例
  必然 RecreationAttempt 闪退（实测），收进程对齐桌面"run 返回即结束"语义

**⑦ 08/09/10 窗口化**：纯音频示例原无输入循环（返回键无效、状态不可见）
→ 全部改为窗口化 Application，状态色上屏（绿=播放中 / 橙=录音中 /
红=失败不闪退），播完/录完自动退出。

**⑧ dialog 分级诊断 + 类加载器自愈**（`dialog.rs`）

- 首版探针用 `find_class` 误报（原生线程 FindClass 只查系统类加载器，
  且 robius 是 define_class 内嵌 dex 路线）→ 改为 `ensure_classloader()`：
  经 Activity 类加载器显式 `loadClass` 验证 dex + `setContextClassLoader`
  自愈（robius 内部查找的回退依赖它）
- 打包管线自动并入 robius 的 `classes.dex` + `hasCode="true"`
  （此前缺 dex 时 pick 必 ClassNotFoundException）

### 关键保证

- **不回退原则成立**：draw 路由、采样器 ClampToEdge（04/05）、权限申请
  均为真实问题修复；UV 笔误修正后立方体桌面/手机渲染全部正确（实机确认）

### 测试状态

- `cargo test --lib`：53 passed / 0 failed
- `cargo check --examples`：桌面 + aarch64-linux-android 双目标全绿
- 卓易通实机：立方体渲染正确（用户确认）✅

---

## 批次 14：xtask 构建工具 + 横竖屏 + dex 管线

### 设计背景

手写 bash 打包脚本不符合编译型路线，且无法承载"特性勾选 + 未来模块扩展"。
落地 xtask 模式：**构建工具本身是 Rust 代码**（`xtask/` crate），跨平台、
无脚本依赖、可测试、可扩展。

### 设计方案

**① 命令面**

```bash
cargo xtask list                                   # 全部示例 + Android 支持注册表
cargo xtask android 03_texture                     # 自动解析 *_android；全链路
cargo xtask android 16_dialog --build              # 仅出 APK
cargo xtask android 15_empty_window --abi x86_64   # 模拟器 ABI
cargo xtask android 15_empty_window --no-default-features --features dialog,gfx
cargo xtask android 13_video_decode --orientation portrait        # 横竖屏
```

**② 特性 → Android 支持注册表**（`FEATURE_ANDROID_SUPPORT` 表）

- gfx/font/video/dialog/net = 可用；gamepad = 空实现占位（构建时警示）；
  未登记特性照常透传 cargo（附提醒）——**未来新增 Rust 模块加一行即可**

**③ 打包管线 Rust 化**：build-tools / android.jar 自动选最高版本；
dex 自动并入（robius build.rs 产物，缺失仅告警）；`.so` 压缩入包
（zip crate，**必须 `ZipWriter::new_append`**——`new` 追加非空包会写坏
中央目录）；apksigner（Windows `.bat` 须经 `cmd /c` 包装）。

**④ 横竖屏**：manifest `__ORIENTATION__` 占位 + `--orientation` 参数
（默认 `landscape`，白名单校验）；bash 兼容脚本走环境变量。

### 关键保证

- 旧 bash 脚本兼容保留，xtask 为主工具
- 模拟器 x86_64 构建路径保留（`--abi x86_64`），arm64 默认

### 测试状态

- `cargo xtask list`：15 示例 + 注册表显示正确
- 12 个 APK 全量出包成功（含 dex 并入 + 签名校验 + zip 完整性）
- `cargo test --lib`：53 passed / 0 failed

---

## 批次 15：卓易通实机回测定论（环境边界固化）

### 定论

| 项 | 状态 | 说明 |
|---|---|---|
| 渲染/循环/窗口/返回退出 | ✅ 实机验证 | 02 / 04（旋转 + 纹理修复后） |
| 字体/gfx/audio 编译与出包 | ✅ | 待逐个回测 |
| dialog | ⚠️ 容器受限 | 类加载自愈通过；robius show 内部 Java 异常无栈可读——容器对 NativeActivity 场景 Fragment/SAF 支持不完整。真机 Android 预计可用 |
| 录音 | ⚠️ 容器受限 | `ensure_permission` 运行时申请已实现，容器不弹授权框 → 红屏不闪退；真机预计正常弹框；容器内可手动授权 |
| video | 待回测 | MediaCodec 链路代码完整（ndk_context 注入验证通过），阶段化错误已就位 |

两项容器限制均为 **卓易通 Java 运行时行为，非引擎缺陷**，已固化至
`reference/android构建与运行指南.md`（坑位 12-14 + 模拟器限制节）。

### 测试状态

- `cargo test --lib`：53 passed / 0 failed
- `cargo check --examples`（桌面）+ `cargo check --lib --target
  aarch64-linux-android`（Android）双目标零 error
- 12 个示例 APK 全量出包（`target/android-apk/`）
