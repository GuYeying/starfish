# 📋 跨平台 Issue 提交指南

本文件列出需要提交到上游仓库的 issue，按优先级排列。
每个 issue 包含：目标仓库、标题、正文（可直接复制粘贴）。

---

## Issue 1 — glow：wasm32 依赖段未排除 emscripten，拉入无法解析的 wasm-bindgen 运行时

> **优先级**：🔴 高（阻塞 Web 路线；已被本项目 `vendor/glow` 补丁临时修复）
> **提交到**：https://github.com/grovesNL/glow/issues/new（可附一行 PR）
> **标签**：`bug` `wasm` `emscripten`
>
> **⚠️ 勘误（2026-09-08）**：本文档最初把根因归到 wgpu 本体——**有误**。
> wgpu 30.0.0 已正确门控（wasm-bindgen 系依赖在
> `cfg(all(target_arch = "wasm32", not(target_os = "emscripten")))` 下声明，
> emscripten 段只拉 wgpu-core + wgpu-hal）。真正的泄漏源有两个：
> **glow 0.17.0**（wgpu-hal gles 的 GL 加载库）与 **wgpu-types 30.0.0**
> （被 wgpu-hal 的 gles feature 无条件激活 `web`）。两者共同的病灶：
> 依赖声明用 `cfg(target_arch = "wasm32")` 门控，未排除 emscripten。

### 标题

```
wasm32 dependency gates don't exclude emscripten: wasm-bindgen runtime leaks into emscripten builds (unresolved __wbindgen_* symbols)
```

### 正文（复制以下全部内容）

```markdown
## Description

`glow 0.17.0`'s `[target.'cfg(target_arch = "wasm32")'.dependencies]`
sections (js-sys, slotmap, wasm-bindgen, web-sys) do not exclude the
emscripten target. `cfg(target_arch = "wasm32")` matches
`wasm32-unknown-emscripten` too, so when glow is used from a crate built
for emscripten (e.g. wgpu-hal's gles backend), the whole wasm-bindgen
runtime family is pulled into the dependency graph and compiled — even
though glow's own code correctly selects the native GL path on
emscripten and never references these crates.

At link time, emcc then sees unresolved `__wbindgen_*` symbols coming
from the wasm-bindgen runtime objects. Projects must pass
`-sERROR_ON_UNDEFINED_SYMBOLS=0` to tolerate them (the symbols are
dead code and get GC'd later), which is fragile and masks real
link errors.

The same pattern exists in `wgpu-types 30.0.0` (js-sys / web-sys declared
for all `cfg(target_arch = "wasm32")`; its `web` feature is activated
unconditionally by wgpu-hal's `gles` feature — which emscripten builds
require).

Note: `wgpu` itself (30.0.0) already gates correctly with
`cfg(all(target_arch = "wasm32", not(target_os = "emscripten")))` —
only glow (and wgpu-types) are affected.

## Environment

- glow: 0.17.0 (also wgpu-types: 30.0.0)
- consumer: wgpu-hal 30.0.0 (gles backend)
- target: `wasm32-unknown-emscripten`
- emsdk: 6.0.9 (emcc 6.0.9)
- rustc: 1.85+

## Steps to reproduce

1. Depend on glow (directly or via wgpu) and build for
   `wasm32-unknown-emscripten`
2. `cargo tree -i wasm-bindgen --target wasm32-unknown-emscripten`
   shows wasm-bindgen / js-sys / web-sys in the graph via glow
3. At emcc link time, unresolved `__wbindgen_*` symbols require
   `-sERROR_ON_UNDEFINED_SYMBOLS=0`

## Fix

Change the dependency gates in `Cargo.toml` from

```toml
[target.'cfg(target_arch = "wasm32")'.dependencies.js-sys]
```

to

```toml
[target.'cfg(all(target_arch = "wasm32", not(target_os = "emscripten")))'.dependencies.js-sys]
```

for js-sys / slotmap / wasm-bindgen / web-sys (all four are only used by
glow's `web_sys.rs` module, which is already compiled only for
`all(target_arch = "wasm32", not(target_os = "emscripten"))` — so this
matches the existing code-level gating exactly and changes nothing for
any platform).

## Impact

Any emscripten build that reaches glow through wgpu's gles backend
needs `-sERROR_ON_UNDEFINED_SYMBOLS=0` to link. After the gate fix, the
flag can be dropped and wasm-bindgen leaves the emscripten graph
entirely (verified in the starfish project: `cargo tree -i wasm-bindgen
--target wasm32-unknown-emscripten` becomes empty, and the emcc link
succeeds without the tolerance flag).
```

---

## Issue 1b — wgpu-types：同款门控漏洞（可与 Issue 1 一起提）

> **提交到**：https://github.com/gfx-rs/wgpu/issues/new
> **标签**：`bug` `wasm` `emscripten`

### 标题

```
wgpu-types: js-sys/web-sys dep gates don't exclude emscripten; wgpu-hal gles feature force-activates wgpu-types/web there
```

### 正文要点（复制以下全部内容）

```markdown
## Description

`wgpu-types 30.0.0` declares optional js-sys / web-sys dependencies under
`[target.'cfg(target_arch = "wasm32")'.dependencies]`, i.e. also on
emscripten. Meanwhile `wgpu-hal`'s `gles` feature unconditionally
activates `wgpu-types/web` — and the `gles` feature is required on
emscripten (it is the only working backend there). Result: on
`wasm32-unknown-emscripten`, js-sys / web-sys / wasm-bindgen are compiled
into the graph even though every consumer of these types is gated behind
the `webgl` cfg alias (which already excludes emscripten).

## Fix

Two coordinated changes make emscripten builds clean:

1. `wgpu-types/Cargo.toml`: gate the js-sys / web-sys sections with
   `cfg(all(target_arch = "wasm32", not(target_os = "emscripten")))`.
2. `wgpu-types/src/texture/external_image.rs`: extend the six
   `#[cfg(... feature = "web")]` gates with
   `not(target_os = "emscripten")` (consistent with the `webgl` alias
   consumers already use; no behavior change on any platform).

Verified in the starfish project (SDL3 + wgpu 30 gles on emscripten):
with both patches applied, `cargo tree -i wasm-bindgen --target
wasm32-unknown-emscripten` is empty and the link succeeds without
`-sERROR_ON_UNDEFINED_SYMBOLS=0`.
```

---

## Issue 2 — sdl3-rs：emscripten 目标编译类型错误

> **优先级**：🟡 中（vendor 补丁已临时修复，提 issue 推动上游正式修复）
> **提交到**：https://github.com/revmischa/sdl3-rs/issues（或 crates.io/crates/sdl3 页面标注的仓库）
> **标签**：`bug` `emscripten`

### 标题

```
[Emscripten] compile error: XlibWindowHandle::new(window as u64) — u64 does not fit c_ulong on 32-bit targets
```

### 正文（复制以下全部内容）

```markdown
## Description

`sdl3 v0.18.4` (and v0.20.0) fails to compile for
`wasm32-unknown-emscripten` due to a type mismatch in
`src/sdl3/raw_window_handle.rs:118`:

```
error[E0308]: mismatched types
   --> sdl3/src/sdl3/raw_window_handle.rs:118:56
    |
118 |     let handle = XlibWindowHandle::new(window as u64);
    |         ----------------------------- ^^^^^ expected `u32`, found `u64`
```

On Emscripten, `c_ulong` is 32-bit (`u32`), but the code casts to `u64`
before passing to `XlibWindowHandle::new()` which expects `c_ulong`.

## Fix

Change the cast to use `as _` (inferred from the parameter type):

```rust
let handle = XlibWindowHandle::new(window as _);
```

This compiles correctly on all platforms.

## Environment

- sdl3: 0.18.4 (also reproduced on 0.20.0)
- target: wasm32-unknown-emscripten
- emsdk: 6.0.9 (emcc 6.0.9)
```

---

## 提交后追踪

| Issue | 仓库 | 状态 | 对我们项目的影响 |
|---|---|---|---|
| glow emscripten 门控 | grovesNL/glow | 待提交（vendor/glow 已临时修复） | 链接配方砍掉 `-sERROR_ON_UNDEFINED_SYMBOLS=0` |
| wgpu-types emscripten 门控 | gfx-rs/wgpu | 待提交（vendor/wgpu-types 已临时修复） | 同上（与 glow 补丁配套） |
| sdl3 emscripten 类型错误 | sdl3-rs | 待提交（vendor/sdl3 已临时修复） | 解锁 sdl3 crate emscripten 编译 |

三个 issue 都被上游修复后，删除项目根目录的 `vendor/sdl3/`、`vendor/glow/`、
`vendor/wgpu-types/` 及 `Cargo.toml` 中对应的 `[patch.crates-io]` 条目即可恢复官方版本；
wgpu 侧还需把 `"webgpu"` 加回 features（若届时需要 wasm32-unknown-unknown 路线）。
