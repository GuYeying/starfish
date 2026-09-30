//! pygame render 探针（P2/P3/P4 渲染能力 + 批次十会话/深度/MRT 验收）
//!
//! 验收面：Screen 装配 / Surface 离屏（fill + 图元）/ Texture 程序化生成 /
//! image 解码（PNG）/ blit 链路（含 blit 源自动 flush）/ **渲染会话**
//! （`render()` Rust 版 with）/ **深度会话**（`render_depth()`：先画红
//! 后画绿、重叠处红赢 = 深度测试生效，与画家序相反）/ **MRT 会话**
//! （`render_targets`：一次 pass 镜像写两个目标）/ font 文字上屏。
//! 600 帧后桌面退出 / web-Android 驻留。
//!
//! 运行：`cargo run --example pygame_probe`

use starfish::base::debug::console_log;
use starfish::base::gfx::geometry::shape2d;
use starfish::base::window::{Event, KeyCode, Window};
use starfish::pygame::draw;
use starfish::pygame::font::Font;
use starfish::pygame::image;
use starfish::pygame::render::{render_targets, DrawTarget, GeometryKind, Screen, Surface};
use starfish::pygame::{sprite, transform, Rect};

/// 自动退出帧数（600 ≈ 10 秒）
const AUTO_EXIT_FRAMES: u32 = 600;

/// 程序化 32×32 棋盘纹理（Rgba8）
fn checkerboard(size: u32, a: [u8; 4], b: [u8; 4], cell: u32) -> Vec<u8> {
    let mut px = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let c = if (x / cell + y / cell) % 2 == 0 { a } else { b };
            px.extend_from_slice(&c);
        }
    }
    px
}

async fn app_body() -> Result<(), Box<dyn std::error::Error>> {
    let mut window = Window::builder()
        .title("starfish pygame render probe")
        .size((800, 600))
        .build()
        .expect("建窗失败");
    let screen = Screen::new(&window).await.expect("GPU 初始化失败");

    // 纹理 quad：程序化棋盘 + image 解码（awesomeface）
    let sprite = Surface::from_rgba8(
        (32, 32),
        &checkerboard(32, [240, 90, 90, 255], [250, 220, 120, 255], 8),
    );
    let face = image::load_from_bytes(include_bytes!("../resources/textures/awesomeface.png"))
        .expect("awesomeface.png 解码失败");

    // 离屏 Surface：橙底 + 圆（Surface fill + 图元路径）
    let offscreen = Surface::new((96, 96));
    offscreen.fill([255, 140, 0, 255]);
    let inner_circle = shape2d::circle([48.0, 48.0].into(), 28.0, 32, [0.2, 0.3, 0.9, 1.0]);
    offscreen.with_batch(|b| {
        b.push_geometry(GeometryKind::Filled, &inner_circle, [0.0, 0.0], [0.2, 0.3, 0.9, 1.0])
    });

    // 深度离屏面（with_depth：深度缓冲 + 深度会话载体）
    let depth_surface = Surface::with_depth((160, 120));
    // MRT 目标对（镜像双写验证）
    let mrt_a = Surface::new((120, 90));
    let mrt_b = Surface::new((120, 90));

    // 文字上屏（font 资源制备路线；字体内嵌三平台零 IO）
    let font = Font::from_bytes(
        include_bytes!("../resources/fonts/Antonio-Regular.ttf").to_vec(),
        48,
    )
    .expect("字体解析失败");
    let text = font.render("starfish pygame", true, [255, 255, 255, 255]);

    // transform（GPU 采样）：缩放 → 旋转 / 水平镜像
    let face_small = transform::scale(&face, (100, 100));
    let face_rot = transform::rotate(&face_small, 45.0);
    let sprite_flip = transform::flip_x(&sprite);

    // set_clip（scissor 裁剪）：圆被裁进 80×80 区域
    let clipped = Surface::new((160, 120));
    clipped.set_clip(Some(Rect::new(40, 20, 80, 80)));
    clipped.fill([20, 60, 110, 255]);
    clipped.with_batch(|b| {
        let c = shape2d::circle([80.0, 60.0].into(), 55.0, 48, [0.3, 0.9, 0.6, 1.0]);
        b.push_geometry(GeometryKind::Filled, &c, [0.0, 0.0], [0.3, 0.9, 0.6, 1.0]);
    });
    clipped.set_clip(None);

    let rect_outline = shape2d::rect_outline([200.0, 420.0].into(), [380.0, 500.0].into(), [1.0; 4]);
    let diag_line = shape2d::line([80.0, 520.0].into(), [720.0, 80.0].into(), [0.3, 1.0, 0.6, 1.0]);

    // sprite Group（Godot 式重设计）：敌人群 z 排序 + 分组 + 巡逻移动
    let mut group = sprite::Group::new();
    let _e1 = group.insert(sprite::Sprite::new(&face).at(120, 200).scale(0.15).z(1).tag("enemy"));
    let _e2 = group.insert(sprite::Sprite::new(&face).at(160, 240).scale(0.15).z(0).tag("enemy"));
    let _player = group.insert(
        sprite::Sprite::new(&sprite).at(700, 480).scale(2.0).z(3).tag("player").flip_y(),
    );

    let mut frames: u32 = 0;
    loop {
        while let Some(event) = window.poll_event() {
            match event {
                Event::CloseRequested => return Ok(()),
                Event::KeyDown { key, .. } if key == KeyCode::Escape => return Ok(()),
                Event::Resized { width, height } => screen.resize(width, height),
                _ => {}
            }
        }

        // sprite 更新：敌人群左移巡逻，出界回绕（查询制，非回调）
        for id in group.with_tag("enemy") {
            group.move_by(id, -2, 0);
            if let Some((x, y)) = group.position(id) {
                if x < -60 {
                    group.set_position(id, 860, y);
                }
            }
        }
        group.tick(1.0 / 60.0); // 动画帧推进（本 demo 精灵无多帧，保持口径）

        // ── 离屏产物会话（先于主会话：主会话 blit 采样本帧内容）──
        // 深度面：底色（2D pass，不写深度）→ 深度 pass：红先画、绿后画，
        // 重叠处红赢（Less 测试剔除后画者）＝ 深度在工作的可见证据
        {
            let f = depth_surface.render();
            f.fill([20, 20, 30, 255]);
            f.end();
        }
        {
            let f = depth_surface.render_depth();
            f.with_batch(|b| {
                b.push_quad(25.0, 25.0, 80.0, 80.0, [0.9, 0.25, 0.25, 1.0], None); // 先：红
                b.push_quad(55.0, 45.0, 80.0, 80.0, [0.25, 0.9, 0.35, 1.0], None); // 后：绿（被剔除）
            });
            f.end();
        }

        // ── MRT 会话：一次 pass 镜像写入两个目标 ──
        {
            let f = render_targets(&[&mrt_a, &mrt_b]);
            f.fill([60, 20, 80, 255]);
            f.with_batch(|b| {
                b.push_quad(30.0, 15.0, 60.0, 60.0, [1.0, 1.0, 1.0, 1.0], Some(face.texture()))
            });
            f.end();
        }

        // ── 主会话：全场景合成（统一 RenderPass 形态）──
        {
            let f = screen.render();
            f.fill([30, 30, 46, 255]);
            f.blit(&sprite, (100, 80));
            f.blit(&sprite, (140, 120)); // 半重叠：验证多 blit 与混合
            f.blit(&sprite_flip, (240, 90)); // transform：水平镜像
            f.blit(&face_rot, (380, 40)); // transform：缩放 100×100 → 旋转 45°
            f.blit(&offscreen, (80, 260));
            f.blit(&depth_surface, (300, 260));
            f.blit(&mrt_a, (520, 260));
            f.blit(&mrt_b, (660, 260));
            f.blit(&clipped, (40, 400)); // set_clip：圆被裁进 80×80
            draw::rect(&f, [0, 200, 90], Rect::new(560, 80, 140, 90), 0); // draw::* 吃会话
            draw::arc(&f, [1.0, 0.7, 0.2, 1.0], Rect::new(460, 470, 200, 120), 0.0, std::f32::consts::PI, 1);
            f.with_batch(|b| {
                b.push_quad(560.0, 80.0, 140.0, 90.0, [0.9, 0.3, 0.5, 1.0], None);
                let circle = shape2d::circle([430.0, 470.0].into(), 40.0, 48, [0.2, 0.8, 1.0, 1.0]);
                b.push_geometry(GeometryKind::Filled, &circle, [0.0, 0.0], [0.2, 0.8, 1.0, 1.0]);
                b.push_geometry(GeometryKind::Line, &rect_outline, [0.0, 0.0], [1.0; 4]);
                b.push_geometry(GeometryKind::Line, &diag_line, [0.0, 0.0], [0.3, 1.0, 0.6, 1.0]);
            });
            group.draw(&f); // sprite 群：按 (z, y) 排序批量入 batch
            f.blit(&text, (80, 520));
            f.end(); // 显式打包（漏写则 Drop 兜底，同路径）
        }

        screen.present();
        frames += 1;
        if frames >= AUTO_EXIT_FRAMES {
            console_log(&format!(
                "[pygame_probe] {AUTO_EXIT_FRAMES} 帧完成，{}",
                if cfg!(target_arch = "wasm32") { "Web 驻留" } else { "正常退出" }
            ));
            // 桌面/Android：真实退出（到点即关）。
            // Web：驻留不返回——应用 future 完成后，迟到的 rAF/微任务
            // 会踩到已 drop 的绑定闭包（console 报错），驻留保持画面
            // 与控制台干净。
            #[cfg(not(target_arch = "wasm32"))]
            return Ok(());
            #[cfg(target_arch = "wasm32")]
            std::future::pending::<()>().await;
        }
        starfish::base::window::next_frame().await;
    }
}

// 平台入口：宏内化三平台驱动器（同 examples/hello.rs）
starfish::app_entry!(app_body());
