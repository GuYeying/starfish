# Starfish 更新日志 2026-09-14

## 批次 10：设备/系统能力三件套——iofi（本地文件）+ dialog（原生对话框）+ net（网络）

### 设计背景

用户三份方案（`dialog方案.md` / `socket方案.md` / `iofi预期`）经评估全部可行，
按用户决策定稿：
- **socket 的 wasm-UDP**：不做 TCP 静默替代，直接声明不支持
  （与硬解唯一策略、多窗口先例同一哲学：无能力→显式报错）
- **dialog 全阻塞式**：本地交互低频且不影响网络 IO 正确性（net 独立线程）；
  本地游戏阻塞可控
- **net 不引入 tokio**（本库实现偏离方案文档的 tokio 选型，用户已认可方向）：
  游戏客户端连接数极少，每连接后台线程 + std 阻塞流即可保证"网络正确性
  不受主线程/本地阻塞影响"，编译时间与二进制体积归零

### 设计方案

**① `base/iofi.rs`（feature `iofi`）——本地文件基础读写**

- 同步阻塞 std::fs 薄封装：read/read_text/write/write_text/append/exists/
  is_file/is_dir/create_dir/remove_file/remove_dir/remove_dir_all/copy/list_dir
- 相对路径 = CWD 相对；**Web 无文件系统不参与**（std 在该目标的既定运行时错误，
  如实声明）；Android/iOS 编译可用（沙箱路径语义随引擎引导细化）

**② `base/dialog.rs`（feature `dialog`）——系统原生对话框（阻塞）**

- 桌面（win/linux(GTK3)/mac）：rfd 同步 API——消息框（Level×Buttons）、
  文件单选/多选/保存（Filter 过滤）
- Web：消息框走 `window.alert / confirm`（稳定 API，同步阻塞语义成立，
  确认映射 Yes/No）；**文件选择 v1 显式不支持**（`<input type=file>` 异步
  事件模型无法阻塞——不做伪阻塞）
- Android/iOS：占位（UnsupportedPlatform），引擎引导后接 robius 系
- `DialogError::{UnsupportedPlatform, Backend}`——不可用平台显式报错

**③ `base/net.rs`（feature `net`）——TCP 消息连接 + UDP 轮询**

- **TCP**：`TcpConn::connect(addr)` 立即返回（后台线程握手，10s 超时）；
  消息语义 = 4 字节大端长度前缀分帧（单帧上限 16MB）；每连接两线程
  （读循环 + 写冲刷），应用侧纯轮询 `state() / send() / try_recv() / close()`
  ——Connecting 期间 send 自动入队缓冲（512 帧上限），连接成功自动冲刷
- **UDP**：`UdpSock` 非阻塞轮询（bind/send_to/try_recv_from）；Web →
  `UnsupportedPlatform`（用户决策：不做 TCP 隐式替代）
- **Web TCP**：WebSocket（js_sys Reflect 自持绑定，稳定 API 无 unstable 门控）；
  `binaryType=arraybuffer`，onopen 冲刷积压 / onmessage 入队 / onclose·onerror
  置 Closed；地址归一（裸 `host:port` → `ws://`）
- Android/iOS：std::net 原生可用（TCP/UDP 全支持；Android 运行需
  INTERNET 权限，属应用清单职责）

### 关键保证

- **一套业务代码跨六平台**：poll 式 API（send/try_recv）对齐手柄/视频泵模式，
  主线程永不因网络阻塞；TCP 帧化与 WebSocket 消息语义对齐
- feature 门控：`iofi`/`dialog`/`net` 入 default，可剔除；
  `--no-default-features` = 最小核心依旧
- 真实测试：TCP 本地 echo 回环（连接状态机 / 分帧重组 / 64KB 大帧跨分段 /
  多帧顺序）+ UDP 回环 + iofi 读写往返——**全部本机自动验证**

### 测试状态

- `cargo test --lib`：55 passed（新增 iofi 2 + net 3）
- 特性矩阵：`--no-default-features` / 单开 iofi / dialog / net 零 error
- 跨目标：wasm（默认 + 全特性）零 error
- 待实机：dialog 真弹窗交互（桌面原生 / 浏览器 alert-confirm）、
  WebSocket 真服务器联调（当前仅 TCP 回环自动验证）

## 批次 11：三件套平台行为一致化（异步 dialog / 统一 iofi / trait 化 net）

### 设计背景（用户决策）

批次 10 落地后三模块平台行为不一致：dialog 桌面阻塞而 Web 文件选择只能
"不支持"；iofi 错误依赖各平台 std 的含糊运行时错误；net 的 TCP/WebSocket
两套实现无统一接口抽象。用户定稿：
- **dialog 统一异步**：阻塞平台在 future 体内直接执行阻塞实现（"模拟异步"，
  await 处即阻塞处），原生异步平台（Web 文件选择）天然适配——**Web 文件
  选择因此解锁**
- **iofi 收拢统一行为**：自有 `IoError`（`UnsupportedPlatform` 显式表达平台
  边界）+ `is_supported()` 查询，Web 不再依赖 std 含糊错误
- **net 抽象 `Connection` trait**：TCP(原生)/WebSocket(Web) 统一接口，
  `TcpConn` 持 `Box<dyn Connection>`；Web-UDP 维持创建即报错

### 设计方案

- **dialog 异步化**：`message/confirm/pick_file/pick_files/save_file` 全部
  `async fn`；返回类型统一为 `PickedFile`（桌面持路径、Web 选择即读入内存，
  调用方只依赖 `name()`/`read()`）；Web 文件选择 = 动态 `<input type=file>`
  + FileReader arrayBuffer（自持单线程 oneshot future 衔接 spawn_local）；
  Web 保存仍是"触发下载"语义差异，待 `save_bytes` 专 API
- **net trait 化**：`pub trait Connection { state/send/try_recv/close }`；
  `NativeTcp`/`WebTcp` 各自实现（state 原子内聚到后端，Web 以 readyState
  反射为权威）；帧上限校验留在 `TcpConn::send` 统一层
- 修正：close 未置位后端 state 原子的疏漏（测试捕获）；oneshot Sender 手动
  Clone（避免 derive 给 T 加多余约束）；WebTcp 方法收敛为 `*_ws` 系列消除双套并存

### 测试状态

- `cargo test --lib`：55 passed（TCP echo 回环捕获过 close 状态疏漏——测试有效性实证）
- 特性矩阵 / wasm / android / darwin 全部零 error

## 批次 12：iofi Web 补全（localStorage 虚拟 FS）+ dialog 移动端实装（robius）

### 设计背景（用户决策）

- **iofi Web 补全**：浏览器无文件系统，但"基本能用的存档/配置"可以用
  web-sys 稳定 API 实现——三条路（OPFS/IndexedDB/localStorage）中，OPFS
  主线程仅异步、IndexedDB 全异步，均与 iofi 的**阻塞契约**冲突；
  **localStorage 是唯一同步底座**（配额≈5MB，域共享）
- **dialog 移动端实装**：回到 dialog方案.md 原方案 A 的 robius 系——
  robius-file-picker 自带 Java/Kotlin 胶水（FilePickerFragment + 
  robius-android-env 注入，应用模板零改动）；⚠️ 方案文档里的
  `robius-alert` 在 crates.io 不存在（臆造名），消息框移动端后端待接
  （AlertDialog/UIAlertController 的 UI 线程模型）

### 设计方案

**iofi Web = localStorage 虚拟文件系统**
- 键 = `sf.iofi:<路径>`（`\`→`/` 归一、剥前导 `./`//）；目录以尾 `/`
  标记键表达；值为字节 Latin1 直映字符串（逐字节↔字符≤0xFF，自写自读
  无损，纯 Rust 转换不经 base64）
- 全部 14 个 API 落地：读/写/追加/存在/类型判定/建删目录（含"非空目录
  不可删"的桌面契约对齐）/递归删/复制/列目录（直接子项合成）
- 配额错误映射 `IoError::Io("存储配额…")`；`is_supported()` 恒 true
- 依赖：iofi feature 接 `dep:js-sys`（web-sys 恒在非 optional）

**dialog 移动端 = robius-file-picker**
- `pick_file`：robius 回调式 → mpsc channel 包装为阻塞（与桌面 rfd 同款
  "模拟阻塞"）；`content://` URI 经 `into_local_file()` 落为临时路径，
  PickedFile 与桌面同构（持路径）
- `pick_files`（robius 无多选 API）/`save_file`（robius 的 save_data 是
  "写数据"语义，与"选路径"不同）→ v1 显式 Unsupported，待统一 `save_bytes`
- 消息框移动端：占位 Unsupported（robius-alert 不存在；AlertDialog/
  UIAlertController 的 UI 线程模型待引擎引导立项）

### 测试状态

- Windows：55 passed；`--no-default-features` / 单开特性零 error
- wasm（localStorage 虚拟 FS 编译验证）/ android（剔除 dialog）/
  darwin（全特性含 robius iOS 后端）零 error
- 已知边界：dialog/Android 构建需 `ANDROID_JAR` 环境变量（robius 编译
  Java 胶水，随引擎安卓引导配齐）；iofi/Web 为 localStorage 实现编译级
  验证，浏览器行为待实机

## 批次 13：iofi 回退 Web 实现——改为编译期显式报错（用户决策）

### 设计背景（用户决策）

批次 12 曾为 iofi 实现 localStorage 虚拟 FS；用户复核后定稿：**Web 端基于
路径的文件加载直接剔除（回退代码）**——Web 的文件读取由既有能力间接完成：
`dialog::pick_file`（选择即入内存的 `PickedFile::read`）或 `base::net`。
要求：启用 `iofi` + wasm 编译 = 显式报错。

### 设计方案

- iofi.rs 回退为纯 std 实现（移除 localStorage 虚拟 FS 与 web 模块）
- `iofi` 移出 default 特性（**opt-in**）——否则所有 wasm 默认构建都会被
  报错拦截（连带破坏示例 11 的 Web 工作流）
- `base/mod.rs`：`#[cfg(all(feature = "iofi", wasm32))] compile_error!`
  报错信息含解决方案（剔除命令 + Web 替代途径指引）
- js-sys 依赖接线从 iofi 移除（localStorage 专用）

### 关键保证

- **编译期报错而非运行期**：与"硬解唯一/无能力显式报错"哲学完全一致，
  且把发现问题的时间点提前到构建阶段
- wasm 默认构建恢复零负担（iofi 本就无外部依赖，回退后 js-sys 接线移除）
- Web IO 路径明确：dialog 读内存 / net 网络加载

### 测试状态

- wasm 默认：干净编译；wasm + iofi：compile_error 按预期触发（错误信息
  含剔除命令与替代方案）
- 桌面默认 53 passed（iofi opt-in 后其 2 测试随特性走）；
  `--features iofi` 55 passed（iofi 测试回归）
- dialog/net 不受影响（wasm + dialog,net 编译通过——Web IO 替代路径可用）

## 批次 14：移除 iofi 模块（std::fs 透传包装无增值）

### 设计背景（用户决策）

批次 13 将 iofi 回退为纯 std 实现 + wasm 编译期报错后，复盘发现其存在价值
归零：**本质是 std::fs 的透传包装**——
- 桌面/移动：Rust 用户直接用 `std::fs`（能力更完整：BufReader/OpenOptions/
  metadata/流式），包装反而隐藏标准能力；
- Web：已定稿不支持（编译期报错），文件读取走 `dialog::pick_file`（读入
  内存）或 `base::net` 间接完成；
- 未来 PyO3 绑定：Python 有内建 `open()`，pygame 自身亦无通用文件模块——
  绑定层无需 starfish 透传文件 API。

用户论证定稿："基于系统 api 和 socket 已覆盖两种 IO 方式，iofi 没必要了"。

### 设计方案

- 删除 `src/base/iofi.rs`、`iofi` feature、base/mod.rs 声明与 compile_error
- 唯一保留的潜在增值点（移动端**沙箱数据目录** helper：Android getFilesDir /
  iOS Documents）不属于 fs 包装范畴——归入引擎引导立项后的 platform 模块

### 测试状态

- `cargo test --lib`：53 passed（iofi 2 测试随模块移除）；wasm 默认干净编译
- 特性矩阵收敛为 6 特性：gfx / font / video / gamepad / dialog / net

## 批次 15：模块说明文档体系（dialog/net 跨平台说明 + 行为审计）

### 内容

- 新建 `reference/dialog模块跨平台说明.md` / `reference/net模块跨平台说明.md`：
  API 清单 / 平台×能力矩阵 / 各平台阻塞·非阻塞精确语义 / 与引擎同步 frame
  的配合模式（代码示例）/ 已知边界
- 运行循环结论：**三件套均无需引擎维护运行循环**——iofi 纯函数式调用；
  dialog 阻塞体/浏览器任务队列自驱；net 线程/回调自驱 + 应用每帧轮询消费
- 行为边界审计入档：net（UDP 1500B 收包缓冲截断 / TCP 入站队列无上限 /
  原生 TCP 无 TLS / Android 需 INTERNET 权限 / 仅客户端 v1）、
  dialog（移动端 message 静默占位 / Web pick 手势要求 / 单发 input·回调
  泄漏策略）
- TLS 行为定稿写入 net 说明（原生明文定稿不支持；Web wss:// 自带）

### 说明

- 本批次为纯文档；后续批次 16/17 的行为变化已同步回写对应文档

## 批次 16：dialog 范围定稿——只做打开/保存（统一异步 + save_bytes 数据驱动）

### 设计背景（用户决策）

- **TLS 定稿不做**（net 明文；Web wss:// 自带）
- **消息框移除**：message/confirm/Level/Buttons/Confirmation 全部撤下
- **打开 + 保存必须完全跨平台**：原 save_file（选路径，桌面专属语义）在
  Web/移动端无法成立（Web 无路径、移动端 save_data 是数据驱动）——
  保存 API 改为**数据驱动**的 `save_bytes(file_name, data)`

### 设计方案

- 公开 API 收敛为三个 async fn：`pick_file` / `pick_files` / `save_bytes`
- `save_bytes(file_name, data)`：桌面 = 保存对话框 + 写入选路径
  （`Ok(Some(路径))`）；Web = Blob + `<a download>` 触发下载（`Ok(None)`，
  无路径概念）；移动端 = robius `save_data` 用户选位置写数据（`Ok(None)`）。
  用户取消三平台统一 `Ok(None)`
- 移除项：message/confirm/Level/Buttons/Confirmation（消息框整体）、
  save_file（被 save_bytes 取代）
- net：TLS 行为定稿写入模块说明（明文；Web wss:// 例外）

### 测试状态

- 桌面/wasm/android(剔除 dialog)/darwin 全目标零 error；53 passed
- dialog 说明文档重写（`reference/dialog模块跨平台说明.md`）：
  打开/保存全平台矩阵、三类执行模型（桌面阻塞模拟 / Web 真异步 / 移动端
  阻塞包装）、与引擎同步 frame 的配合模式

## 批次 17：移除多选文件（pick_files）——API 收敛为打开单文件 + 保存

### 设计背景（用户决策）

多选无需求（robius 移动端本就无多选 API）——与其保留"桌面/Web 可用、
移动端 Unsupported"的残缺 API，不如收敛掉。

### 设计方案

- 移除 `pick_files` 公开 API 与三平台 imp 实现
- dialog 最终 API 面：`pick_file` + `save_bytes` 两个 async fn
- CLAUDE.md 特性表与 dialog 说明文档同步

### 测试状态

- Windows：53 passed；wasm / darwin 零 error

## 批次 18：移除 camera 模块（含 GPS 取消）——用户决策

### 设计背景

camera 半成品（mod.rs 状态机 + windows MF 设备源后端）完成后，用户复盘定稿：
**与硬件/系统强绑定的能力（摄像头/GPS），跨平台抽象层不如针对目标平台
直接调用 API**——维护 N 套后端的成本高于抽象收益，且各平台行为差异
（权限模型/生命周期/像素格式）本就无法被抽象完全抹平。GPS 同步取消。

### 设计方案

- 删除 `src/base/camera/`（mod.rs 状态机 + backend seam + windows MF 后端）
- `camera` feature 与 default 移除；`base/mod.rs` 声明移除
- 进度记录：摄像头/GPS 标注取消；后续设备能力（如有）直接针对目标平台调 API

### 保留的相邻成果

- `base/yuv.rs`（NV12→RGBA 共享转换层）保留——video 仍使用，且作为纯函数
  共享层定位正确

### 测试状态

- `cargo test --lib`：53 passed；`cargo check --lib` 零 error；wasm 默认编译通过

## 批次 19：多窗口与相关接口整体剔除——回归单窗口模型（用户决策）

### 设计背景（用户决策）

Ubuntu 实机验证结论：**多窗口在 Linux 上表现很差，且各平台支持度不一**
（winit 多窗口在 Wayland/X11 的质量为已知生态短板）。用户决策：削减维护
成本，**多窗口与相关接口整体剔除**，回归单窗口模型。

### 设计方案（app.rs 单窗口化）

- `Ctx`：`windows: Vec<Window>` + `pending_windows` → **`window: Window`**
  （唯一主窗）；移除 `create_window()` / `windows()` / `window_count()`
- `Application` trait：移除 `window_created` / `window_closed` 两个生命周期
  钩子（单窗口模型下 start/退出已覆盖其语义）；trait 收敛为
  **start / event / frame** 三回调
- `Adapter::window_event`：按窗路由 → **非主窗事件直接忽略**；
  `CloseRequested` → 直接置 `ctx.exit`（关闭 = 退出）
- `Adapter::about_to_wait`：移除动态窗口物化块（InitSlot::fill 随之移除）；
  `request_redraw` 收敛为单窗口
- `InitSlot` 保留：它是"Web 异步资源初始化"的跨平台设施（示例 11 使用），
  与多窗口无关
- 删除示例 12_multi_window；渲染层 `surface_from_context` 能力保留
  （进阶用法，文档注明非官方支持形态）

### 关键保证

- 引擎循环骨架不变：启动门（Web 0×0 竞态消除）、节流、事件翻译、
  状态表刷新全部原样
- `event(win, ...)` 签名保持（win 恒为主窗口）——示例与未来绑定层
  无需感知此重构
- 渲染层 `surface_from_context` 作为基础能力保留（进阶用法）

### 测试状态

- `cargo check --lib` / `--examples` 零 error；`cargo test --lib` 53 passed
- 多窗口 API 全库零残留（grep 验证：app.rs / render_entry.rs / README / CLAUDE）
