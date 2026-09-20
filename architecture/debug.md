# debug 架构

> 开发者诊断设施：跨平台日志、wasm 崩溃转发、主线程契约守卫。后续调试
> 工具（帧率统计、GPU 标签、性能打点）统一落此模块。

## 关键文件

| 文件 | 职责 |
|---|---|
| `debug/mod.rs` | 统一入口：`console_log` / `assert_main_thread` + 线程契约说明 |
| `debug/web.rs` | wasm 实现（console.log / panic hook） |
| `debug/native.rs` | 原生实现（stdout；panic 天然走 stderr） |

## 架构与数据流

```
console_log(msg)      → 桌面 stdout / wasm console.log   # 无头测试唯一判读通道
install_panic_hook()  → wasm 崩溃转发浏览器控制台(app_entry! 自动调)
mark_main_thread()    → pub(crate),inner_run 唯一调用点(OnceLock 幂等)
assert_main_thread(site) → debug 构建立即 panic 指明现场;release 零成本
```

## 公开 API 速览

`debug::console_log(&str)`；`debug::assert_main_thread(site: &str)`；
`install_panic_hook`（仅 wasm 语义，宏内部消费）。

## 平台差异收敛点

**统一只统一"两边都有的部分"（日志）**；平台专属能力保持平台门控
（install_panic_hook 仅 wasm）——统一入口 + 平台 uneven 的能力面。
文件夹化（按平台分文件）对齐 video 模式，底层调试**不做强行归一**。

## 设计纪律

- **console_log 是探针判读的契约通道**：格式 `[probe] TAG PASS|SKIP|FAIL
  detail`——wasm 上 `println!` 无处可去，一切诊断输出必须走这里。
- 线程契约：仅主线程 = run/窗口/事件/present；跨线程误用 debug 构建
  立即 panic（`assert_main_thread`），release 零成本。
- 吸收史：web.rs（诊断命名遗留）与 rt.rs（线程契约）并入本模块——
  新调试设施进这里，不再另立模块。

## 测试锚点

无直接 lib 测试（设施层）；效力体现在全部探针的无头收割链路。

## 深入入口

`doc/log/starfish_changelog_2026-09-19.md` 批次 6⑦（web.rs + rt.rs 归一并
文件夹化，用户定案"严格不做强行归一"）。
