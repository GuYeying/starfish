# starfish 更新日志 2026-09-30

> 今日主线 = pygame-rs Web 音频闭环 + 绑定扩容四连（批次 I–L）+ 控制台
> 噪音清理。三端（桌面 / Web / Android APK）全绿，binding_probe 运行时
> 自测 10/10。starfish 与 pygame-rs 两侧改动同录本文件。

## 批次二十二：Web 音频闭环——AudioContext 捕获修复 + 手势门定约

### 背景

Web 音频验收阶段发现三件事：① 用户在 starfish `device.rs` 手加的
AudioContext 捕获调用 `stream.audio_context()` 在 wasm32 编译失败
（E0599）；② Web 上"AudioContext 未建先调 play"会因混音器惰性创建落进
suspended 上下文，形成"静音卡死"态（play 正常返回、get_busy 恒 True、
永不发声、占用通道）；③ pygame-rs `bridge::pump()` 的手势解锁在第一帧
即被消费（不看手势、当时混音器未建，解锁空操作后永久失效）。

### 落地（starfish 侧）

- **`base/audio/device.rs`**：cpal 0.18 平台类型实为动态分发包装
  （`Stream(StreamInner)` 枚举），`audio_context()` 只在具体
  `webaudio::Stream` 上——经 `stream.as_inner()` 解包
  `StreamInner::WebAudio(s)` 捕获（该方法的返回是 `&AudioContext`
  而非 Result；仅非 atomics 路径提供，atomics = audioworklet 后端届时另议）。
  `WEB_AUDIO_CTX` 捕获功能自此真正可用。
- `web_resume_audio()` 语义不变（幂等 resume），调用时机由 pygame-rs
  手势门驱动（见下）。

### 落地（pygame-rs 侧）

- **`pygame.mixer.needs_gesture()`**（cfg 编译期判定，Web=True）：平台
  差异经绑定面显式暴露，Web 的"延后到手势触发"模式不再强加给
  桌面/Android（原生直接创建播放，demo 平台分支）。
- **手势前 play 跳过定约**：`Sound.play` / `music.play` 经
  `bridge::audio_gesture_locked()` 门控（挡在 with_mixer 之前——不建
  混音器、不建 AudioContext、不占通道；不排队、静默丢弃；debug 构建
  打一条日志）。替代"静音卡死"。
- **`GESTURE_SEEN` 手势门**：pump() 真正检查手势事件
  （KeyDown/MouseButtonDown/Touch）才置位；首个手势时
  `web_resume_audio()` 兜底恢复"显式 init 提前建出的 suspended ctx"。
- demo：`snd.play(-1)`（无限循环，引擎音频回调内回卷）+ mousedown
  触发（Web 与 keydown 同为 User Activation）。
- CLAUDE.md mixer 段定约同步。

### 验证

| 路径 | 结果 |
|---|---|
| wasm32 starfish/pygame-rs 编译 | ✅（桌面编译本就通过，wasm32 才暴露） |
| Web 按键/点击后提示音无缝循环 | ✅（Chromium sticky activation + cpal play 内部 resume） |
| 桌面/Android 即时出声（无手势概念） | ✅ |

## 批次二十三：绑定扩容四连（pygame-rs 批次 I–L）

### 背景

绑定层盘点（对照 `pygame绑定层API设计稿.md` 里程碑 + `pygame兼容层
模块架构.md` Tier 表）：Tier 0–2 与 sndarray 已齐，"引擎底层已有、只差
Python 面"的低垂果实集中四项，用户全选按序推进。

### 落地（pygame-rs 批次 I：pygame.touch）

- `pygame.touch.get_count/get_fingers/get_pos`（批次十三手指状态表的
  Python 面）。定约差异：pygame 官方"设备→手指"两级编号不适配多点，
  本层按**手指 id** 单层组织；桌面恒空表。

### 落地（starfish 批次 J 同捆：gamepad facade + pygame-rs 批次 J）

- **`pygame/gamepad.rs`（新）**：`base::gamepad::GamepadState` 的
  thread_local 持有 + 查询面（poll/connected/primary/is_connected/
  is_pressed/just_pressed/axis）；poll 由宿主泵每帧排水（gilrs/Web
  轮询型后端，无窗口事件流；just_pressed 是帧间边沿，一帧恰好一次）。
- **`base/gamepad.rs`**：`BUTTON_ORDER`/`AXIS_ORDER` 标准布局索引表
  （Web Gamepad API / SDL 惯例）+ `from_index`（越界 None）；
  `poll_web` 复用公共表。
- **`pygame.gamepad` 绑定**：get_count/connected/primary/is_connected/
  get_pressed/just_pressed/get_axis；常量 = **整数标准布局索引**
  （pygame.controller 同惯例；Web 同序）。typed 实例方案因嵌套模块
  pyclass 宏不展开而改道（见附注）。
- **gilrs TLS 析构崩溃修**：进程退出期 thread_local 槽析构 drop gilrs
  会 fail-fast（STATUS_STACK_BUFFER_OVERRUN）——`GamepadState::drop`
  故意泄漏 gilrs 句柄（进程级资源，同 mixer_quit 教训）。
- 新增单测 `empty_table_queries_are_safe`。

### 落地（pygame-rs 批次 K：math 3D）

- `Vector3`（dot/cross/length/normalize/lerp/distance_to/
  scale_to_length 就地）、`Quaternion`（length/normalize/dot/slerp）、
  `Mat4`（mul_mat4/transform_point3）pyclass + 模块函数
  `quat_identity/quat_from_axis_angle/quat_from_euler/mat4_identity/
  mat4_from_quat/mat4_translation/mat4_scale`（度数制入口）。
- pygame-rs 加 `glam = "0.33"` 直绑（与 starfish 同 semver 合并实例）。
- 数值语义验证：右手系叉积、90° 轴角四元数（z/w=√2/2）、平移×缩放
  复合变换点，sig_probe 全过。

### 落地（批次 L：draw/transform 补全）

- **`pygame.draw.ellipse/arc`**：starfish 引擎本就有（arc 弧度制，
  v1 描边恒 1px）——Screen 会话 + SurfaceSession + bridge 契约面
  （sf_draw_ellipse/arc，进 sig_probe 探针面）三路补齐。
- **`transform.smoothscale`**：零引擎改动——采样器恒线性为本层定约
  （transform.rs：smoothscale 与 scale 等同，nearest 变体位 v2 预留），
  绑定别名 + 差异标注。
- **`Surface.set_alpha/get_alpha`（starfish + 绑定）**：Surface 加
  表面级 alpha 字段（Cell<u8>，默认 255）；三处 DrawTarget::blit
  顶点色改走 `blit_tint()`（白 × alpha——着色器纹理×顶点色按分量乘，
  pygame blit 语义）。差异：真 pygame 按源像素 alpha 位协作，本层恒
  全局乘。
- binding_probe：椭圆 / 弧线 / 半透明平滑缩放贴图上屏。

### 验证

| 路径 | 结果 |
|---|---|
| sig_probe（契约回归） | ✅ 98/98（touch 空表语义、gamepad 类型/越界、math 3D 数值、ellipse/arc 绑定） |
| starfish 单测 | ✅ gamepad 空表查询 + 存量 29 项音频测试无回归 |
| 桌面真跑 binding_probe | ✅ 自测 10/10 + 持续运行 + 每帧数学驱动绘制 |
| Web / Android APK 打包 | ✅（wasm 49MB / APK 29MB，lib_name 自检 ✓） |

### 附注：嵌套 #[pymodule] 宏实测限制

嵌套模块内的 `#[pyclass(no_attr)]` struct 宏不展开（PyClassImpl 不
生成）；用户版 `module_exec` 需 `pub(crate)`（外层宏跨模块调用）、
函数由外层 `child.__init_methods` 自动补挂。gamepad 常量因此改整数
方案（与 pygame.controller 惯例一致，Web 标准布局天然整数索引）——
typed 实例形态如需恢复，走父作用域定义 + 子模块 set_attr。

## 批次二十四：控制台噪音清理 + 运行时自测段

### 背景

周期性打点（frame#N / session_begin/end 每 60 帧 / wgpu 管线
Debug dump 157 行 / adapter info）淹没模块 debug 输出；绑定扩容后
需要打包运行时的可观测验收。

### 落地

- **starfish**：`render_pipeline/builder.rs` 管线 dump 两条 println、
  `render_entry.rs` adapter info + supported formats println 删除。
- **pygame-rs**：`bridge::pump` DIAG 收缩（begin/end 计数删除，仅存
  sf_draw_rect 首调门）；宿主壳 frame#N 打点删除。
- **binding_probe 运行时自测段**：帧 1 执行一次，`[selftest]` 行三端
  同源（桌面终端 / Web F12 / Android logcat·bp_log.txt）——touch 空
  表语义、gamepad 常量与无硬件安全语义、math 3D 数值（右手系叉积 /
  90° 轴角 / 复合矩阵）、离屏 ellipse/arc 绘制、smoothscale+set_alpha
  回读，10 项 + 汇总行。
- **math 驱动视觉自证**：四元数→Mat4→transform_point3 旋转指针 +
  Vector3.lerp 脉冲圆（图元承载，零新建 GPU 表面——ADR-5 延迟提交
  红线；数学产物 float，绘制坐标收 int——SfPoint 契约严格拒 float）。

### 验证

| 路径 | 结果 |
|---|---|
| 桌面真跑（timeout 20s 截停法） | ✅ 自测 10/10、零异常、汇总行后每帧输出 0 |
| Web / APK 终版打包 | ✅ |

### 下一步

剩余缺口（盘点定案未排批）：mask/surfarray/PixelArray/BufferProxy
（受像素回读桥约束）、sprite（纯 Python）、cursors、display flags
翻译、dialog/io/net、面 B（starfish 直通模块）、.pyi 桩体系、M5
launcher；行为回归集（旧仓库 M2 的 71 例迁移）。

## 附带补全：Android 系统 CJK 字体候选链（真机验收通过）

批次 G 的 CJK 预载在 Android 缺源（Windows 走 msyh.ttc、Web 走 fetch
cjk.ttf，Android 只有相对路径 fonts/cjk.ttf——设备上不存在 → 中文缺字
回退拉丁）。补全：binding_probe.rs Android 分支加 `/system/fonts` 系统
字体候选链（NotoSansCJK/NotoSansSC/NotoSerifCJK/HarmonyOS_Sans_SC/
miui/DroidSansFallback 七候选，命中即停），经 starfish io 绝对路径
直读 std::fs（与 Windows 系统字体方案同构，零打包体积增加）。
真机验收 ✅（中文行正常上屏；命中字体见启动日志 CJK 预载行）。

## 批次二十五：pygame.dialog / pygame.io / pygame.net 绑定（Tier 3 直映射）

### 背景

盘点缺口三模块（契约 Tier 3："base 已有，直接映射"）。核心设计问题 =
base 的 async API 与 Python 同步面的落差：dialog/io 原生实现本就是
"阻塞实现 + async 体外壳"（await 处即阻塞处），net 则是全同步轮询式。

### 落地（pygame-rs；starfish 零改动）

- **pygame.io**：read/read_text/write/exists/set_base_dir——原生
  pollster block_on（基准目录语义全保留：Android = 应用私有目录自动
  注入，绝对路径原样）；Web v1 不提供同步面（fetch 真异步无法承载，
  调用即报错——宿主预加载/资源句柄模式承接，同 Font("cjk") 先例）。
- **pygame.net**：`TCP(addr)` 消息分帧连接（4B 大端长度前缀、单帧
  16MB 上限；connect 立即返回后台握手，Connecting 期间 send 入队缓冲
  ——send/recv/state/close 轮询面，原生后台线程 / Web 自动 WebSocket
  三平台统一）；`UDP(local)` 数据报（send_to/recv_from/set_broadcast，
  原生专属，Web 显式报错）。
- **pygame.dialog**（桌面专属）：`open_file(title, filters)` 模态
  （pollster block_on——rfd 阻塞实现体 await 处即阻塞处，语义等价；
  rfd 内置模态消息循环窗口保持响应）+ `save_bytes(file_name, data)`
  + `PickedFile.name()/read()`。filters 的 `&'static` 标记生命周期
  经 Box::leak 承接（一次性用户动作，次数有界）。Web/Android v1
  报错标注（Android 轮询式 job API 为后续批次）。
- pygame-rs 加 `pollster = "0.4"` 直绑。
- 宏坑追加：FromArgs 对嵌套 `Vec<tuple>` 不通（RP 无 Vec/tuple
  TryFromObject）——filters 改 PyObjectRef + downcast 元组 as_slice
  逐元素提取；payload 非 Debug 时手写 Debug impl（PyTcp/PyUdp/
  PyPickedFile）。

### 验证

| 路径 | 结果 |
|---|---|
| sig_probe | ✅ 110/110（io 读写往返/str 编码/exists、TCP 非阻塞构造与轮询、越界/边界报错、dialog 函数面） |
| wasm32 编译 | ✅（三模块 cfg 边界全过） |
| 三平台打包 | ✅ |

### 下一步

向量算术协议（Vector `+ - * /`）、display flags 翻译、mask 绑定、
dialog 轮询式 job API（Android SAF）、剩余缺口同批次二十四清单。

## 批次二十六：向量算术协议（pygame-rs 批次 N——Vector2/3 的 `+ - * /`）

### 背景

pygame Vector 的灵魂体验 = 算术运算符。RP pymethod 宏**禁止** dunder
数字方法（`#[pymethod] fn __add__` 直接报错指路 `impl AsNumber`）——
正确路径 = `impl AsNumber`（静态 `PyNumberMethods` 表 + 闭包算子）+
`#[pyclass(with(..., AsNumber))]`。

### 落地（pygame-rs 批次 N）

- **Vector2/Vector3**：`+` `-`（仅向量——pygame 语义：向量不与标量
  加减，标量返回 NotImplemented → TypeError）、`*` `/`（标量缩放或
  向量元素级——glam 语义，差异标注：真 pygame 乘法仅标量）、取负、
  零除 → ZeroDivisionError、`+=` `-=` 就地变更（恒等保持——
  inplace_add/inplace_subtract 槽，Cell 变异返回自身）。
- **标量×向量对称分派**：PyNumberMethods **无 right_\* 反射字段**——
  RP 反射乘法经"尝试 b 的槽"以同参数序到达，arith_mul 必须双侧处理
  （实测 `2 * v` 先 TypeError 后修正）。
- 语义标注：乘除元素级为 glam 语义（真 pygame 乘法仅标量）。

### 验证

| 路径 | 结果 |
|---|---|
| sig_probe | ✅ 122/122（新增 12 项：add/sub/标量乘 int 兼收/rmul/元素级乘/truediv/neg/标量加 TypeError/零除/iadd 就地/Vector3 抽样） |
| 三平台打包 | ✅ |

### 下一步

display flags 翻译、mask 绑定、cursors、dialog 轮询式 job API
（Android SAF）、剩余缺口同批次二十四清单。

## 批次二十七 + 二十八：display flags 落地 + Quaternion/Mat4 算术（pygame-rs 批次 O/P）

### 批次二十七（flags 落地；starfish + 绑定）

set_mode 的 flags 从"收下不解析"转正——**starfish `pygame/display.rs`
新增 `apply_window_flags(flags: u32)`**：locals::display 位集 →
Window::set_* 方法族运行时生效（RESIZABLE→set_resizable、
FULLSCREEN→set_fullscreen、NOFRAME→set_borderless、HIDDEN→
set_visible（运行时垫底语义）、ALWAYS_ON_TOP→set_always_on_top；
OPENGL 垫底 console_log——引擎恒 wgpu）。绑定侧 set_mode 收
int 位掩码或 [位,...] 序列（非法项 TypeError）后转发。常量面补
ALWAYS_ON_TOP 暴露。真身窗口由宿主壳先行装配——创建期一次性旗标
（透明）无法追溯，垫底收下（契约 §六既定）。

### 批次二十八（quatmat 算术；纯绑定）

Quaternion/Mat4 补 AsNumber：`*`（quat×quat 组合 / mat×mat 组合 /
标量缩放，全对称分派——同批次 N 无 right_\* 槽的反射结论）+ 取负。
`transform_point3` 无 w 除（glam 语义，与 project_point3 区分——
sig_probe 首版断言期望值据此修正）。

### 验证

| 路径 | 结果 |
|---|---|
| sig_probe | ✅ 132/132（flags 位集/序列/非法项/垫底、quat 组合 90°×2=180°/标量乘/取负、mat 组合/取负） |
| 三平台打包 | ✅ |

### 下一步

mask 绑定、dialog 轮询式 job API（Android SAF）、剩余缺口同批次二十四
清单（cursors 已剪除——用户定案：光标图标为桌面专属概念，不跨平台；
窗口级 set_cursor_visible 仍保留于 base）。

**剪除记录**：`cursors` 模块不做（2026-09-30 用户定案，同 scrap/camera
列）——移动端无光标、Web CSS 光标价值/成本比低；对齐契约文档
"剪除"清单更新。

## 批次三十：pygame.gfxdraw SDL_gfx 兼容签名面

### 背景

用户提案：绘制当前是 `with x.render() as f: f.circle(...)` 会话方法
形态，补一条 `pygame.gfxdraw.circle(f, x, y, r, color)` 的 SDL_gfx
风格兼容签名——两者性能原理完全一致（契约原则 3"一个实现多个兼容
名"：gfxdraw 独立栅格与 AA 语义已并入 draw/wgpu 采样，本层只是
签名适配）。

### 落地（pygame-rs；starfish 零改动）

- **`pygame.gfxdraw`**：pixel/hline/vline/line/aaline/circle/aacircle/
  filled_circle/ellipse/aaellipse/filled_ellipse/arc/trigon/aatrigon/
  filled_trigon/polygon/aapolygon/filled_polygon/rectangle/box/
  rounded_rectangle/rounded_box 22 函数。签名惯例 = SDL_gfx：目标
  会话在前、坐标分离、颜色在后（与 draw 族颜色在前互补）。
- **目标路由**：SurfaceSession 直绘（自有 batch）/ Screen 会话走
  with_session 活跃会话（须在 with 块内调用）；非会话目标 TypeError。
  `bridge::with_session` 转正 pub(crate)。
- **语义映射**：aa* = 兼容别名（AA 由 wgpu 线性采样承载，同实现）；
  circle/ellipse = 1px 描边、filled_* = 填充（width 1/0 同源）；
  arc **度数制**（SDL_gfx 惯例，引擎弧度制在此转换）；rounded_*
  垫底降级直角（圆角管线 v2）；trigon 族 = 三点 polygon。
- **实现注记**：trigon 9 参超 RP 原生函数实现上限（8）——坐标收进
  FromArgs 结构体（按声明序扁平展开，Python 签名不变）；FromArgs 对
  嵌套 Vec<tuple> 不通（同批次 M），gfxdraw 无嵌套参数不受影响。

### 验证

| 路径 | 结果 |
|---|---|
| sig_probe | ✅ 145/145（gfxdraw 函数面注册 + 非法目标 TypeError 路由） |
| binding_probe 自测段 | ✅ gfxdraw.circle/box 离屏会话绘制（selftest 11/11） |
| 活体上屏 | ✅ demo 会话内 gfxdraw.circle/hline/filled_circle/box 与会话方法混绘 |
| 三平台打包 | ✅ |

### 下一步

mask 绑定、dialog 轮询式 job API（Android SAF）、面 B（starfish
直通模块）、.pyi 桩体系、M5 launcher、行为回归集（旧仓库 M2 的
71 例迁移）。

## 批次三十二：flags 补齐——set_mode 尺寸/旗标全权威

### 背景

用户指出 flags 未闭环：批次 O 只做了旗标运行时生效，**尺寸不跟随**
（真身窗口由宿主壳先行装配 800×600，脚本 set_mode 尺寸被忽略——
真 pygame set_mode 会重设窗口大小）。

### 落地

- **starfish `pygame/display.rs`**：`apply_window_flags` 升级为
  `apply_window_request(size, flags)`——set_size 跟随 + 旗标
  set_* 族运行时生效（Resized 事件 → handle_resized 自愈链既有）。
- **绑定 set_mode**：尺寸（负值钳 0）+ 旗标位集/序列一并转发。
- font_test 改 `set_mode((1000, 700), RESIZABLE)` 作活体验证
  （桌面真跑：窗口 1000×700 可调，11/11 断言无回归）。

### 验证

| 路径 | 结果 |
|---|---|
| 桌面真跑 | ✅ set_mode (1000,700)+RESIZABLE 零异常，180s 持续运行，11/11 断言 |
| 三平台打包 | ✅ |

## 批次三十一：逐模块独立测试阶段启动（共享宿主 testhost + font_test 首例）

### 背景

用户决策：对每个 pygame 模块建独立详细测试案例（保留 binding_probe/
sig_probe 不动）。为避免 ~15 个案例各自拷贝 200 行宿主壳，装配/
预载/事件泵/帧循环/收口固化进共享宿主。

### 落地

- **`pygame-rs/src/rpy/testhost.rs`**（新）：`pub async fn run(script,
  tag)`——set_mode(800×600) + 铺底 → CJK 三平台预载链（与
  binding_probe 一字不差）→ RP 装配（install 在 enter 内）→ 脚本
  编译/执行/生成器 → 主循环（pump → 帧步进 → audio_pump → next_frame）
  → 关窗先于卸载 + mixer 显式收口；安卓 TCP 收集器/文件兜底日志同构。
- **font_test 首例**（examples/font_test.rs + .py，双注册 + build_web
  REGISTRY 窗口类条目）：11 项断言（构造族/度量/render 一等化/CJK/
  路径字体/SysFont shim/坏字节与缺参负路径）+ 字体样张上屏。
- **差异发现**：Font 构造**双参定约**（source, size 均必填）——
  `Font(b"坏字节")` 单参在参数层 TypeError，坏字节 ValueError 需带
  size（真 pygame Font(path) 可省 size——差异标注）。路径字体测试
  语义 = 开发者自填路径（系统字体不支持为既定定约）：Web SKIP
  （无文件系统）、安卓无候选路径时 SKIP 非回归、桌面真测。
- **文字旋转/缩放/半透明**（transform 全族吃一等化 Surface）：
  rotate 90° 宽高互换/smoothscale ×2/rotozoom 复合/set_alpha 断言
  全过 + 样张三行。**GL 平台坑（安卓实测）**：transform 的**源表面**
  也必须保活——临时源纹理销毁后 GL ID 立即复用，已提交未执行的绘制
  采到"新纹理"串内容（SMOOTHSCALE 行显示了旋转图；ADR-5 红线同族，
  桌面 wgpu 有生命周期托管不复现）。引擎侧 wgpu-GL 提交期资源保活
  为独立待办。

### 验证

| 路径 | 结果 |
|---|---|
| 桌面真跑（timeout + PYTHONUNBUFFERED） | ✅ 11/11 PASS（构造/度量/坏字节/CJK/路径字体/render 一等化），持续运行零异常 |
| Web / Android APK 打包 | ✅（REGISTRY 新增 font_test 窗口类条目） |

## 批次三十三：后端声明旗标 + 窗口透明/透明度（桌宠形态 v1）

### 背景

用户需求：① 后端旗标 OPENGL/VULKAN/DIRECTX/METAL/OPENGLES——失败让
wgpu 自己隐式退化；② 窗口透明（桌宠/悬浮件形态）+ 透明度/透明像素
接口。底座全部现成：`WindowFlags::TRANSPARENT`（创建位）、
`SurfaceSettings.alpha_mode`（合成模式）、`begin_frame(clear_color,
clear_depth)`（清屏可配）、`Window::set_opacity`（整体透明度）、
`GpuSettings.with_backends`（后端位集）。

### 落地（starfish）

- **locals**：TRANSPARENT(1<<7，直通 WindowFlags)/VULKAN/DIRECTX/
  METAL/OPENGLES(1<<8..11，GpuSettings 声明位) 入 KNOWN_MASK。
- **display::set_mode_ex**：TRANSPARENT → 建窗创建位 +
  `SurfaceSettings.with_alpha_mode(Auto)` + Screen 透明底模式；后端
  位 → GpuSettings.backends 位集（**OPENGL/OPENGLES = wgpu GL**，
  旧"显式报错"垫底表废除）；声明后端不可用 → **隐式退化**回全后端
  重试一次（用户定案：失败让 wgpu 自己退化）。
- **`pygame/display.rs`**：apply_window_flags →
  **apply_window_request(size, flags)**——尺寸跟随 + 旗标全权威
  （脚本 set_mode 的尺寸不再被忽略）；新增 `set_opacity(f32)`。
- **Screen**：transparent_bg 位（begin_frame 清屏改 alpha 0）+
  set_transparent_bg。

### 落地（pygame-rs）

- 绑定常量：TRANSPARENT/VULKAN/DIRECTX/METAL/OPENGLES 暴露。
- **`pygame.display.set_opacity(v)`** 绑定（0.0 透明 .. 1.0）。
- **testhost::run 第三参 transparent**：透明创建 + 跳过铺底（不透明
  底色会盖掉透明合成——透明窗脚本自绘内容即窗体，桌宠形态 v1 通道）。

### 语义边界（定约标注）

- 后端声明在**首次 set_mode 时**给出（GPU 进程一次，运行期不可重选）；
  绑定面脚本期声明 = 垫底记录（GPU 已由宿主装配）。
- 透明 = **创建期**（宿主声明），运行期不可追溯——绑定面 TRANSPARENT
  位对已建窗口为垫底记录。
- 桌宠渲染约定：透明窗内**勿填不透明底色**（fill alpha 0 或只画主体）。

### 验证

| 路径 | 结果 |
|---|---|
| 桌面真跑 font_test（不透明回归） | ✅ 15/15 PASS，持续运行零异常 |
| wasm32 / 三平台打包 | ✅ |

### 下一步

桌宠样例（透明窗 + gfxdraw 主体绘制 + set_opacity 呼吸）作为透明链
活体验收；mask 绑定、dialog 轮询式 job API（Android SAF）、面 B、
.pyi 桩体系、M5 launcher、行为回归集。

## 批次三十五：透明链点亮（真机验收通过）+ 透明相关接口整理

### 突破与修复（续批次三十三/三十四）

透明未生效的三段式根因全部击破：
1. **变量遮蔽 bug**（批次三十三引入）：set_mode_ex 内 `gpu_settings`
   被二次 `let` 遮蔽——后端声明位与透明呈现设置（DxgiFromVisual）从未
   到达建窗，实配恒 DxgiFromHwnd+Backends::all → wgpu 挑了 Vulkan →
   Vulkan Win32 表面只报 Opaque → 回退不透明；
2. **透明 = DX12 专属**：Vulkan Win32 表面 alpha_modes 只报 Opaque
   （与建窗样式无关）——TRANSPARENT 时强制 `Backends::DX12` +
   `DxgiFromVisual` + 表面 PreMultiplied 三件套；
3. **demo 测试面纱干扰**：全屏半透明白色 fill 是半透明混合测试件
   （有意为之）——桌宠形态改为小面积局部面板 + 主体不透明
   （body.set_alpha(255)）。

### 真机验收（AMD 780M / Vulkan+DX12 双后端机器）

| 项 | 结果 |
|---|---|
| caps alpha_modes | ✅ [Auto, Inherit, Opaque, PostMultiplied, **PreMultiplied**]（Dx12 后端） |
| 桌宠形态 | ✅ 窗口矩形不可见（背景全透出桌面）、主体实心、AA 文字边缘平滑无描边（非 colorkey，Qt 级） |
| set_opacity 呼吸 | ✅ 桌宠本体外的局部面板/整体透明度运行时可控 |

### 透明相关接口整理（桌面桌宠形态完整面）

| 接口 | 语义 |
|---|---|
| `pygame.TRANSPARENT` | 创建旗标（宿主声明，透明 = DX12 + DComp 逐像素） |
| `pygame.DIRECTX` | 后端声明（透明场景与 TRANSPARENT 同用强制 DX12） |
| `pygame.display.set_opacity(v)` | 窗口整体不透明度 0..1（运行时） |
| `Surface.set_alpha/get_alpha` | 表面级 alpha（主体/局部的半透明） |
| testhost `run(script, tag, transparent)` | 透明创建宿主通道 |
| 渲染定约 | 透明窗内**勿铺不透明底色**；主体实心、周围全透；AA 边缘平滑（预乘正确） |

### 下一步

桌宠/悬浮件样例深化（拖拽、贴边、点击穿透）；mask 绑定、dialog
轮询式 job API（Android SAF）、面 B、.pyi 桩体系、M5 launcher、
行为回归集。

## 批次三十六：透明窗口支持矩阵定案（平台 cfg 门控）

### 定案（用户拍板）

透明窗口支持范围 = **桌面 Windows 专属**（已真机验收）；Linux/macOS
"尽可能"经核查收敛——starfish-window 的 Linux/macOS 后端本身是
UnsupportedPlatform stub（整窗都未实现），透明随平台后端落地；
Android/iOS/WASM 排除（Vulkan/移动表面无逐像素、Canvas 无透明语义）。

### 落地

- **starfish `pygame/display.rs`**：透明分支（DxgiFromVisual +
  PreMultiplied + 强制 DX12 后端）加 `#[cfg(target_os = "windows")]`
  平台门控——其他平台不携带 DX12 专属语义；透明支持矩阵入注释。
- 透明窗口支持矩阵：**Windows ✓**（DX12+DComp，已验收）；
  Linux/macOS = 随窗口后端实现（后端现为 stub）；Android/iOS/WASM =
  排除（用户定案）。

### 下一步

桌宠深化（拖拽/贴边/点击穿透）；mask 绑定、dialog 轮询式 job API
（Android SAF）、面 B、.pyi 桩体系、M5 launcher、行为回归集。

## 批次三十七：Web 深度/颜色附件脱节修复（draw_test Web 实测）

### 根因

draw_test Web 端 BeginRenderPass 校验失败：depth(1260×945=建窗时
canvas 尺寸) 与颜色(800×600=set_mode 请求尺寸) 不匹配。链路断点 =
**web `set_size` 直接写 canvas 属性但不发 Resized 事件**——wgpu Web
表面自动跟随 canvas 属性，引擎 Screen（swapchain+深度）停在旧尺寸，
应用侧 `handle_resized` 永不触发 → 深度纹理不重建。Windows 侧 set_size
经 SetWindowPos→WM_SIZE→wndproc 自动推 Resized，web 缺这一环
（resize 监听只在浏览器窗口缩放时推，脚本 set_mode 的 set_size 不经过）。

### 修复

starfish-window `web::set_size` 写属性后补发 `Event::Resized`
（event 特性门控；物理尺寸为准）——引擎 resize 链（handle_resized →
Screen::resize → swapchain+深度重建）恢复闭环。

### 验证

| 路径 | 结果 |
|---|---|
| wasm32 编译 | ✅ 零错误 |
| Web 打包 | ✅（draw_test 重打包，浏览器刷新验收深度/颜色匹配） |

### 追加修正（同日）

首版修复（set_size 补发 Resized(参数)）仍脱节：深度 1400×1050 vs
颜色 800×600。根因 = **Web 高 DPI 尺寸语义冲突**——set_size 把参数当
物理像素直写 canvas 属性（800），但 canvas 的 CSS 布局（client）仍
800 CSS px，resize 监听按 client×dpr=1400 重建深度，而 wgpu swapchain
跟随 canvas 属性（800）→ 两条路径对"窗口尺寸"定义不一致。
修正：set_size 语义改**逻辑尺寸**（canvas 属性 = 逻辑 × dpr），与
sync_canvas_size 完全同源——swapchain 与深度统一为 canvas 物理属性，
脱节消除（dpr≠1 设备此前必现，dpr=1 时恰好一致故未暴露）。

## 批次三十四：透明链实测——缺失链接露 + 优雅降级（transparent_test 首跑）

### 实测结论（桌面 Windows）

transparent_test 首跑暴露透明链的**真实断点**：
1. **starfish-window 的 `WindowFlags::TRANSPARENT` 只定义未消费**——
   Windows 建窗未启用透明合成（自研窗口层非 winit，需要 Win32 侧
   DWM enable-behind / WS_EX_NOREDIRECTIONBITMAP 工作）→ 表面能力
   仅 `[Opaque]`；
2. 请求 PreMultiplied → wgpu configure **panic**（无协商）。

### 修复（优雅降级）

- **`base/render/settings.rs`**：alpha_mode 走"许愿 → 掩码"同款回退
  （与 present_mode 先例一致）——请求模式不在表面能力集 → 回落首个
  支持模式 + console_log 标注（"窗口透明需要建窗侧透明合成支持，
  否则整窗不透明"）。configure panic 消除。
- transparent_test：`import math` 撞冻结 stdlib 缺口
  （ModuleNotFoundError）——三角波纯算术替代；**math 薄 shim**（设计
  文档"按需自补"预案）列为待办。

### 验证

| 路径 | 结果 |
|---|---|
| 桌面真跑 transparent_test | ✅ 无 panic、自测 3/3、呼吸/面板/桌宠本体渲染正常（窗口当前 Opaque——降级语义如实） |
| alpha 回退日志 | ✅ `[render] alpha mode PreMultiplied 不受表面支持，回落 Opaque` |

### 待办（透明链真正点亮）

1. **starfish-window Windows 透明建窗**（Win32：DWM
   enable-behind/WS_EX_NOREDIRECTIONBITMAP + 表面能力上报
   PreMultiplied）——链路最后断点；
2. math 薄 shim（add_native_module 按需自补，设计文档预案）；
3. 点亮后 transparent_test 即成桌宠活体验收（肉眼：AA 边缘无 fill 色
   描边 = Qt 级）。

### 实现进展（同日追加）

- **starfish-window Windows 建窗已消费 TRANSPARENT 位**：CreateWindowExW
  加 `WS_EX_NOREDIRECTIONBITMAP`（DWM 直通合成；纯 wgpu 渲染无 GDI
  依赖不受影响）；set_opacity 对此类窗口跳过 LAYERED 路径（分层与
  直通合成冲突）。
- **实测**：建窗/交换链/渲染全正常（无 panic），但 wgpu-hal 对 HWND
  目标的 alpha 能力为**静态上报 [Opaque]**（不看窗口样式）→ 仍走
  Opaque 回退。**肉眼验收未定**：DXGI 对无重定向位图窗口的未指定
  alpha 可能按预乘语义合成（winit+wgpu 生态透明窗的通行做法）——
  跑 transparent_test 看桌面是否透出即知。若仍不透明 → 逐像素路线
  在 wgpu-30 d3d12 HWND 表面被锁死，桌面宠物改走 UpdateLayeredWindow
  分层窗专线（用户已确认桌宠形态诉求；SDL colorkey 描边缺陷在该路
  线不存在）。

### 下一步

逐模块案例推进（次序待定：draw → event → key+mouse → mixer →
transform → image → touch → gamepad → io/net/dialog → gfxdraw）；
每案例完成后即同步 REGISTRY 与本文档。

## 批次二十九：pygame.mouse / pygame.key 完整度补全

### 背景

cursors 剪除后对 mouse/key 两模块做完成度审计（对照真 pygame 面）：
mouse 缺 get_rel/set_visible/get_visible/get_focused/set_pos；key 缺
get_mods（+KMOD_* 常量）/name/get_focused/set_repeat/get_repeat。
引擎侧底子部分现成（窗口层 KeyDown 载荷本就带 `Modifiers`、Focus
事件存在——pygame 层翻译时丢弃未接）。

### 落地

- **starfish `pygame/key.rs`**：MODS 位集（KeyDown/KeyUp 事件的
  Modifiers 喂入）+ KMOD_SHIFT/CTRL/ALT/META 位常量（位值自定：1/2/
  4/8）+ get_mods/get_focused（FocusGained/Lost 喂 FOCUSED 态）+
  name（W3C KeyboardEvent.code 命名——Debug 即代码名）+ set_repeat/
  get_repeat 垫底（自动重复由 OS repeat 标记承载，无软件合成）。
- **starfish `pygame/event.rs`**：KeyDown/KeyUp 翻译顺带喂 mods；
  FocusGained/Lost 翻译接聚焦态。
- **starfish `pygame/mouse.rs`**：get_rel（LAST_READ 基准位）+
  set_visible/get_visible（窗口 set_cursor_visible + 本地态）+
  get_focused（与键盘聚焦同源简化）+ set_pos 转正 pub（垫底：仅
  状态表生效——starfish-window v1 无光标扭曲，无合成事件）。
- **绑定面**：mouse +5（get_rel/set_visible/get_visible/get_focused/
  set_pos）、key +5（get_mods/name/get_focused/set_repeat/get_repeat）
  + KMOD_* 常量暴露。

### 验证

| 路径 | 结果 |
|---|---|
| sig_probe | ✅ 143/143（mouse 补全 6 项、key 补全 6 项、存量无回归） |
| 三平台打包 | ✅ |
