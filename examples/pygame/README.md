# pygame 模块运行测试案例（examples/pygame/）

pygame 兼容层**全模块运行自检**套件（批次二十五起、批次二十六全模块覆盖）：
每模块一个可运行案例，桌面 / Web / Android 三端同一份源码（`_android`
双注册出包真机验收）。

## 运行方式

```bash
# 桌面（逐个跑，各 ~10s 自动退出；窗口 × / ESC 可提前退）
cargo run --example pygame_test_rect            # 案例名见下表

# Web（产物已生成：web/pygame_test_<m>.html；索引页 web/pygame_test_index.html）
cargo build --release --target wasm32-unknown-unknown --example pygame_test_rect
wasm-bindgen --out-dir web --target web target/wasm32-unknown-unknown/release/examples/pygame_test_rect.wasm
python -m http.server 8000 --directory web   # → http://localhost:8000/pygame_test_index.html

# Android（统一打包脚本，产物 target/android-apk/<名>_android.apk）
python pygame-rs/scripts/build_apk.py pygame_test_rect --dir .
```

## 判读约定

- **顶部结果横幅**：绿 = 该模块自检全过 / 红 = 有败；
- **横幅下色块**：每格一项自检（顺序 = 案例头注释所列），绿 = 过 / 红 = 败；
- **诊断**：桌面终端 / Android `adb logcat -s RustStdoutStderr` /
  Web F12 console（`common::log` 三端可见，每案例退出前打印汇总
  `[模块] N 帧完成，自检 x/y`）。

## 案例索引（25 案例）

### 核心套件（批次二十五定约的 9 案例）

| 案例 | 自检项（状态条顺序） |
|---|---|
| `pygame_test_rect` | new / center / move_ / inflate / collidepoint / colliderect / contains / clamp |
| `pygame_test_color` | from_name / new+rgb / from_tuple / normalize / lerp / grayscale / invert / premul_alpha / hsva |
| `pygame_test_time` | Clock::new / delay(0) / tick 毫秒 / get_fps / raw_delta（后三项第 30 帧晚登记） |
| `pygame_test_draw` | rect 填充 / rect 描边 / circle / line / lines / ellipse / polygon / arc |
| `pygame_test_surface` | new 尺寸 / new 无深度 / with_depth / render_depth 会话 / blit 回环 |
| `pygame_test_mask` | set_at/get_at / count / invert / scale / overlap / overlap_area / overlap_mask / to_surface / from_surface |
| `pygame_test_display` | get_init / 尺寸一致 / caption+opacity / flip 稳定 120 帧（晚登记） |
| `pygame_test_event` | 泵可用 / 240 帧泵稳定（晚登记）；交互计数进 console |
| `pygame_test_io` | 错误路径×2 + 文本回环 + 二进制回环（原生；Web 仅错误路径） |

### 扩展案例（批次二十六；纯逻辑族）

| 案例 | 自检项 |
|---|---|
| `pygame_test_version` | VERSION 非空 / vernum 一致 / get_sdl_version 兼容值 |
| `pygame_test_locals` | K_a / K_ESCAPE / K_UP / K_RETURN 映射 / KMOD 位独立 / BUTTON_LEFT / KNOWN_MASK |
| `pygame_test_math` | lerp / clamp / inv_lerp / remap / smoothstep 单调 / smootherstep 端点 |
| `pygame_test_key` | get_pressed 面 / name 映射 / get_mods / get_repeat 垫底 / get_focused |
| `pygame_test_mouse` | get_pos / get_pressed / set_pos 回环（原生）/ set_visible 回环 / get_focused |
| `pygame_test_transform` | scale 尺寸 / rotate 90 交换宽高 / flip_x / flip_y / rotozoom 放大 |
| `pygame_test_sprite` | insert/count / tag 过滤 / move_by/position / is_alive / remove / tick+draw |

### 扩展案例（批次二十六；资产/硬件族——无设备/资产环境安全）

| 案例 | 自检项 | 说明 |
|---|---|---|
| `pygame_test_font` | from_bytes / line_height / size / render / 双字号 | Antonio-Regular 内嵌，三平台零 IO |
| `pygame_test_image` | load_from_bytes / get_extended / 非法格式 Err / save→load 回环（原生） | 回环产物 `__pygame_test_img__.png` 留 base_dir |
| `pygame_test_sndarray` | mono 构造 / mono 回环 / stereo 回环 / 非法声道 Err | 纯 CPU 桥，无需音频设备 |
| `pygame_test_video` | VideoSystem::new / 缺失文件 Err / 非媒体扩展 Err | 真实播放需样本资产（见 probe_video） |
| `pygame_test_net` | 非法地址 Err / UDP bind / UDP 自环回包（原生） | Web 无 UDP，仅错误路径 |
| `pygame_test_gamepad` | poll / count 一致 / 未连接查询安全 / primary 一致 | 插手柄 = 灯阵+摇杆可玩 |
| `pygame_test_touch` | count 一致 / 无效 finger None / 连续查询稳定 | 桌面恒空表 |
| `pygame_test_dialog` | API 面链接 | **不弹系统对话框**（交互流程留人工验收） |
| `pygame_test_permission` | ensure(Internet) / ensure_async / 枚举 Debug | **绝不触碰 Microphone**（Android 阻塞弹框） |

## 共用框架（common.rs）

- `Checks`：自检登记（`require(name, ok)`，晚登记合法——时序类检查在
  指定帧登记）；
- `draw_status`：状态横幅 + 色块条（任何 DrawTarget 均可作载体）；
- `quit_requested`：事件泵 + Quit/ESC 判定（每帧一次；自带泵的案例
  如 event 自行排空）；
- `paced_frame`：跨平台节流（原生 sleep 16ms / Web rAF 帧拍）；
- `reside`：Web 驻留（应用 future 不得完成）；原生空实现 → 自动退出；
- `EXIT_FRAMES = 600`：自动退出帧数（60fps ≈ 10s）。

## 惯例与已知坑（实战沉淀）

- **循环四步缺一不可**：fill → draw → `display::flip()` → 节流帧拍
  （无 flip = 无 present = 白屏/黑屏，批次二十五根因定案）；
- `screen.render()` 返回会话，`f.fill/with_batch/blit/size` 是
  `DrawTarget` trait 方法——**调用文件需 `use starfish::pygame::render::DrawTarget;`**；
- `get_screen()` 返回引用——传 `draw_status(screen, …)` 本身即可，
  `&screen` 变 `&&T` 不匹配；
- `#[path = "common.rs"]` 相对源文件目录（兄弟文件名）；
- 闭包参数名勿与外层业务变量重名（with_batch 闭包实测遮蔽坑）。
