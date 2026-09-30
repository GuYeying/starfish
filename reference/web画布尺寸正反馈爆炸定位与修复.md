# Web 画布尺寸正反馈爆炸定位与修复（DevTools 停靠 / 窗口缩放）

> 2026-09-27。starfish-window web 后端的 canvas 尺寸正反馈循环：
> DevTools 停靠或缩放窗口连发 resize 事件，画布分辨率指数放大直至爆掉
> GPU 纹理上限，渲染全链失效。本文记录症状、机理、复现手法、修复与
> 注意事项。关联：changelog 2026-09-27 批次三；
> `web/resize_probe.html`（回归探针，保留）。

---

## 一、症状

- 平时渲染正常；**打开 DevTools（停靠改变视口）或缩放窗口后**渲染逐渐失效；
- 控制台刷屏：

```
Texture size ([Extent3D width:22981, height:17243, ...])
    exceeded maximum texture size ([8192, 8192, ...])
  - While validating [TextureDescriptor "depth_texture"]
[Invalid Texture "depth_texture"] is invalid due to a previous error
  - While calling [Invalid Texture].CreateView(...)
Could not create the swapchain texture.
[Invalid TextureView] is invalid ... While validating colorAttachments[0]
[Invalid CommandBuffer] is invalid ... While calling [Queue].Submit(...)
WebGPU: too many warnings, no more warnings will be reported
```

- 失效链：depth_texture 超限 → 其 view 无效 → swapchain 纹理无效 →
  每帧 clear pass 编码/提交全部报废 → **画面死掉**；
- 迷惑点：**应用帧循环还在正常跑**（`[hello] 帧 N` 持续增长）——
  "逻辑活着、渲染死了"，容易误判为 wgpu/WebGPU 后端 bug。

## 二、定位过程

### 2.1 数字特征指向"增长循环"

失效报错的两个尺寸 `22981×17243` 与 `70380×52806`：都是 4:3（与初始
800×600 同比例），后者约为前者的 3 倍——**同一机制在不同时刻的连续
放大结果**，不是一次性算错。

### 2.2 代码审读锁定反馈源（starfish-window web 后端）

canvas **没有 CSS 尺寸时，布局尺寸 = width/height 属性**。而
`sync_canvas_size` 只做"属性 = client 尺寸 × dpr"：

```
属性 800 ──决定布局──▶ client 800
   ▲                      │
   │                      ▼
属性 = client × dpr ──── 800×1.25 = 1000
   ▲                      │
   └── 布局跟随属性变 1000 ◀┘
→ 下一轮 client 1000 → 属性 1250 → …（每次 resize 事件 ×dpr）
```

- `sync_canvas_size` 读 `client_width()`（布局尺寸）乘 dpr 写回属性；
- 属性变化又改变布局 → 下一轮读数已放大 → **每次 resize 事件 ×dpr**；
- 触发源：停靠/收起 DevTools、拖窗口边缘、浏览器缩放（zoom 还会改
  dpr，步长突变）——全部连发 `window.resize`，正反馈被反复点燃。

### 2.3 探针复现（证据闭环）

`web/resize_probe.html`：每 300ms 派发一次 window resize 并上报
canvas 属性，headless 加 `--force-device-scale-factor=1.25`：

```
tick=1  canvas=1250x938      （= 800×1.25）
tick=2  canvas=1563x1173     （×1.25）
...
tick=14 canvas=22763x17073   （14 轮即用户所见量级）
```

与线上报错数字同源同机制，定性完成。

## 三、修复（starfish-window/src/platform/web/mod.rs）

### 3.1 `sync_canvas_size` 钉住内联 CSS 尺寸（核心）

写属性前，先把 `style.width/height` 钉成当前 CSS 像素：

```rust
let rect = canvas.get_bounding_client_rect();
let css_w = rect.width().round().max(1.0);
if 内联样式未定宽高 { style.set_property("width", &format!("{css_w}px")); }
let width = ((css_w * dpr).round() as u32).max(1);
canvas.set_width(width);   // 属性 = 物理
```

钉住后布局与属性**解耦**：属性只管渲染密度（物理像素），样式只管
布局尺寸（CSS 像素）——这是 canvas HiDPI 的标准形态，循环从结构上
消除。**仅当内联样式未定宽高时才钉**：自建画布的 `100vw/100vh` 等
响应式样式不覆盖，保留页面布局语义（响应式样式本身已与属性解耦）。

### 3.2 resize 监听去重

`sync_canvas_size` 返回属性是否真的变化；未变不再上报 `Resized`
（停靠 DevTools 常触发同尺寸 resize），避免应用侧无效 surface 重建
（重建 = 重新 configure + 重建 depth/MSAA 纹理，并不便宜）。

### 3.3 `set_size` 双重缩放修复（同源顺带）

原实现"写属性后走 sync（再乘一次 dpr）"= 物理参数被二次缩放。
现按物理像素语义直接写两份：**属性 = 物理像素，内联样式 = 物理/dpr**。

## 四、验证

| 项 | 修复前 | 修复后 |
|---|---|---|
| 探针 12+ 次 resize（dpr=1.25） | ×1.25 指数增长至 22763×17073 | **恒定 1000×750**（800×600 CSS × 1.25） |
| 纹理超限/Invalid 报错 | 逐级报废 | 零报错 |
| 渲染截图 | 死 | 纯绿（probe 页 + 常规页双验证） |
| dpr=1.5 常规页回归 | — | 纯绿零报错 |

复现/回归命令：

```bash
# 起服务后（web/ 目录）：
python -m http.server 8881 --directory web
# 复现（修复前应见 canvas 尺寸指数增长）：
msedge --headless=new --enable-logging=stderr \
  --force-device-scale-factor=1.25 --virtual-time-budget=8000 \
  http://localhost:8881/resize_probe.html
# 回归（修复后 canvas 应恒定 1000x750，截图纯绿）
```

## 五、注意事项

1. **渲染失效后"too many warnings"会静默**：WebGPU 对同一 GPUDevice
   的告警有上限，之后不再报错——看到告警停了**不是恢复**，判断渲染
   是否活着要靠帧日志 + 画面，不靠告警流。
2. **canvas HiDPI 契约**（starfish-window web 后端的既定语义，页面
   编写者需知）：`canvas.width/height` 属性 = 物理像素（渲染分辨率），
   内联 `style.width/height` = CSS 像素（布局尺寸），二者由引擎维护
   同步；页面给 canvas 写内联尺寸即接管布局（引擎不覆盖），写 CSS
   类样式（非内联）的响应式布局仍会被首次同步钉住——需要页面级响应
   式画布时用内联样式（如 `100vw`）。
3. **`Resized` 事件的语义**：只在物理分辨率真变时上报；web 上来源是
   `window.resize` 事件 + 尺寸比对。局限：纯布局变化（如兄弟元素
   撑改画布盒）不触发 window resize，v1 不接 ResizeObserver——需要
   时再立项。
4. **`set_size` 是物理像素语义**（与桌面/Android 约定一致）；web 侧
   换算成 CSS 像素写样式，调用方不做 dpr 换算。
5. **headless 复现手法**：本 bug 是事件驱动而非时间驱动，虚拟时间
   模式（`--virtual-time-budget`）可复用；关键是
   `--force-device-scale-factor=1.25`——默认 dpr=1 时反馈环稳定
   （×1），**dpr=1 的环境复现不了此类 bug**，HiDPI 问题一律带
   scale-factor 跑。
6. **修复位于 starfish-window crate**（独立 crate，路径依赖）：改动
   对所有下游生效，无版本联动成本，但 starfish-rewrite 重建 wasm 产物
   后修复才落地。
