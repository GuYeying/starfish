//! NV12 → RGBA 颜色空间转换（BT.601 有限范围，视频解码的标准输出约定）
//!
//! 纯函数、零依赖——单元测试直接验证色值（见下方 tests）。
//!
//! 性能要点（1080p 单帧是本模块的热路径，debug 构建同样敏感）：
//! - 整数 16.16 定点替代逐像素 f32（`round`/`clamp` 在 debug 下不内联，开销放大数倍）
//! - 4:2:0 水平减半：相邻两像素共享一份 U/V
//! - v2 计划：转换上移到采样着色器（Y/UV 双纹理直传，零 CPU 色彩转换）

/// 定点系数（`round(coef × 65536)`；与浮点公式误差 ≤ 1/65536）
const YC: i32 = 76_279; // 1.164
const UG: i32 = -25_690; // -0.392（G 行）
const VG: i32 = -53_281; // -0.813（G 行）
const VR: i32 = 104_595; // 1.596
const UB: i32 = 132_167; // 2.017
/// 四舍五入偏移（`>> 16` 前加半 LSB）
const HALF: i32 = 1 << 15;

#[inline]
fn clamp8(x: i32) -> u8 {
    x.clamp(0, 255) as u8
}

/// 单帧 NV12 → RGBA8。
///
/// - `nv12`：Y 平面（`stride × 对齐高`，行距/高度对齐可大于显示尺寸）+
///   交错 UV 平面（每行 `stride` 字节、U 在前 V 在后，共 `对齐高/2` 行）
/// - `width`/`height`：显示尺寸；Y 平面行数（对齐高）由缓冲长度反推——
///   解码器常把 Y 平面按 16 行对齐（1080 → 1088），色度平面起点跟随对齐高
///   而非显示高（读错即顶部绿条：填充字节 0 → U=V=0 强绿偏色）
/// - 输出：`width × height × 4`；定点舍入与浮点公式色差 ±1
pub fn nv12_to_rgba(nv12: &[u8], stride: usize, width: u32, height: u32) -> Vec<u8> {
    let (w, h) = (width as usize, height as usize);
    let mut rgba = vec![0u8; w * h * 4];

    // 对齐高 = buf_len × 2 / (stride × 3)（NV12 总长 = stride × 对齐高 × 3/2）；
    // 恰好紧凑排布时退化为 h。取偶 + 下限 h 保证切片安全。
    let stride = stride.max(1);
    let aligned_h = ((nv12.len() * 2) / (stride * 3)).max(h) & !1;
    let uv_base = stride * aligned_h; // UV 平面起始（紧随 Y 平面的对齐区）

    for row in 0..h {
        let y_row = &nv12[row * stride..row * stride + w];
        // UV 行按 2 行共享一份（4:2:0），取样位置取 (row/2)
        let uv_row = &nv12[uv_base + (row / 2) * stride..][..w];
        let out = &mut rgba[row * w * 4..(row + 1) * w * 4];

        // 相邻两像素共享一份 U/V；NV12 尺寸约定为偶数，奇宽尾部逐像素退化处理
        let mut col = 0;
        while col + 1 < w {
            let u = uv_row[col] as i32 - 128;
            let v = uv_row[col + 1] as i32 - 128;
            let y0 = YC * (y_row[col] as i32 - 16);
            let y1 = YC * (y_row[col + 1] as i32 - 16);

            let o = col * 4;
            out[o] = clamp8((y0 + VR * v + HALF) >> 16);
            out[o + 1] = clamp8((y0 + UG * u + VG * v + HALF) >> 16);
            out[o + 2] = clamp8((y0 + UB * u + HALF) >> 16);
            out[o + 3] = 255;
            out[o + 4] = clamp8((y1 + VR * v + HALF) >> 16);
            out[o + 5] = clamp8((y1 + UG * u + VG * v + HALF) >> 16);
            out[o + 6] = clamp8((y1 + UB * u + HALF) >> 16);
            out[o + 7] = 255;
            col += 2;
        }
        if col < w {
            let u = uv_row[col] as i32 - 128;
            let v = uv_row[col + 1] as i32 - 128;
            let y0 = YC * (y_row[col] as i32 - 16);
            let o = col * 4;
            out[o] = clamp8((y0 + VR * v + HALF) >> 16);
            out[o + 1] = clamp8((y0 + UG * u + VG * v + HALF) >> 16);
            out[o + 2] = clamp8((y0 + UB * u + HALF) >> 16);
            out[o + 3] = 255;
        }
    }
    rgba
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 构造纯色 NV12 帧（Y 全 `y`，UV 全 `uv`）
    fn solid_nv12(y: u8, uv: u8, w: u32, h: u32) -> (Vec<u8>, usize) {
        let stride = w as usize;
        let y_len = stride * h as usize;
        let mut nv12 = vec![y; y_len + stride * (h as usize) / 2];
        nv12[y_len..].fill(uv);
        (nv12, stride)
    }

    #[test]
    fn black_is_zero_rgb() {
        // Y=16, UV=128 → BT.601 有限范围的黑
        let (nv12, stride) = solid_nv12(16, 128, 8, 8);
        let rgba = nv12_to_rgba(&nv12, stride, 8, 8);
        assert_eq!(&rgba[0..4], &[0, 0, 0, 255]);
    }

    #[test]
    fn white_is_full_rgb() {
        // Y=235, UV=128 → 有限范围的白
        let (nv12, stride) = solid_nv12(235, 128, 8, 8);
        let rgba = nv12_to_rgba(&nv12, stride, 8, 8);
        assert_eq!(&rgba[0..4], &[255, 255, 255, 255]);
    }

    #[test]
    fn mid_gray_is_130() {
        // Y=128, UV=128 → ~130（BT.601 有限范围中灰）
        let (nv12, stride) = solid_nv12(128, 128, 8, 8);
        let rgba = nv12_to_rgba(&nv12, stride, 8, 8);
        assert_eq!(
            &rgba[0..8],
            &[130, 130, 130, 255, 130, 130, 130, 255],
            "Y=128/UV=128 应为中灰"
        );
    }

    #[test]
    fn alpha_is_opaque() {
        let (nv12, stride) = solid_nv12(128, 128, 4, 4);
        let rgba = nv12_to_rgba(&nv12, stride, 4, 4);
        assert!(rgba.iter().skip(3).step_by(4).all(|&a| a == 255));
    }

    #[test]
    fn uv_chroma_plane_is_shared_per_2x2() {
        // 4:2:0：UV 逐 2×2 块共享——同一块的 4 像素必须同色（含 alpha）
        let w = 4u32;
        let h = 4u32;
        let stride = 4usize;
        let mut nv12 = vec![128u8; stride * (h as usize) * 3 / 2];
        // Y 平面全 128（灰），UV 平面首 2×2 块涂 U=64,V=192（tint）
        let uv = &mut nv12[stride * h as usize..];
        uv[0] = 64;
        uv[1] = 192;
        let rgba = nv12_to_rgba(&nv12, stride, w, h);
        let px = |row: usize, col: usize| {
            let o = (row * 4 + col) * 4;
            &rgba[o..o + 3]
        };
        // 块(0,0) 的 4 像素共享 UV(64,192) → 互相一致
        assert_eq!(px(0, 0), px(0, 1));
        assert_eq!(px(0, 0), px(1, 0));
        assert_eq!(px(0, 0), px(1, 1));
        // 块(0,1)（UV 128）与之不同（证明块间独立）
        assert_ne!(px(0, 0), px(0, 2));
        assert_ne!(px(0, 0), px(0, 3));
    }
}
