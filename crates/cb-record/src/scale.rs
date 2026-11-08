//! Frame scaling into the encoder's output size (letterboxed, bilinear).

/// Scale premultiplied opaque RGBA (`sw`×`sh`) into BGRA `dst` (`dw`×`dh`),
/// keeping the aspect ratio and filling the borders with black.
pub fn fit_rgba_to_bgra(src: &[u8], sw: u32, sh: u32, dst: &mut [u8], dw: u32, dh: u32) {
    let (sw_, sh_, dw_, dh_) = (sw as usize, sh as usize, dw as usize, dh as usize);
    if sw == 0 || sh == 0 || src.len() < sw_ * sh_ * 4 || dst.len() < dw_ * dh_ * 4 {
        return;
    }
    if sw == dw && sh == dh {
        for (d, s) in dst.chunks_exact_mut(4).zip(src.chunks_exact(4)) {
            d[0] = s[2];
            d[1] = s[1];
            d[2] = s[0];
            d[3] = 255;
        }
        return;
    }
    let k = (dw as f64 / sw as f64).min(dh as f64 / sh as f64);
    let (fw, fh) = (((sw as f64 * k).round() as usize).clamp(1, dw_), ((sh as f64 * k).round() as usize).clamp(1, dh_));
    let (ox, oy) = ((dw_ - fw) / 2, (dh_ - fh) / 2);
    // Black borders.
    for (y, row) in dst.chunks_exact_mut(dw_ * 4).enumerate().take(dh_) {
        for (x, px) in row.chunks_exact_mut(4).enumerate() {
            if y < oy || y >= oy + fh || x < ox || x >= ox + fw {
                px.copy_from_slice(&[0, 0, 0, 255]);
            }
        }
    }
    // Precompute horizontal taps (16.16 fixed point). When shrinking by more
    // than 2x, average a 2x2 neighbourhood around each tap to limit aliasing.
    let sx = sw as f64 / fw as f64;
    let sy = sh as f64 / fh as f64;
    let xs: Vec<(usize, usize, u32)> = (0..fw)
        .map(|x| {
            let fx = ((x as f64 + 0.5) * sx - 0.5).max(0.0);
            let x0 = (fx as usize).min(sw_ - 1);
            (x0, (x0 + 1).min(sw_ - 1), ((fx - x0 as f64) * 65536.0) as u32)
        })
        .collect();
    for y in 0..fh {
        let fy = ((y as f64 + 0.5) * sy - 0.5).max(0.0);
        let y0 = (fy as usize).min(sh_ - 1);
        let y1 = (y0 + 1).min(sh_ - 1);
        let wy = ((fy - y0 as f64) * 65536.0) as u32;
        let r0 = &src[y0 * sw_ * 4..(y0 + 1) * sw_ * 4];
        let r1 = &src[y1 * sw_ * 4..(y1 + 1) * sw_ * 4];
        let drow = &mut dst[((oy + y) * dw_ + ox) * 4..((oy + y) * dw_ + ox + fw) * 4];
        for (x, d) in drow.chunks_exact_mut(4).enumerate() {
            let (x0, x1, wx) = xs[x];
            for c in 0..3 {
                let a = r0[x0 * 4 + c] as u32;
                let b = r0[x1 * 4 + c] as u32;
                let cc = r1[x0 * 4 + c] as u32;
                let dd = r1[x1 * 4 + c] as u32;
                let top = a * (65536 - wx) + b * wx;
                let bot = cc * (65536 - wx) + dd * wx;
                let v = ((top as u64 * (65536 - wy) as u64 + bot as u64 * wy as u64) >> 32) as u8;
                d[2 - c] = v;
            }
            d[3] = 255;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_size_swizzles() {
        let src = [10u8, 20, 30, 255, 40, 50, 60, 255];
        let mut dst = [0u8; 8];
        fit_rgba_to_bgra(&src, 2, 1, &mut dst, 2, 1);
        assert_eq!(dst, [30, 20, 10, 255, 60, 50, 40, 255]);
    }

    #[test]
    fn letterboxes_wider_output() {
        // 4x4 white into 8x4: 2 px black bars left and right.
        let src = vec![255u8; 4 * 4 * 4];
        let mut dst = vec![7u8; 8 * 4 * 4];
        fit_rgba_to_bgra(&src, 4, 4, &mut dst, 8, 4);
        let px = |x: usize, y: usize| &dst[(y * 8 + x) * 4..(y * 8 + x) * 4 + 4];
        assert_eq!(px(0, 0), &[0, 0, 0, 255]);
        assert_eq!(px(7, 3), &[0, 0, 0, 255]);
        assert_eq!(px(3, 1), &[255, 255, 255, 255]);
    }

    #[test]
    fn downscale_keeps_colour() {
        let src: Vec<u8> = [200u8, 100, 50, 255].repeat(100 * 50);
        let mut dst = vec![0u8; 40 * 20 * 4];
        fit_rgba_to_bgra(&src, 100, 50, &mut dst, 40, 20);
        assert_eq!(&dst[(10 * 40 + 20) * 4..(10 * 40 + 20) * 4 + 4], &[50, 100, 200, 255]);
    }
}
