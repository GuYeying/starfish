# io 架构（feature = "io"）

> 统一异步文件读写：原生 std::fs 直实现、Web fetch 直实现，
> `set_base_dir` 全平台对称。**运行环境状态的唯一事实源**（Android 私有
> 目录由引擎 `run` 注入于此）。

## 关键文件

| 文件 | 职责 |
|---|---|
| `io.rs` | 五个异步函数 + `BASE_DIR` 三件套（native 段 / wasm 段各一份实现，统一签名） |

## 架构与数据流

```
read / read_text / write / write_text / exists(path)      # 统一 async API
  ├─ native: std::fs 直实现(阻塞包 async 壳)
  └─ wasm:   fetch 直实现 —— GET 读 / POST 保存,path 即 URL

set_base_dir(dir) 全平台对称:
  ├─ 原生 = 目录拼接(Android 由引擎 run 自动注入私有目录,桌面默认 CWD 可改)
  └─ web   = URL 前缀拼接(资源挂子路径 / 资产走 CDN 跨源前缀)
             `/` 开头 = 绝对语义不受影响;未设置 = 各平台默认(CWD / 浏览器相对)
base_dir() / clear_base_dir()                             # 只读查询 / 复位
```

- 文件管理操作（list_dir/删除等）v1 **不设**：Web fetch 无对应语义——
  原生项目直接用 std::fs。

## 公开 API 速览

`io::{read, write, exists, read_text, write_text}`（全 async）；
`io::set_base_dir(impl AsRef<Path>)` / `io::base_dir() -> Option<PathBuf>` /
`io::clear_base_dir()`；`IoError`。

## 平台差异收敛点

模块内 native/wasm 两段实现同一签名——调用方零 cfg。Android 的运行环境
注入时序：`run`（EventLoop 创建前）→ `io::set_base_dir`，`start()/frame()`
必然晚于注入。

## 设计纪律

**"应用的 io 沙箱根"只此一处**：其他模块需要应用数据目录时查
`io::base_dir()`（如 kit::asset_path 的 Android 分支），不得另立全局——
双全局存一个事实是已被剔除的设计（9-19 批次 6⑤）。

## 测试锚点

lib：base_dir 重定向测试。probe_io：`WRITE/EXISTS/READ/WRITE_TXT/READ_TXT
PASS → ALL PASS`（三平台；web 走 server.py 的 POST 端点）。

## 深入入口

`doc/log/starfish_changelog_2026-09-18.md`（io 立项）、09-19 批次 6③⑤
（Web set_base_dir 补齐 / 数据目录归 io）。
