//! base/dialog —— 文件打开 / 保存（统一异步 API，平台行为一致化）
//!
//! 范围定稿（用户决策）：**只做打开与保存**，消息框不做；TLS 亦不做（net 明文）。
//!
//! **统一异步**：全部 API 以 `async fn` 暴露——
//! - 原生阻塞平台（桌面 rfd、移动端 robius 回调）：阻塞实现 + async 体外壳
//!   （"模拟异步"）——await 处即阻塞处，语义与原阻塞版一致
//! - 原生异步平台（Web `<input type=file>` + FileReader）：天然适配，
//!   await 挂起不挡帧（引擎帧循环继续跑）
//!
//! 返回值统一为 [`PickedFile`]（桌面/移动持路径、Web 选择即读入内存）——
//! 调用方只依赖 `name()` + `read()`，不感知平台差异。保存统一为
//! [`save_bytes`]（数据驱动：桌面写入选路径 / Web 触发下载 / 移动端
//! 用户选位置写数据）。
//!
//! 平台矩阵：
//! - Windows / Linux(GTK3) / macOS：rfd
//! - Android / iOS：robius-file-picker（自带 Java/Kotlin 胶水；
//!   **Android 构建需 `ANDROID_JAR` 环境变量**）
//! - Web：`<input type=file>` + FileReader（读）；保存 = 触发下载
//!   （浏览器语义是下载而非保存对话框）
//!
//! Linux 构建注意：rfd 后端为 GTK3，需 `libgtk-3-dev`（仅 dialog 特性时）。

use std::path::PathBuf;

/// 对话框错误（不可用平台显式报错，不做静默降级）
#[derive(Debug)]
pub enum DialogError {
    /// 当前平台不支持该对话框
    UnsupportedPlatform,
    /// 后端错误（含平台原生信息）
    Backend(String),
}

impl std::fmt::Display for DialogError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DialogError::UnsupportedPlatform => write!(f, "dialog: 该平台不支持此对话框"),
            DialogError::Backend(s) => write!(f, "dialog 后端错误: {s}"),
        }
    }
}
impl std::error::Error for DialogError {}

/// 文件过滤项（展示名 + 扩展名列表），如 `("文本文件", &["txt", "md"])`
///
/// Web 端映射为 `<input accept=".txt,.md">`；移动端 robius 内部消费。
pub type Filter = (&'static str, &'static [&'static str]);

/// 用户选取的文件（平台无关）
///
/// 桌面/移动：持路径，`read()` 惰性读盘；Web：选择时即读入内存
/// （浏览器模型），`read()` 返回内存副本。
pub struct PickedFile {
    /// 文件名（含扩展名）
    name: String,
    #[cfg(not(target_arch = "wasm32"))]
    path: PathBuf,
    #[cfg(target_arch = "wasm32")]
    data: Vec<u8>,
}

impl PickedFile {
    /// 文件名（含扩展名）
    pub fn name(&self) -> &str {
        &self.name
    }

    /// 读取全部字节：桌面/移动读盘；Web 返回选择时载入的数据
    pub fn read(&self) -> std::io::Result<Vec<u8>> {
        #[cfg(not(target_arch = "wasm32"))]
        return std::fs::read(&self.path);
        #[cfg(target_arch = "wasm32")]
        return Ok(self.data.clone());
    }
}

// ── 桌面：rfd 阻塞实现（在 async 体内直接执行 = 模拟异步）─────────

#[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
mod imp {
    use super::*;
    use rfd::FileDialog;

    fn apply(dialog: FileDialog, title: Option<&str>, filters: &[Filter]) -> FileDialog {
        let mut dialog = match title {
            Some(t) => dialog.set_title(t),
            None => dialog,
        };
        for (name, exts) in filters {
            dialog = dialog.add_filter(*name, exts);
        }
        dialog
    }

    fn to_picked(path: PathBuf) -> PickedFile {
        PickedFile {
            name: path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            path,
        }
    }

    // ── 轮询式任务（线程 + 通道：与 Android 端 Job API 对齐，UI 不被 rfd 阻塞）──

    pub(super) struct PickJob {
        rx: std::sync::mpsc::Receiver<Result<Option<PickedFile>, DialogError>>,
    }

    impl PickJob {
        /// 每帧轮询：Some = 用户已选择/取消/出错
        pub fn try_result(&mut self) -> Option<Result<Option<PickedFile>, DialogError>> {
            self.rx.try_recv().ok()
        }
    }

    pub(super) fn pick_file_start_impl(
        title: Option<&str>,
        filters: &[Filter],
    ) -> Result<PickJob, DialogError> {
        let (tx, rx) = std::sync::mpsc::channel();
        let title = title.map(String::from);
        let filters: Vec<(String, Vec<String>)> = filters
            .iter()
            .map(|(n, e)| ((*n).to_string(), e.iter().map(|s| (*s).to_string()).collect()))
            .collect();
        std::thread::spawn(move || {
            let mut dialog = FileDialog::new();
            if let Some(t) = &title {
                dialog = dialog.set_title(t);
            }
            for (n, exts) in &filters {
                let ext_refs: Vec<&str> = exts.iter().map(String::as_str).collect();
                dialog = dialog.add_filter(n, &ext_refs);
            }
            let picked = dialog.pick_file();
            let _ = tx.send(Ok(picked.map(to_picked)));
        });
        Ok(PickJob { rx })
    }

    pub(super) struct SaveJob {
        rx: std::sync::mpsc::Receiver<Result<Option<PathBuf>, DialogError>>,
    }

    impl SaveJob {
        /// 每帧轮询：Some = 已写入所选路径/取消/出错
        pub fn try_result(&mut self) -> Option<Result<Option<PathBuf>, DialogError>> {
            self.rx.try_recv().ok()
        }
    }

    pub(super) fn save_bytes_start_impl(
        file_name: &str,
        data: Vec<u8>,
    ) -> Result<SaveJob, DialogError> {
        let (tx, rx) = std::sync::mpsc::channel();
        let file_name = file_name.to_string();
        std::thread::spawn(move || {
            let mut dialog = FileDialog::new().set_file_name(&file_name);
            dialog = dialog.set_title("保存");
            let res = match dialog.save_file() {
                Some(path) => std::fs::write(&path, &data)
                    .map(|_| Some(path))
                    .map_err(|e| DialogError::Backend(format!("写入所选路径失败: {e}"))),
                None => Ok(None), // 用户取消
            };
            let _ = tx.send(res);
        });
        Ok(SaveJob { rx })
    }

    pub(super) async fn pick_file_impl(
        title: Option<&str>,
        filters: &[Filter],
    ) -> Result<Option<PickedFile>, DialogError> {
        let picked = apply(FileDialog::new(), title, filters).pick_file();
        Ok(picked.map(to_picked))
    }

    pub(super) async fn save_bytes_impl(
        file_name: &str,
        data: &[u8],
    ) -> Result<Option<PathBuf>, DialogError> {
        let mut dialog = FileDialog::new().set_file_name(file_name);
        dialog = dialog.set_title("保存");
        match dialog.save_file() {
            Some(path) => {
                std::fs::write(&path, data).map_err(|e| {
                    DialogError::Backend(format!("写入所选路径失败: {e}"))
                })?;
                Ok(Some(path))
            }
            None => Ok(None), // 用户取消
        }
    }
}

// ── Web：input[file]/FileReader（读）+ Blob 下载（写，浏览器语义）──

#[cfg(target_arch = "wasm32")]
mod imp {
    use super::*;
    use js_sys::Object;
    use wasm_bindgen::prelude::JsValue;
    use wasm_bindgen::prelude::Closure;
    use wasm_bindgen::JsCast;
    use wasm_bindgen_futures::spawn_local;

    /// 单线程 oneshot（wasm 无需 Send；驱动方 = spawn_local）
    pub(super) mod oneshot {
        use std::cell::RefCell;
        use std::future::Future;
        use std::pin::Pin;
        use std::rc::Rc;
        use std::task::{Context, Poll, Waker};

        struct Inner<T> {
            value: Option<T>,
            waker: Option<Waker>,
        }

        pub struct Sender<T> {
            inner: Rc<RefCell<Inner<T>>>,
        }

        // 手动实现：避免 derive 给 T 加多余的 Clone 约束（Rc 本身恒可克隆）
        impl<T> Clone for Sender<T> {
            fn clone(&self) -> Self {
                Self {
                    inner: self.inner.clone(),
                }
            }
        }

        impl<T> Sender<T> {
            pub fn send(self, value: T) {
                let mut inner = self.inner.borrow_mut();
                inner.value = Some(value);
                if let Some(w) = inner.waker.take() {
                    w.wake();
                }
            }
        }

        pub struct Receiver<T> {
            inner: Rc<RefCell<Inner<T>>>,
        }

        impl<T> Future for Receiver<T> {
            type Output = T;
            fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<T> {
                let mut inner = self.inner.borrow_mut();
                if let Some(v) = inner.value.take() {
                    return Poll::Ready(v);
                }
                inner.waker = Some(cx.waker().clone());
                Poll::Pending
            }
        }

        pub fn channel<T>() -> (Sender<T>, Receiver<T>) {
            let inner = Rc::new(RefCell::new(Inner {
                value: None,
                waker: None,
            }));
            (Sender { inner: inner.clone() }, Receiver { inner })
        }
    }

    fn document() -> Result<web_sys::Document, DialogError> {
        web_sys::window()
            .and_then(|w| w.document())
            .ok_or(DialogError::Backend("无 document".into()))
    }

    async fn read_file_bytes(file: &web_sys::File) -> Result<Vec<u8>, DialogError> {
        // web-sys 该绑定异常自动传播（无 Result 壳）
        let promise = file.array_buffer();
        let buf_val = wasm_bindgen_futures::JsFuture::from(promise)
            .await
            .map_err(|e| DialogError::Backend(format!("文件读取失败: {e:?}")))?;
        let buf: js_sys::ArrayBuffer = buf_val
            .dyn_into()
            .map_err(|_| DialogError::Backend("ArrayBuffer 类型异常".into()))?;
        Ok(js_sys::Uint8Array::new(&buf).to_vec())
    }

    /// 动态 `<input type=file>` + FileReader 读取（浏览器异步流程）
    fn open_picker(
        multiple: bool,
        filters: &[Filter],
    ) -> Result<oneshot::Receiver<Result<Option<Vec<PickedFile>>, DialogError>>, DialogError>
    {
        let document = document()?;
        let input: web_sys::HtmlInputElement = document
            .create_element("input")
            .map_err(|e| DialogError::Backend(format!("{e:?}")))?
            .dyn_into()
            .map_err(|_| DialogError::Backend("input 创建失败".into()))?;
        input.set_type("file");
        input.set_multiple(multiple);
        if !filters.is_empty() {
            let accept: Vec<String> = filters
                .iter()
                .flat_map(|(_, exts)| exts.iter().map(|e| format!(".{e}")))
                .collect();
            input.set_accept(&accept.join(","));
        }

        let (tx, rx) = oneshot::channel::<Result<Option<Vec<PickedFile>>, DialogError>>();

        // onchange：FileList → 逐文件读字节 → 一次性回传
        let on_change = {
            let input = input.clone();
            let tx = tx.clone();
            Closure::<dyn FnMut(JsValue)>::new(move |_e: JsValue| {
                let tx = tx.clone();
                let input = input.clone();
                spawn_local(async move {
                    let result = (|| async move {
                        let files = input
                            .files()
                            .ok_or_else(|| DialogError::Backend("files 缺失".into()))?;
                        let mut picked = Vec::new();
                        for i in 0..files.length() {
                            let file: web_sys::File = files
                                .get(i)
                                .ok_or_else(|| DialogError::Backend("文件项缺失".into()))?;
                            let data = read_file_bytes(&file).await?;
                            picked.push(PickedFile {
                                name: file.name(),
                                data,
                            });
                        }
                        if picked.is_empty() {
                            Ok(None) // 未选文件直接确认 = 取消
                        } else {
                            Ok(Some(picked))
                        }
                    })()
                    .await;
                    tx.send(result);
                });
            })
        };

        // 单发对话框：input 与回调随调用泄漏（量级可忽略，免去自引用清理）
        if let Err(e) = js_sys::Reflect::set(&input, &"onchange".into(), &on_change.as_ref()) {
            return Err(DialogError::Backend(format!("{e:?}")));
        }
        std::mem::forget(on_change);
        input.click();
        std::mem::forget(input);
        Ok(rx)
    }

    pub(super) async fn pick_file_impl(
        title: Option<&str>,
        filters: &[Filter],
    ) -> Result<Option<PickedFile>, DialogError> {
        let _ = title; // 浏览器文件选择器无自定义标题
        let mut rx = open_picker(false, filters)?;
        match rx.await {
            Ok(Some(mut files)) => Ok(files.pop()),
            Ok(None) => Ok(None),
            Err(e) => Err(e),
        }
    }

    pub(super) async fn save_bytes_impl(
        file_name: &str,
        data: &[u8],
    ) -> Result<Option<PathBuf>, DialogError> {
        // 浏览器语义：Blob → <a download> click → 触发下载（无路径概念）
        use wasm_bindgen::JsValue;

        let document = document()?;
        let arr = js_sys::Uint8Array::from(data);
        let parts = js_sys::Array::new();
        parts.push(&arr);
        let blob = web_sys::Blob::new_with_u8_array_sequence(&parts)
            .map_err(|e| DialogError::Backend(format!("Blob 构造失败: {e:?}")))?;
        let url = web_sys::Url::create_object_url_with_blob(&blob)
            .map_err(|e| DialogError::Backend(format!("objectURL 创建失败: {e:?}")))?;

        let anchor = document
            .create_element("a")
            .map_err(|e| DialogError::Backend(format!("{e:?}")))?;
        js_sys::Reflect::set(&anchor, &"href".into(), &url.clone().into())
            .map_err(|e| DialogError::Backend(format!("{e:?}")))?;
        js_sys::Reflect::set(&anchor, &"download".into(), &JsValue::from_str(file_name))
            .map_err(|e| DialogError::Backend(format!("{e:?}")))?;
        if let Ok(click) = js_sys::Reflect::get(&anchor, &"click".into()) {
            let _ = click.dyn_into::<js_sys::Function>().and_then(|f| f.call0(&anchor));
        }
        web_sys::Url::revoke_object_url(&url).map_err(|e| DialogError::Backend(format!("{e:?}")))?;
        Ok(None) // Web 下载无路径概念
    }
}

// ── Android / iOS：文件选择 + 保存 = robius-file-picker ──────────
// （自带 Java/Kotlin 胶水：Android DocumentPicker / iOS UIDocumentPicker，
//   content:// URI 经 into_local_file 落为临时本地路径——与桌面语义对齐；
//   保存 = save_data 用户选位置写数据。回调经 channel 收敛为阻塞。）

#[cfg(any(target_os = "android", target_os = "ios"))]
mod imp {
    use super::*;
    use robius_file_picker::FileDialog as RobiusDialog;
    use std::sync::{Arc, Mutex};

    // ── 轮询式任务（Android 推荐：发起后主线程保持事件循环，SAF 期间不卡生命周期）──

    pub(super) struct PickJob {
        slot: Arc<Mutex<Option<Result<Option<PickedFile>, DialogError>>>>,
    }

    impl PickJob {
        /// 每帧轮询：Some = 用户已选择/取消/出错
        pub fn try_result(&mut self) -> Option<Result<Option<PickedFile>, DialogError>> {
            self.slot.lock().unwrap().take()
        }
    }

    pub(super) fn pick_file_start_impl(
        title: Option<&str>,
        filters: &[Filter],
    ) -> Result<PickJob, DialogError> {
        // 类加载器自愈是 Android 容器专属（原生线程 FindClass 查不到应用类）；
        // iOS 无 Java 世界，直接跳过（iOS 支持编译验证所需，实机待验）
        #[cfg(target_os = "android")]
        ensure_classloader().map_err(DialogError::Backend)?;
        let slot = Arc::new(Mutex::new(None));
        let slot_cb = slot.clone();
        let mut dialog = RobiusDialog::new();
        if let Some(t) = title {
            dialog = dialog.set_title(t);
        }
        for (name, exts) in filters {
            dialog = dialog.add_filter(*name, exts);
        }
        // 回调在 Java 线程触发：填槽即完成（robius 负责回调指针生命周期）
        let r = dialog.pick_file(move |res| {
            let mapped = match res {
                Ok(Some(pf)) => to_picked(pf).map(Some),
                Ok(None) => Ok(None),
                Err(e) => Err(DialogError::Backend(format!("{e}"))),
            };
            *slot_cb.lock().unwrap() = Some(mapped);
        });
        r.map_err(|e| DialogError::Backend(format!("{e:?}")))?;
        Ok(PickJob { slot })
    }

    pub(super) struct SaveJob {
        slot: Arc<Mutex<Option<Result<Option<PathBuf>, DialogError>>>>,
    }

    impl SaveJob {
        /// 每帧轮询：Some = 用户已选位置写入/取消/出错（Android 语义 = Ok(None)）
        pub fn try_result(&mut self) -> Option<Result<Option<PathBuf>, DialogError>> {
            self.slot.lock().unwrap().take()
        }
    }

    pub(super) fn save_bytes_start_impl(
        file_name: &str,
        data: Vec<u8>,
    ) -> Result<SaveJob, DialogError> {
        #[cfg(target_os = "android")]
        ensure_classloader().map_err(DialogError::Backend)?;
        let slot = Arc::new(Mutex::new(None));
        let slot_cb = slot.clone();
        let mut dialog = RobiusDialog::new();
        dialog = dialog.set_file_name(file_name);
        // data 的所有权交给 robius（内部保活至用户选完位置写出）
        let r = dialog.save_data(data, move |res| {
            let mapped = match res {
                Ok(_) => Ok(None), // 用户选完位置即写入完成（无路径概念返回 None）
                Err(e) => Err(DialogError::Backend(format!("保存失败: {e}"))),
            };
            *slot_cb.lock().unwrap() = Some(mapped);
        });
        r.map_err(|e| DialogError::Backend(format!("{e:?}")))?;
        Ok(SaveJob { slot })
    }

    /// 回调式 robius → 阻塞（channel 收敛，与桌面 rfd 阻塞语义一致）
    fn block_with<T>(
        start: impl FnOnce(
            std::sync::mpsc::Sender<Result<T, DialogError>>,
        ) -> Result<(), DialogError>,
    ) -> Result<T, DialogError> {
        let (tx, rx) = std::sync::mpsc::channel();
        start(tx)?;
        rx.recv()
            .map_err(|e| DialogError::Backend(format!("对话框回调通道关闭: {e}")))?
    }

    fn to_picked(pf: robius_file_picker::PickedFile) -> Result<PickedFile, DialogError> {
        let name = pf.file_name().unwrap_or_default().to_string();
        let local = pf
            .into_local_file()
            .map_err(|e| DialogError::Backend(format!("content URI 落地失败: {e}")))?;
        Ok(PickedFile {
            name,
            path: local.path().to_path_buf(),
        })
    }

    /// 捕获当前线程残留的 pending Java 异常详情
    ///
    /// jni 的 check_exception 只查不清——robius 内部把异常映射为
    /// `Error::JavaException`（无详情）后异常仍挂在线程上，此处读出
    /// 完整异常类名 + message 供上屏诊断，读完 ExceptionClear 解锁线程。
    /// 诊断 + 自愈：原生线程的 FindClass 只查系统类加载器，查不到应用类——
    /// 显式经 Activity 类加载器加载 Fragment 类（同时验证 dex 是否真被加载），
    /// 并把该加载器设为当前线程上下文类加载器（robius 内部查找的回退依赖）
    #[cfg(target_os = "android")]
    fn ensure_classloader() -> Result<(), String> {
        use jni::objects::JValue;
        let inner = robius_android_env::with_activity(|env, activity| -> Result<(), String> {
            let loader = env
                .call_method(activity, "getClassLoader", "()Ljava/lang/ClassLoader;", &[])
                .and_then(|v| v.l())
                .map_err(|e| format!("STEP-A0: getClassLoader failed: {e:?}"))?;
            let name = env
                .new_string("robius.file_picker.FilePickerFragment")
                .map_err(|e| format!("STEP-A0: new_string failed: {e:?}"))?;
            env.call_method(
                &loader,
                "loadClass",
                "(Ljava/lang/String;)Ljava/lang/Class;",
                &[JValue::from(&name)],
            )
            .map_err(|e| {
                format!(
                    "STEP-A: loadClass failed — dex not loaded by container: {e:?}"
                )
            })?;
            // 设为当前线程上下文类加载器（robius 内部查找的回退依赖）
            let thread = env
                .call_static_method(
                    "java/lang/Thread",
                    "currentThread",
                    "()Ljava/lang/Thread;",
                    &[],
                )
                .and_then(|v| v.l())
                .map_err(|e| format!("STEP-A1: currentThread failed: {e:?}"))?;
            env.call_method(
                &thread,
                "setContextClassLoader",
                "(Ljava/lang/ClassLoader;)V",
                &[JValue::Object(&loader)],
            )
            .map_err(|e| format!("STEP-A2: setContextClassLoader failed: {e:?}"))?;
            Ok(())
        });
        match inner {
            Ok(r) => r,
            Err(e) => Err(format!("STEP-0: env attach failed: {e:?}")),
        }
    }

    /// Android：运行时申请权限 → 已迁往 [`crate::base::permission`]（2026-09-19：
    /// 权限是 OS 能力而非对话框关切；录音等音频场景因此不再依赖 dialog 特性）
    pub(super) async fn pick_file_impl(
        title: Option<&str>,
        filters: &[Filter],
    ) -> Result<Option<PickedFile>, DialogError> {
        #[cfg(target_os = "android")]
        if let Err(msg) = ensure_classloader() {
            return Err(DialogError::Backend(msg));
        }
        let r = block_with(|tx| {
            let mut dialog = RobiusDialog::new();
            if let Some(t) = title {
                dialog = dialog.set_title(t);
            }
            for (name, exts) in filters {
                dialog = dialog.add_filter(*name, exts);
            }
            let tx_cb = tx.clone(); // 回调持克隆，tx 留给启动失败路径
            let r = dialog.pick_file(move |res| {
                let mapped = match res {
                    Ok(Some(pf)) => to_picked(pf).map(Some),
                    Ok(None) => Ok(None),
                    Err(e) => Err(DialogError::Backend(format!("{e}"))),
                };
                let _ = tx_cb.send(mapped);
            });
            if let Err(e) = r {
                let _ = tx.send(Err(DialogError::Backend(format!("{e}"))));
            }
            Ok(())
        });
        r
    }

    pub(super) async fn save_bytes_impl(
        file_name: &str,
        data: &[u8],
    ) -> Result<Option<PathBuf>, DialogError> {
        #[cfg(target_os = "android")]
        if let Err(msg) = ensure_classloader() {
            return Err(DialogError::Backend(msg));
        }
        let r = block_with(|tx| {
            let mut dialog = RobiusDialog::new();
            dialog = dialog.set_file_name(file_name);
            let data_owned = data.to_vec(); // 消除 &[u8] 生命周期约束
            let tx_cb = tx.clone(); // 回调持克隆，tx 留给启动失败路径
            let r = dialog.save_data(data_owned, move |res| {
                // 用户选完位置即写入完成（无路径概念返回 None）
                let mapped = match res {
                    Ok(_) => Ok(None),
                    Err(e) => Err(DialogError::Backend(format!("保存失败: {e}"))),
                };
                let _ = tx_cb.send(mapped);
            });
            if let Err(e) = r {
                let _ = tx.send(Err(DialogError::Backend(format!("{e}"))));
            }
            Ok(())
        });
        r
    }
}

// ── 公开 API（统一异步；平台行为一致化）──────────────────────────

/// 打开单个文件（`Ok(None)` = 用户取消）
///
/// `filters`：`&[("文本文件", &["txt", "md"])]`（Web 端映射 accept 属性）
pub async fn pick_file(
    title: Option<&str>,
    filters: &[Filter],
) -> Result<Option<PickedFile>, DialogError> {
    imp::pick_file_impl(title, filters).await
}

/// 保存数据到用户指定的位置（**数据驱动**，全平台语义一致）
///
/// - 桌面：系统保存对话框 → 写入所选路径 → `Ok(Some(路径))`
/// - Web：以 `file_name` 触发浏览器下载 → `Ok(None)`（无路径概念）
/// - Android/iOS：robius 用户选位置写数据 → `Ok(None)`
/// - 用户取消 → `Ok(None)`
pub async fn save_bytes(file_name: &str, data: impl AsRef<[u8]>) -> Result<Option<PathBuf>, DialogError> {
    imp::save_bytes_impl(file_name, data.as_ref()).await
}

// ── 轮询式 API（非 wasm 平台；Android 强烈推荐）──────────────────────
//
// 与 [`pick_file`]/[`save_bytes`] 的 async 阻塞式相对：先 start 得到任务句柄，
// 之后每帧 `try_result()` 轮询——**主线程全程保持事件循环**。Android 上
// SAF 选择器是独立 Activity，覆盖期间会触发本应用的生命周期流转
// （surface 销毁/重建、Resume），阻塞式等待会让事件泵停摆（实测返回后
// 卡顿无法回应用）；轮询式从根上消除。
//
// ```ignore
// let mut job = dialog::pick_file_start(Some("选择"), &[("任意", &["*"])])?;
// // 每帧：
// if let Some(res) = job.try_result() { /* 完成：res = Ok(Some(file)) / Ok(None)=取消 */ }
// ```

/// 非阻塞发起"选择文件"，返回可轮询的任务句柄
#[cfg(not(target_arch = "wasm32"))]
pub fn pick_file_start(
    title: Option<&str>,
    filters: &[Filter],
) -> Result<PickJob, DialogError> {
    Ok(PickJob { imp: imp::pick_file_start_impl(title, filters)? })
}

/// [`pick_file_start`] 的任务句柄（每帧 [`try_result`](PickJob::try_result) 轮询）
#[cfg(not(target_arch = "wasm32"))]
pub struct PickJob {
    imp: imp::PickJob,
}

#[cfg(not(target_arch = "wasm32"))]
impl PickJob {
    pub fn try_result(&mut self) -> Option<Result<Option<PickedFile>, DialogError>> {
        self.imp.try_result()
    }
}

/// 非阻塞发起"保存数据"（数据立即移交，可安全释放原缓冲）
#[cfg(not(target_arch = "wasm32"))]
pub fn save_bytes_start(file_name: &str, data: Vec<u8>) -> Result<SaveJob, DialogError> {
    Ok(SaveJob { imp: imp::save_bytes_start_impl(file_name, data)? })
}

/// [`save_bytes_start`] 的任务句柄（每帧 [`try_result`](SaveJob::try_result) 轮询）
#[cfg(not(target_arch = "wasm32"))]
pub struct SaveJob {
    imp: imp::SaveJob,
}

#[cfg(not(target_arch = "wasm32"))]
impl SaveJob {
    pub fn try_result(&mut self) -> Option<Result<Option<PathBuf>, DialogError>> {
        self.imp.try_result()
    }
}
