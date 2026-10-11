# starfish 更新日志 2026-09-29

## 批次二十一：xtask 退役——统一打包脚本落地（pygame-rs 自持，全 crate 共用）

### 背景（用户决策）

xtask 仅支持根 Cargo.toml 示例注册表，pygame-rs（兄弟 crate）的示例
一直走手动五步链——**双打包路径并存**的代价在批次十八已付过学费
（lib_name 坑即手动链缺陷）。用户决策：**打包工具归 pygame-rs 自持**，
starfish 根示例共用同一脚本，xtask 退役。另：pygame-rs 将独立为
D:\Projects\Rust 下的平行 crate（pygame-rs / starfish / starfish-window
三分）——脚本因此按"无仓库根假设"设计。

### 落地

- **`pygame-rs/scripts/build_apk.py`**：五步链参数化固化
  （cargo ndk → llvm-strip（nm 自检 android_main）→ manifest 模板替换 +
  aapt2 link 重造 base（lib_name 对齐，**批次十八根因的永久哨兵自检**）
  → 组装（dex 探测 + .so DEFLATED）→ zipalign → apksigner 签名验证）。
  - 用法：`python pygame-rs/scripts/build_apk.py <示例名> [--dir <crate目录>]`
    ——默认自持 pygame-rs；根示例 `--dir .`；产物 `<crate>/target/android-apk/`；
  - **独立化友好**：无仓库根假设（模板搜索 crate 自有 android/ 优先、
    dex 从本 crate target 扫描、产物 crate 本地）——pygame-rs 迁出后开箱即用；
  - `pygame-rs/android/AndroidManifest.xml`：自持模板副本（含存储权限，
    独立后不依赖 starfish 目录）。
- **xtask 退役**：`xtask/` 目录删除、`.cargo/config.toml` 的 `cargo xtask`
  别名移除、CLAUDE.md（布局/常用命令/坑位 #6）同步。

### 验证

| 路径 | 结果 |
|---|---|
| pygame-rs（binding_probe，自持模板） | ✅ 五步全绿，lib_name 自检 ✓，产物 26MB |
| 根 crate（hello，--dir .） | ✅ 五步全绿，lib_name 自检 ✓，产物 2MB |
| 单元回归 | ✅ `cargo test --lib pygame::` 39/39（批次二十接线无回归） |

### 附带修复

- `build_apk.py`：Windows 控制台 GBK 编码崩溃（stdout 重配 utf-8）；
- 诊断 TCP 通道：connect 加 300ms 超时 + 一次失败拉黑（无超时阻塞
  主线程 = ANR，批次十九坑的脚本侧同步修复）。

### 下一步

Surface 批次 v1.1（架构 §九定约实现）→ rp_main_loop 挂起排查 →
pygame-rs 独立迁移（三 crate 平行布局，脚本已按独立形态就绪）。

---

## 批次二十二：`next_frame` 归位 starfish-window（0.2.0）+ 用户扩展重验

### 背景（用户决策）

用户指出：**web 上没有 `next_frame`，starfish-window 的 poll 模型就是残缺
的**——rAF Promise 桥非平凡且易错，不该让每个消费者手写。判定成立：
逐帧让出原语属于"循环如何逐帧让出"的本体，随 crate 发布才是开箱即用。

### 落地

- **starfish-window 0.1.0 → 0.2.0**：新增公共 API
  `starfish_window::next_frame()`（桌面/Android no-op async；Web =
  rAF Promise 桥）。依赖面新增（wasm target）：`js-sys`、
  `wasm-bindgen-futures`（与 wasm-bindgen =0.2.127 配对）；
  **cfg 双胞胎各自需文档注释**（crate `#![deny(missing_docs)]`——
  桌面孪生有文档、wasm 孪生没有 → 仅 wasm 编译失败的哑雷，已补）；
- `starfish/src/base/window.rs`：两段 cfg 实现替换为一行再导出
  （引擎稳定路径保留；原内嵌的周期性 debug 打点随实现移除——
  非六标记成员，需要时可由壳侧另挂）。

### 三端重验（用户改码后 + next_frame 移动后）

| 端 | 结果 |
|---|---|
| Windows | ✅ 截图实证：探针已大幅扩展（CJK/Canterbury 字体、Surface、transform、音频手势分支）全量渲染；771 帧无异常 |
| Android | ✅ 统一脚本打包 29MB（lib_name 自检 ✓），真机确认无异常 |
| Web | ✅ 六标记全绿（headless 帧计数为已知 hidden-rAF 伪影）；glue/html 重建 |

### 附带发现（非缺陷）

- stdout 重定向到文件 = 块缓冲：无每帧打点后，杀进程丢未刷缓冲——
  "日志停在 50 行"是缓冲假象非卡死；**取证改用窗口截图**（一眼定论）；
- 绑定层演进确认：`pygame_shim.py` 已由用户删除，原生模块注入改为
  Rust 侧 `rpy::bridge`（批次 B 起）——v1 shim 路线被更优方案取代，
  架构文档 §5.3 的 shim 描述已过时（随批次更新）。

---

## 批次二十三·续：帧边界延迟 resize——修复"深度新/颜色旧"附件竞态

### 用户真机/浏览器实测暴露（响应式画布 + Ctrl 缩放场景）

```
The depth stencil attachment size (1693×1397) does not match the size of
the other attachments' base plane (1260×945).
- While encoding BeginRenderPass("clear_screen")
```

### 根因

`Resized` 事件的消费点（`pygame.event.get()`）在 **Python `__next__`
内部**被调用——若此刻帧已 begin（交换链纹理已 acquire、批次非空），
`handle_resized → surface.resize` 会在**帧进行中**重配置表面 + 重建
深度纹理 → 同帧打包提交时"深度新/颜色旧"尺寸不匹配 → 命令缓冲无效。
批次二十的接线让 resize 真正发生，从而暴露了这个潜伏竞态（此前无人
resize 所以从未触发；桌面强改窗口测试通过纯属时序运气）。

### 修复（标准帧边界延迟模式，Screen 层）

- `Screen` 新增 `pending_resize: Cell<Option<(u32,u32)>>`；
- 新增 `request_resize(w,h)`：帧进行中 → 挂起（一帧多次缩放
  last-wins）；帧边界 → 立即 resize；
- `present()` 末尾（begun=false、提交+上屏完成后）应用挂起的 resize；
- `display::handle_resized` 改调 `request_resize`。

### 验证

| 项 | 结果 |
|---|---|
| 桌面强改窗口 500×900 | ✅ 帧内 resize 安全落地：全幅正常渲染、图元跟随新正交、零异常零崩溃（截图 resize_test.png） |
| 单元回归 | ✅ 40/40（用户本批新增 1 测试） |
| 三端产物 | ✅ web glue + Android APK（29MB）已重建 |

### 定约

**任何渲染表面的 resize 一律走 `request_resize`**（帧边界安全）；
直接调 `resize` 仅限确定帧未 begin 的场合。用户侧无需回退响应式
html——该错误是被接线暴露的潜伏竞态，非响应式布局引起。

---

## 批次二十四：真机全链时序打点——"启动长等待"根因定位与修复

### 手段（用户提案，采纳）

全链 `[t] +Nms 阶段` 打点（shell 各阶段 + 权限耗时 + 首帧 present），
真机自跑一轮即得完整时序。

### 真机时序（nova 15 / 卓易通）

```
+539ms   set_mode OK
+15601ms 权限申请完成 ok=false（耗时 15057ms = 打满 15s 轮询超时）
+15678ms CJK 字体已预载（77ms）
+15770ms RP genesis 完成（92ms）
+15809ms 原生模块注入完成
+15857ms generator created
+16556ms frame #1 presented（首帧含管线编译 +699ms ✓）
```

### 结论

- **"启动长等待"根因 = shell 验证钩子里的 `ensure(Microphone)` 阻塞
  15s**（ok=false——15s 轮询窗口内授权状态未变；卓易通容器内
  requestPermissions/checkSelfPermission 行为待查：弹框未弹 or 轮询
  失效）。genesis（92ms）/字体预载（77ms）/注入/首帧全部非瓶颈；
- **修复**：移除启动期权限调用（demo 只播放不录音，无需
  RECORD_AUDIO——`audio/record.rs` 构造时隐式申请的既定约定不受影响）。
  启动预期 ≤1.6s 出完整画面（待装机复验）；
- **遗留（权限模块）**：ok=false 待查——弹框未弹（容器静默失败）vs
  轮询失效（点了允许但状态未变）。随录音功能立项时以非阻塞方式
  （授权状态回调/查询）重做验证。

---

## 批次二十四·终验（2026-10-11）：Android 渲染全通——"异常"真相全部为观感层

### 品红二分实验（诊断三角定案）

用户真机跑品红诊断版，报告"**全屏品红 + 一些线段渲染**"→

- **全屏品红** = push_geometry Filled 路径在 GLES **完全正常**（诊断三角
  按绘制顺序覆盖全场——它本就是 with 块最后一个绘制）；
- **线段透出** = batch 双流顺序（填充流→线流两次管线切换），线流后绘
  于其上——正确行为；
- 结论：**几何管线在 Android 从来没有坏**——此前的"清屏色无内容"实为
  诊断三角盖场（品红版）与加载进度条滞留观感（无品红版）。

### 各"异常"现象的最终解释（全部非管线缺陷）

| 现象 | 真相 |
|---|---|
| "进入深蓝屏无内容" | 防白闪铺底帧（设计行为）+ genesis/字体预载阻塞期的静态加载帧 |
| "白色实心长方形" | 加载进度条前景条（120,200,255 @100%），加载帧正确呈现的证明 |
| "紫红屏幕+一些线段" | 品红诊断三角（盖场）+ 线流后置透出——**场景正在正常渲染** |
| "过一会才出现" | 权限阻塞 15s（已移除）+ genesis 真机 92ms（非瓶颈） |

### 终态（用户确认 + 时序实测）

```
+554ms set_mode OK
+590~622ms CJK 字体预载（后台线程，77ms）
+622ms RP genesis 完成（92ms）
+645ms generator created
+1073ms frame #1 presented          ← 首帧呈现（含管线编译）
+1083/1091ms frame #2/#3
[用户确认] 深蓝底 + 全部图元 + Canterbury/中文文本 ✓
```

**Android 启动到出内容 ≤1.1s**——三端（Windows/Android/Web）M1 绑定面
探针全部达到"启动即出画面"状态。批次十六~二十四的 RP 嵌入主线
（嵌入原理/跨平台/魔改/绑定/排障）全部闭环，沉淀于
`architecture/rustpython嵌入三端全案.md` + 本日志 + 诊断手册。

### 终验补充（2026-10-11，诊断探针撤除版）

品红三角/纯红 fill 为诊断探针，终版已撤（恢复深蓝底 + 正常场景）。
用户三端复验：**Windows / Android 真机 / Web 全部正确渲染，无白块、
无权限阻塞、缩放相对缩放正常**——批次二十四全项关账。

### 收尾整理（随终验）

- 渲染链路诊断打点收敛到 `base::debug::devlog`（println + Android 文件
  追加 bp_log.txt；渲染循环内刻意无 TCP——连接超时会干扰帧节奏），
  稳定后整体撤除；
- **wasm 启动 panic 修复（本批引入本批灭）**：加载轮给壳加的耗时打点
  用了 `std::time::Instant`——**std Instant 在 wasm32-unknown-unknown
  直接 panic**（sys/time/unsupported.rs，启动首行即炸、全无标记）。
  时基改 `base::time::since_start_ms`（wasm 走 performance.now 桥）。
  教训：**壳/引擎代码时序一律走 base::time，禁 std Instant 上 wasm**；
- 临时取证文件清理（快照/hilog 日志/ps1）；
- 用户侧音频 WIP 遗留 3 个 unused variable 警告（sfx/worker/stream，
  属用户开发中代码，未代动）。

---

## 批次二十五（2026-10-11）：pygame 模块运行测试案例套件（examples/pygame/）

### 落地

- **`examples/pygame/` 目录**：每模块一个可运行案例 + 共用框架
  （`common.rs`：Checks 自检登记 / 色块状态呈现 / 240 帧自动退出 /
  Web 驻留）+ `README.md`（案例索引与运行方式）；
- **9 案例**：rect（碰撞族强断言）/ color（构造族）/ time（Clock tick +
  fps）/ draw（六图元族一轮）/ surface（with_depth/has_depth/
  render_depth/blit）/ mask（位操作+overlap）/ display（尺寸+呈现链路）/
  event（泵 240 帧+交互计数）/ io（文本+二进制回环）；
- **呈现约定**：顶部结果横幅（绿=全过/红=有败）+ 每自检项一格色块
  （顺序=登记顺序，名称见案例头注释）——无字体依赖（色块化）；
- **双形态定约落地**：`pygame::surface` 升格正式模块（绑定定约文档 +
  `new/with_depth` 薄入口，公开路径 pygame::surface::Surface 不变）。

### 排障沉淀（本批实测）

- `#[path]` 相对源文件目录（兄弟文件 = `common.rs` 非 `pygame/common.rs`）；
- `get_screen()` 返回引用——二次取引用（`&&T`）不满足 `&impl DrawTarget`
  （传引用本身即可）；
- `DrawTarget::size` 是 trait 方法（需 trait 引入作用域）；
- with_batch 闭包参数名遮蔽外层同名变量（mask 案例实测）——闭包参数
  避免与业务变量重名。

### 冒烟

9 案例（rect/color/time/draw/surface/mask/display/event/io）各 4s：
panic/脚本异常 = 0（全部干净）。绑定层（bridge/native_pygame/
pygame_impls）三端产物同步重建。

### Android 真机包（批次二十五补，2026-10-11）

九案例全部补 `_android` 双注册并出包（`starfish/target/android-apk/
pygame_test_<m>_android.apk`，各 ~2MB，统一脚本批量出包 lib_name 自检
全 ✓）——真机"实际运行能力"验证就绪：装机 → 启动 → 状态横幅绿=该模块
自检全过 / 红=有败（每格一项，顺序见案例头注释）。建议顺序：先强自检
四件（rect/color/mask/io）再 GPU 三件（draw/surface/display/time）。

### 根因定案（2026-10-11，用户找到）：缺失 `display::flip()`

九案例从未调用 `display::flip()`——**没有 flip 就没有 present，绘制指令
永远停留在交换链，屏幕永不刷新**。此前所有"异常"的统一解释：

| 现象 | 真相 |
|---|---|
| Windows 白屏 | 窗口背景（白刷）透过未呈现的交换链 |
| "只有正确的清屏颜色" | 加载帧的 navy（加载帧有自己 flip 所以能显示），场景帧无 flip 永不覆盖 |
| web 黑屏 | 画布无 present → 透明 → 页面黑底透出 |
| "过一会才出现" | 加载帧有 flip 能显示；场景无 flip 永不覆盖 |

**教训入册**：
1. **flip = present = 画面刷新——pygame 循环四步（fill→draw→flip→tick）
   缺一不可**；教程心智模型里 flip 不是可选步骤；
2. **阶段打点必须覆盖每一阶段（含 flip/present 后）**——本批 [t] 标记
   集曾有"flip 前/paced 后"却无"flip 后"，翻转失败在日志里不可见——
   打点集的盲区 = 排障的盲区；
3. **用户建议的对照法（pygame_hello 逐行对比）一击定位**——控制变量
   + 最小差异对比，比任何远程推测都快。

九案例已补 flip，Windows/Android/Web 渲染全部正常（用户确认）。

---

## 批次二十六（2026-10-11）：pygame 模块测试套件全模块覆盖（examples/pygame/，25 案例）

### 背景

批次二十五定约 9 案例，但磁盘仅存 rect/surface 两个，且与档案不符
（rect 头注释列 8 项自检实际未实现、surface 缺 flip/自动退出）——
本批重建并对齐档案口径，同时按用户决策扩展到 **pygame 层全模块**。

### 落地

- **修复 2 案例**：rect（8 项自检实现 + 矩形场景可视化）、surface
  （补 flip/自动退出/晚登记项）；
- **补齐套件 7 案例**（Cargo.toml 既有注册，文件此批落地）：color /
  time / draw / mask / display / event / io；
- **扩展 16 案例（新增桌面+_android 双注册）**——纯逻辑族：
  version / locals / math / key / mouse / transform / sprite；
  资产/硬件族（无设备/资产环境安全，降级自检 = API 面 + 错误路径）：
  font / image / sndarray / video / net / gamepad / touch / dialog /
  permission。安全红线：dialog 绝不弹系统对话框、permission 绝不触碰
  Microphone（Android 阻塞弹框）；
- **common.rs 增 `quit_requested()`**：事件泵 + Quit/ESC 判定统一入口
  （自带泵的案例如 event 自行排空）；
- **examples/pygame/README.md 重建**：25 案例索引 / 判读约定 / 运行
  方式 / 已知坑清单。

### 排障沉淀（本批实测）

- **内存耗尽的幻觉错误**：25 路并行 rustc 触发"页面文件太小
  (os error 1455)"→ E0786 invalid metadata / E0463 can't find crate /
  "no_std" 幻觉全是其连锁症状——`cargo build -j 4` 限流后干净通过。
  大目标集批量构建先限并行；
- **`_android` cdylib 不走宿主机构建**：宿主 `cargo build --examples`
  混编 bin+cdylib 全集触发 rlib 单元冲突（"required to be available
  in rlib format"）——android 变体只经 build_apk.py（cargo ndk 交叉）
  编译，桌面验收只跑 bin 案例；
- **`pygame::time::Clock::tick` 返回秒**（非 pygame 的毫秒；绑定位
  换算）——time 案例首版按毫秒断言恒败，已按秒修正；
- **`TcpConn::connect` 立即返回**：坏地址可能 Ok + 异步失败
  （state → Closed），断言须容两态；轮询型检查须带"已收到"标记防
  超时分支重复登记（net 案例实测——UDP 自环本身一次通过）；
- **语义偏差备查**：Rect::clamp 对界内矩形原样返回（测 clamp 须造真
  越界）；水平线 bounds 高度可为 0；font render 面宽随字符数、不随
  字号单调（比字号用行高）；
- **KMOD_* 常量在 `pygame::key`**（locals 只有 K_*/BUTTON_*/display
  旗标——常量枢纽按子模块组织）；
- **案例文件缺失教训**：批次二十五档案与磁盘漂移（9 案例 → 实存 2），
  接手先核对磁盘实物再补账。

### 冒烟

25 案例桌面全量：exit=0 × 25，自检合计 **129/129 全绿**（rect/time/
draw/font/net 五案例首跑暴露案例侧断言 bug，修正后复跑全过；引擎侧
零改动）。Android 真机包沿用统一脚本：`python pygame-rs/scripts/
build_apk.py pygame_test_<m> --dir .`。

### 其他

- **启动沙漏定案（不修）**：Windows 启动时鼠标短暂沙漏 = 系统 app-
  starting 光标——建窗（display.rs:105）与主循环首次泵消息之间隔着
  GPU 装配的同步阻塞段，泵一启动即自愈，零功能影响。候选修法
  （建窗后排空消息，Windows 分支特例）经用户定案**不做**：跨平台
  行为一致性优先，不为纯观感问题在引擎统一路径加平台特例；
- **Cargo.toml**：上游窗口 crate 更名 starfish-window → starfish-we
  （v0.2.0），以 `package = "starfish-we"` 别名过渡——代码路径
  `starfish_window::` 零改动；vendor 副本确认缺失（HEAD 从未入库）；
- **CLAUDE.md**：仓库布局两行同步（examples/pygame/ 套件指针 +
  starfish-we 别名说明）。

### 批次二十六补（2026-10-11）：三端产物再生成 + 历史遗留清理

- **清理**：根目录 11 个测试运行残留（9 张取证 png + __io_test__.*×2）；
  web/ 22 个调试残渣（rect_v3~v7.log、*_shot.png、bp_*.log 等）+ 旧
  9 套件过期 wasm 产物（源码已重写）；根目录 jniLibs/（手工打包时代
  .so 暂存）与空 tools/；target/flycheck0（rust-analyzer 临时目录）；
- **web 产物再生成（25 案例）**：release wasm 构建（-j 4）→
  wasm-bindgen（0.2.127）→ 25 × html 加载页 + `pygame_test_index.html`
  索引页重写为三组 25 条目；
- **Android 产物再生成（25 案例）**：统一脚本逐个出包
  （build_apk.py，APK 落 `target/android-apk/pygame_test_<m>_android.apk`）；
- **案例 wasm 适配两处**（runtime `cfg!` → 属性 `#[cfg]`，引擎侧
  平台门控对齐）：mask 案例走 `from_surface_async`（wasm 回读异步）；
  image 案例 save 回环桌面专属（Web 无后端环境仅解码自检）。教训：
  **引擎侧 `#[cfg]` 门控的 API，案例侧必须用 `#[cfg]` 属性分叉**，
  runtime `cfg!` 会把不存在的一侧也编译出来。

### 批次二十六补二（web 产物抽查结论）

headless（虚拟时间 + `--force-device-scale-factor=1`）抽查 rect/gamepad/net：
wasm 载入、GPU 渲染、状态条渲染全通——rect 状态条 8 格全绿（与桌面
129/129 一致），场景几何逐矩形对位正确（m 覆盖 r 的 L 形露边、clamp
白块位置均符合）。**观察项（不属本批，留单独排查）**：web 呈现色彩
整体偏亮（绿→青、黄→白，疑似 sRGB/linear 呈现换算偏差）；headless
验证遵循坑 2/3/7——虚拟时间截图会撕裂（--timeout 真实时间模式全黑，
rAF 不走真实时钟），色彩核对须存活模式。

### 批次二十六补四（"web 色彩偏亮"根因定案：案例侧颜色约定错用，非平台问题）

- **像素级取证**（headless 截图逐点采样）：fill 路径的画布底
  (30,30,46) 分毫不差，而 push_quad 的横幅 (0,220,90)→(0,255,255)、
  紫 (160,60,200)→白、暗蓝 (60,70,110)→白——**规律 = ≥1 的分量钳到
  1、<1 原样保留**，即 u8 风格 f32 值进了 0..1 约定管线被静默钳制；
- **根因**：`push_quad` 颜色参数约定为**归一化 f32 (0..1)**（验收过的
  pygame_probe 传 [0.9, 0.25, 0.25, 1.0]），而套件案例（本批 + 批次
  二十五原版）传了 u8 风格 [220.0, 90.0, ...]——**全平台一致钳白**，
  与 web 无关（桌面冒烟只读退出日志没核色相，web 截图才第一次被
  看了像素）；fill/draw::* 走 Into<Color>（u8 自动归一化）所以一直
  正确——双颜色约定并存是本次错用的土壤；
- **修复**：20 个案例文件 57 处字面量 + color.rs 两处计算表达式
  机械归一化；像素复测五点全部分毫不差（横幅/绿/紫/黄/底）；
- **防呆建议（引擎侧，留决策）**：push_quad 家族改收 impl Into<Color>
  （与 draw::* 同约定，u8/f32 归一化都接受）可整类消灭此错用；
- 修复后 web/APK 全量重出。

### 批次二十六补六（time.tick 帧率控制正确化：上限节流 + vsync 量化带）

- **问题**（用户三端实测）：实际帧率 win > web > android。根因 = 节流
  拆在两处互相竞争——`flip` 的 Fifo 回压（vsync，三端默认）+ 帧循环
  睡眠：Windows 回压在下一帧 acquire 才阻塞（睡眠可与之重叠）；
  Android GLES 的 eglSwapBuffers **在 present 内部硬等**，裸 sleep
  16ms 与 vsync 16.7ms 完全串联 → ~30fps；套件 `paced_frame` 的裸
  sleep 正是 Android 慢的元凶；
- **修复一（引擎 `Clock::tick`）**：语义改为**上限节流（cap）**——
  实际帧距已 ≥ 目标周期（vsync 吃满）不再补睡（消除"睡+等 vsync"
  双重节流的半速竞态）；剩余量不足目标周期 1/4 时也放行（vsync
  量化带内补睡只会增加跨越 vblank 边界的风险，实测 28.8ms 尖峰
  由此而来）。`tick(60)`@60Hz = 纯 vsync 稳 60；`tick(0)` = 跟帧正典；
  - **验证**：time 案例桌面 fps 53.8 → **59.6**，帧距 18-28.8ms →
    13.7-16.7ms；新增单测 `tick_is_cap_not_force` ✓；
- **修复二（套件 `paced_frame`）**：裸 sleep(16ms) → `tick(0)`
  follow-vsync——三端各自跑到原生刷新率（Android 不再串联减半）；
- **Web 说明**：睡眠在 wasm 是 no-op，web 帧率恒 = 浏览器 rAF（刷新
  率），`tick` 只作度量——高刷屏 web 会快于 60，属平台语义；
- **真机验收**：Android 装 `pygame_test_time` 看 console 的
  `tick min/max` 与 `fps` 行（预期 ≈ 设备刷新率、无倍频尖峰）。

### 批次二十六补五（色彩统一定案：桌面交换链默认改 linear——引擎侧修改）

- **续补四的取证**：像素归一化修复后用户仍报"web 略深"→ 桌面/同点
  采样对比定案——**web 字节级正确，被提亮的是 Windows 桌面**：
  桌面横幅 (0,220,90)→(0,239,160)、底 (30,30,46)→(96,96,118)、
  边框 (40,90,220)→(110,160,239)，三项全部精确命中
  linear→sRGB 编码公式 = 桌面交换链选中了 **sRGB 格式**（wgpu caps
  首选），写入时多做一次编码；web canvas 受浏览器限制只有 linear
  直通（= 创作值）；安卓 GL 表面同为 linear；
- **修复（引擎侧）**：`settings.rs::to_wgpu` 默认选格式跳过 sRGB
  变体（`find(!is_srgb())` 回落 caps[0]）——回到契约 ④ Rgba8Unorm
  的"创作值即显示值"直通语义（SDL/pygame 同路数）；显式指定的
  color_format 不受影响；
- **验证**：桌面像素复测三色字节级命中（横幅绿 16424 px / 底
  83339 px / 边框蓝 4680 px，旧提亮值清零）；web 端此前已验证直通
  正确——**三端字节级一致达成**（用户第一优先级）；
- 影响面：桌面渲染整体亮度回落到创作值（此前所有桌面截图/观感
  偏亮的都应以此为准）；透明窗口 alpha_mode 逻辑未动。

### 批次二十六补七（续）：time.tick 帧率控制正确化

- **问题**（用户三端实测帧率 win > web > android）：节流拆在
  flip 的 Fifo 回压与帧循环睡眠两处互相竞争——Android GLES 在
  present 内部硬等 vsync，与裸 sleep 串联减半（~30fps）；
- **修复（引擎 `base::time::Clock::tick`，pygame 层纯委托）**：
  语义改**上限节流（cap）**——实际帧距已 ≥ 目标周期不再补睡 +
  量化带（剩余 < 1/4 周期）放行；套件 `paced_frame` 裸 sleep →
  `tick(0)` follow-vsync；
- **验证**：time 案例桌面 fps 53.8 → **59.6**，帧距 18-28.8ms →
  13.7-16.7ms（尖峰消失）；新增单测 `tick_is_cap_not_force`；
- Web 平台睡眠为 no-op，帧率恒 = 浏览器 rAF（平台语义）。

---

## 版本 v0.9.10 发布注记（2026-10-11）

自 v0.9.9 以来的主要变更：

- **新增**：pygame 模块运行测试套件（25 案例 / 129 项自检，三端产物
  与索引页；`examples/pygame/README.md`）；
- **修复**：三端色彩字节级统一（案例颜色归一化 + 桌面交换链默认
  linear）；`time::Clock::tick` 上限节流语义（vsync 双重节流消除，
  Android 串联减半修复）；
- **兼容**：上游窗口 crate 更名 starfish-we，Cargo 别名过渡（代码
  路径 `starfish_window::` 不变）；
- pygame 层契约面无破坏性变更。

### 批次二十六补三（用户 web 实测反馈：io 噪音 + video 2/4）

- **io 案例 console 404 = 预期噪音**：该案例的检查本体就是"读缺失
  文件必须 Err"——web 上 fetch 404 → Err 即被验能力，浏览器把失败
  请求打到 console 不可避免；headless 实测 web 端 3/3 全绿，判定
  不受噪音影响；
- **video 案例 web 2/4 = 案例断言错配（已修）**：web 的
  `VideoSystem::open` 走 `<video>`/fetch **异步失败语义**（open 立即
  Ok、错误延后），同步 Err 断言仅原生成立——按 `#[cfg]` 分叉：web
  形态 = new + 系统面可用（且不主动发起必 404 请求），web 实测绿；
  wasm/APK 均已重出；
- **shell 教训**：`cargo build … | grep -cE "^error"` 匹配 0 条时
  grep 退出码非零 → `&&` 链静默断裂，后续 wasm-bindgen 绑到**旧
  wasm**（截图复现旧版中间态才暴露）——产物链验证要看产物时间戳，
  不信中间命令的"成功"。
