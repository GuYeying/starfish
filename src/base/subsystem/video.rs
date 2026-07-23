use sdl3::{Error, Sdl, VideoSubsystem as SdlVideoSubsystem, video::{Display, SystemTheme, WindowBuilder}};


pub struct VideoSubsystem{
    inner:SdlVideoSubsystem,
}

impl VideoSubsystem {

    pub fn new(sdl:&Sdl)->Self{
        let inner = sdl.video().expect("VideoSubsystem init failed.");
        Self{inner}
    }

    pub fn inner(&self)->&SdlVideoSubsystem{
        &self.inner
    }

    /// 窗口构建器（对接wgpu窗口必备）
    pub fn window(&self, title: &str, width: u32, height: u32) -> WindowBuilder {
        WindowBuilder::new( &self.inner,title, width, height)
    }

    // -------- 显示器/主屏信息（游戏分辨率、全屏、居中用）--------
    pub fn displays(&self) -> Result<Vec<Display>, Error> { self.inner.displays()}

    pub fn get_primary_display(&self) -> Result<Display, Error> {self.inner.get_primary_display()}

    // -------- 屏保控制（游戏强制保留）--------
    pub fn is_screen_saver_enabled(&self) -> bool {self.inner.is_screen_saver_enabled()}

    pub fn enable_screen_saver(&self) {self.inner.enable_screen_saver()}

    pub fn disable_screen_saver(&self) {self.inner.disable_screen_saver()}

    // -------- 可选：视频驱动（调试用，可删）--------
    pub fn current_video_driver(&self) -> &'static str { self.inner.current_video_driver()}

    pub fn num_video_drivers(&self) -> Result<i32, Error> { self.inner.num_video_drivers()}

    // -------- 可选：系统主题（UI配色用，无UI则删除）--------
    pub fn get_system_theme(&self) -> SystemTheme {SdlVideoSubsystem::get_system_theme()}
}