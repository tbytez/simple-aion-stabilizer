//! Area-filter straight RGBA with alpha-weighted color: no dark transparent halo.
pub(crate) fn resize(src: &[u8], width: u32, height: u32, side: u32) -> Vec<u8> {
    let mut out = vec![0; (side * side * 4) as usize];
    for y in 0..side { for x in 0..side {
        let x0 = x as f64 * width as f64 / side as f64;
        let x1 = (x + 1) as f64 * width as f64 / side as f64;
        let y0 = y as f64 * height as f64 / side as f64;
        let y1 = (y + 1) as f64 * height as f64 / side as f64;
        let mut sum = [0.0; 4];
        for sy in y0.floor() as u32..(y1.ceil() as u32).min(height) {
            for sx in x0.floor() as u32..(x1.ceil() as u32).min(width) {
                let weight = (x1.min((sx + 1) as f64) - x0.max(sx as f64)) * (y1.min((sy + 1) as f64) - y0.max(sy as f64));
                let i = ((sy * width + sx) * 4) as usize;
                let alpha = src[i + 3] as f64 * weight;
                for c in 0..3 { sum[c] += src[i + c] as f64 * alpha; }
                sum[3] += alpha;
            }
        }
        let i = ((y * side + x) * 4) as usize;
        if sum[3] > 0.0 { for c in 0..3 { out[i + c] = (sum[c] / sum[3]).round() as u8; } }
        out[i + 3] = (sum[3] / ((x1 - x0) * (y1 - y0))).round() as u8;
    } }
    out
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn transparent_pixels_do_not_darken_edges() {
        assert_eq!(resize(&[255,255,255,255, 0,0,0,0, 0,0,0,0, 0,0,0,0],2,2,1), [255,255,255,64]);
    }
    #[test]
    fn fractional_dpi_sizes_preserve_opaque_color() {
        let src = [12,34,56,255].repeat(256 * 256);
        for side in [16,20,24,28,32,36,40,48,64,128] {
            assert_eq!(resize(&src,256,256,side), [12,34,56,255].repeat((side * side) as usize));
        }
    }
}
