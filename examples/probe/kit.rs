//! probe 共享 harness：状态面板 + 三态判定 + 平台感知资源路径
//!
//! 铁律：**probe 应用代码零 `#[cfg]`**——平台差异只允许收敛在本文件与
//! 库入口（`starfish::app_entry!`）内部。每个 probe 经
//! `#[path = "kit.rs"] mod kit;` 独立引入（cargo 以示例为独立 crate，
//! 各自编译、无运行时共享）。
//!
//! 无头判读锚点：所有判定经 [`StatusPanel::verdict`] 落浏览器控制台，
//! 格式 `[{probe}] {TAG} {PASS|SKIP|FAIL} {detail}`（桌面/安卓对应
//! stdout / logcat `RustStdoutStderr`）。

#![allow(dead_code)]

use starfish::base::app::{Ctx, InitSlot};
use starfish::base::font::{self, Font, GlyphAtlas};
use starfish::base::render::bind_group::bind_group::BindGroup;
use starfish::base::render::mesh::mesh::Mesh;
use starfish::base::render::pipeline::RenderPipeline;
use starfish::base::render::render_entry::RenderEntry;
use starfish::base::render::render_resource_access::RenderResourceAccess;
use starfish::base::render::render_surface::RenderSurface;
use starfish::base::render::RenderContext;
use starfish::base::debug::console_log;
use starfish::base::window::Window;
use wgpu::BufferUsages;

use std::sync::Arc;

// ── 常量 ───────────────────────────────────────────────────────────

/// 图集字符集（94 可打印 ASCII，与旧探针逐字节一致）；上屏文本中
/// 不在集内的字符统一替换 '?'（CJK 等宽字符不进图集，省显存）
pub const ATLAS_CHARS: &str = " !\"#$%&'()*+,-./0123456789:;<=>?@ABCDEFGHIJKLMNOPQRSTUVWXYZ[\\]^_`abcdefghijklmnopqrstuvwxyz{|}~";

const FONT_BYTES: &[u8] = include_bytes!("../../resources/fonts/Antonio-Regular.ttf");
const FONT_SIZE: f32 = 30.0;
const TEXT_X: f32 = 24.0;
const TOP_Y: f32 = 50.0;
const ROW_PITCH: f32 = 62.0;

// ── 三态判定 ────────────────────────────────────────────────────────

/// 探针判定三态 + 过程态：能力不存在 → `Skip`（如 Web 无 UDP），
/// 不是失败；`Info`/`Pend` 仅用于过程行。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Pass,
    Skip,
    Fail,
    Info,
    Pend,
}

impl Status {
    pub fn word(self) -> &'static str {
        match self {
            Status::Pass => "PASS",
            Status::Skip => "SKIP",
            Status::Fail => "FAIL",
            Status::Info => "INFO",
            Status::Pend => "....",
        }
    }

    /// 行文字色
    pub fn line_color(self) -> [f32; 4] {
        match self {
            Status::Pass => [0.50, 1.00, 0.55, 1.0],
            Status::Skip => [0.55, 0.75, 1.00, 1.0],
            Status::Fail => [1.00, 0.35, 0.35, 1.0],
            Status::Info => [0.75, 0.85, 1.00, 1.0],
            Status::Pend => [0.60, 0.60, 0.65, 1.0],
        }
    }

    /// 清屏色（判定态整屏着色，一眼可读；Info/Pend 用中性色）
    pub fn bg(self) -> wgpu::Color {
        match self {
            Status::Pass => wgpu::Color { r: 0.05, g: 0.30, b: 0.08, a: 1.0 },
            Status::Skip => wgpu::Color { r: 0.05, g: 0.10, b: 0.30, a: 1.0 },
            Status::Fail => wgpu::Color { r: 0.40, g: 0.05, b: 0.05, a: 1.0 },
            Status::Info => wgpu::Color { r: 0.15, g: 0.15, b: 0.18, a: 1.0 },
            Status::Pend => wgpu::Color { r: 0.10, g: 0.10, b: 0.13, a: 1.0 },
        }
    }
}

// ── GPU 资源包 ──────────────────────────────────────────────────────

/// 面板渲染侧家当（字段对 probe 可见：经 `with_gpu` 作用域访问）
pub struct Gpu {
    pub _context: RenderContext,
    pub access: RenderResourceAccess,
    pub surface: RenderSurface,
    pub atlas: GlyphAtlas,
    pub camera_buffer: Arc<wgpu::Buffer>,
    pub camera_bind: BindGroup,
    pub atlas_bind: BindGroup,
    pub text_meshes: Vec<Mesh>,
    pub pipeline: Arc<RenderPipeline>,
}

// ── 状态面板 ────────────────────────────────────────────────────────

/// 多行文本状态面板（吞掉旧探针各 ~100-120 行的装配样板）。
/// 行数据存 panel 侧，Gpu 经 InitSlot 异步装配（桌面阻塞填槽 /
/// web spawn_local 排队，应用代码两平台零 cfg）。
pub struct StatusPanel {
    gpu: InitSlot<Gpu>,
    lines: Vec<(String, [f32; 4])>,
    bg: wgpu::Color,
    dirty: bool,
    frame: u64,
}

fn line_transform(row: usize) -> glam::Mat4 {
    glam::Mat4::from_translation(glam::Vec3::new(
        TEXT_X,
        TOP_Y + row as f32 * ROW_PITCH,
        0.0,
    ))
}

impl StatusPanel {
    pub fn new() -> Self {
        Self {
            gpu: InitSlot::default(),
            lines: vec![("init...".to_string(), Status::Pend.line_color())],
            bg: Status::Pend.bg(),
            dirty: true,
            frame: 0,
        }
    }

    /// `Application::start` 调用一次：异步装配渲染家当 + 内嵌字体图集。
    pub fn init(&mut self, window: &Window) {
        let window = window.clone();
        let slot = self.gpu.clone();
        let first = self.lines[0].clone();
        slot.init(async move {
            let (context, access, surface) = RenderEntry::async_new(
                &window,
                starfish::base::render::settings::SurfaceSettings::default(),
                starfish::base::render::settings::GpuSettings::default(),
            )
            .await
            .expect("RenderContext 初始化失败");
            let font = Font::from_bytes(FONT_BYTES.to_vec(), FONT_SIZE).expect("字体加载失败");
            let atlas = font.build_atlas(ATLAS_CHARS.chars(), &access).expect("图集构建失败");
            let camera_buffer = access.create_raw_buffer(
                Some("probe_camera"),
                64,
                BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            );
            let camera_bind = access
                .bind_group_builder()
                .uniform_raw(0, camera_buffer.clone(), 64)
                .build(Some("probe_camera_bind"));
            let atlas_bind = font::atlas_bind_group(&access, &atlas);
            let first_mesh =
                font::text_mesh_tf(&access, &atlas, &first.0, &line_transform(0), 1.0, first.1);
            let pipeline = font::text_pipeline(&access, &camera_bind, &atlas_bind, &first_mesh, 1);
            Gpu {
                _context: context,
                access,
                surface,
                atlas,
                camera_buffer,
                camera_bind,
                atlas_bind,
                text_meshes: vec![first_mesh],
                pipeline,
            }
        });
    }

    /// GPU 家当是否就绪（web 首帧排队期为 false）
    pub fn ready(&self) -> bool {
        self.gpu.get().is_some()
    }

    /// 已推进帧数（探针"帧 30 自动起跑"门沿用旧探针惯例）
    pub fn frame_no(&self) -> u64 {
        self.frame
    }

    /// Resized 事件转发（surface.resize；未就绪静默跳过）
    pub fn on_resize(&mut self, width: u32, height: u32) {
        self.with_gpu(|gpu| gpu.surface.resize(width, height));
    }

    // ── 行管理（收敛旧探针三种风格）──────────────────────────────

    /// 整组替换（Info 色）
    pub fn set_lines(&mut self, lines: &[&str]) {
        self.lines = lines
            .iter()
            .map(|t| (filter_atlas(t), Status::Info.line_color()))
            .collect();
        self.dirty = true;
    }

    /// 定点更新一行（保持原色；内容无变化不标脏——省一次网格重建）
    pub fn set_line(&mut self, idx: usize, text: &str) {
        let text = filter_atlas(text);
        match self.lines.get_mut(idx) {
            Some(slot) if slot.0 != text => {
                slot.0 = text;
                self.dirty = true;
            }
            _ => {}
        }
    }

    /// 追加一行（带状态色）
    pub fn line(&mut self, text: &str, status: Status) {
        self.lines.push((filter_atlas(text), status.line_color()));
        self.dirty = true;
    }

    /// 三态判定：屏显 `TAG: PASS detail`（同 TAG 行被替换而非追加）+
    /// console 锚点 `[{probe}] {TAG} PASS detail` + 判定态清屏着色。
    /// Info/Pend 不改清屏色。
    pub fn verdict(&mut self, probe: &str, tag: &str, status: Status, detail: &str) {
        let word = status.word();
        console_log(&format!("[{probe}] {tag} {word} {detail}").trim_end().to_string());
        let text = if detail.is_empty() {
            format!("{tag}: {word}")
        } else {
            format!("{tag}: {word} {detail}")
        };
        let text = filter_atlas(&text);
        let color = status.line_color();
        match self.lines.iter_mut().find(|(t, _)| {
            t.split(':').next().is_some_and(|head| head == tag)
        }) {
            Some(slot) => *slot = (text, color),
            None => self.lines.push((text, color)),
        }
        if matches!(status, Status::Pass | Status::Skip | Status::Fail) {
            self.bg = status.bg();
        }
        self.dirty = true;
    }

    /// 判定态清屏色（不产生行）
    pub fn set_bg(&mut self, status: Status) {
        self.bg = status.bg();
    }

    /// 作用域化 GPU 访问（视频打开、自建网格上传等）。闭包返回 owned 值，
    /// 不外借引用——规避 RefMut 跨语句存活导致的借用/重入冲突。
    pub fn with_gpu<R: 'static>(&mut self, f: impl FnOnce(&mut Gpu) -> R) -> Option<R> {
        self.gpu.get_mut().map(|mut gpu| f(&mut gpu))
    }

    /// 每帧收尾：脏网格重建 → 清屏 → overlay（probe 自定义绘制，先画）
    /// → 文本 → 投影按 ctx.size() 每帧写入（旧 20 号缺投影写入的 bug 在
    /// 此根治）→ present。未就绪返回 false 且整帧跳过。
    ///
    /// `depth` = 是否挂深度附件（3D 场景需要深度测试时传 true）。深度
    /// 模式下拆两个 pass：overlay 用深度开启管线；文本走独立无深度 pass
    /// （wgpu 要求管线深度格式与 pass 完全一致，深度关闭管线不能进
    /// 深度 pass）。无深度模式单 pass：overlay + 文本同 pass。
    pub fn render(
        &mut self,
        ctx: &mut Ctx,
        depth: bool,
        overlay: impl FnOnce(
            &mut starfish::base::render::render_pass::render_pass::RenderPass,
            &RenderResourceAccess,
        ),
    ) -> bool {
        self.frame += 1;
        if self.dirty {
            let lines = self.lines.clone();
            self.with_gpu(|gpu| {
                gpu.text_meshes = lines
                    .iter()
                    .enumerate()
                    .filter(|(_, (t, _))| !t.is_empty())
                    .map(|(i, (t, c))| {
                        font::text_mesh_tf(&gpu.access, &gpu.atlas, t, &line_transform(i), 1.0, *c)
                    })
                    .collect();
            });
            self.dirty = false;
        }
        let Some(mut gpu) = self.gpu.get_mut() else {
            return false;
        };

        gpu.surface.begin_frame(self.bg, 1.0);
        let color_attachment = gpu.surface.get_current_color_attachment();
        let depth_attachment = if depth {
            gpu.surface.get_current_depth_attachment()
        } else {
            None
        };
        let mut encoder = gpu.access.create_command_encoder();
        let color_atts = [&color_attachment];

        // ① 内容 pass（depth 模式挂深度附件——probe 需用深度开启管线；
        //    wgpu 要求管线深度格式与 pass 一致，深度关闭管线在此非法）
        {
            let mut pass =
                encoder.begin_render_pass("probe", &color_atts, depth_attachment, None, None, None);
            overlay(&mut pass, &gpu.access);
            if !depth {
                // 无深度模式：文本同 pass 叠加（文本管线即深度关闭）
                draw_text(&mut pass, &gpu);
            }
            pass.end();
        }
        // ② 深度模式：文本独立 pass（无深度附件 → 深度关闭文本管线合法；
        //    颜色 attachment 为 Load 语义，不会抹掉 ① 的内容）
        if depth {
            let mut text_pass =
                encoder.begin_render_pass("probe_text", &color_atts, None, None, None, None);
            draw_text(&mut text_pass, &gpu);
            text_pass.end();
        }

        // ③ 像素空间正交投影每帧写入（跟随窗口尺寸/自愈尺寸）
        let (w, h) = ctx.size();
        let (w, h) = (w.max(1) as f32, h.max(1) as f32);
        let projection = glam::Mat4::from_cols_array(&[
            2.0 / w, 0.0, 0.0, 0.0, //
            0.0, -2.0 / h, 0.0, 0.0, //
            0.0, 0.0, 1.0, 0.0, //
            -1.0, 1.0, 0.0, 1.0,
        ]);
        gpu.access
            .write_buffer(&gpu.camera_buffer, 0, bytemuck::bytes_of(&projection.to_cols_array_2d()));

        gpu.surface.submit([encoder.finish()]);
        gpu.surface.present();
        true
    }
}

impl Default for StatusPanel {
    fn default() -> Self {
        Self::new()
    }
}

/// 状态文本绘制（面板管线的标准画法）
fn draw_text(
    pass: &mut starfish::base::render::render_pass::render_pass::RenderPass,
    gpu: &Gpu,
) {
    for mesh in &gpu.text_meshes {
        pass.set_pipeline(&gpu.pipeline);
        pass.set_bind_group(0, &gpu.camera_bind);
        pass.set_bind_group(1, &gpu.atlas_bind);
        pass.set_mesh(mesh);
        pass.draw(0..mesh.vertex_count(), 0..1);
    }
}

/// 上屏文本过滤：非图集字符 → '?'（与旧探针约定一致）
fn filter_atlas(text: &str) -> String {
    text.chars()
        .map(|c| if ATLAS_CHARS.contains(c) { c } else { '?' })
        .collect()
}

// ── 平台感知查询（kit 内 cfg，调用点零 cfg）─────────────────────────

/// 逻辑资源名 → 本平台可加载路径。
///
/// - 桌面 / iOS：原样相对路径（CWD = 仓库根，如 `resources/videos/x.mp4`）
/// - Web：原样 URL（server.py `--resources-dir` 挂载仓库 resources 目录）
/// - Android：`embedded()` 字节幂等落盘 `{data_dir}/probe_assets/{logical}`
///   后返回绝对路径（引擎 `run` 自动注入 data_dir）
///
/// `data_dir` 未注入（在 `App::new` 里过早调用）→ `Err`，上屏 FAIL 可诊断
/// 而非 panic。规则：资产访问放 `start()` 之后。
pub fn asset_path(
    logical: &str,
    embedded: impl FnOnce() -> &'static [u8],
) -> Result<String, String> {
    #[cfg(target_os = "android")]
    return android_asset_path(logical, embedded);
    #[cfg(not(target_os = "android"))]
    {
        let _ = embedded; // 桌面/web 直接用逻辑路径，嵌入字节不参与（体积零负担）
        Ok(logical.to_string())
    }
}

#[cfg(target_os = "android")]
fn android_asset_path(
    logical: &str,
    embedded: impl FnOnce() -> &'static [u8],
) -> Result<String, String> {
    // 私有目录的唯一事实源 = io 的 base_dir（引擎 run 自动注入）
    let dir = starfish::base::io::base_dir()
        .ok_or_else(|| "base_dir 未注入（资产访问须在 start 之后）".to_string())?
        .to_string_lossy()
        .into_owned();
    let path = format!("{dir}/probe_assets/{logical}");
    let p = std::path::Path::new(&path);
    if !p.is_file() {
        let parent = p.parent().ok_or("bad asset path")?;
        std::fs::create_dir_all(parent).map_err(|e| format!("mkdir {parent:?}: {e}"))?;
        std::fs::write(p, embedded()).map_err(|e| format!("extract {path}: {e}"))?;
    }
    Ok(path)
}

/// io 保存名 → 平台合适的相对路径。
///
/// - Web：补 `saves/` 前缀命中 server.py 的 `/saves/<name>` POST 端点
/// - 桌面：原名（CWD 下散文件，对齐旧 10 号 recording.wav 惯例）
/// - Android：原名（引擎 `run` 已把 io base_dir 注入为私有目录）
pub fn save_path(name: &str) -> String {
    #[cfg(target_arch = "wasm32")]
    return format!("saves/{name}");
    #[cfg(not(target_arch = "wasm32"))]
    return name.to_string();
}

pub fn page_hostname() -> Option<String> {
    #[cfg(target_arch = "wasm32")]
    {
        let host = web_sys::window()?.location().host().ok()?;
        Some(host.rsplit_once(':').map(|(ip, _)| ip.to_string()).unwrap_or(host))
    }
    #[cfg(not(target_arch = "wasm32"))]
    None
}

/// 启用 UDP 广播发送（发现类探针需要；`set_broadcast` 仅原生平台存在，
/// Web 无 UDP 无此概念——此处吸收 cfg）
pub fn enable_broadcast(sock: &starfish::base::net::UdpSock) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = sock.set_broadcast(true);
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = sock;
    }
}

// ── 统一轮询式对话框 Job（调用点零 cfg）────────────────────────────

/// 对话框平台差异的收敛点：
/// - 原生（含安卓）：`pick_file_start` / `save_bytes_start` 轮询式任务
/// - Web：真异步 `pick_file` / `save_bytes`（spawn_local 驱动），结果填槽
///   后用同一 `try_result` 接口收割
///
/// 结果统一简化为 `Result<Option<String>, String>`（文件名 / 保存路径 /
/// 取消 = Ok(None)）。
pub struct PickJob(imp::PickJob);
pub struct SaveJob(imp::SaveJob);

pub fn pick_file_start(title: Option<&str>) -> Result<PickJob, String> {
    imp::pick_start(title).map(PickJob)
}

pub fn save_bytes_start(file_name: &str, data: Vec<u8>) -> Result<SaveJob, String> {
    imp::save_start(file_name, data).map(SaveJob)
}

impl PickJob {
    /// 每帧轮询：Some = 已选择 / 取消 / 出错
    pub fn try_result(&mut self) -> Option<Result<Option<String>, String>> {
        self.0.try_result()
    }
}

impl SaveJob {
    /// 每帧轮询：Some = 已写出 / 取消 / 出错
    pub fn try_result(&mut self) -> Option<Result<Option<String>, String>> {
        self.0.try_result()
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod imp {
    use starfish::base::dialog;

    pub struct PickJob(dialog::PickJob);

    pub fn pick_start(title: Option<&str>) -> Result<PickJob, String> {
        dialog::pick_file_start(title, &[("any", &["*"])])
            .map(PickJob)
            .map_err(|e| format!("{e:?}"))
    }

    impl PickJob {
        pub fn try_result(&mut self) -> Option<Result<Option<String>, String>> {
            self.0.try_result().map(|r| {
                r.map(|o| o.map(|pf| pf.name().to_string()))
                    .map_err(|e| format!("{e:?}"))
            })
        }
    }

    pub struct SaveJob(dialog::SaveJob);

    pub fn save_start(file_name: &str, data: Vec<u8>) -> Result<SaveJob, String> {
        dialog::save_bytes_start(file_name, data)
            .map(SaveJob)
            .map_err(|e| format!("{e:?}"))
    }

    impl SaveJob {
        pub fn try_result(&mut self) -> Option<Result<Option<String>, String>> {
            match self.0.try_result() {
                None => None,
                Some(r) => Some(match r {
                    Ok(Some(p)) => Ok(Some(p.display().to_string())),
                    Ok(None) => Ok(None), // 用户取消
                    Err(e) => Err(format!("{e:?}")),
                }),
            }
        }
    }
}

#[cfg(target_arch = "wasm32")]
mod imp {
    use std::cell::RefCell;
    use std::rc::Rc;
    use wasm_bindgen_futures::spawn_local;

    type Slot = Rc<RefCell<Option<Result<Option<String>, String>>>>;

    fn make_slot() -> (Slot, Rc<RefCell<Option<Result<Option<String>, String>>>>) {
        let slot = Rc::new(RefCell::new(None));
        (slot.clone(), slot)
    }

    pub struct PickJob(Slot);

    pub fn pick_start(title: Option<&str>) -> Result<PickJob, String> {
        let (job, slot) = make_slot();
        let title = title.unwrap_or("probe pick").to_string();
        spawn_local(async move {
            let msg = match starfish::base::dialog::pick_file(Some(&title), &[("any", &["*"])]).await
            {
                Ok(Some(pf)) => Ok(Some(pf.name().to_string())),
                Ok(None) => Ok(None),
                Err(e) => Err(format!("{e:?}")),
            };
            *slot.borrow_mut() = Some(msg);
        });
        Ok(PickJob(job))
    }

    impl PickJob {
        pub fn try_result(&mut self) -> Option<Result<Option<String>, String>> {
            self.0.borrow_mut().take()
        }
    }

    pub struct SaveJob(Slot);

    pub fn save_start(file_name: &str, data: Vec<u8>) -> Result<SaveJob, String> {
        let (job, slot) = make_slot();
        let file_name = file_name.to_string();
        spawn_local(async move {
            let msg = match starfish::base::dialog::save_bytes(&file_name, data).await {
                Ok(path) => Ok(path.map(|p| p.display().to_string())),
                Err(e) => Err(format!("{e:?}")),
            };
            *slot.borrow_mut() = Some(msg);
        });
        Ok(SaveJob(job))
    }

    impl SaveJob {
        pub fn try_result(&mut self) -> Option<Result<Option<String>, String>> {
            self.0.borrow_mut().take()
        }
    }
}
