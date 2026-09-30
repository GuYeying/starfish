//! pygame hello（契约 §七 教程对照示例：三平台同一份代码，逐行可读）
//!
//! ```python
//! # 对位的 pygame 教程代码
//! screen = pygame.display.set_mode((800, 600))
//! clock = pygame.time.Clock()
//! pos = pygame.Rect(370, 270, 60, 60)
//! while True:
//!     for event in pygame.event.get():
//!         if event.type == pygame.QUIT: exit()
//!     keys = pygame.key.get_pressed()
//!     if keys[pygame.K_LEFT]:  pos.move_ip(-4, 0)
//!     if keys[pygame.K_RIGHT]: pos.move_ip(4, 0)
//!     screen.fill((30, 30, 46))
//!     pygame.draw.rect(screen, (0, 200, 90), pos)
//!     pygame.display.flip()
//!     clock.tick(60)
//! ```
//!
//! 差异仅三处（平台执行模型所致）：`set_mode` 一处 `.await`（WebGPU
//! 无阻塞模型）；循环末尾 `next_frame().await` 帧拍（Web 的 rAF 让出，
//! 桌面 no-op）；`exit()` → `return Ok(())`（app_entry 错误出口）。

use starfish::pygame::{display, draw, event, key, time, Event, Rect};

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    display::set_mode((800, 600)).await?;
    let mut clock = time::Clock::new();
    let mut pos = Rect::new(370, 270, 60, 60);
    loop {
        for e in event::get() {
            if let Event::Quit = e {
                return Ok(());
            }
        }
        if key::get_pressed().left() {
            pos.move_ip(-4, 0);
        }
        if key::get_pressed().right() {
            pos.move_ip(4, 0);
        }
        // 会话形态（Rust 版 with 语法）：render() 开始打包，end() 完成
        {
            let f = display::get_screen().render();
            f.fill([30, 30, 46, 255]);
            draw::rect(&f, [0, 200, 90], pos, 0);
            f.end();
        }
        display::flip();
        // 帧拍（三平台统一调用）：桌面 no-op；Web = rAF yield——调用方
        // 持循环的模型下，Web 必须让出事件循环，否则冻结渲染进程。
        starfish::base::window::next_frame().await;
        // Fifo 呈现的 vsync 回压已在节流——传 60 会叠加降帧（~30fps），
        // 见 time 模块文档「回压叠加警告」；教程心智 = 这一行就是节流。
        clock.tick(0);
    }
}

starfish::app_entry!(run());
