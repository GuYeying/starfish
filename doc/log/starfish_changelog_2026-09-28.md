# starfish 更新日志 2026-09-28

## 批次十八：RP 嵌入三平台全通——window_test 桌面/Android/Web 同日齐跑

### 设计背景

批次十六~十七打通 RP 嵌入与生成器门后，window_test 一直是"桌面 ✅ /
Android 闪退（无日志）/ Web 待验证"的残局。本批次集中清剿 Android 闪退
与 Web 验证，达成 **RP 嵌入三平台（Windows / Android 真机 / Web）同时
跑通**：同一份 Rust 壳 + 同一份 Python 生成器脚本，三端逐帧驱动渲染。

### 根因与修复（Android 闪退）

- **真凶：`android.app.lib_name` 不匹配**。手动"替换 APK 内 .so"式重打包
  只换了库文件，manifest 元数据仍是旧示例名（rp_main_loop_android）→
  NativeActivity 按名 dlopen 失败秒崩——**应用代码从未执行过**，此前
  一切日志缺失/寿命差异（55ms~2s）全是该 crash 的表症噪音。
- 一锤定音手段：x86_64 包进**模拟器 + `adb logcat -b crash`**，直接取得
  `IllegalArgumentException: Unable to find native library ...`。
- **修复 = 正规打包链**：模板占位符替换（`__LIB_NAME__`/`__ORIENTATION__`
  =landscape）→ `aapt2 link` 重造 base（manifest+resources.arsc）→ 组装
  classes.dex + .so（ZIP_DEFLATED）→ zipalign → apksigner。
- 细节与证伪清单见 `reference/安卓APK打包lib_name坑与真机诊断手册.md`。

### 次要坑（本批次实测）

1. **启动睡眠 × 横屏旋转**：诊断性睡眠把 set_mode 推迟到启动 6s 后 →
   旋转（TerminateWindow/InitWindow）重建 ANativeWindow → 迟到的 set_mode
   拿旧 window 配 surface → `Invalid surface` 退出。**定约：启动后立即
   set_mode**，旋转交帧循环 `Resized → resize()` 消化。
2. **android-activity 0.6.1 胶水**：进程死亡后 <2s 急速重启 → "销毁中途
   重建" abort（2.8s 死亡即此）——已知上游限制，表现为"马上重开秒退"。
3. **Web 无头验证**：headless 截图黑屏 ≠ 失败——headless 将页面标记
   hidden 使 rAF 停摆；以 console 标记流为准（--enable-logging=stderr
   过滤 CONSOLE 行）。

### 诊断方法论（沉淀）

hilog `ANCO CREATE/DIED` 寿命 + **睡眠标记法**（零日志二分定位）、模拟器
logcat（pre-run 死亡一锤定音）、屏幕阶段色、TCP 直传 PC 收集器（需手机
PC 同网段）、卓易通沙盒 shell 不可读（应用日志走 /sdcard 或 TCP）。
详见 reference 手册 §四。

### 落地（代码面）

- `pygame-rs/examples/window_test.rs`：日志路径修正
  （/sdcard/Download/wt_log.txt）+ Python 原生名对齐扁平名（fill/rect/
  flip/quit_requested）+ 诊断脚手架（已拆除，保留 dlog 文件+TCP 上报）。
- `pygame-rs/src/rpy/sf.rs`：扁平名注册（批次十七遗留的命名统一收尾）。
- `devlog_server.py`：TCP 日志收集器（:9000 → devlog.txt，诊断辅助件）。

### 关键保证

- 同一份 `examples/window_test.rs` + 同一份内嵌 Python 脚本，三平台零
  分叉（平台差异全部压进 `app_entry!` 宏与 starfish-window 后端）。
- RP genesis（release）三平台全部通过；Python 脚本编译执行、生成器创建、
  逐帧 `__next__` 驱动渲染，链路每一环均有日志锚点。
- 依赖单向红线保持：pygame-rs → starfish 单向，starfish 零 Python 资产。

### 测试状态（批次十八）

| 平台 | 结果 |
|---|---|
| Windows 桌面（release） | ✅ 8s 存活冒烟 + 桌面运行 |
| Android 真机（nova 15 / 卓易通） | ✅ TCP 通道六标记全通：starting → set_mode OK → RP genesis 完成 → executing script → generator created → entering frame loop |
| Web（wasm32 / Edge） | ✅ console 六标记全通 + next_frame 帧拍运行（headless 截图黑系 hidden 停 rAF，非故障）；**批次十六"RP-on-wasm 闭包崩溃"未复现，销项** |
| 模拟器（x86_64，逻辑层） | ✅ 用于 crash 取证；GLES 死穴照旧（环境层，非回归） |

### 遗留（独立批次）

- rp_main_loop 真机黑屏挂起：旧 APK（lib_name 正确、进程确实启动）挂于
  genesis 附近；且当前源码 set_mode 调用缺失（批次十七日志与现文件不符，
  疑编辑丢失）。同套 genesis 在 window_test 中三平台顺利通过 → 非 genesis
  本身问题，待单独排查。
- xtask `--dir` 兄弟 crate 支持（pygame-rs 示例 APK 一键打包）。
- 批次日志补记：本日早前 window_test 源码修复（脚本名对齐/日志路径/
  dlog 报错显式化）并入本批次。

---

## 批次十九：M1 绑定面 v1——标准 pygame 写法三端跑通（shim + with 会话）

### 落地

- **`pygame_shim.py`**（新增，include 静态嵌入）：`types.ModuleType` 组装
  `pygame` 包 + `sys.modules` 注册（display/draw/event/time + 常量），
  `with screen.render():` = `sf_session_begin/__exit__→sf_session_end`
  （ADR-5 Python 绑定形态）。**可维护性 ADR**：加 API 优先改 Python 侧。
- **`sf.rs` 扩容**：`session_begin/end`（RenderPass<'static> 驻留
  thread-local——get_screen() 为 &'static）+ `draw_rect/circle/line/
  polygon`（路由活跃会话）+ `events`（tag 化 tuple 流 + pump 重构为
  待派发队列）+ `get_ticks/delay/dlog`。
- `base/time`：`since_start_ms()`（web 走 performance.now 桥）。
- `examples/binding_probe.{rs,py}`：标准 pygame 写法探针（生成器门壳）。

### RP 原生函数坑位（本批实测，sf.rs/shim 注释同步）

1. **7 元组上限**：第 8+ 参数 → PyNativeFnInternal 不满足（颜色收敛为
   单 tuple 参数）；
2. **tuple FromArgs = 平铺消费连续位置参数**（非嵌套 tuple！）——
   `color: (i32,i32,i32,i32)` 要在 Python 侧 `*rgba(color)` 散开传；
3. `Vec<(i32,i32)>` FromArgs/TryFromObject 均不支持 → 多边形扁平化
   `[x,y,x,y,...]` + 原生层 chunks 组对；
4. `types.ModuleType` 的常量必须 `setattr` 到模块对象（裸 global ≠
   模块属性，`pygame.QUIT` 才可见）。

### 诊断设施坑位（本批实测）

1. **TCP 无超时阻塞 connect 在主线程 = ANR**——connect_timeout(300ms)
   + 一次失败即拉黑（AtomicBool）；
2. **manifest 缺存储权限 → /sdcard 写入 EACCES**（此前 wt_log/bp_log
   全部静默失败的根因）——补 `WRITE/READ_EXTERNAL_STORAGE` +
   `requestLegacyExternalStorage`（卓易通按 legacy 授予，本批实证可写）；
3. `devlog` 通道定约：println + console_log + 文件 + TCP 四路，
   诊断计数器（session_begin/draw/session_end/frame）落
   `Download/bp_log.txt`。

### 测试状态（批次十九）

| 平台 | 结果 |
|---|---|
| Windows（Vulkan） | ✅ 截图实证：rect/circle/line/polygon 全渲染，frame ticks 递增 |
| Android 真机（卓易通） | ✅ 截图实证：四类图元全渲染（含 with 会话路径）；单形态版无异常 |
| Web（WebGL2） | ✅ 用户目视确认渲染；console 六标记全通 |
| 模拟器 | ✅ lib_name crash 取证（批次十八同款手段） |

### 定约修订（本批收尾，用户决策）

**绘制 API 收敛为单形态**：`with screen.render() as r: r.rect(...)`——
**移除** `pygame.draw.*(screen, …)` 兼容形态（shim 已重写、三端产物已
重建）。理由：双形态在 Surface/MRT 场景产生二义性（会话自有 batch，
传 screen 即错）；pygame 兼容保留在数据类型/事件/模块组织层面，绘制
入口统一走会话（架构 §九.1）。配套：`_Session` 补 fill/rect/circle/
line/polygon 五方法 + `Screen.render_depth` 占位（v1.1 随 Surface 批次）。

### 待观察 / 遗留

- **安卓首发图元缺失（偶发，未复现）**：某版构建首启只见清屏色不见图元，
  重装后消失。改动因果未实锤（权限/TCP 修复逻辑上不涉及图元）——主嫌疑
  为首发旋转竞态。诊断计数器已就位，复发时 bp_log.txt 直接留证。
- **pygame 层 `Resized` → `Screen::resize()` 接线缺失**（确定性改进）：
  桌面/Web 无旋转无感，Android 旋转重建表面后正交相机/交换链不跟随——
  下一批应修。
- rp_main_loop 挂起、xtask `--dir`：承批次十八遗留。

---

## 批次二十：`Resized` 自愈接线——渲染表面/正交相机跟随物理尺寸

### 背景（批次十九遗留债）

pygame 层事件泵对 `Resized` 事件无人处理：Android 首发横屏旋转重建
ANativeWindow 后，交换链与正交相机停留在旧尺寸——疑似"首发图元缺失"
偶发问题的根因（桌面/Web 无旋转无感，缺口一直潜伏）。

### 落地

- `pygame/display.rs`：新增 `pub(crate) handle_resized(w, h)`（get_init
  守卫 → `get_screen().resize(w, h)`——表面交换链重配置 + 正交相机跟随）；
- `pygame/event.rs`：`translate` 的 Resized 分支自动调用（ADR-7 同款
  "翻译时喂状态"模式，与 key::press/mouse::set_pos 并列）；
- **ADR-8 保持**：display 逻辑尺寸（set_mode 的 800×600 语义）不跟随，
  仅渲染表面重建。

### 验证

| 项 | 结果 |
|---|---|
| 单元回归 | ✅ `cargo test --lib pygame::` 39/39 |
| 桌面 resize 实测 | ✅ Win32 强改窗口 800×600 → 500×900：交换链铺满新尺寸、图元按新正交正确定位、帧流不断（截图 resize_test.png） |
| Android 真机 | ✅ 用户确认无异常（含此前首发图元缺失场景） |
| Web | ✅ 用户目视确认渲染 + console 六标记全通 |

**批次二十关账（2026-09-29）**：三端验证闭环，"首发图元缺失"偶发问题
随自愈接线终结（后续若复发，bp_log.txt 计数器留证）。

### 定约

事件翻译层的 Resized 分支 = 渲染表面自愈的唯一路径；应用代码无需
（也不应）自行处理表面重建。
