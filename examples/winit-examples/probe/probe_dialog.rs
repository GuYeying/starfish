//! probe_dialog：对话框模块验证（pick 轮询式任务 → save 轮询式任务）
//!
//! 轮询式 = 对话框期间主线程事件循环不停摆（Android SAF 独立 Activity
//! 场景的既定约束，见批次 18）。全平台同一源码零 `#[cfg]`——平台差异
//! （原生轮询任务 / Web 真异步填槽）由 kit 统一 Job 收敛。
//!
//! 阶段推进设计（2026-09-19 设备实测修正）：
//! - 帧 30 自动发起 pick（无头环境可自动判定 START）
//! - **Web 真实浏览器**：文件选择器必须在用户手势内打开——自动尝试会
//!   被拒（控制台一条激活错误），**点击屏幕即重试**，选择器正常打开
//! - **Android**：pick 结果后**不自动连发 save**——背靠背拉起两个 SAF
//!   Activity 在卓易通容器上会卡死；改为点击后进入 save 阶段
//!
//! 判据：console 锚点 `[dialog] START PASS`（发起成功）、
//! `[dialog] RESULT PASS/...`（选择/取消都算机制正常）、
//! `[dialog] SAVE PASS/...`。

#[path = "kit.rs"]
mod kit;

use kit::{Status, StatusPanel};
use starfish::base::app::{Application, Ctx, WindowConfig};
use starfish::base::window::{Window, WindowEvent};

enum Phase {
    /// 等待发起（帧 30 自动 / 点击随时重试——Web 选择器需要用户激活）
    Idle,
    /// pick 任务轮询中
    Picking(kit::PickJob),
    /// pick 已判定，等待点击进入 save（Android：避免背靠背 SAF）
    PickDone,
    /// save 任务轮询中
    Saving(kit::SaveJob),
    /// 全部判定完成（点击重跑全流程）
    Done,
}

struct DialogProbe {
    panel: StatusPanel,
    phase: Phase,
    /// 发起请求：帧 30 自动置位一次；点击随时再置位（重试/重跑的点火键）
    armed: bool,
    /// save 阶段的点火键（PickDone 态点击置位）
    save_armed: bool,
}

impl DialogProbe {
    fn new() -> Self {
        Self {
            panel: StatusPanel::new(),
            phase: Phase::Idle,
            armed: true, // 自动起跑（无头/原生路径）
            save_armed: false,
        }
    }

    fn restart(&mut self) {
        self.phase = Phase::Idle;
        self.armed = true;
        self.save_armed = false;
        self.panel.set_lines(&["dialog probe: re-probing..."]);
    }
}

impl Application for DialogProbe {
    fn start(&mut self, ctx: &mut Ctx) {
        self.panel.init(ctx.window());
    }

    fn event(&mut self, _win: &Window, event: &WindowEvent, _ctx: &mut Ctx) {
        if !matches!(event, WindowEvent::MousePressed(_)) {
            return;
        }
        match &self.phase {
            // 点击 = 万能点火键：Idle/Picking 重试发起（Web 选择器需用户激活，
            // 自动尝试被拒后点击即恢复）/ Done 重跑全流程
            Phase::Done | Phase::Idle | Phase::Picking(_) => self.restart(),
            // PickDone 态点击 = 进入 save 阶段
            Phase::PickDone => self.save_armed = true,
            Phase::Saving(_) => {}
        }
    }

    fn frame(&mut self, ctx: &mut Ctx) {
        let f = self.panel.frame_no();

        match &mut self.phase {
            Phase::Idle => {
                // 自动起跑（帧 30）或点击点火（f 已 >30 时点击重试立即生效）
                if self.armed && f >= 30 {
                    self.armed = false;
                    match kit::pick_file_start(Some("probe pick")) {
                        Ok(job) => {
                            self.panel
                                .verdict("dialog", "START", Status::Pass, "pick_file_start");
                            self.phase = Phase::Picking(job);
                        }
                        Err(e) => {
                            self.panel
                                .verdict("dialog", "START", Status::Fail, &format!("{e:?}"));
                            self.phase = Phase::Done;
                        }
                    }
                }
            }
            Phase::Picking(job) => {
                // 轮询收割：选择/取消都算对话框机制正常，Err 才 FAIL
                if let Some(res) = job.try_result() {
                    match res {
                        Ok(Some(name)) => {
                            self.panel.verdict(
                                "dialog",
                                "RESULT",
                                Status::Pass,
                                &format!("picked {name}"),
                            );
                        }
                        Ok(None) => {
                            self.panel.verdict("dialog", "RESULT", Status::Pass, "cancelled")
                        }
                        Err(e) => {
                            self.panel
                                .verdict("dialog", "RESULT", Status::Fail, &format!("{e:?}"));
                        }
                    }
                    self.panel.line("click to test save...", Status::Info);
                    self.phase = Phase::PickDone;
                }
            }
            Phase::PickDone => {
                // Android：pick 与 save 之间隔一次点击——背靠背拉起两个
                // SAF 独立 Activity 在卓易通容器上会卡死（设备实测）
                if self.save_armed {
                    self.save_armed = false;
                    match kit::save_bytes_start("starfish_probe_save.txt", b"probe".to_vec()) {
                        Ok(job) => {
                            self.panel.verdict("dialog", "SAVE_START", Status::Pass, "");
                            self.phase = Phase::Saving(job);
                        }
                        Err(e) => {
                            self.panel
                                .verdict("dialog", "SAVE_START", Status::Fail, &format!("{e:?}"));
                            self.phase = Phase::Done;
                        }
                    }
                }
            }
            Phase::Saving(job) => {
                if let Some(res) = job.try_result() {
                    match res {
                        Ok(where_) => self.panel.verdict(
                            "dialog",
                            "SAVE",
                            Status::Pass,
                            &where_.unwrap_or_else(|| "written".into()),
                        ),
                        Err(e) => {
                            self.panel
                                .verdict("dialog", "SAVE", Status::Fail, &format!("{e:?}"));
                        }
                    }
                    self.phase = Phase::Done;
                }
            }
            Phase::Done => {}
        }

        self.panel.render(ctx, false, |_, _| {});
    }
}

starfish::app_entry!(
    DialogProbe::new(),
    WindowConfig::new("probe dialog", 800, 600)
        .with_fps_cap(60)
        .with_web_canvas_id("canvas")
);
