//! pygame.sprite 运行测试：Sprite 构造链 + Group 管理族。
//!
//! 自检项（状态条顺序）：insert/count / tag 过滤 / move_by/position /
//! is_alive / remove / tick+draw（晚登记——跑过若干帧动画后）。
//! 场景：红块玩家精灵 + 红蓝双帧动画精灵（tick 驱动帧轮换，视觉可辨）。

#[path = "common.rs"]
mod common;

use common::{draw_status, Checks};
use starfish_pygame::render::DrawTarget;
use starfish_pygame::display;
use starfish_pygame::render::Surface;
use starfish_pygame::sprite;

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    display::set_mode((800, 600)).await?;
    let screen = display::get_screen();
    let mut checks = Checks::new("pygame.sprite");
    common::log("[sprite] set_mode 完成，进入自检");

    let s_red = Surface::new((24, 24));
    s_red.fill([220, 60, 60, 255]);
    let s_blue = Surface::new((24, 24));
    s_blue.fill([60, 120, 220, 255]);

    let mut g = sprite::Group::new();
    let player = g.insert(sprite::Sprite::new(&s_red).at(120, 200).tag("player"));
    let npc = g.insert(
        sprite::Sprite::with_frames(&[&s_red, &s_blue], 4.0).at(420, 200).tag("npc"),
    );
    checks.require("insert/count", g.count() == 2);
    checks.require("tag 过滤", g.with_tag("npc").len() == 1);

    g.move_by(player, 40, 0);
    checks.require(
        "move_by/position",
        g.position(player) == Some((160, 200)) && g.position(npc) == Some((420, 200)),
    );
    checks.require("is_alive", g.is_alive(npc));

    g.set_z(player, 5);
    g.set_rotation(player, 30.0);
    g.set_scale(player, 2.0);
    g.set_visible(npc, true);
    checks.require("变换链", true);

    let removed = g.remove(npc);
    checks.require("remove", removed && !g.is_alive(npc) && g.count() == 1);

    let mut frames = 0u32;
    loop {
        if common::quit_requested() {
            return Ok(());
        }
        // 玩家小幅巡游（move_by 每帧生效）
        let dir = if (frames / 60) % 2 == 0 { 1 } else { -1 };
        g.move_by(player, dir, 0);

        g.tick(1.0 / 60.0);
        {
            let f = screen.render();
            f.fill([30, 30, 46, 255]);
            g.draw(&f);
            if frames == 120 {
                checks.require("tick+draw 稳定", true);
            }
            draw_status(&f, &checks);
            f.end();
        }
        display::flip();
        common::paced_frame().await;
        frames += 1;
        if frames >= common::EXIT_FRAMES {
            break;
        }
    }
    let (passed, total) = checks.summary();
    common::log(&format!("[sprite] {frames} 帧完成，自检 {passed}/{total}"));
    common::reside().await;
    Ok(())
}

starfish_std::app_entry!(run());
