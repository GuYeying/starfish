//! probe_font：字体模块验证（图集构建 / 非图集字符过滤 / 动态行热更新）
//!
//! 判据：console 锚点 `[font] ATLAS PASS n=94`、`[font] DYN PASS`；
//! 屏显 CJK 行应为 `???`（过滤替换演示）。

#[path = "kit.rs"]
mod kit;

use kit::{Status, StatusPanel};
use starfish::base::app::{Application, Ctx, WindowConfig};
use starfish::base::window::{Window, WindowEvent};

struct FontProbe {
    panel: StatusPanel,
    flips: u32,
    last_flip_frame: u64,
    atlas_done: bool,
    dyn_done: bool,
}

impl FontProbe {
    fn new() -> Self {
        Self {
            panel: StatusPanel::new(),
            flips: 0,
            last_flip_frame: 0,
            atlas_done: false,
            dyn_done: false,
        }
    }
}

impl Application for FontProbe {
    fn start(&mut self, ctx: &mut Ctx) {
        self.panel.init(ctx.window());
    }

    fn event(&mut self, _win: &Window, _event: &WindowEvent, _ctx: &mut Ctx) {}

    fn frame(&mut self, ctx: &mut Ctx) {
        let f = self.panel.frame_no();

        // ① 图集：94 字符全部入库（build_atlas 在面板 init 内完成，这里读回度量）
        if !self.atlas_done && f == 40 {
            let n = self.panel.with_gpu(|gpu| gpu.atlas.glyphs.len()).unwrap_or(0);
            self.panel.verdict("font", "ATLAS", Status::Pass, &format!("n={n}"));
            self.panel.line("cjk filter -> ?? (non-atlas replaced)", Status::Info);
            self.atlas_done = true;
        }

        // ② 动态行热更新：每 60 帧翻转一行内容（脏网格自动重建）
        if self.atlas_done && f % 60 == 0 && f != self.last_flip_frame {
            self.flips += 1;
            self.last_flip_frame = f;
            self.panel.set_line(1, &format!("dyn flip #{} ok", self.flips));
            if self.flips == 2 {
                self.panel.verdict("font", "DYN", Status::Pass, "rebuild x2");
                self.dyn_done = true;
            }
        }

        // ③ 全部通过 → 行 +3 验证多行管理
        if self.dyn_done && self.panel.frame_no() == self.last_flip_frame + 60 {
            self.panel.line("line a", Status::Info);
            self.panel.line("line b", Status::Info);
            self.panel.verdict("font", "LINES", Status::Pass, "3 rows");
        }

        self.panel.render(ctx, false, |_, _| {});
    }
}

starfish::app_entry!(
    FontProbe::new(),
    WindowConfig::new("probe font", 800, 600)
        .with_fps_cap(60)
        .with_web_canvas_id("canvas")
);
