//! pygame.sprite 对应物——**Godot 式重设计**（细案：`reference/pygame
//! sprite 的 Godot 式重设计细案.md`；不做 1:1 兼容）。
//!
//! 取 Godot 之骨：Node2D 式 transform/visible/z、Sprite2D 的
//! centered/offset/flip/alpha、AnimatedSprite2D 的 frames+fps、分组标签。
//! 去解释器之形：❌场景树父子 ❌信号系统 ❌_process 回调（违 poll 哲学）
//! ——Rust 形态 = **池化 Group + 世代 id 句柄**，用户在自己的循环里
//! tick/查询，绘制经会话批量入 batch（按 z,y 排序）。
//!
//! 碰撞不内置信号：`rect(id)` / `mask::Mask` 组合由用户判断
//! （分层：sprite 管组织，Rect/Mask 管判定）。

use crate::pygame::render::{Texture, DrawTarget, Surface};
use crate::pygame::Rect;

/// 精灵句柄（世代 id——remove 后旧句柄自动失效）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpriteId {
    index: u32,
    generation: u64,
}

/// 精灵描述（builder 风格构造；数据全量 owned——Texture 廉价克隆句柄）
#[derive(Clone)]
pub struct Sprite {
    frames: Vec<Texture>,
    fps: f32,
    frame: usize,
    frame_t: f32,
    x: i32,
    y: i32,
    rotation: f32,
    scale_x: f32,
    scale_y: f32,
    flip_x: bool,
    flip_y: bool,
    alpha: f32,
    centered: bool,
    offset: (i32, i32),
    visible: bool,
    z: i32,
    tags: Vec<&'static str>,
}

impl Sprite {
    /// 单纹理精灵（对位 Godot Sprite2D；centered 默认 true）
    pub fn new(surface: &Surface) -> Self {
        Self {
            frames: vec![surface.texture.clone()],
            fps: 0.0,
            frame: 0,
            frame_t: 0.0,
            x: 0,
            y: 0,
            rotation: 0.0,
            scale_x: 1.0,
            scale_y: 1.0,
            flip_x: false,
            flip_y: false,
            alpha: 1.0,
            centered: true,
            offset: (0, 0),
            visible: true,
            z: 0,
            tags: Vec::new(),
        }
    }

    /// 多帧精灵（对位 Godot AnimatedSprite2D；`fps` = 帧率，0 = 不自动播）
    pub fn with_frames(surfaces: &[&Surface], fps: f32) -> Self {
        assert!(!surfaces.is_empty(), "Sprite 至少需要一帧");
        Self {
            frames: surfaces.iter().map(|s| s.texture.clone()).collect(),
            fps,
            frame: 0,
            frame_t: 0.0,
            x: 0,
            y: 0,
            rotation: 0.0,
            scale_x: 1.0,
            scale_y: 1.0,
            flip_x: false,
            flip_y: false,
            alpha: 1.0,
            centered: true,
            offset: (0, 0),
            visible: true,
            z: 0,
            tags: Vec::new(),
        }
    }

    /// 位置（centered=true 时为精灵中心，false 时为左上角）
    pub fn at(mut self, x: i32, y: i32) -> Self {
        self.x = x;
        self.y = y;
        self
    }

    /// 统一缩放
    pub fn scale(mut self, factor: f32) -> Self {
        self.scale_x = factor;
        self.scale_y = factor;
        self
    }

    /// xy 独立缩放
    pub fn scale_xy(mut self, fx: f32, fy: f32) -> Self {
        self.scale_x = fx;
        self.scale_y = fy;
        self
    }

    /// 旋转（度，逆时针）
    pub fn rotated(mut self, degrees: f32) -> Self {
        self.rotation = degrees;
        self
    }

    /// 水平镜像
    pub fn flip_x(mut self) -> Self {
        self.flip_x = true;
        self
    }

    /// 垂直镜像
    pub fn flip_y(mut self) -> Self {
        self.flip_y = true;
        self
    }

    /// 整体透明度（0.0~1.0，乘制）
    pub fn alpha(mut self, a: f32) -> Self {
        self.alpha = a.clamp(0.0, 1.0);
        self
    }

    /// 锚点语义：true（默认）= 位置即精灵中心；false = 左上角
    pub fn centered(mut self, centered: bool) -> Self {
        self.centered = centered;
        self
    }

    /// 绘制偏移（在锚点基础上叠加）
    pub fn offset(mut self, x: i32, y: i32) -> Self {
        self.offset = (x, y);
        self
    }

    /// 绘制层（大者后画、覆盖在前；同 z 按 y 升序）
    pub fn z(mut self, z: i32) -> Self {
        self.z = z;
        self
    }

    /// 分组标签（可多次调用叠多个标签）
    pub fn tag(mut self, tag: &'static str) -> Self {
        self.tags.push(tag);
        self
    }

    /// 可见性
    pub fn visible(mut self, visible: bool) -> Self {
        self.visible = visible;
        self
    }

    /// 无纹理测试构造（cfg(test)：容器/排序/动画推进逻辑的零 GPU 测试用）
    #[cfg(test)]
    pub(crate) fn dummy() -> Self {
        Self {
            frames: Vec::new(),
            fps: 0.0,
            frame: 0,
            frame_t: 0.0,
            x: 0,
            y: 0,
            rotation: 0.0,
            scale_x: 1.0,
            scale_y: 1.0,
            flip_x: false,
            flip_y: false,
            alpha: 1.0,
            centered: true,
            offset: (0, 0),
            visible: true,
            z: 0,
            tags: Vec::new(),
        }
    }

    /// 当前帧纹理（动画推进后变化）
    pub fn current(&self) -> &Texture {
        &self.frames[self.frame.min(self.frames.len() - 1)]
    }
}

struct Slot {
    sprite: Sprite,
    generation: u64,
}

/// 精灵容器（池化 + 世代 id；绘制按 (z, y) 排序批量入 batch）
#[derive(Default)]
pub struct Group {
    slots: Vec<Option<Slot>>,
    generations: Vec<u64>,
    free: Vec<usize>,
}

impl Group {
    pub fn new() -> Self {
        Self::default()
    }

    /// 插入精灵，返回句柄
    pub fn insert(&mut self, sprite: Sprite) -> SpriteId {
        if let Some(index) = self.free.pop() {
            let generation = self.generations[index];
            self.slots[index] = Some(Slot { sprite, generation });
            SpriteId { index: index as u32, generation }
        } else {
            let index = self.slots.len();
            let generation = 0;
            self.slots.push(Some(Slot { sprite, generation }));
            self.generations.push(generation as u64);
            SpriteId { index: index as u32, generation }
        }
    }

    /// 移除（旧句柄此后全部失效）
    pub fn remove(&mut self, id: SpriteId) -> bool {
        if !self.is_alive(id) {
            return false;
        }
        self.slots[id.index as usize] = None;
        self.generations[id.index as usize] += 1;
        self.free.push(id.index as usize);
        true
    }

    /// 句柄是否仍有效
    pub fn is_alive(&self, id: SpriteId) -> bool {
        self.slots
            .get(id.index as usize)
            .zip(self.generations.get(id.index as usize))
            .map(|(slot, g)| slot.is_some() && *g == id.generation)
            .unwrap_or(false)
    }

    /// 精灵数
    pub fn count(&self) -> usize {
        self.slots.iter().filter(|s| s.is_some()).count()
    }

    pub fn is_empty(&self) -> bool {
        self.count() == 0
    }

    fn slot(&self, id: SpriteId) -> Option<&Slot> {
        if self.is_alive(id) {
            self.slots[id.index as usize].as_ref()
        } else {
            None
        }
    }

    fn slot_mut(&mut self, id: SpriteId) -> Option<&mut Slot> {
        if self.is_alive(id) {
            self.slots[id.index as usize].as_mut()
        } else {
            None
        }
    }

    // ── 位置/状态操作 ──
    pub fn position(&self, id: SpriteId) -> Option<(i32, i32)> {
        self.slot(id).map(|s| (s.sprite.x, s.sprite.y))
    }
    pub fn set_position(&mut self, id: SpriteId, x: i32, y: i32) {
        if let Some(s) = self.slot_mut(id) {
            s.sprite.x = x;
            s.sprite.y = y;
        }
    }
    pub fn move_by(&mut self, id: SpriteId, dx: i32, dy: i32) {
        if let Some(s) = self.slot_mut(id) {
            s.sprite.x += dx;
            s.sprite.y += dy;
        }
    }
    pub fn set_visible(&mut self, id: SpriteId, visible: bool) {
        if let Some(s) = self.slot_mut(id) {
            s.sprite.visible = visible;
        }
    }
    pub fn set_z(&mut self, id: SpriteId, z: i32) {
        if let Some(s) = self.slot_mut(id) {
            s.sprite.z = z;
        }
    }
    pub fn set_rotation(&mut self, id: SpriteId, degrees: f32) {
        if let Some(s) = self.slot_mut(id) {
            s.sprite.rotation = degrees;
        }
    }
    pub fn set_scale(&mut self, id: SpriteId, factor: f32) {
        if let Some(s) = self.slot_mut(id) {
            s.sprite.scale_x = factor;
            s.sprite.scale_y = factor;
        }
    }

    /// 分组查询（返回句柄列表；插入序）
    pub fn with_tag(&self, tag: &str) -> Vec<SpriteId> {
        self.slots
            .iter()
            .enumerate()
            .filter(|(_, slot)| {
                slot.as_ref()
                    .map(|s| s.sprite.tags.iter().any(|t| *t == tag))
                    .unwrap_or(false)
            })
            .map(|(i, _)| SpriteId { index: i as u32, generation: self.generations[i] })
            .collect()
    }

    /// 推进动画帧（每帧一次；delta = 秒）
    pub fn tick(&mut self, delta: f32) {
        for slot in self.slots.iter_mut().flatten() {
            let s = &mut slot.sprite;
            if s.fps <= 0.0 || s.frames.len() <= 1 {
                continue;
            }
            s.frame = advance_frame(s.frame, &mut s.frame_t, s.frames.len(), s.fps, delta);
        }
    }

    /// 绘制全部可见精灵（按 (z, y, 插入序) 排序；活句柄外的忽略）
    pub fn draw(&self, target: &impl DrawTarget) {
        for slot in self.draw_order() {
            let s = &slot.sprite;
            if s.frames.is_empty() {
                continue; // 无纹理（dummy/未加载）跳过
            }
            let tex = s.current();
            let (tw, th) = (tex.size().0 as f32, tex.size().1 as f32);
            let (w, h) = (tw * s.scale_x, th * s.scale_y);
            let (cx, cy) = if s.centered {
                (s.x as f32 + s.offset.0 as f32, s.y as f32 + s.offset.1 as f32)
            } else {
                (
                    s.x as f32 + w / 2.0 + s.offset.0 as f32,
                    s.y as f32 + h / 2.0 + s.offset.1 as f32,
                )
            };
            let (c, sn) = (
                s.rotation.to_radians().cos(),
                s.rotation.to_radians().sin(),
            );
            let corner = |ox: f32, oy: f32| {
                [cx + ox * c - oy * sn, cy + ox * sn + oy * c]
            };
            let (hw, hh) = (w / 2.0, h / 2.0);
            let corners = [
                corner(-hw, -hh),
                corner(hw, -hh),
                corner(hw, hh),
                corner(-hw, hh),
            ];
            let (u0, v0, u1, v1) = (
                if s.flip_x { 1.0 } else { 0.0 },
                if s.flip_y { 1.0 } else { 0.0 },
                if s.flip_x { 0.0 } else { 1.0 },
                if s.flip_y { 0.0 } else { 1.0 },
            );
            let uvs = [[u0, v0], [u1, v0], [u1, v1], [u0, v1]];
            let color = [1.0, 1.0, 1.0, s.alpha];
            target.with_batch(|b| b.push_quad_corners(corners, uvs, color, Some(tex)));
        }
    }

    /// 绘制顺序（纯逻辑）：可见精灵按 (z, y, 插入序) 排序
    pub(crate) fn draw_order(&self) -> Vec<&Slot> {
        let mut order: Vec<(i32, i32, u32, &Slot)> = self
            .slots
            .iter()
            .enumerate()
            .filter_map(|(i, slot)| {
                let slot = slot.as_ref()?;
                if !slot.sprite.visible {
                    return None; // 隐藏不画
                }
                Some((slot.sprite.z, slot.sprite.y, i as u32, slot))
            })
            .collect();
        order.sort_by_key(|(z, y, seq, _)| (*z, *y, *seq));
        order.into_iter().map(|(_, _, _, slot)| slot).collect()
    }

    /// 绘制顺序的位置快照（零 GPU 测试观察口）
    #[cfg(test)]
    pub(crate) fn order_positions(&self) -> Vec<(i32, i32)> {
        self.draw_order().iter().map(|s| (s.sprite.x, s.sprite.y)).collect()
    }
}

/// 动画帧推进（纯逻辑）：返回新帧号
fn advance_frame(
    frame: usize,
    frame_t: &mut f32,
    frame_count: usize,
    fps: f32,
    delta: f32,
) -> usize {
    *frame_t += delta;
    let step = 1.0 / fps;
    let mut f = frame;
    while *frame_t >= step {
        *frame_t -= step;
        f = (f + 1) % frame_count;
    }
    f
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_remove_and_generation() {
        let mut g = Group::new();
        let a = g.insert(Sprite::dummy().at(100, 100));
        assert!(g.is_alive(a));
        assert_eq!(g.count(), 1);
        let b = g.insert(Sprite::dummy());
        assert!(g.remove(a));
        assert!(!g.is_alive(a));
        assert!(!g.remove(a), "重复移除返回 false");
        assert!(g.is_alive(b), "兄弟句柄不受影响");
        assert_eq!(g.count(), 1);
    }

    #[test]
    fn draw_order_by_z_then_y() {
        let mut g = Group::new();
        let a = g.insert(Sprite::dummy().at(100, 100).z(1));
        let b = g.insert(Sprite::dummy().at(300, 100).z(0));
        assert_eq!(
            g.order_positions(),
            vec![(300, 100), (100, 100)],
            "z 小者先画"
        );
        g.set_z(b, 2);
        assert_eq!(
            g.order_positions(),
            vec![(100, 100), (300, 100)],
            "改 z 后顺序翻转"
        );
        let _ = (a, b);
    }

    #[test]
    fn tags_visibility_and_tick() {
        let mut g = Group::new();
        let _e1 = g.insert(Sprite::dummy().tag("enemy"));
        let _e2 = g.insert(Sprite::dummy().tag("enemy"));
        let _p = g.insert(Sprite::dummy().tag("player"));
        assert_eq!(g.with_tag("enemy").len(), 2);
        assert_eq!(g.with_tag("player").len(), 1);

        // 隐藏精灵不参与绘制顺序
        let _hidden = g.insert(Sprite::dummy().visible(false));
        assert_eq!(g.order_positions().len(), 3, "隐藏者被排除");

        // 动画推进（纯逻辑）：10fps、累计 0.1s 翻一帧
        let mut frame = 0usize;
        let mut ft = 0.0f32;
        frame = advance_frame(frame, &mut ft, 2, 10.0, 0.05);
        assert_eq!(frame, 0, "半帧不翻页");
        frame = advance_frame(frame, &mut ft, 2, 10.0, 0.05);
        assert_eq!(frame, 1, "累计一帧翻页");
    }
}
