//! 输入视图层：`StarfishView`（触摸 → `Touch` 事件 + 合成鼠标）与
//! `StarfishField`（隐藏 UITextField：软键盘文本/删除/硬件按键）。
//! Input views: `StarfishView` (touches → `Touch` events + synthesized
//! mouse) and `StarfishField` (a hidden UITextField: soft-keyboard text,
//! deletion and hardware key presses).
//!
//! 为什么是隐藏 UITextField 而非实现 `UIKeyInput`：objc2-ui-kit 0.3 未
//! 生成该协议（只有字符串常量），而 UITextField 天生符合 `UIKeyInput`
//! ——子类拦截 `insertText:` / `deleteBackward` 且**不调 super**，字段
//! 内文本恒空，`hasText` 恒 `NO`（SDL 同款套路）。
//! Why a hidden UITextField instead of implementing `UIKeyInput`:
//! objc2-ui-kit 0.3 does not generate that protocol (only the string
//! constants), while UITextField already conforms to `UIKeyInput`. The
//! subclass intercepts `insertText:` / `deleteBackward` **without calling
//! super**, so the field stays empty and `hasText` is always `NO` (the
//! same trick SDL uses).

use std::sync::Mutex;

use objc2::define_class;
use objc2::rc::Retained;
use objc2::runtime::{Bool, NSObjectProtocol};
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_core_foundation::CGRect;
use objc2_foundation::{NSSet, NSString};
use objc2_ui_kit::{
    UIEvent, UIPress, UIPressesEvent, UITextField, UITouch, UIView,
};

use super::push_event;
use super::translate;
use crate::event::{Event, MouseButton, TouchPhase};

/// 合成鼠标跟随的"主手指"：`UITouch` 对象地址（一次触控生命周期内稳定、
/// 全局唯一）。SDL 惯例：鼠标跟随第一根落下的手指。
/// The "primary finger" the synthesized mouse follows: the `UITouch`
/// object address (stable for the lifetime of one touch, globally unique).
/// SDL convention: the mouse follows the first finger that went down.
static PRIMARY_FINGER: Mutex<Option<u64>> = Mutex::new(None);

/// 触摸对象标识（指针地址）。
/// Touch identity (pointer address).
fn touch_id(touch: &UITouch) -> u64 {
    std::ptr::from_ref(touch) as u64
}

/// 单根手指的阶段投递：先发 `Touch`，再按主手指合成鼠标。
/// `scale` = 视图 `contentScaleFactor`（点 → 物理像素）。
/// Per-finger phase delivery: emit `Touch` first, then synthesize the
/// mouse from the primary finger. `scale` = the view's
/// `contentScaleFactor` (points → physical pixels).
fn deliver_touch(scale: f64, touch: &UITouch, phase: TouchPhase) {
    // locationInView(nil) = 窗口坐标（点）；视图铺满窗口，等价客户区。
    // 物理像素 = 点 × scale（与模块文档的坐标定约一致）。
    // locationInView(nil) = window coordinates (points); the view fills
    // the window, so this equals the client area. Physical pixels =
    // points × scale (per the module's coordinate contract).
    let location = touch.locationInView(None);
    let x = location.x as f64 * scale;
    let y = location.y as f64 * scale;
    let finger = touch_id(touch);

    push_event(Event::Touch {
        phase,
        finger,
        x,
        y,
    });

    let mut primary = PRIMARY_FINGER
        .lock()
        .expect("PRIMARY_FINGER poisoned / PRIMARY_FINGER 锁中毒");
    match phase {
        TouchPhase::Started => {
            if primary.is_none() {
                *primary = Some(finger);
                push_event(Event::MouseMoved { x, y });
                push_event(Event::MouseDown {
                    button: MouseButton::Left,
                });
            }
        }
        TouchPhase::Moved => {
            if *primary == Some(finger) {
                push_event(Event::MouseMoved { x, y });
            }
        }
        TouchPhase::Ended | TouchPhase::Cancelled => {
            if *primary == Some(finger) {
                *primary = None;
                push_event(Event::MouseUp {
                    button: MouseButton::Left,
                });
            }
        }
    }
}

/// 硬件键盘按键（UIPress）→ Key 按下/抬起 + 文本。挂在视图与字段两个
/// responder 上：字段是第一响应者时事件到它为止，否则沿窗口链落到视图
/// ——两处都实现即全覆盖，且不会双份（链在第一个实现者处停止）。
/// Hardware key presses (UIPress) → Key down/up + text. Installed on both
/// responders: when the field is first responder the chain stops there,
/// otherwise it falls through the window chain to the view — full coverage
/// without double delivery (the chain stops at the first implementer).
unsafe fn deliver_presses(presses: &NSSet<UIPress>, pressed: bool) {
    for press in presses.to_vec() {
        let Some(mtm) = MainThreadMarker::new() else {
            return; // presses 事件只发生在主线程 / presses only happen on the main thread
        };
        let Some(key) = press.key(mtm) else {
            continue; // 非键盘按压（按钮等）/ not a keyboard press (buttons etc.)
        };
        let modifiers = translate::modifiers_from_flags(key.modifierFlags());
        if let Some(key_code) = translate::hid_usage_to_key_code(key.keyCode()) {
            if pressed {
                push_event(Event::KeyDown {
                    key: key_code,
                    modifiers,
                    repeat: false,
                });
            } else {
                push_event(Event::KeyUp {
                    key: key_code,
                    modifiers,
                });
            }
        }
        // 硬件键盘的文本通道（普通字符；特殊 UIKeyInput* 字符串与控制
        // 字符由消费方过滤）。
        // Hardware text channel (plain characters; special `UIKeyInput*`
        // strings and control characters are filtered by the consumer).
        if pressed {
            let characters = key.characters().to_string();
            if !characters.is_empty() {
                push_event(Event::TextInput {
                    text: characters,
                });
            }
        }
    }
}

define_class!(
    /// 全屏透明输入视图：铺满窗口、只吃触摸（渲染仍走 window.layer，见
    /// `raw_handle`——本视图不参与绘制）。
    /// The full-screen transparent input view: fills the window and only
    /// consumes touches (rendering still goes through `window.layer`, see
    /// `raw_handle` — this view never draws).
    #[unsafe(super(UIView))]
    #[thread_kind = MainThreadOnly]
    #[name = "StarfishView"]
    pub(super) struct StarfishView;

    unsafe impl NSObjectProtocol for StarfishView {}

    impl StarfishView {
        #[unsafe(method(touchesBegan:withEvent:))]
        unsafe fn touches_began(&self, touches: &NSSet<UITouch>, _event: Option<&UIEvent>) {
            self.deliver_touches(&touches.to_vec(), TouchPhase::Started);
        }

        #[unsafe(method(touchesMoved:withEvent:))]
        unsafe fn touches_moved(&self, touches: &NSSet<UITouch>, _event: Option<&UIEvent>) {
            self.deliver_touches(&touches.to_vec(), TouchPhase::Moved);
        }

        #[unsafe(method(touchesEnded:withEvent:))]
        unsafe fn touches_ended(&self, touches: &NSSet<UITouch>, _event: Option<&UIEvent>) {
            self.deliver_touches(&touches.to_vec(), TouchPhase::Ended);
        }

        #[unsafe(method(touchesCancelled:withEvent:))]
        unsafe fn touches_cancelled(&self, touches: &NSSet<UITouch>, _event: Option<&UIEvent>) {
            self.deliver_touches(&touches.to_vec(), TouchPhase::Cancelled);
        }

        // 硬件按键的兜底落点（无第一响应者时沿窗口链到这里）。
        // Fallback for hardware presses (no first responder → window chain
        // ends up here).
        #[unsafe(method(pressesBegan:withEvent:))]
        unsafe fn presses_began(&self, presses: &NSSet<UIPress>, _event: Option<&UIPressesEvent>) {
            // SAFETY: unsafe fn 体内调用 unsafe fn（edition 2024 需显式块）。
            // SAFETY: calling an unsafe fn inside an unsafe fn (edition
            // 2024 requires the explicit block).
            unsafe { deliver_presses(presses, true) };
        }

        #[unsafe(method(pressesEnded:withEvent:))]
        unsafe fn presses_ended(&self, presses: &NSSet<UIPress>, _event: Option<&UIPressesEvent>) {
            unsafe { deliver_presses(presses, false) };
        }
    }
);

// 辅助方法放宏外的普通 impl 块（define_class! 的 impl 段只收 unsafe 方法）。
// Helper methods live in a plain impl block outside the macro (the
// define_class! impl section only accepts unsafe methods).
impl StarfishView {
    /// 装配入口（frame = 窗口 bounds）。
    /// Construction entry (frame = the window's bounds).
    pub(super) fn new(mtm: MainThreadMarker, frame: CGRect) -> Retained<Self> {
        // SAFETY: `initWithFrame:` 是该类的指定初始化器（继承自
        // UIView，method_family = init）；mtm 保证主线程。
        // SAFETY: `initWithFrame:` is the designated initializer
        // (inherited from UIView, method_family = init); mtm
        // guarantees the main thread.
        unsafe { objc2::msg_send![Self::alloc(mtm), initWithFrame: frame] }
    }

    /// 本视图全部触摸的阶段投递（scale 取自视图自身）。
    /// Phase delivery for all touches of this view (scale from the
    /// view itself).
    fn deliver_touches(&self, touches: &[Retained<UITouch>], phase: TouchPhase) {
        let scale = self.contentScaleFactor() as f64;
        for touch in touches {
            deliver_touch(scale, touch, phase);
        }
    }
}

define_class!(
    /// 隐藏键盘字段：拦截软键盘输入（不调 super，字段恒空），同时承接
    /// 硬件按键（作为第一响应者时）。
    /// The hidden keyboard field: intercepts soft-keyboard input (no
    /// super call, the field stays empty) and also takes hardware presses
    /// (when it is first responder).
    #[unsafe(super(UITextField))]
    #[thread_kind = MainThreadOnly]
    #[name = "StarfishField"]
    pub(super) struct StarfishField;

    unsafe impl NSObjectProtocol for StarfishField {}

    impl StarfishField {
        // ── UIKeyInput 拦截（UITextField 已符合该协议，这里覆写）──
        // ── UIKeyInput interception (UITextField already conforms; we
        //    override) ──

        #[unsafe(method(insertText:))]
        unsafe fn insert_text(&self, text: &NSString) {
            for event in translate::insert_text_to_events(&text.to_string()) {
                push_event(event);
            }
            // 不调 super：字段文本恒空，`hasText` 恒 NO。
            // No super call: the field's text stays empty, `hasText` NO.
        }

        #[unsafe(method(deleteBackward))]
        unsafe fn delete_backward(&self) {
            // Backspace 以 Key 按下/抬起对投递（消费方按物理键处理）。
            // Backspace as a Key down/up pair (consumers handle it as a
            // physical key).
            push_event(Event::KeyDown {
                key: crate::event::KeyCode::Backspace,
                modifiers: crate::event::Modifiers::default(),
                repeat: false,
            });
            push_event(Event::KeyUp {
                key: crate::event::KeyCode::Backspace,
                modifiers: crate::event::Modifiers::default(),
            });
        }

        #[unsafe(method(hasText))]
        unsafe fn has_text(&self) -> Bool {
            Bool::NO
        }

        // 硬件按键（字段为第一响应者时的主落点）。
        // Hardware presses (the primary landing spot when the field is
        // first responder).
        #[unsafe(method(pressesBegan:withEvent:))]
        unsafe fn presses_began(&self, presses: &NSSet<UIPress>, _event: Option<&UIPressesEvent>) {
            // SAFETY: unsafe fn 体内调用 unsafe fn（edition 2024 需显式块）。
            // SAFETY: calling an unsafe fn inside an unsafe fn (edition
            // 2024 requires the explicit block).
            unsafe { deliver_presses(presses, true) };
        }

        #[unsafe(method(pressesEnded:withEvent:))]
        unsafe fn presses_ended(&self, presses: &NSSet<UIPress>, _event: Option<&UIPressesEvent>) {
            unsafe { deliver_presses(presses, false) };
        }
    }
);

// 辅助方法放宏外（同 [`StarfishView`]）。
// Helper methods outside the macro (same as [`StarfishView`]).
impl StarfishField {
    /// 零尺寸字段（隐藏但可成为第一响应者）。
    /// The zero-sized field (hidden yet can become first responder).
    pub(super) fn new(mtm: MainThreadMarker) -> Retained<Self> {
        // SAFETY: 同 `StarfishView::new`（指定初始化器，主线程）。
        // SAFETY: same as `StarfishView::new` (designated initializer,
        // main thread).
        unsafe {
            let this: Retained<Self> =
                objc2::msg_send![Self::alloc(mtm), initWithFrame: CGRect::ZERO];
            this
        }
    }

    /// 显示/隐藏软键盘（`becomeFirstResponder` 触发系统键盘）。
    /// Show/hide the soft keyboard (`becomeFirstResponder` triggers it).
    pub(super) fn set_visible(&self, visible: bool) -> bool {
        if visible {
            self.becomeFirstResponder()
        } else {
            self.resignFirstResponder()
        }
    }
}
