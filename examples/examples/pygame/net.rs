//! pygame.net 运行测试：连接面 + UDP 自环回包。
//!
//! 自检项（状态条顺序）：非法地址 → Err / UDP bind（原生）/
//! UDP 自环回包（原生，晚登记——回包到达即登记，超时登记败）/
//! Web 形态（Web 平台专属——无 UDP，仅错误路径）。
//! 场景：等待回包时状态灯呼吸（收到 = 绿闪；超时 = 红横条，由状态条呈现）。

#[path = "common.rs"]
mod common;

use common::{draw_status, Checks};
use starfish_pygame::render::DrawTarget;
use starfish_pygame::display;
use starfish_pygame::net;

const UDP_PORT: u16 = 47231;
const PAYLOAD: &[u8] = b"starfish-net-selfloop";

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    display::set_mode((800, 600)).await?;
    let screen = display::get_screen();
    let mut checks = Checks::new("pygame.net");
    common::log("[net] set_mode 完成，进入自检");

    // 错误路径（三平台一致）：非法地址串安全返回失败（Err 或后台失败态
    // ——connect 立即返回、握手表可能异步失败，故两种形态都算"安全"）
    let bad = net::TcpConn::connect("not-an-address");
    let safe_fail = match bad {
        Err(_) => true,
        Ok(c) => c.state() != net::ConnState::Connected,
    };
    checks.require("非法地址安全失败", safe_fail);

    // UDP 自环（原生专属）：绑本地端口 → 发给自己 → 轮询收回
    #[cfg(not(target_arch = "wasm32"))]
    let mut sock = match net::UdpSock::bind(&format!("127.0.0.1:{UDP_PORT}")) {
        Ok(s) => {
            checks.require("UDP bind", true);
            Some(s)
        }
        Err(e) => {
            checks.require("UDP bind", false);
            common::log(&format!("[net] UDP bind 失败（端口被占？）：{e:?}"));
            None
        }
    };
    #[cfg(target_arch = "wasm32")]
    checks.require("Web 形态：仅错误路径自检", true);

    let mut frames = 0u32;
    #[cfg(not(target_arch = "wasm32"))]
    let mut received = false;
    loop {
        if common::quit_requested() {
            return Ok(());
        }
        frames += 1;

        #[cfg(not(target_arch = "wasm32"))]
        if let Some(s) = sock.as_ref() {
            if frames == 1 {
                let _ = s.send_to(PAYLOAD, &format!("127.0.0.1:{UDP_PORT}"));
            }
            if let Some((data, _from)) = s.try_recv_from() {
                received = true;
                checks.require("UDP 自环回包", data == PAYLOAD);
                common::log("[net] 自环回包已收到");
            } else if frames == 300 && !received {
                checks.require("UDP 自环回包", false);
                common::log("[net] 自环回包超时（300 帧）");
            }
        }

        {
            let f = screen.render();
            f.with_batch(|b| {
                let pulse = (frames as f32 * 0.08).sin() * 0.5 + 0.5;
                let c = if pulse > 0.5 {
                    [0.3529, 0.7843, 0.4314, 1.0000]
                } else {
                    [0.2353, 0.4706, 0.3137, 1.0000]
                };
                b.push_quad(360.0, 250.0, 80.0, 80.0, c, None);
            });
            draw_status(&f, &checks);
            f.end();
        }
        display::flip();
        common::paced_frame().await;
        if frames >= common::EXIT_FRAMES {
            break;
        }
    }
    let (passed, total) = checks.summary();
    common::log(&format!("[net] {frames} 帧完成，自检 {passed}/{total}"));
    common::reside().await;
    Ok(())
}

starfish_std::app_entry!(run());
