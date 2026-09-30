//! 运行时权限申请（全平台统一入口）
//!
//! 定位：**权限是独立的平台能力域**——需要权限的模块（如音频的录音、
//! 将来的相机）在自己的构造/初始化路径上**隐式调用** [`ensure`]，应用
//! 侧零感知零 cfg；需要精确控制授权时序的应用也可显式调用。
//!
//! 调用方只声明"需要什么能力"（[`Permission`] 枚举），平台差异全部由
//! 本模块内部消化：
//! - Android：翻译为权限字符串，`requestPermissions` 系统弹授权框，
//!   **受控阻塞轮询**至用户操作或 ~15s 超时（NativeActivity 无 Java 结果
//!   回调可收）。须在窗口/事件循环就绪后调用（系统对话框需叠加显示）。
//! - 桌面：无运行时权限模型，恒返回 true。
//! - Web：无前置权限 API——授权发生在紧随其后的设备 API 首次使用
//!   （如 `getUserMedia`），本函数恒返回 true（**委派语义**：主动预申请
//!   意味着白白开一次设备流，浏览器的"首次使用时授权"即其权限模型）。
//!
//! Android 权限的两类区分（决定 `ensure` 的行为）：
//! - **dangerous**（如 `Microphone`）：清单声明 + 运行时申请双管齐下——
//!   未授权时 `ensure` 阻塞弹框等待
//! - **normal**（如 `Internet`）：清单声明即安装时自动授予——`ensure`
//!   首次检查即通过，无弹框无等待（清单由 xtask 的 APK 模板统一声明）
//!
//! JNI 依赖（jni / robius-android-env）为 Android 非可选依赖（对齐
//! ndk-context 的批次 17 先例：核心路径使用 → 非可选）。

/// 可申请的权限集合（跨平台）
///
/// 枚举值是调用方与平台细节之间的翻译层：这里声明"需要什么能力"，
/// Android 权限字符串 / Web 浏览器授权时机 / 桌面无模型等差异由模块消化。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Permission {
    /// 麦克风（录音）——Android dangerous 权限：运行时申请弹框
    Microphone,
    /// 网络（TCP/UDP socket）——Android normal 权限：清单声明即授予，
    /// `ensure` 瞬时通过（语义声明用途，见模块文档的两类区分）
    Internet,
}

impl Permission {
    /// Android 权限字符串（仅 android 后端消费）
    #[cfg(target_os = "android")]
    fn android_name(self) -> &'static str {
        match self {
            Permission::Microphone => "android.permission.RECORD_AUDIO",
            Permission::Internet => "android.permission.INTERNET",
        }
    }
}

/// 运行时权限申请（全平台签名；返回 true = 已授权）
///
/// 平台语义见模块文档。**隐式调用约定**：需要权限的模块在自己的构造/
/// 初始化路径上调用本函数（如 AudioRecorder 构造时申请麦克风）——调用
/// 方无需感知；已授权时立即返回，重复调用无害。
pub fn ensure(permission: Permission) -> bool {
    #[cfg(target_os = "android")]
    return android::ensure(permission.android_name());
    #[cfg(not(target_os = "android"))]
    {
        let _ = permission;
        true
    }
}

#[cfg(target_os = "android")]
mod android {
    use super::Permission;
    use jni::objects::{JObject, JString, JValue};

    /// 已授权检测（PERMISSION_GRANTED = 0）
    fn granted(
        env: &mut robius_android_env::JNIEnv,
        activity: &JObject,
        perm: &JString,
    ) -> bool {
        env.call_method(
            activity,
            "checkSelfPermission",
            "(Ljava/lang/String;)I",
            &[JValue::Object(perm)],
        )
        .and_then(|v| v.i())
        .map(|i| i == 0)
        .unwrap_or(false)
    }

    /// requestPermissions + 轮询授权状态（NativeActivity 无 Java 回调可收
    /// 结果；系统弹授权对话框，须在窗口/事件循环就绪后调用）
    pub fn ensure(android_name: &str) -> bool {
        let r = robius_android_env::with_activity(|env, activity| -> bool {
            let Ok(perm) = env.new_string(android_name) else {
                return false;
            };
            if granted(env, activity, &perm) {
                return true;
            }
            if let Ok(arr) = env.new_object_array(1, "java/lang/String", &perm) {
                let _ = env.call_method(
                    activity,
                    "requestPermissions",
                    "([Ljava/lang/String;I)V",
                    &[JValue::Object(&arr), JValue::Int(7001)],
                );
            }
            for _ in 0..150 {
                std::thread::sleep(std::time::Duration::from_millis(100));
                if granted(env, activity, &perm) {
                    return true;
                }
            }
            false
        });
        r.unwrap_or(false)
    }
}
