use sdl3::{event::Event, video::WindowFlags};
use starfish::base::render::render_entry::RenderEntry;
use starfish::base::subsystem::{EventSubsystem, VideoSubsystem};
use starfish::base::window::Window;
use wgpu::Color;

fn main() {
    let sdl = sdl3::init().expect("SDL init failed.");
    let video = VideoSubsystem::new(&sdl);
    let _event = EventSubsystem::new(&sdl);
    let window = Window::new(
        &video,
        "Hello World",
        (800, 600),
        WindowFlags::default(),
    )
        .expect("Window creation failed");
    let (context,resouce,mut surface) = RenderEntry::new(&window, None, None)
        .expect("RenderContext 初始化失败");

    let mut running = true;
    while running {
        for event in sdl.event_pump().unwrap().poll_iter() {
            match event {
                Event::Quit { .. } => running = false,
                _ => {}
            }
        }
        surface.begin_frame(Color {r: 0.1, g: 0.1, b: 0.15, a: 1.0,},1.0);
        surface.present();
    }
}