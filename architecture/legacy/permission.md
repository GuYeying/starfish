# permission 架构

> 枚举声明式权限申请：调用方只声明"需要什么能力"，平台细节（Android 权限
> 字符串、弹框时序）私有化。**隐式申请**是本设计的核心。

## 关键文件

| 文件 | 职责 |
|---|---|
| `permission.rs` | `Permission` 枚举（`Microphone` / `Internet`）+ `ensure(permission) -> bool` + android imp（JNI requestPermissions 受控阻塞） |

## 架构与数据流

```
应用显式:   permission::ensure(Permission::Microphone)   // 需要精确控制时序时
模块隐式:   需要权限的模块在自己的构造路径上自动调 ensure
              ├─ AudioRecorder::new_*  → 隐式申请 Microphone
              └─ TcpConn::connect / UdpSock::bind → 隐式声明 Internet
```

- **Android 两类权限**：dangerous（`Microphone`：清单 + 运行时申请双管齐下，
  `ensure` 阻塞等弹框结果）vs normal（`Internet`：清单声明即安装时授予，
  `ensure` 瞬时通过无弹框，纯语义声明）。清单由 xtask 的 APK 模板统一声明。
- **桌面/Web = 恒 true**：桌面无运行时权限模型；Web 是**委派语义**——授权
  发生在紧随其后的系统 API（如 getUserMedia），主动预申请 = 白白开一次
  设备流，恒 true 即正确实现而非缺失。
- 依赖位：jni / robius-android-env 为 **Android 非可选**（核心路径使用）。

## 公开 API 速览

`Permission::{Microphone, Internet}`；`ensure(permission) -> bool`。
Android 权限字符串经 `Permission::android_name()` 内部翻译——调用方
永不见 `"android.permission.RECORD_AUDIO"`。

## 平台差异收敛点

全平台同一签名 `ensure(Permission)`；android imp 段内 cfg。命名原则：按
**概念域**命名（permission，不是 os/platform——后者被用户否决过）。

## 设计纪律

- 新增需要权限的能力：权限申请**归本模块**，消费方模块只做隐式调用——
  权限是 OS 能力，不随功能模块走（音频场景曾因此跨域依赖 dialog，已纠）。
- 隐式申请必须让"零感知"成立：构造即申请，失败不 panic（返回 false 可查）。

## 测试锚点

probe_record `PERM PASS`（Android 实机弹框 → 授权链路）；probe_net 回归
`VERDICT PASS`（Internet 隐式声明零行为变化）。

## 深入入口

`doc/log/starfish_changelog_2026-09-19.md` 批次 6⑧（权限隐式化定案，
含 `base::os` 命名被否决的记录）。
