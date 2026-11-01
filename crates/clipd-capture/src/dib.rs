//! Device-independent bitmap (CF_DIB / CF_DIBV5) <-> RGBA8 conversion.
//! Pure byte manipulation, no OS calls.

use anyhow::{Result, bail, ensure};

const BI_RGB: u32 = 0;
const BI_BITFIELDS: u32 = 3;
/// Refuse absurd images (64 Mpx = 256 MB of RGBA).
const MAX_PIXELS: u64 = 64 * 1024 * 1024;

fn u16_at(d: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([d[o], d[o + 1]])
}
fn u32_at(d: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([d[o], d[o + 1], d[o + 2], d[o + 3]])
}
fn i32_at(d: &[u8], o: usize) -> i32 {
    u32_at(d, o) as i32
}

/// One colour channel of a 32-bit pixel described by a bit mask.
#[derive(Clone, Copy)]
struct Channel {
    mask: u32,
    shift: u32,
    max: u32,
}

impl Channel {
    fn new(mask: u32) -> Self {
        let shift = if mask == 0 { 0 } else { mask.trailing_zeros() };
        Channel {
            mask,
            shift,
            max: mask >> shift,
        }
    }
    fn present(self) -> bool {
        self.mask != 0
    }
    fn extract(self, px: u32) -> u8 {
        let v = (px & self.mask) >> self.shift;
        if self.max == 255 {
            v as u8
        } else {
            ((u64::from(v) * 255 + u64::from(self.max) / 2) / u64::from(self.max)) as u8
        }
    }
}

/// Convert a packed DIB (the bytes of CF_DIB or CF_DIBV5, starting at the
/// info header) into `(width, height, RGBA8 top-down)`.
///
/// Supports 24-bit and 32-bit `BI_RGB` / `BI_BITFIELDS`, bottom-up and
/// top-down (negative height). A 32-bit DIB whose alpha channel is entirely
/// zero is treated as opaque.
pub fn dib_to_rgba(data: &[u8]) -> Result<(u32, u32, Vec<u8>)> {
    ensure!(data.len() >= 40, "DIB too short");
    let header_size = u32_at(data, 0) as usize;
    ensure!(
        header_size >= 40 && header_size <= data.len(),
        "bad DIB header size {header_size}"
    );
    let width = i32_at(data, 4);
    let height = i32_at(data, 8);
    let bpp = u16_at(data, 14);
    let compression = u32_at(data, 16);
    let clr_used = u32_at(data, 32) as usize;
    ensure!(
        width > 0 && height != 0 && height != i32::MIN,
        "bad DIB size {width}x{height}"
    );
    let (w, h) = (width as usize, height.unsigned_abs() as usize);
    let top_down = height < 0;
    ensure!((w as u64) * (h as u64) <= MAX_PIXELS, "DIB too large");
    ensure!(bpp == 24 || bpp == 32, "unsupported DIB bit depth {bpp}");
    ensure!(
        compression == BI_RGB || (compression == BI_BITFIELDS && bpp == 32),
        "unsupported DIB compression {compression}"
    );
    ensure!(clr_used <= 1 << 16, "bad DIB colour table size");

    let (mut mr, mut mg, mut mb, mut ma) = (
        0x00FF_0000u32,
        0x0000_FF00u32,
        0x0000_00FFu32,
        0xFF00_0000u32,
    );
    let mut offset = header_size;
    if compression == BI_BITFIELDS {
        if header_size == 40 {
            // Masks follow the 40-byte header; no alpha mask.
            ensure!(data.len() >= 52, "DIB truncated (masks)");
            (mr, mg, mb, ma) = (u32_at(data, 40), u32_at(data, 44), u32_at(data, 48), 0);
            offset += 12;
        } else {
            // V3/V4/V5 headers carry the masks themselves.
            ensure!(header_size >= 52, "bad DIB header size {header_size}");
            mr = u32_at(data, 40);
            mg = u32_at(data, 44);
            mb = u32_at(data, 48);
            ma = if header_size >= 56 {
                u32_at(data, 52)
            } else {
                0
            };
        }
    }
    offset += clr_used * 4;

    let bytes_pp = usize::from(bpp / 8);
    let stride = (w * usize::from(bpp)).div_ceil(32) * 4;
    // Tolerate a missing padding tail on the last row.
    let needed = offset + stride * (h - 1) + w * bytes_pp;
    if data.len() < needed {
        bail!("DIB truncated: have {} bytes, need {needed}", data.len());
    }

    let (cr, cg, cb, ca) = (
        Channel::new(mr),
        Channel::new(mg),
        Channel::new(mb),
        Channel::new(ma),
    );
    ensure!(
        bpp == 24 || (cr.present() && cg.present() && cb.present()),
        "DIB has empty colour masks"
    );
    let mut out = vec![0u8; w * h * 4];
    let mut any_alpha = false;
    for y in 0..h {
        let src_row = if top_down { y } else { h - 1 - y };
        let src = &data[offset + src_row * stride..];
        let dst = &mut out[y * w * 4..(y + 1) * w * 4];
        if bpp == 24 {
            for x in 0..w {
                let (s, d) = (&src[x * 3..x * 3 + 3], &mut dst[x * 4..x * 4 + 4]);
                d.copy_from_slice(&[s[2], s[1], s[0], 255]);
            }
        } else {
            for x in 0..w {
                let px = u32_at(src, x * 4);
                let a = if ca.present() { ca.extract(px) } else { 255 };
                // For plain BI_RGB the 4th byte is "alpha" if any pixel uses it.
                any_alpha |= a != 0;
                dst[x * 4..x * 4 + 4].copy_from_slice(&[
                    cr.extract(px),
                    cg.extract(px),
                    cb.extract(px),
                    a,
                ]);
            }
        }
    }
    if bpp == 32 && !any_alpha {
        for px in out.chunks_exact_mut(4) {
            px[3] = 255;
        }
    }
    Ok((width as u32, h as u32, out))
}

/// Build a packed 32-bit bottom-up `BI_RGB` DIB (BITMAPINFOHEADER + BGRA
/// pixels) from RGBA8 top-down data, suitable for CF_DIB.
pub fn rgba_to_dib32(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>> {
    let (w, h) = (width as usize, height as usize);
    ensure!(w > 0 && h > 0, "empty image");
    ensure!(
        (w as u64) * (h as u64) <= MAX_PIXELS && rgba.len() == w * h * 4,
        "RGBA buffer size does not match {width}x{height}"
    );
    let mut out = Vec::with_capacity(40 + rgba.len());
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&(width as i32).to_le_bytes());
    out.extend_from_slice(&(height as i32).to_le_bytes()); // positive = bottom-up
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&32u16.to_le_bytes());
    out.extend_from_slice(&BI_RGB.to_le_bytes());
    out.extend_from_slice(&(rgba.len() as u32).to_le_bytes());
    out.extend_from_slice(&[0u8; 16]); // ppm x/y, clrUsed, clrImportant
    for row in rgba.chunks_exact(w * 4).rev() {
        for px in row.chunks_exact(4) {
            out.extend_from_slice(&[px[2], px[1], px[0], px[3]]);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(width: i32, height: i32, bpp: u16, compression: u32, size: u32) -> Vec<u8> {
        let mut h = Vec::new();
        h.extend_from_slice(&size.to_le_bytes());
        h.extend_from_slice(&width.to_le_bytes());
        h.extend_from_slice(&height.to_le_bytes());
        h.extend_from_slice(&1u16.to_le_bytes());
        h.extend_from_slice(&bpp.to_le_bytes());
        h.extend_from_slice(&compression.to_le_bytes());
        h.extend_from_slice(&[0u8; 20]);
        h
    }

    // 2x2 image, top row: red, green; bottom row: blue, white.
    const RED: [u8; 4] = [255, 0, 0, 255];
    const GREEN: [u8; 4] = [0, 255, 0, 255];
    const BLUE: [u8; 4] = [0, 0, 255, 255];
    const WHITE: [u8; 4] = [255, 255, 255, 255];
    fn expected() -> Vec<u8> {
        [RED, GREEN, BLUE, WHITE].concat()
    }

    fn rows24() -> (Vec<u8>, Vec<u8>) {
        // BGR, rows padded to 8 bytes (2*3=6 -> 8).
        let top = vec![0, 0, 255, 0, 255, 0, 0, 0];
        let bottom = vec![255, 0, 0, 255, 255, 255, 0, 0];
        (top, bottom)
    }

    #[test]
    fn dib24_bottom_up() {
        let (top, bottom) = rows24();
        let mut d = header(2, 2, 24, BI_RGB, 40);
        d.extend(bottom);
        d.extend(top);
        assert_eq!(dib_to_rgba(&d).unwrap(), (2, 2, expected()));
    }

    #[test]
    fn dib24_top_down() {
        let (top, bottom) = rows24();
        let mut d = header(2, -2, 24, BI_RGB, 40);
        d.extend(top);
        d.extend(bottom);
        assert_eq!(dib_to_rgba(&d).unwrap(), (2, 2, expected()));
    }

    #[test]
    fn dib24_tolerates_missing_last_row_padding() {
        let (top, bottom) = rows24();
        let mut d = header(2, 2, 24, BI_RGB, 40);
        d.extend(bottom);
        d.extend(&top[..6]);
        assert_eq!(dib_to_rgba(&d).unwrap().2, expected());
    }

    fn bgra(px: [u8; 4], a: u8) -> [u8; 4] {
        [px[2], px[1], px[0], a]
    }

    #[test]
    fn dib32_bi_rgb_with_alpha_both_orientations() {
        let top: Vec<u8> = [bgra(RED, 10), bgra(GREEN, 20)].concat();
        let bottom: Vec<u8> = [bgra(BLUE, 30), bgra(WHITE, 40)].concat();
        let want: Vec<u8> = [
            [255, 0, 0, 10],
            [0, 255, 0, 20],
            [0, 0, 255, 30],
            [255, 255, 255, 40],
        ]
        .concat();

        let mut up = header(2, 2, 32, BI_RGB, 40);
        up.extend(&bottom);
        up.extend(&top);
        assert_eq!(dib_to_rgba(&up).unwrap().2, want);

        let mut down = header(2, -2, 32, BI_RGB, 40);
        down.extend(&top);
        down.extend(&bottom);
        assert_eq!(dib_to_rgba(&down).unwrap().2, want);
    }

    #[test]
    fn dib32_all_zero_alpha_becomes_opaque() {
        let mut d = header(2, -2, 32, BI_RGB, 40);
        for px in [RED, GREEN, BLUE, WHITE] {
            d.extend(bgra(px, 0));
        }
        assert_eq!(dib_to_rgba(&d).unwrap().2, expected());
    }

    #[test]
    fn dib32_bitfields_40_byte_header_is_opaque() {
        let mut d = header(2, -2, 32, BI_BITFIELDS, 40);
        for m in [0x00FF_0000u32, 0x0000_FF00, 0x0000_00FF] {
            d.extend(m.to_le_bytes());
        }
        // Alpha byte is junk without an alpha mask; must be ignored.
        for px in [RED, GREEN, BLUE, WHITE] {
            d.extend(bgra(px, 7));
        }
        assert_eq!(dib_to_rgba(&d).unwrap().2, expected());
    }

    #[test]
    fn dibv5_bitfields_rgba_masks() {
        // 108-byte header with RGBA byte order masks (R in low byte).
        let mut d = header(2, -2, 32, BI_BITFIELDS, 108);
        d.resize(40, 0);
        for m in [0x0000_00FFu32, 0x0000_FF00, 0x00FF_0000, 0xFF00_0000] {
            d.extend(m.to_le_bytes());
        }
        d.resize(108, 0);
        for (px, a) in [(RED, 1u8), (GREEN, 2), (BLUE, 3), (WHITE, 4)] {
            d.extend([px[0], px[1], px[2], a]);
        }
        let want: Vec<u8> = [
            [255, 0, 0, 1],
            [0, 255, 0, 2],
            [0, 0, 255, 3],
            [255, 255, 255, 4],
        ]
        .concat();
        assert_eq!(dib_to_rgba(&d).unwrap().2, want);
    }

    #[test]
    fn rejects_bad_input() {
        assert!(dib_to_rgba(&[0; 10]).is_err());
        let d = header(2, 2, 8, BI_RGB, 40);
        assert!(dib_to_rgba(&d).is_err()); // 8-bit unsupported
        let d = header(2, 2, 24, BI_RGB, 40);
        assert!(dib_to_rgba(&d).is_err()); // no pixels
        let d = header(i32::MAX, -i32::MAX, 32, BI_RGB, 40);
        assert!(dib_to_rgba(&d).is_err()); // too large
    }

    #[test]
    fn rgba_to_dib32_round_trips() {
        let rgba: Vec<u8> = (0..3 * 2 * 4).map(|i| (i * 7 + 1) as u8 | 1).collect();
        let dib = rgba_to_dib32(3, 2, &rgba).unwrap();
        assert_eq!(dib_to_rgba(&dib).unwrap(), (3, 2, rgba));
        // bottom-up: first stored row is the image's last row
        assert_eq!(i32_at(&dib, 8), 2);
    }
}
