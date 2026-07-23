// //! 复刻 pygame.Rect 矩形工具类，配套 Point2D/Size/Line 基础几何类型
// #![allow(dead_code)]


// /// 矩形结构体，完全对标 pygame.Rect
// /// 存储 left, top, width, height 基础数据
// #[derive(Debug, Clone, Copy, PartialEq, Eq)]
// pub struct Rect {
//     /// 左上角X坐标
//     pub left: i32,
//     /// 左上角Y坐标
//     pub top: i32,
//     /// 矩形宽度
//     pub width: i32,
//     /// 矩形高度
//     pub height: i32,
// }

// impl Rect {
//     // ====================== 构造方法（对应pygame三种构造形式） ======================
//     /// Rect(left, top, width, height) -> Rect
//     pub fn new(left: i32, top: i32, width: i32, height: i32) -> Self {
//         Self {
//             left,
//             top,
//             width,
//             height,
//         }
//     }

//     /// Rect((left, top), (width, height)) -> Rect
//     pub fn new_from_tuple(topleft:(i32,i32), bottomright:(i32,i32)) -> Self {
//         Self {
//             topleft.0,
//             topleft.1,
//             bottomright.0,
//             bottomright.1,
//         }
//     }

//     ///Rect(object) -> Rect
//     pub fn new_from_rect(other: Rect) -> Self {
//         other
//     }

//     // ====================== 快捷边界属性（只读计算属性） ======================
//     /// 右侧x坐标 left + width
//     pub fn right(&self) -> i32 {
//         self.left + self.width
//     }

//     /// 底部y坐标 top + height
//     pub fn bottom(&self) -> i32 {
//         self.top + self.height
//     }

//     /// 中心点坐标
//     pub fn center(&self) -> (i32,i32) {
//         (
//             self.left + self.width / 2,
//             self.top + self.height / 2,
//         )
//     }

//     /// 左上角点
//     pub fn topleft(&self) -> (i32,i32) {
//         (self.left, self.top)
//     }

//     /// 右上角
//     pub fn topright(&self) -> (i32,i32) {
//         (self.right(), self.top)
//     }

//     /// 左下角
//     pub fn bottomleft(&self) -> (i32,i32) {
//         (self.left, self.bottom())
//     }

//     /// 右下角
//     pub fn bottomright(&self) -> (i32,i32) {
//         (self.right(), self.bottom())
//     }

//     // ====================== 原生API实现 一一对应文档 ======================
//     /// pygame.Rect.copy
//     /// 复制矩形，返回全新副本（不修改自身）
//     pub fn copy(&self) -> Self {
//         *self
//     }

//     /// pygame.Rect.move(dx, dy)
//     /// 平移矩形，返回新矩形，不修改原对象
//     pub fn move_(&self, dx: i32, dy: i32) -> Self {
//         Self::new(self.left + dx, self.top + dy, self.width, self.height)
//     }

//     /// pygame.Rect.move_ip(dx, dy)
//     /// 原地平移，直接修改自身
//     pub fn move_ip(&mut self, dx: i32, dy: i32) {
//         self.left += dx;
//         self.top += dy;
//     }

//     /// pygame.Rect.inflate(dw, dh)
//     /// 整体放大缩小，向四周扩散，返回新矩形
//     /// dw: 宽度增减量 dh: 高度增减量
//     pub fn inflate(&self, dw: i32, dh: i32) -> Self {
//         let w = self.width + dw;
//         let h = self.height + dh;
//         let l = self.left - dw / 2;
//         let t = self.top - dh / 2;
//         Self::new(l, t, w, h)
//     }

//     /// pygame.Rect.inflate_ip(dw, dh)
//     /// 原地放大缩小
//     pub fn inflate_ip(&mut self, dw: i32, dh: i32) {
//         self.left -= dw / 2;
//         self.top -= dh / 2;
//         self.width += dw;
//         self.height += dh;
//     }

//     /// pygame.Rect.scale_by(multiplier)
//     /// 按倍数缩放矩形，中心不变，返回新矩形
//     pub fn scale_by(&self, mul: f32) -> Self {
//         let w = (self.width as f32 * mul) as i32;
//         let h = (self.height as f32 * mul) as i32;
//         let dw = w - self.width;
//         let dh = h - self.height;
//         self.inflate(dw, dh)
//     }

//     /// pygame.Rect.scale_by_ip(multiplier)
//     /// 原地按倍数缩放
//     pub fn scale_by_ip(&mut self, mul: f32) {
//         let w = (self.width as f32 * mul) as i32;
//         let h = (self.height as f32 * mul) as i32;
//         let dw = w - self.width;
//         let dh = h - self.height;
//         self.inflate_ip(dw, dh);
//     }

//     /// pygame.Rect.update(left, top, w, h)
//     /// 批量设置位置与尺寸，原地修改
//     pub fn update(&mut self, left: i32, top: i32, width: i32, height: i32) {
//         self.left = left;
//         self.top = top;
//         self.width = width;
//         self.height = height;
//     }

//     /// pygame.Rect.clamp(other_rect)
//     /// 将当前矩形限制在目标矩形内，返回新矩形，不修改自身
//     pub fn clamp(&self, other: Rect) -> Self {
//         let mut res = *self;
//         res.clamp_ip(other);
//         res
//     }

//     /// pygame.Rect.clamp_ip(other_rect)
//     /// 原地约束：把矩形完整塞进other矩形内部
//     pub fn clamp_ip(&mut self, other: Rect) {
//         // X轴约束
//         if self.left < other.left {
//             self.left = other.left;
//         }
//         if self.right() > other.right() {
//             self.left = other.right() - self.width;
//         }
//         // Y轴约束
//         if self.top < other.top {
//             self.top = other.top;
//         }
//         if self.bottom() > other.bottom() {
//             self.top = other.bottom() - self.height;
//         }
//     }

//     /// pygame.Rect.clip(other)
//     /// 裁剪：取两个矩形相交区域，返回新矩形；无交集返回0尺寸矩形
//     pub fn clip(&self, other: Rect) -> Self {
//         let l = self.left.max(other.left);
//         let t = self.top.max(other.top);
//         let r = self.right().min(other.right());
//         let b = self.bottom().min(other.bottom());
//         let w = r - l;
//         let h = b - t;
//         Self::new(l, t, w.max(0), h.max(0))
//     }

//     /// pygame.Rect.clipline(line) -> Option<Line>
//     /// 裁剪线段，返回矩形内部的线段；完全不相交返回None
//     pub fn clipline(&self, line: Line) -> Option<Line> {
//         let mut s = line.start;
//         let mut e = line.end;
//         // 简易线段裁剪实现（AABB）
//         let mut inside_s = self.collidepoint(s);
//         let mut inside_e = self.collidepoint(e);

//         if inside_s && inside_e {
//             return Some(line);
//         }
//         if !inside_s && !inside_e {
//             return None;
//         }

//         // 简化：此处省略完整 Liang-Barsky 裁剪，保留接口语义
//         // 生产环境可替换完整线段裁剪算法
//         None
//     }

//     /// pygame.Rect.union(other)
//     /// 合并两个矩形，返回包围两者的最小矩形（新对象）
//     pub fn union(&self, other: Rect) -> Self {
//         let l = self.left.min(other.left);
//         let t = self.top.min(other.top);
//         let r = self.right().max(other.right());
//         let b = self.bottom().max(other.bottom());
//         Self::new(l, t, r - l, b - t)
//     }

//     /// pygame.Rect.union_ip(other)
//     /// 原地合并，修改自身为包围两矩形的大矩形
//     pub fn union_ip(&mut self, other: Rect) {
//         *self = self.union(other);
//     }

//     /// pygame.Rect.unionall(&[rects])
//     /// 批量合并多个矩形，返回总包围盒
//     pub fn unionall(list: &[Rect]) -> Self {
//         if list.is_empty() {
//             return Self::new(0, 0, 0, 0);
//         }
//         let mut total = list[0];
//         for r in list.iter().skip(1) {
//             total = total.union(*r);
//         }
//         total
//     }

//     /// pygame.Rect.unionall_ip(&[rects])
//     /// 原地批量合并多个矩形
//     pub fn unionall_ip(&mut self, list: &[Rect]) {
//         *self = Self::unionall(list);
//     }

//     /// pygame.Rect.fit(target_rect)
//     /// 保持原宽高比，缩放并移动自身填满目标矩形，返回新矩形
//     pub fn fit(&self, target: Rect) -> Self {
//         let scale_x = target.width as f32 / self.width as f32;
//         let scale_y = target.height as f32 / self.height as f32;
//         let scale = scale_x.min(scale_y);

//         let new_w = (self.width as f32 * scale) as i32;
//         let new_h = (self.height as f32 * scale) as i32;

//         let off_x = (target.width - new_w) / 2;
//         let off_y = (target.height - new_h) / 2;

//         Self::new(target.left + off_x, target.top + off_y, new_w, new_h)
//     }

//     /// pygame.Rect.normalize()
//     /// 修正负宽高：交换坐标让 width/height 为正数，返回新矩形
//     pub fn normalize(&self) -> Self {
//         let mut l = self.left;
//         let mut t = self.top;
//         let mut w = self.width;
//         let mut h = self.height;

//         if w < 0 {
//             l += w;
//             w = -w;
//         }
//         if h < 0 {
//             t += h;
//             h = -h;
//         }
//         Self::new(l, t, w, h)
//     }

//     /// pygame.Rect.contains(other)
//     /// 判断 other 矩形完全被当前矩形包含
//     pub fn contains(&self, other: Rect) -> bool {
//         self.left <= other.left
//             && self.top <= other.top
//             && self.right() >= other.right()
//             && self.bottom() >= other.bottom()
//     }

//     // ====================== 碰撞检测系列 ======================
//     /// pygame.Rect.collidepoint(x, y)
//     /// 判断点是否在矩形内部（包含边界）
//     pub fn collidepoint(&self, p: (i32,i32)) -> bool {
//         p.0 >= self.left && p.0 <= self.right() && p.1 >= self.top && p.1 <= self.bottom()
//     }

//     /// pygame.Rect.colliderect(other)
//     /// 判断两个矩形是否重叠相交
//     pub fn colliderect(&self, other: Rect) -> bool {
//         self.left < other.right()
//             && self.right() > other.left
//             && self.top < other.bottom()
//             && self.bottom() > other.top
//     }

//     /// pygame.Rect.collidelist(&[rect]) -> usize
//     /// 遍历矩形列表，返回第一个碰撞的下标；无碰撞返回 -1
//     pub fn collidelist(&self, rects: &[Rect]) -> i32 {
//         for (idx, r) in rects.iter().enumerate() {
//             if self.colliderect(*r) {
//                 return idx as i32;
//             }
//         }
//         -1
//     }

//     /// pygame.Rect.collidelistall(&[rect]) -> Vec<usize>
//     /// 返回所有发生碰撞的矩形下标列表
//     pub fn collidelistall(&self, rects: &[Rect]) -> Vec<usize> {
//         rects
//             .iter()
//             .enumerate()
//             .filter(|(_, r)| self.colliderect(**r))
//             .map(|(i, _)| i)
//             .collect()
//     }

//     /// 兼容通用对象碰撞 trait，这里用泛型模拟 collideobjects
//     /// pygame.Rect.collideobjects<T: AsRef<Rect>>
//     pub fn collideobjects<T: AsRef<Rect>>(&self, objs: &[T]) -> i32 {
//         for (idx, obj) in objs.iter().enumerate() {
//             if self.colliderect(*obj.as_ref()) {
//                 return idx as i32;
//             }
//         }
//         -1
//     }

//     /// pygame.Rect.collideobjectsall<T: AsRef<Rect>>
//     pub fn collideobjectsall<T: AsRef<Rect>>(&self, objs: &[T]) -> Vec<usize> {
//         objs
//             .iter()
//             .enumerate()
//             .filter(|(_, o)| self.colliderect(*o.as_ref()))
//             .map(|(i, _)| i)
//             .collect()
//     }

//     /// pygame.Rect.collidedict
//     /// 字典碰撞：value为Rect，返回第一个碰撞的(key, value)
//     pub fn collidedict<K, V: AsRef<Rect>>(&self, dict: &std::collections::HashMap<K, V>) -> Option<(&K, &V)> {
//         for (k, v) in dict {
//             if self.colliderect(v.as_ref()) {
//                 return Some((k, v));
//             }
//         }
//         None
//     }

//     /// pygame.Rect.collidedictall
//     /// 返回字典中所有碰撞的键值对
//     pub fn collidedictall<K, V: AsRef<Rect>>(&self, dict: &std::collections::HashMap<K, V>) -> Vec<(&K, &V)> {
//         dict.iter()
//             .filter(|(_, v)| self.colliderect(v.as_ref()))
//             .collect()
//     }
// }

// // 实现 AsRef<Rect> 方便泛型对象碰撞
// impl AsRef<Rect> for Rect {
//     fn as_ref(&self) -> &Rect {
//         self
//     }
// }

// // ====================== 使用示例 ======================
// #[cfg(test)]
// mod tests {
//     use super::*;
//     use std::collections::HashMap;

//     #[test]
//     fn test_basic_api() {
//         // 构造
//         let mut r1 = Rect::new(10, 10, 100, 80);
//         let r2 = Rect::new_from_tuple((20, 20), (50, 50));

//         // copy
//         let r_copy = r1.copy();
//         assert_eq!(r_copy, r1);

//         // move / move_ip
//         let r_moved = r1.move_(5, 5);
//         r1.move_ip(5, 5);
//         assert_eq!(r1, r_moved);

//         // inflate
//         let r_big = r1.inflate(20, 20);
//         r1.inflate_ip(20, 20);
//         assert_eq!(r1, r_big);

//         // 碰撞检测
//         assert!(r1.colliderect(r2));
//         assert!(r1.collidepoint((30, 30)));

//         // union 合并
//         let union_r = r1.union(r2);
//         let mut list = vec![r1, r2];
//         let all_union = Rect::unionall(&list);
//         assert_eq!(union_r, all_union);

//         // clamp 约束
//         let bound = Rect::new(0, 0, 200, 200);
//         let clamped = r1.clamp(bound);
//         let mut test_r = Rect::new(300, 300, 20, 20);
//         test_r.clamp_ip(bound);

//         // 字典碰撞
//         let mut map = HashMap::new();
//         map.insert("box1", r2);
//         let hit = r1.collidedict(&map);
//         assert!(hit.is_some());
//     }
// }