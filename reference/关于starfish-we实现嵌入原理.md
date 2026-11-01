Java 壳（ScriptActivity）
  │ attachSurface(Surface)          ← JNI 入口（pygame-rs-dev 调 PygameRsDev）
  ▼
【写】attach_surface_impl (embedded.rs:60)
  ANativeWindow_fromSurface → EXTERNAL = Some({ptr, w, h})
  + PENDING 队列 push Resized 事件
  │
  ▼【读】窗口后端（platform/android/mod.rs）四处消费：
  │
  ├─ ① WindowInner::new (:122)     external_active()?
  │     → 嵌入模式跳过 NativeActivity 的 10 秒 glue 等待
  │
  ├─ ② size() (:186)               external 尺寸优先（2412×1084）
  │     → ③建窗 set_mode_ex 的物理尺寸来源
  │
  ├─ ③ raw_handle() (:291)         ext.ptr → RawWindowHandle::Android
  │     → wgpu Surface 创建的窗口句柄（③建窗时渲染后端消费）
  │
  └─ ④ poll_event (:145)           事件不走 glue 泵，直读 PENDING 队列
        ← injectTouch / surfaceResized 由 Java 侧推进来


如何判断是否是嵌入模式：

// 四个消费点的统一开头模式：
if let Some(ext) = EXTERNAL.lock()...as_ref() {
    // 注入模式：用 Java 给的窗口/尺寸/事件队列
} else {
    // 独立模式：走 NativeActivity/glue 原生路径
}


形态 A：pygame.rsdev（调试壳，Java 注入式）
  ScriptActivity.onCreate
    → SurfaceView 创建 → surfaceCreated 回调
    → PygameRsDev.attachSurface(Surface)     ← 此刻 EXTERNAL = Some
    → （之后用户点脚本）loadCode → 引擎启动
    → ③建窗 → raw_handle 看到 EXTERNAL → 用注入的窗口 ✓

形态 B：com.starfish.test（独立运行形态，NativeActivity）
  android_main → 引擎直接启动（没有任何 Java 壳调 attachSurface）
    → EXTERNAL 恒为 None
    → ③建窗 → raw_handle 走 NativeActivity 窗口路径 ✓