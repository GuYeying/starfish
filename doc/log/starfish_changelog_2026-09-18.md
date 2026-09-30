# Starfish 更新日志 2026-09-18

> 本日主题：**Android 实机回测收尾 + 视频硬解全链路打通**——Web 录音自持后端、
> 权限失败根因实锤与修复（ndk-context Application→Activity）、SAF 卡顿根治
> （轮询式 Job API）、退出闪退优雅化、视频 MediaCodec 三连修复后实机播放成功。
> 全程由卓易通真机回测驱动定位。

## 批次 16：Web 录音自持后端（audio/device_web.rs）

### 设计背景

README 跨平台矩阵中 Web 录音标记"明确不支持"。查证 cpal 0.18.2 源码：
WebAudio 后端的 `build_input_stream_raw` 是**写死的 `Err("Device does not
support input")`**——上游未实现，而浏览器能力本身完备（getUserMedia 授权 →
ScriptProcessor/AudioWorklet 逐帧 PCM；SDL3 在 emscripten 上即自持此层）。
结论：不是做不了，是 cpal 没做。

### 设计方案

- **抽离为独立模块 `audio/device_web.rs`**（对齐 video 按平台分文件的既有模式：
  windows/linux/apple/web 各一文件）——wasm 采集后端自包含，回退 = 删该文件 +
  `audio/mod.rs` 的 mod 声明 + `device.rs` 的 wasm 输入分支，三处
- 链路：`getUserMedia({audio})`（浏览器自动弹授权框）→
  `createMediaStreamSource` → `ScriptProcessor(4096, 1, 0)`（输出声道 0：
  必须接入音频图才回调、但不外放防啸叫）→ 单声道复制为立体声帧 →
  喂给既有 `RingSink`（环形缓冲/溢出计数/WAV 导出全复用）
- **公开 API 零变化**：`AudioRecorder::new_with_capacity / sample_rate /
  dropped / read / clear / save_wav` 原样；将来 cpal 支持输入后整体回退
- Cargo.toml：wasm web-sys 增补九项特性（Navigator/MediaDevices/
  MediaStreamConstraints/MediaStream/MediaStreamAudioSourceNode/
  AudioContext/ScriptProcessorNode/AudioProcessingEvent/AudioBuffer/
  AudioDestinationNode）

### 关键保证

- device.rs 的 wasm `DeviceStream` 变体：`Option<cpal::Stream>`（输出）+
  `Option<WebInput>`（输入）；pause/resume 按持有者路由
- wasm 单线程模型的 Send 包装（`SendWrap<T>`）：JS 引用只在主线程触碰，
  `AudioOutputBackend: Send` 约束在该平台平凡满足
- Drop 语义：`ctx.close()` 停止采集回调 + 释放麦克风（浏览器收回使用指示）

### 测试状态

- wasm32 / 原生双目标编译零 error；53 逻辑测试全绿
- 浏览器实测待做（需 web 录音示例，矩阵标记 ⏳ 已实现待浏览器实测）

---

## 批次 17：Android 权限失败根因实锤与修复（ndk-context Application→Activity）

### 设计背景

卓易通实机回测两处失败同源：录音"进入即红屏、无授权框"（上轮已改红屏不
闪退）+ dialog "PICK ERR: Java exception was thrown"（无栈可读）。用户提议
**单变量隔离**：打包一个 hello 窗口 + 权限的探针。

### 探针演进（三轮，方法论教训）

1. **首版探针误报**：用 `find_class` 探测 Fragment 类 → 误报
   `STEP-A: class missing`。原因有二：原生线程的 FindClass 只查系统类加载器
   （应用类不可见）；且 robius 是 **define_class 内嵌 dex** 路线
   （`FILE_PICKER_FRAGMENT_BYTECODE = include_bytes!(classes.dex)`），
   find_class 根本测不到它的机制
2. **二版**：改经 Activity 类加载器显式 `loadClass` 验证 +
   `setContextClassLoader` 自愈——真机回测**自愈通过**（类可加载），
   但 robius show 流程仍抛异常 → 排除 dex/类加载问题
3. **三版（17_permission_probe）**：权限全流程探针（hello 窗口 + 字体上屏
   诊断 + 分步 CHECK/REQ/POLL/RESULT），用户回读屏显：

   ```
   CHECK: DENIED -> requesting
   REQ ERR: requestPermissions threw: JavaException |
   no non-static method "Landroid/app/Application;.requestPermissions..."
   ```

### 根因实锤

错误消息中对象类名为 **`android/app/Application`**——android-activity 0.6.1
的 init.rs 在初始化 ndk-context 时存入的是 **`getApplication()` 的
Application 全局引用**，而非 NativeActivity。而
`requestPermissions` / `getFragmentManager` 是 **Activity 独有方法**：
- 录音权限申请 → Application 上无此方法 → 抛异常 → 失败
- dialog（robius show 流程调 `getFragmentManager`）→ 同根因 → 失败

### 修复（`app.rs run_android`）

```rust
// SAFETY：release 后立刻以同一 VM + NativeActivity 全局引用重初始化
unsafe {
    ndk_context::release_android_context();
    ndk_context::initialize_android_context(
        android_app.vm_as_ptr().cast(),
        android_app.activity_as_ptr().cast(),
    );
}
```

把 ndk-context 的 context 从 Application 换成**真正的 NativeActivity**
（`activity_as_ptr()` = android-activity 持有的 Activity 全局引用）。
- `ndk-context` 改为 android 目标非可选依赖（app.rs 核心路径使用；
  video 特性对其的 `dep:` 引用同步移除）
- Activity 本身也是 Context，既有消费方（video 的 MediaCodec 链路）不受影响
- android-activity 自身从不调用 `release_android_context`（实查）→ 无双重
  释放风险

### 测试状态

- 卓易通实机：**dialog 文件选择成功**（用户确认）；权限流程探针各步上屏正常
- `cargo test --lib`：53 passed / 0 failed

---

## 批次 18：SAF 期间卡顿根治——轮询式对话框 Job API

### 设计背景

dialog 修复后实机新问题：**选择文件返回后卡顿，无法正常回到应用**。
根因：SAF 选择器是**独立 Activity**，覆盖期间会触发本应用完整生命周期流转
（surface 销毁/重建、Pause/Resume）；而 16 用 `block_on` 在事件循环线程上
等待结果——**事件泵停摆**，积压的生命周期指令无法处理。

### 设计方案

新增**轮询式对话框 API**（对齐项目既定轮询哲学：gamepad 状态表 /
`try_recv` / 视频手动泵）：

```rust
let mut job = dialog::pick_file_start(Some("选择"), &[("任意文件", &["*"])])?;
// 每帧：
if let Some(res) = job.try_result() { /* 完成 */ }
```

- `pick_file_start` / `save_bytes_start`：非阻塞发起，立即返回任务句柄
- `PickJob::try_result` / `SaveJob::try_result`：每帧收割结果
  （Android = robius 回调填槽；桌面 = 线程 + mpsc 通道，rfd 阻塞不冻结 UI）
- 16 示例改造为 Job 模式 + 字体多行文本上屏（状态/文件名/异常详情）
- 主线程全程保持事件循环 → SAF 期间生命周期正常流转，**卡顿从根上消除**

### 关键保证

- **Android 上对话框一律用 Job 轮询式，禁用阻塞式等待**——阻塞主线程
  会停摆事件泵（已有 `ensure_permission` 同理是受控阻塞，15s 上限）
- 既有 async API（`pick_file`/`save_bytes`）保持不变，桌面语义不受影响

### 测试状态

- 卓易通实机：文件选择成功、返回不再卡顿（用户确认）

---

## 批次 19：退出后重进闪退——优雅化处理

### 设计背景

上轮 `exit(0)` 修复（批 17 前的退出链路）在卓易通上**未完全生效**：
返回退出后点击图标仍闪退，且反复点击持续闪退，必须后台杀死进程才能进入。
杀进程能恢复 = 存在僵尸缓存进程。

### 设计方案

winit 的门：`EVENT_LOOP_CREATED` 是**进程级静态 AtomicBool**，
`EventLoop::build` 时 `swap(true)` 后 Android 上**永不复位**
（`allow_event_loop_recreation` 仅 web 平台编译）——缓存进程里的二次
`android_main` 必然 `RecreationAttempt`。

修复（`run_android`）：EventLoop 构建改为 match——

```rust
Err(e) => {
    eprintln!("[starfish] 二次实例进入（{e:?}），退出残留进程");
    std::process::exit(0);
}
```

二次实例**不再 panic/abort**（消除闪退观感），而是干净 `exit(0)` 清除
僵尸进程——用户**下一次点击即正常进入**，无需后台手动杀死。

### 遗留观察点

- 返回退出后进程为何残留（exit(0) 未生效的路径）待真机现象进一步定位；
  当前降级体验：异常退出后第一次点击可能"无反应"（实为清除僵尸），
  第二次正常进入
- 模拟器对照实验（x86_64 + 18_exit_probe）：启动 → 自动退出 → 进程死亡 ✓
  → 二次启动正常 ✓——干净环境无此问题

---

## 批次 20：Android 视频硬解全链路打通（实机验证 ✅）

### 设计背景

13 调试模式（字体统计）实测：`pos` 从 0 持续推进到 5s、`tex: ready`、
`ENDED`——**MediaCodec 硬解 + NV12 提取 + 纹理上传全链路打通**。调试期
先后暴露三个运行时问题，逐一定位修复后恢复视频画面渲染。

### 问题与修复清单

**① JNI 签名笔误（dequeueInputBuffer / dequeueOutputBuffer）**

- 现象：`dequeueInputBuffer: Invalid number or type of arguments passed to
  java method`
- 根因：`dequeueInputBuffer(long timeoutUs)` 的参数是 **long**，JNI 签名
  误写 `(I)I`（int）且实参传 `JValue::Long`——声明与实参类型不匹配。
  dequeueOutputBuffer 的第二参同病
- 修复：`(I)I` → `(J)I`、`(...BufferInfo;I)I` → `(...BufferInfo;J)I`

**② Java 堆 OOM（每帧 3.1MB 分配）**

- 现象：`new_byte_array: Failed to allocate a 3133456 byte allocation ...
  growth limit 402653184`（1080p NV12 一帧 = 3,133,456 字节，分毫不差）
- 根因：旧实现**每帧在 Java 堆 new 一个 3.1MB 中转数组**，30fps 分配率
  ~90MB/s，GC 追不上 → OOM
- 修复：MediaCodec 的输入/输出缓冲本身就是 **direct ByteBuffer**——
  `GetDirectBufferAddress` 拿指针直接读写（输入：样本 copy 进输入缓冲；
  输出：`from_raw_parts` 切片拷入 Rust 堆）。**全程零 Java 堆分配**，
  这是媒体播放器的标准做法

**③ 容器 MediaFormat 包装对象方法解析失败**

- 现象：`get_int(width): ... | java: no non-static method
  "Landroid/media/MediaFormat;.getInteger(...)"`，且 NoSuchMethodError
  **挂起在线程上污染后续所有 JNI 调用**（症状漂移到 dequeueInputBuffer，
  极具迷惑性）
- 修复：调试模式**不读 format 键**——几何以 demux 为准，stride 暂取
  width；INFO_FORMAT_CHANGED 仅消费事件。渲染验证通过，若后续色彩/
  几何异常再补“显式类 + CallNonvirtual”的稳妥读取

### 经验沉淀

- **诊断基建先行**：13 的阶段化错误（红/橙/紫/正常四态）+ 字体上屏让
  三轮问题全部通过"装 APK → 读屏显 → 精准定位"闭环，未依赖 logcat
- JNI 调用的签名/异常/参数三查：jni 0.21 的 call_method 不清异常，
  pending 异常会污染后续调用——关键路径统一接 `jerr_ex`（读异常详情）
- MediaCodec 输出缓冲必须直读（direct ByteBuffer），Java 堆中转在大
  分辨率下必 OOM

### 测试状态

- 卓易通实机：**视频正常播放**（硬解 → NV12 → RGBA → 纹理 → 全屏），
  播完自动退出 ✅（用户确认"可以了，完美"）
- `cargo test --lib`：53 passed / 0 failed；桌面 + Android 双目标零 error

---

## 本日测试状态汇总

- `cargo test --lib`：53 passed / 0 failed
- 三目标编译零 error：wasm32-unknown-unknown / aarch64-linux-android / 桌面
- 14 个 Android 示例 APK 全量出包（`target/android-apk/`，含 17 权限探针与
  18 退出探针）
- **卓易通实机确认**：dialog 文件选择 ✅、录音（含 SAF 保存到可见位置）✅、
  渲染/循环/返回退出/重进 ✅、**视频硬解播放 ✅（MediaCodec 全链路）**——
  至此渲染/窗口/循环/音频/字体/gfx/dialog/video 八大件在 Android 实机全通


---

## 批次 21：跨平台 IO 模块（base/io.rs，重启旧 iofi 场景）

### 设计背景

批次 14 曾以"std::fs 透传包装无增值"移除 iofi。现在前提变了：**Web 端有了
fetch 真实现方案**（读取 = GET / 保存 = POST 到服务端点）——一套业务代码
三端读写数据的诉求成立，模块价值回归。用户定稿架构：**一套通用 API +
平台各自直调目标 API**（对齐 video 按平台分文件的模式）。

### 设计方案

- 新建 `base/io.rs`（feature `io`，入 default；无新增外部依赖）
- 统一异步 API（dialog 模式）：`read / read_text / write / write_text /
  exists`——原生为 std::fs 阻塞实现 + async 体外壳（"模拟异步"），
  Web 为 fetch 真异步（挂起不挡帧）
- Web 端契约：`path` 即 URL（相对页面地址或绝对）；read = GET、
  write = POST（body 原始字节）、exists = HEAD；非 2xx → Backend 错误
- v1 范围：只做读/存（fetch 有对应语义的操作）；list_dir/create_dir/
  删除等不设——原生用户直接用 std::fs（能力更完整），Web fetch 无对应语义
- 2 个新测试：字节往返 / 文本往返（pollster 驱动 async 测试）

### 关键保证

- 与批次 14 撤销决策的关系：**不是翻案**——当时撤的是"无 Web 实现的纯
  std 透传"；现在 Web fetch 实现使模块价值成立（三端读写一套代码）
- feature 门控：`io` 入 default，可剔除；`--no-default-features` 干净编译
- CLAUDE.md 特性表与"文件 IO 不设库模块"旧决策段落已同步更新

### 测试状态

- `cargo test --lib`：55 passed / 0 failed（含 io 往返 2 测试）
- 三目标编译零 error：桌面 / wasm32-unknown-unknown / aarch64-linux-android

---

## 批次 22：Web 端 net / video 无头实测打通（修三处 + 无头方法论沉淀）

### 测试背景

Web 迁移后 video 与 net 一直未实测跑通。本轮以 `examples/server/server.py`
（同源静态 :8021 + TCP echo :8022 + UDP 发现 :8023 + WS echo :8024）为对端，
用 `21_web_probe`（NET WS 行）与 `22_video_web` 做无头闭环测试。

### 问题与修复清单

**① net "跑不通"实为 demo 服务器 bug（库本身无罪）**

- 现象：浏览器 WS 握手成功到达 :8024 后再无回显
- 根因：`server.py ws_echo_client` 帧循环调用 `read_exact(...)`——函数实际
  名为 `ws_read_exact`，`NameError` 当场炸掉处理线程（握手后服务器死亡）
- 修复：4 处调用点改名对齐。修复后 `21_web_probe` 端到端
  **NET WS echo PASS**（连接 → 发送 → 回显 → 比对一致）

**② 22_video_web 三处叠加（编译错误 + 黑屏 + 静默）**

- `Gpu.error` 字段声明未初始化（E0063，桌面/Web 均编译失败）——按字段注释
  意图补全：打开/泵失败 → 紫屏 + 详情走控制台
- 未调 `with_web_canvas_id("canvas")`——winit 自建 canvas 不入 DOM，Web 上
  必然黑屏（CLAUDE.md 坑位 3 的活案例）
- 诊断全用 `println!/eprintln!`——wasm 上无处可去，失败也无声。21/22 全部
  改 `base::web::console_log`，22 增加每秒进度心跳
  （`pos/size/ended` 上报，无头判读的正典证据源）

**③ 无头测试方法论：--virtual-time-budget 会冻结 delta（重要坑位）**

- 现象：虚拟时间预算下 `ctx.delta()` 恒为 0（逐帧日志实锤），视频主时钟
  推不动、泵永不运转、也永无报错——纯静默
- 机理：rAF 自续链（每帧 request_redraw）+ 无外部真实事件时，无头虚拟时间
  不前进，rAF 全部命中同一 `performance.now()` 读数。21 号能 PASS 是 WebSocket
  真实网络事件充当了"解锁事件"
- 结论：**这是无头测试模式的伪象，非真实浏览器 bug**。视频/动画类 web 测试
  必须用**存活模式**：不带 `--virtual-time-budget`/`--timeout`/`--screenshot`
  启动 `msedge --headless=new <url>`，靠 `--enable-logging=stderr` 收割
  console 流，真实时间等足后 `taskkill /T` 收尾
- 次坑：`--screenshot` 相对路径落在 Edge 自己的版本目录（非 shell cwd），
  一律用绝对路径

### 测试状态

- **net ✅**：`21_web_probe` NET WS echo PASS（console 日志 + 截图文字像素双证）
- **video ✅**：`22_video_web` 真实时间下 fetch(2.8MB) → mp4 解复用 →
  WebCodecs 解码 → 1080p 帧纹理实时上传 → **全屏采样绘制**，pos 0→5.63s
  实时推进，`ended=true` 后 1s 自动退出（全程无 PUMP FAIL / 无 wgpu 校验
  错误）。初版探针 frame() 只清屏不绘制（文档声称全屏渲染但实现缺失），
  已移植 13 的已验证画法补全（全屏三角形 + texture.wgsl + 首帧懒装配），
  并补上 Resized → surface.resize（web 尺寸竞态坑位 2）
- 双示例 wasm + 桌面 `cargo check` 零 error；`web/` 部署物与源码同步
- 测试基建残留：server.py 以 `--web-dir ./web` 同源服务为 web 示例的
  标准对端（本会话后台任务随会话结束退出，复测时重启即可）

### 文档同步（矩阵实测状态回写）

- **README 模块 × 平台矩阵**：视频硬解 Web `⏳ WebCodecs → ✅`、
  网络 Web `⚠️ WS 可用 → ⚠️ WS ✅ 实测 / UDP ❌`；视频硬解 Linux
  `⏳ → ✅`（进度记录早已记实机验证，README 漏同步）；矩阵抬头日期
  2026-09-18；自动化测试计数对齐（55 passed / 14 APK）
- **开发进度记录**：Web 视频 → 实测通过、Android 视频 → 实机验证
  （批次 20 卓易通全链路，此前漏更）、网络 → WS 实测通过
- 保持 ⏳ 不动：macOS 全列、手柄 Web（待插设备）、对话框 Linux/macOS
  （GTK3 待验）——无实测证据不虚标

---

## 批次 23：回退 render_pass 的"draw 自动路由"改动（误诊修复，未提交即废弃）

### 设计背景

工作区遗留一处未提交的引擎级改动（`render_pass.rs`，即根目录
"可能要回退的代码.txt" 所记）：`RenderPass` 增加 `pending_index_count`
隐藏状态，`set_mesh` 记录索引数后 `draw()` 自动路由 `draw_indexed`。
起因是 04 号示例"纹理横向拉成竖条纹"的误诊——把原因归结为
"wgpu 原生 `draw()` 无视已绑定的索引缓冲"。

### 误证实锤（回退依据）

1. **04 的绘制路径从未坏过**：v0.7.0（HEAD）里 04 就用
   `draw_mesh(cube_mesh)`，而 `draw_mesh` 一直正确路由 indexed/linear
2. **竖条纹真凶 = UV 复制粘贴错**：工作区同一批 diff 里修正了立方体
   左后面顶点 UV（`1.0,1.0 → 0.0,1.0`）——UV 错位采样才是经典病因
3. **全仓库调用点审计**：所有 `set_mesh + draw` 站点（02/07/11/16/17/
   19/20/21）绘制的全是**线性网格**（字体网格无索引），自动路由零收益；
   索引网格全部经 `draw_mesh` / `draw_mesh_instanced`（两版本均正确路由）
4. **架构原则相悖**：原三层语义（`draw` 裸线性 / `draw_indexed` 裸索引 /
   `draw_mesh` 便利路由）清晰无隐藏状态；改动使 `draw()` 的顶点范围参数
   被静默忽略，违背"无隐藏状态"的既定设计

### 处置

- `git checkout HEAD -- src/base/render/render_pass/render_pass.rs`
  （该文件 diff 仅含此一处改动，回退干净）；04 保留现状（UV 修复 +
  `draw_mesh` + Android 双注册均正确）

### 测试状态

- `cargo test`：55 passed / 0 failed；`cargo check --examples` 零 error
- wasm 重建部署后无头实测：21 `net echo PASS`（连续两轮）、22 视频管线
  就绪 + pos 实时推进——文本 `set_mesh + draw` 路径与 `draw_mesh` 路径
  回退后行为均无变化，与审计结论一致
