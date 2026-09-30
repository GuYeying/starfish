# Starfish 更新日志 2026-09-19

> 本日主题：**probe 家族落地——按模块独立测试案例 + 统一入口宏，验证
> "一套源码、零 `#[cfg]` 跑全平台"**。旧示例（学习路线 01~22）整体保留
> 归档 `examples/old/`；新增 `examples/probe/` 十个模块探针 + 共享 kit.rs；
> 库侧入口经用户三轮审读收敛为**三平台同名 `run`**（`app_entry!` 一行门面），
> io 基准目录补齐 Web 对称。

## 批次 1：库侧统一入口（base::platform + app_entry! + run_android 注入）

### 设计背景

跨平台验证一直靠 platform/ 探针，但样板重复约 600 行、入口必须手写
cfg 块（Android 的 android_main）、平台分支渗入应用代码。定案重构方向：
**探针的核心使命 = 验证"一个接口跨平台"**——应用代码第一行到最后一行
零 `#[cfg]`，平台差异全部由库与 harness 内部吸收。

### 设计方案

- **`base::platform`（新模块）**：`AndroidApp` 库内再导出（app_entry! 的
  android 分支经 `$crate::base::platform::AndroidApp` 取类型，用户 crate
  零 winit 依赖）+ 应用私有目录 `OnceLock` 全局（`set_data_dir` 幂等注入 /
  `data_dir()` 只读消费，未注入返回 None 可诊断不 panic）
- **`run_android` 收编样板**：`RUST_BACKTRACE=1`（原 14 处示例 android_main
  各自设置）+ internal_data_path 注入（platform 全局 + `io::set_base_dir`
  挂 feature="io" 门）——时序在 EventLoop 创建前，start()/frame() 必然晚于
  注入；旧 19 号双注入同值幂等无害
- **`app_entry!` 宏**（lib.rs，与 web_entry! 并列）：
  `starfish::app_entry!(App::new(), WindowConfig::new(..))` 一行覆盖全平台。
  三分支：android = `#[no_mangle] android_main → run_android`；
  非 android = `fn main → run`（wasm 也保留 main——bin 目标必需，浏览器走
  start 永不调用）；wasm = `#[wasm_bindgen(start)]`（panic hook + main）。
  依据：`run` 双定义（桌面 `!` / wasm `()`）使两分支类型均正确
- **顺手修复**：dialog 的 iOS 编译破损（`ensure_classloader` android-only
  函数在 android/ios 共用 imp 模块被无条件调用）——两处调用点加
  `#[cfg(target_os = "android")]` 门（类加载器自愈是 Android 容器专属）

### 关键保证

- 入口对仗结构：`run`（桌面/iOS/Web）+ `run_android`（Android）+
  `app_entry!`（统一门面）——iOS 编译通过即结构证明（实机待验）；
  本结构经用户审读在**批次 6** 进一步收敛为三平台同名 `run`
- 宏参数约束：纯构造表达式（两处展开仅参与类型检查，运行时单次求值）

### 测试状态

- `cargo check --lib`：桌面 / aarch64-linux-android / aarch64-apple-ios
  三目标零 error；`cargo test --lib` 55 passed

---

## 批次 2：旧示例归档（examples/old/）

- 22 个示例文件 `git mv` **平铺**进 `examples/old/`——平铺后目录深度不变，
  14 个文件的 `include_bytes!("../../resources/…")` 嵌入路径**零修改**
- Cargo.toml 37 条 `[[example]]` 注册只改 path，名称/required-features/
  crate-type 全不动（`cargo run --example 13_video_decode` 等命令照旧）
- xtask 零改动（动态解析 Cargo.toml）；验证 `cargo check --examples` 全过

---

## 批次 3~5：probe 家族（examples/probe/，10 模块 + 共享 kit.rs）

### 设计方案

**kit.rs（共享 harness，kit 内允许 cfg，probe 应用代码零 cfg）**：

- `StatusPanel`：多行状态面板（内嵌 Antonio 字体 + 94 字符图集 + 相机/
  管线装配 + 脏网格重建 + 每帧按 ctx.size() 写投影——根治旧 20 号
  camera uniform 从未写入的 bug）；`with_gpu` 作用域化 GPU 访问
  （规避 RefMut 跨语句借用冲突）；Resized→surface.resize 内置
- **三态判定** `Status::{Pass, Skip, Fail, Info, Pend}`：能力缺失
  （Web 无 UDP）= **SKIP 非 FAIL**；`verdict` 双通道 = 屏显 +
  console 锚点 `[{probe}] {TAG} PASS|SKIP|FAIL detail`
- 平台感知查询（调用点零 cfg）：`asset_path(逻辑路径, include_bytes!)`
  （桌面/ios 原样相对路径；web 原样 URL；Android 内嵌字节幂等落盘
  `{data_dir}/probe_assets/`）；`save_path`（web 补 saves/ 前缀命中
  server POST 端点）；`page_hostname`（web 拼 ws:// 目标）；
  `record_permission_granted`（Android ensure_permission / 其余直接过）；
  `enable_broadcast`（set_broadcast 仅原生存在）；统一轮询式对话框 Job
  （原生 pick_file_start / Web pick_file spawn_local 填槽，同一 try_result）

**10 个探针**（全部 app_entry! 一行入口，bin + `*_android` cdylib 双注册，
required-features 按模块）：

| 探针 | 判据（console 锚点） |
|---|---|
| probe_window | `[window] SIZE PASS` |
| probe_font | `[font] ATLAS PASS n=95` + `DYN PASS` |
| probe_gfx | `[gfx] BUILD PASS` + `DRAW PASS`（overlay 自建双管线 4 形状） |
| probe_audio | `[audio] DECODE/PLAY/DONE PASS` |
| probe_record | `[record] PERM PASS` + `WAV PASS bytes=N`（>44B 头才算采样，头部-only = SKIP） |
| probe_video | `[video] OPEN/FIRST/ENDED PASS`（六平台同一硬解链路） |
| probe_gamepad | `[gamepad] POLL PASS pads=N`（pads=0 也 PASS——验证 API 可调用） |
| probe_dialog | `[dialog] START PASS`（无头可自动判定）+ RESULT/SAVE（需人工） |
| probe_net | `[net] TCP PASS` + `UDP PASS/SKIP` + `VERDICT PASS`（TCP 为底线） |
| probe_io | `[io] WRITE/EXISTS/READ/WRITE_TXT/READ_TXT PASS` → `ALL PASS` |

**server.py 配套**：`--resources-dir`（默认 ./resources）挂载——URL
`resources/<rel>` → RESOURCES_DIR/<rel>（带目录穿越防护）。效果：视频等
资产**三平台同一逻辑路径字符串** `resources/videos/sample-5s.mp4`。

### 测试状态（全部实测）

- **wasm 无头存活模式**（msedge headless + console 锚点收割）：
  probe_window `SIZE PASS 800x600` ✅；probe_io 五步 `ALL PASS` ✅；
  probe_font `ATLAS PASS n=95` + `DYN PASS` ✅；probe_audio
  `DECODE/PLAY/DONE PASS` ✅（--autoplay-policy flag）；probe_gamepad
  `POLL PASS pads=0` ✅；probe_gfx `BUILD/DRAW PASS` ✅；
  probe_dialog `START PASS` ✅（RESULT 需人工，无头允许等待态）；
  probe_record `WAV = 44B 头`→ 判据改为 SKIP（无头无真实输入设备）✅；
  probe_net `UDP SKIP + TCP PASS + VERDICT PASS` ✅；
  probe_video `OPEN/FIRST PASS 1920x1080 → 实时推进 → ENDED PASS` ✅
- **桌面**：probe_gfx 冒烟运行无 panic；probe_net
  `DISCOVER PASS 192.168.31.9 → TCP PASS → UDP PASS → VERDICT PASS` ✅
- **Android**：probe_io / probe_dialog / probe_gfx / probe_video 四件
  APK 出包签名通过（`cargo xtask android <名> --build`）
- **回归**：`cargo test --lib` 55 passed / 0 failed；桌面 + Android +
  iOS lib check 零 error；`cargo check --examples` 全部 57 条注册通过

### 经验沉淀

- **借用纪律**：探针状态机的阶段借用内只计算结果（enum Out），`finish()`
  等动 self 的调用放借用结束后——`if let &mut self.step` 里直接调
  `self.finish()` 必然 E0499
- **平台差异三消化点**：库 API 行为（UdpSock::bind web 返回 Err → SKIP）、
  kit 内部 cfg（asset_path/save_path/对话框 Job）、app_entry! 宏——
  应用代码零 cfg 是可实现的，且三个消化点各有职责边界
- 旧 20 号"文本能显示但从未写投影"之谜未深究（旧版留档 old/ 不修），
  kit 每帧写投影根治

---

## 批次 6：入口抽象收敛 + 零 cfg 对称性补齐（用户审读驱动，同日三轮）

> 本日批次 1 的入口结构经用户三轮审读逐级收敛，另带两处对称性/稳定性
> 补齐——每轮都由"读代码发现问题"驱动，是审读即测试的直接例证。

### ① inner_run 抽象收敛——Android 接入共用驱动尾

`run_android` 复制了 `inner_run` 的整段尾部（节流策略 + Adapter 装配 +
`run_app` + 退出语义），抽象不完整。重构：

- `inner_run` 改为**接收已构建好的 EventLoop**（`EventLoop<()>` 入参），
  只负责"驱动循环 + 退出语义"；三平台退出语义差异（桌面 0/1、Android
  恒 exit(0) 清缓存进程、Web 永不返回）以 cfg 尾收敛在此一处
- 三个入口各自只做**平台专属引导**：桌面/Web = `EventLoop::new`；
  Android = 样板注入 + ndk-context 修正 + android-activity 循环构建
  （RecreationAttempt 兜底 exit(0) 保留在构建处）

### ② 入口终极对仗——`run` 三平台同名，`run_android` 降级垫片

追问"run_android 能否也同步成 run"——可以：

- **障碍**：Android 的 `AndroidApp` 句柄只能从 OS 调用的 `android_main` 拿到
- **解法**：`app_entry!` 生成的 `android_main` 只做"捕获句柄到
  `base::platform::ANDROID_APP` 全局槽 → 调 `main()`"；`fn main` 改为
  全平台无条件生成；Android 的 `run` 从槽 `take` 句柄完成引导
  （样板注入 / ndk-context 修正 / android-activity 循环构建全部随迁）
- **`run_android` 降级为 3 行兼容垫片**（捕获 + 委托 run），旧示例手写的
  android_main 不受影响
- 前置核实：AndroidApp = `#[derive(Debug, Clone)]` + 上游文档明确
  "implements Send and Sync" → `OnceLock<AndroidApp>` 静态槽合法

### ③ `io::set_base_dir` 补齐 Web 实现（对称性破洞）

原实现仅原生存在（`cfg(not(wasm32))`）——任何想指定基准的桌面应用写
`io::set_base_dir(..)` 时必须 cfg 排除 wasm，违背零 cfg 铁律。用户指出
"Web 本质也是拼接字符串"，成立：

- Web 新增 `BASE_URL` 前缀槽 + `resolve_url`：相对路径 → `{base}/{path}`
  （base 尾斜杠归一）；`/` 开头 = origin 绝对、`http(s)://` 与 `//` 开头 =
  绝对 URL，均原样——对齐原生"绝对路径不受影响"
- `set_base_dir` 签名统一 `impl AsRef<Path>` 全平台；未设置时 Web 保持
  浏览器相对语义（默认行为零变化）。用途：资源挂子路径 / 资产走 CDN
  跨源前缀 / API 面对称

### ⑤⑥ platform 模块剔除——句柄事务并入入口模块（用户审读驱动）

两轮收敛（⑤ 瘦身 → ⑥ 剔除）：

**⑤ 数据目录归 io**：`platform::DATA_DIR` 三件套（`set_data_dir` /
`data_dir`）与 io 的 `BASE_DIR` 在 Android 上**同值双写**（都是
internal_data_path），双全局存一个事实——删除 platform 侧，io 新增只读
getter **`base_dir() -> Option<PathBuf>`**（native-gated；Web 的"基准"是
URL 前缀、无读取方故不设）。`set`/`get` 对偶补全，消费者迁移：
kit::asset_path 的 android 分支改 `io::base_dir()`（资产提取跟随应用的
io 沙箱根决策，语义更连贯）。

**⑥ 模块整体剔除**：⑤ 后 platform 仅剩 AndroidApp 再导出 + 捕获槽，全部
消费者（app_entry! 宏 + run）都在入口路径上——整模块并入 `base::app`
（`pub use AndroidApp` + `ANDROID_APP` 槽 + `set/take_android_app`），
`src/base/platform.rs` 删除。入口相关的 OS 事务自此全部内聚在入口模块。

**rt 主线程锚点三重调用收敛**（用户问"run 与 inner_run 重复调用"）：
`mark_main_thread` 是 `OnceLock` 幂等写，三处调用无害但冗余——收敛为
仅 `inner_run` 一处（所有入口的必经漏斗）。

### ⑦ 诊断设施归一——`base::debug`（吸收 web.rs + rt.rs，用户审读驱动）

用户指出 rt 属于调试设施，应收拢进"类似 web.rs 的诊断模块"。落地方案
比照原样更彻底：**web.rs 与 rt.rs 合并为 `base::debug`**——web.rs 的
console_log / panic hook 本就是诊断输出（叫 web 只是历史命名），rt 的
线程契约是调试断言，三者同属"开发者诊断设施"：

- `debug::console_log`：跨平台日志（路径变更，语义不变）
- `debug::install_panic_hook`：wasm 崩溃转发（web_entry!/app_entry! 自动调）
- `debug::mark_main_thread`（pub(crate)，inner_run 唯一调用）+
  **`debug::assert_main_thread(site)` 公开**——开发者的主线程专属 API
  可自保：跨线程误用 debug 构建立即 panic 指明现场，release 零成本
- 后续调试工具（帧率统计、GPU 标签、性能打点）统一落此模块

### ⑧ `base::permission` 新模块 + 权限隐式化（用户定案）+ debug 文件夹化

用户两项设计决策（⑧ 初稿的 `base::os` 命名被用户否决——"名字级别不对"，
改为按概念域命名；权限处理从显式调用升级为**模块构造时隐式申请**）：

**permission 模块（权限申请迁家 + 隐式化）**：`ensure_permission` 曾误居
dialog——权限是 OS 能力、对话框是 UI 通道，音频场景因此跨域依赖 dialog
特性。新建 **`base::permission`**（按概念域命名）：

- `permission::ensure(permission: Permission)` **全平台签名 + 枚举入参**
  （用户定案：调用方只声明"需要什么能力"，平台细节私有化——Android 权限
  字符串由 `Permission::android_name()` 内部翻译，调用方永不见
  "android.permission.RECORD_AUDIO"）：Android = JNI requestPermissions
  受控阻塞（实现自 dialog 随迁）；桌面/Web = 恒 true（**Web 为委派语义**：
  浏览器模型下授权发生在紧随其后的 getUserMedia，主动预申请 = 白白开一次
  设备流，恒 true 即正确实现而非缺失）
- **隐式权限（本设计的核心）**：需要权限的模块在自己的构造路径上隐式调
  `ensure`——`AudioRecorder::new_with_capacity` 已内置麦克风申请，应用侧
  零感知零 cfg（与 Web 现状对齐：getUserMedia 授权本就在 AudioRecorder
  内部触发）。需要精确控制授权时序的应用仍可显式调用（API 公开）
- **Cargo 依赖随迁**：jni / robius-android-env 从 dialog/video 的可选
  依赖升为 **Android 非可选**（对齐批次 17 的 ndk-context 先例：核心路径
  使用 → 非可选）；dialog/video 特性面相应瘦身
- 消费者迁移：kit 删除 `record_permission_granted` 包装、old/10 与
  probe_record 直用 `permission::ensure`
- **`Permission::Internet` 补齐（socket 权限）**：Android 权限分两类——
  dangerous（`Microphone`：清单+运行时申请双管齐下，`ensure` 阻塞弹框）
  与 normal（`Internet`：清单声明即安装时授予，`ensure` 瞬时通过无弹框，
  纯语义声明）。清单由 xtask 的 APK 模板统一声明（INTERNET/RECORD_AUDIO
  均已声明）；net 的 `TcpConn::connect` / `UdpSock::bind` 隐式声明
  Internet（与 audio 的隐式模式统一，行为零变化——probe_net 回归
  `VERDICT PASS`、APK 出包确认）
**debug 文件夹化（用户定案：底层调试严格不做强行归一）**：
`debug.rs` → `debug/` 文件夹，按平台分文件（对齐 video 模式）：

```
debug/
├── mod.rs      # 统一入口（console_log / assert_main_thread）+ 线程契约
├── web.rs      # wasm 实现（console.log / panic hook）
└── native.rs   # 原生实现（stdout；panic 天然走 stderr）
```

统一只统一"两边都有的部分"（日志）；平台专属能力保持平台门控
（install_panic_hook 仅 wasm）——**统一入口 + 平台 uneven 的能力面**。

### ⑨ mixer 流式声部 API + play 家族 fade 参数退役（video 音轨地基 ①）

用户确认对称性设计后开工。**mixer 分层定稿**：channel（混音总线槽位）下
三类声源各归其位——SFX 声部（`play_with`，一次性全量缓冲，隐式回收）/
流式声部（**`open_stream_voice`，推式 PCM，显式三段式** ★新增）/
music（文件驱动流，mixer 自带解码线程，保持不动）。

**`StreamVoice`**（`audio/voice.rs`，句柄 Clone 共享声部）：

- `push_interleaved(&[f32]) -> usize`：推交错立体声帧；环形缓冲（16384
  帧 ≈ 340ms @48kHz）满时**背压少收**——推方节奏被消费端拽住，这正是
  阶段④ A/V 同步的物理锚点（推帧按 `output_sample_rate`，v1 不重采样）
- `set_volume` / `set_muted` / `fade_in(ms)` / `fade_out_and_close(ms)`
  （淡出走完自动关闭——视频 ended 收尾标准打法）/ `close`
- 混音回调接入：mix 尾部对每路流式声部读环求和（音量 × 静音 × fade 增益，
  fade_out 走完自动摘除声部），软限幅前汇入

**play 家族 fade 参数退役**（用户指出：`channel_fade_in/out` 已存在，
播放时 fade 参数冗余——核实成立，`play_with` 后跟一句
`channel_fade_in(channel, ms)` 听感等价）：

- `play_with(sound, loops)` / `play_in_group(group, sound, loops)` /
  `SfxChannel::with_sound(sound, loops)` 全部摘 fade；`loops` 保留
  （环境音循环是独立真实能力）
- SFX 淡变能力归通道层 `channel_fade_in/out`（既有 API，与流式声部的
  fade 方法族对偶）
- 连带修正一个 lib 测试的旧签名调用

### 验证（批次 6 汇总）

- `cargo test --lib` **56 passed**（14+ 连跑全绿，flaky 修复确认）
- 桌面 / Android lib + example（新旧两种 android_main 形态）check、
  wasm 构建全过；`cargo xtask android probe_video/io --build` 真实 cdylib
  链接出包；桌面 probe_window 冒烟 `SIZE PASS`；probe_io 无头回归
  `ALL PASS`（set_base_dir 默认行为零变化）

---

---

## 批次 7：probe 首轮真实环境实测反馈修正（web 浏览器 + 卓易通）

### 反馈与修正（probe_dialog 两处，均属探针设计缺陷）

**① Web：文件选择器被用户激活策略拒绝**

- 现象：控制台 `File chooser dialog can only be shown with a user
  activation`，RESULT 永不到来
- 根因：帧 30 自动发起的 `input.click()` 无用户手势——浏览器的文件
  选择器必须在用户激活内打开（无头 + autoplay flag 环境测不出此项）
- 修正：保留自动尝试（无头/原生路径），**点击屏幕即重试发起**——
  真实浏览器点一下选择器即正常打开

**② Android（卓易通）：pick 后自动连发 save 导致卡死**

- 现象：选择后长时间卡顿不回应用窗口，返回键同样卡死；进程被杀后
  再点图标闪退（批次 19 的僵尸进程症状被上游卡死触发）
- 根因：probe 在 pick 返回后**下一帧立即发起 save**——背靠背拉起两个
  SAF 独立 Activity，容器上活动管理竞态导致卡死（16 号的已验证模式是
  手动交替，两次 SAF 之间有人的间隔）
- 修正：pick 结果后**等一次点击再进入 save 阶段**（屏显提示），两次
  SAF 拉起之间隔开人的操作间隔
- 连带修复：点击重跑的点火门原为 `f == 30`——重跑后 f 已过 30 永不
  触发（隐性 bug）；改为显式 `armed` 标志

### 重验

- probe_dialog 三平台重建（Windows exe / wasm+页面 / APK）；web 无头
  START 锚点正常；安卓实机重测待用户执行（重点：pick→点击→save 全程、
  以及卡死修复后僵尸闪退是否随之消失）

---

## 批次 8：old 示例删除 + 旧兼容代码清理（用户指令，probe 已完全接管）

- **`examples/old/` 整体删除**（22 文件 / 37 条 `[[example]]` 注册）——
  probe 家族已覆盖全部模块且效果对齐（probe_gfx 升级为 06 的 3D 场景
  呈现），git 历史可追溯；`resources/` 不动（probe 的内嵌资产引用其中
  多项）
- **`run_android` 垫片删除**——app_entry! 的捕获槽方案使其无消费者
- **`web_entry!` 宏删除**——app_entry! 全平台覆盖后无消费者；相关文档
  引用（debug 模块 / lib.rs）同步改指 `app_entry!`
- 入口 API 面收敛为：**`run` + `app_entry!` + `set_android_app`（宏内部
  消费）**；`android_main` 手写形态与 cfg 分家模板自此退出仓库
- 文档同步：CLAUDE.md（示例分区句 / run_android / 旧示例保留句 / Android
  双注册描述改为 app_entry! 形态）、README（目录树 / 运行命令换 probe 家族
  / 示例详解表换 probe 速查——完整重写待用户截图后另行执行）

### 测试状态

- `cargo check --examples`（20 条 probe 注册）零 error；`cargo test --lib`
  56 passed；Android lib check 零 error；`cargo xtask list` 20 条全双平台

---

---

## 下阶段立项交接：video 音轨 ②③（开新会话执行）

①（mixer 流式声部）已落地。②③ 的完整设计与风险如下，新会话按此执行：

### ② mp4_demux 音轨提取 + AAC 解码

- **demux 扩展**（`base/video/mp4_demux.rs`）：解析音频 track（`mdia/hdlr`
  type == `soun`），暴露 `next_audio_sample()` 与音轨元数据（codec/采样率/
  声道数）；MP4 内音频通常是 AAC-LC（esds 携带 AudioSpecificConfig）
- **AAC 解码**：symphonia 需加 `"aac"` feature（当前 features 只有
  ogg/mp3/flac/wav/pcm）
- **⚠️ 主要技术风险**：mp4 里是 raw AAC（无 ADTS 头），symphonia 的 AAC
  解码入口期望 ADTS——成熟做法是按采样率/声道数**手动打 7 字节 ADTS 头**
  再喂解码器
- **采样率适配**：AAC 常见 44100/48000 ≠ 设备混音域——推入声部前用
  线性插值重采样（复用 `SoundData::resample` 同款算法的流式版）

### ③ video 集成（API 已定，见批次 6⑨）

```rust
let video = VideoModule::new(dev, q).open(path)?;                        // 无声：音频链零初始化
let video = VideoModule::new(dev, q).open_with_audio(path, voice)?;      // 有声：音轨解码推入声部
video.set_muted(true); video.set_audio_volume(0.5);                      // 直通声部
```

- `Video` 增加可选音轨泵：**在现有 `update(dt)` 内驱动**（AAC 解码 →
  `push_interleaved`；背压满即停推，不阻塞帧）——维持"共用核心不假设
  线程存在"
- 时钟：v1 = 视频时钟（同帧起播近似同步）；④ = 锚定声部 ring 读点
  （背压已提供物理锚）

### 测试资产注意

`resources/videos/sample-5s.mp4` 是否含 AAC 音轨**未验证过**（探针一直
无声跑）——② 开工前先 ffprobe 检查；无音轨则转码生成一个带 AAC 的
测试资产（放 `resources/videos/`，Android 内嵌路径同步）。

### 验证设计

- lib 测试：demux 音轨样本数 > 0；AAC 解码 PCM 非全零；推入声部后
  `ring.available()` 增长
- 桌面实听：probe_video 变体（或 probe_audio 加视频音轨演示）
- 三平台编译 + wasm 无头回归照旧

---

## 本日测试状态汇总

- `cargo test --lib`：**56 passed / 0 failed**（含 base_dir 重定向新测试；
  5 连跑稳定——flaky 修复见批次 6④）
- 三目标 lib check：桌面 / aarch64-linux-android / aarch64-apple-ios 零 error
- examples 57 条注册（old 37 + probe 20）桌面全编译；probe 10 件 wasm 全构建
- web 无头：10 探针全部收割 PASS 锚点（record 按环境容差 SKIP、dialog
  RESULT 等人工）；批次 6 重构后 probe_io 回归 `ALL PASS`
- Android：4 件 APK 出包（probe_io/dialog/gfx/video，其中 probe_io 为
  批次 6 重构后的新宏路径真实链接），实机部署待用户执行
