use std::ffi::{c_int, c_uint, c_void};
use sdl3::{Error, EventSubsystem as SdlEventSubsystem, Sdl, event::{Event, EventSender, EventType, EventWatch, EventWatchCallback}};

/// Rust 内部专用：对接 SDL UserEvent 的通用事件结构
/// 仅在 Rust 层使用，不直接暴露给 Python
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct CustomEvent {
    pub type_id: c_uint,
    pub code: c_int,
    pub data1: *mut c_void,
    pub data2: *mut c_void,
}

pub struct EventSubsystem {
    inner: SdlEventSubsystem,
}

impl EventSubsystem {
    pub fn new(sdl:&Sdl) -> Self {
        let inner = sdl.event().expect("EventSubsystem init failed.");
        Self { inner }
    }

    
    // 基础队列操作
    pub fn flush_event(&self, event_type: EventType) {
        self.inner.flush_event(event_type);
    }

    pub fn flush_events(&self, min_type: u32, max_type: u32) {
        self.inner.flush_events(min_type, max_type);
    }

    pub fn peek_events<B>(&self, max_amount: u32) -> B
    where
        B: FromIterator<Event>,
    {
        self.inner.peek_events(max_amount)
    }

    pub fn push_event(&self, event: Event) -> Result<(), Error> {
        self.inner.push_event(event)
    }


    /*
    // 注册，只需要保证成功
    event_subsys.register_custom_event::<CustomEvent>()?;
    // 推送自定义事件，框架内部自动查表获取对应event_id
    push_custom_event(CustomEvent { ... })?;
    */
    pub unsafe  fn register_custom_event(&self) -> Result<(), Error>{
        self.inner.register_custom_event::<CustomEvent>()
    }

    // 注册 SDL 自定义事件ID
    #[inline(always)]
    pub unsafe fn register_event(&self) -> Result<u32, Error> {
        unsafe { self.inner.register_event() }
    }

    pub unsafe fn register_events(&self, nr: u32) -> Result<Vec<u32>, Error> {
        unsafe { self.inner.register_events(nr) }
    }

    /// 内部：推送自定义事件给 SDL
    #[inline(always)]
    pub fn push_custom_event(&self, event: CustomEvent) -> Result<(), Error> {
        let sdl_ev = Event::User {
            timestamp: 0,
            window_id: 0,
            type_: event.type_id,
            code: event.code,
            data1: event.data1,
            data2: event.data2,
        };
        self.push_event(sdl_ev)
    }

    // 通用附属接口
    pub fn event_sender(&self) -> EventSender {
        self.inner.event_sender()
    }

    pub fn add_event_watch<CB: EventWatchCallback>(&self, callback: CB) -> EventWatch<CB> {
        self.inner.add_event_watch(callback)
    }

    // 全局事件开关
    pub fn set_event_enabled(event_type: EventType, enabled: bool) {
        SdlEventSubsystem::set_event_enabled(event_type, enabled);
    }

    pub fn event_enabled(event_type: EventType) -> bool {
        SdlEventSubsystem::event_enabled(event_type)
    }
}