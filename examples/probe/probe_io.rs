//! probe_io：io 模块全流程验证（write → exists → read 逐字节比对 → 文本往返）
//!
//! 全平台同一套 async 调用（原生 std::fs 直实现 / Web fetch POST·GET），
//! 应用代码零 `#[cfg]`——异步序列经 InitSlot 驱动（桌面阻塞填槽 /
//! web spawn_local，两平台同构）。
//!
//! 判据：console 锚点 `[io] ALL PASS`；Web 端对端 = examples/server/
//! server.py 的 `/saves/<name>` POST·GET 端点（服务器日志同步打印保存）。

#[path = "kit.rs"]
mod kit;

use kit::{Status, StatusPanel};
use starfish::base::app::{Application, Ctx, InitSlot, WindowConfig};
use starfish::base::window::{Window, WindowEvent};

/// 单步结果：(TAG, 判定, 详情)
type Step = (&'static str, Status, String);

struct IoProbe {
    panel: StatusPanel,
    io: InitSlot<Vec<Step>>,
    started: bool,
    applied: bool,
}

impl IoProbe {
    fn new() -> Self {
        Self {
            panel: StatusPanel::new(),
            io: InitSlot::default(),
            started: false,
            applied: false,
        }
    }
}

/// io 四步串行（任一步 Err 时后续步骤照常执行并各自 FAIL，凑齐全量诊断）
async fn run_io_steps() -> Vec<Step> {
    let mut out: Vec<Step> = Vec::new();
    let bin = kit::save_path("starfish_probe_io.bin");
    let payload: Vec<u8> = (0..=15u8).map(|i| i.wrapping_mul(17)).collect();

    // ① write（16B 特征模式）
    match starfish::base::io::write(&bin, payload.clone()).await {
        Ok(_) => out.push(("WRITE", Status::Pass, format!("{}B", payload.len()))),
        Err(e) => out.push(("WRITE", Status::Fail, format!("{e:?}"))),
    }

    // ② exists
    match starfish::base::io::exists(&bin).await {
        Ok(true) => out.push(("EXISTS", Status::Pass, String::new())),
        Ok(false) => out.push(("EXISTS", Status::Fail, "missing".into())),
        Err(e) => out.push(("EXISTS", Status::Fail, format!("{e:?}"))),
    }

    // ③ read 逐字节比对
    match starfish::base::io::read(&bin).await {
        Ok(data) if data == payload => {
            out.push(("READ", Status::Pass, format!("{}B", data.len())))
        }
        Ok(data) => out.push(("READ", Status::Fail, format!("mismatch {}B", data.len()))),
        Err(e) => out.push(("READ", Status::Fail, format!("{e:?}"))),
    }

    // ④ 文本往返（独立文件；内容含非 ASCII 验证 UTF-8 通道）
    let txt = kit::save_path("starfish_probe_io.txt");
    let text = "starfish io roundtrip 123";
    match starfish::base::io::write_text(&txt, text).await {
        Ok(_) => out.push(("WRITE_TXT", Status::Pass, String::new())),
        Err(e) => out.push(("WRITE_TXT", Status::Fail, format!("{e:?}"))),
    }
    match starfish::base::io::read_text(&txt).await {
        Ok(t) if t == text => out.push(("READ_TXT", Status::Pass, String::new())),
        Ok(t) => out.push(("READ_TXT", Status::Fail, format!("mismatch `{t}`"))),
        Err(e) => out.push(("READ_TXT", Status::Fail, format!("{e:?}"))),
    }

    out
}

impl Application for IoProbe {
    fn start(&mut self, ctx: &mut Ctx) {
        self.panel.init(ctx.window());
    }

    fn event(&mut self, _win: &Window, _event: &WindowEvent, _ctx: &mut Ctx) {}

    fn frame(&mut self, ctx: &mut Ctx) {
        // 帧 30 自动起跑（旧探针惯例：等面板家当装配完成）
        if !self.started && self.panel.frame_no() == 30 {
            self.started = true;
            let slot = self.io.clone();
            slot.init(async { run_io_steps().await });
        }

        // 收割异步结果并逐行判定
        if self.started && !self.applied {
            if let Some(steps) = self.io.get() {
                self.applied = true;
                let all_pass = steps.iter().all(|(_, s, _)| *s == Status::Pass);
                for (tag, s, detail) in steps.iter() {
                    self.panel.verdict("io", tag, *s, detail);
                }
                self.panel.verdict(
                    "io",
                    "ALL",
                    if all_pass { Status::Pass } else { Status::Fail },
                    "",
                );
            }
        }

        self.panel.render(ctx, false, |_, _| {});
    }
}

starfish::app_entry!(
    IoProbe::new(),
    WindowConfig::new("probe io", 800, 600)
        .with_fps_cap(60)
        .with_web_canvas_id("canvas")
);
