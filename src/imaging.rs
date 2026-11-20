//! Bildekode uten GTK: PNG, beskjæring, frihåndsmaske, nedskalering og pixelering.
//! Alt her kjøres utenfor UI-tråden.

use crate::capture::geometry::{Pt, Rect};
use klipp_system::i18n::{tr, tr_fmt};
use std::fs::File;
use std::io::{BufReader, BufWriter, Write};
use std::path::Path;

/// RGBA med rett alfa, tett pakket.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Image {
    pub w: u32,
    pub h: u32,
    pub data: Vec<u8>,
}

impl Image {
    pub fn new(w: u32, h: u32) -> Self {
        Self {
            w,
            h,
            data: vec![0; w as usize * h as usize * 4],
        }
    }
}

pub fn decode_png(path: &Path) -> Result<Image, String> {
    let file = File::open(path).map_err(|e| {
        tr_fmt(
            "Kunne ikke åpne bildet: {error}",
            &[("error", &e.to_string())],
        )
    })?;
    let mut dec = png::Decoder::new(BufReader::new(file));
    dec.set_transformations(
        png::Transformations::normalize_to_color8() | png::Transformations::ALPHA,
    );
    decode(dec)
}

#[cfg(test)]
pub fn decode_png_bytes(bytes: &[u8]) -> Result<Image, String> {
    let mut dec = png::Decoder::new(std::io::Cursor::new(bytes));
    dec.set_transformations(
        png::Transformations::normalize_to_color8() | png::Transformations::ALPHA,
    );
    decode(dec)
}

fn decode<R: std::io::BufRead + std::io::Seek>(dec: png::Decoder<R>) -> Result<Image, String> {
    let mut reader = dec
        .read_info()
        .map_err(|e| tr_fmt("Ugyldig PNG: {error}", &[("error", &e.to_string())]))?;
    let size = reader
        .output_buffer_size()
        .ok_or_else(|| tr("PNG-en er for stor").to_string())?;
    let mut buf = vec![0; size];
    let info = reader
        .next_frame(&mut buf)
        .map_err(|e| tr_fmt("Ugyldig PNG: {error}", &[("error", &e.to_string())]))?;
    let rgba = match info.color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::GrayscaleAlpha => {
            let mut out = Vec::with_capacity(buf.len() * 2);
            for px in buf.chunks_exact(2) {
                out.extend_from_slice(&[px[0], px[0], px[0], px[1]]);
            }
            out
        }
        other => {
            return Err(tr_fmt(
                "Ukjent PNG-format: {format}",
                &[("format", &format!("{other:?}"))],
            ));
        }
    };
    let mut rgba = rgba;
    rgba.truncate(info.width as usize * info.height as usize * 4);
    Ok(Image {
        w: info.width,
        h: info.height,
        data: rgba,
    })
}

/// Skriv PNG. Uten alfa lagres RGB, som gir mindre filer og raskere koding.
pub fn encode_png<W: Write>(img: &Image, alpha: bool, out: W) -> Result<(), String> {
    let err = |e: png::EncodingError| {
        tr_fmt("Kunne ikke kode PNG: {error}", &[("error", &e.to_string())])
    };
    let mut enc = png::Encoder::new(out, img.w, img.h);
    enc.set_color(if alpha {
        png::ColorType::Rgba
    } else {
        png::ColorType::Rgb
    });
    enc.set_depth(png::BitDepth::Eight);
    enc.set_compression(png::Compression::Fast);
    let mut w = enc.write_header().map_err(err)?;
    if alpha {
        w.write_image_data(&img.data).map_err(err)?;
    } else {
        let mut rgb = Vec::with_capacity(img.w as usize * img.h as usize * 3);
        for px in img.data.chunks_exact(4) {
            rgb.extend_from_slice(&px[..3]);
        }
        w.write_image_data(&rgb).map_err(err)?;
    }
    w.finish().map_err(err)
}

pub fn encode_png_file(img: &Image, alpha: bool, file: File) -> Result<(), String> {
    let mut w = BufWriter::with_capacity(1 << 18, file);
    encode_png(img, alpha, &mut w)?;
    w.flush().map_err(|e| {
        tr_fmt(
            "Kunne ikke skrive bildet: {error}",
            &[("error", &e.to_string())],
        )
    })
}

pub fn encode_png_bytes(img: &Image, alpha: bool) -> Result<Vec<u8>, String> {
    let mut out = Vec::with_capacity(img.data.len() / 4);
    encode_png(img, alpha, &mut out)?;
    Ok(out)
}

/// Beskjær rå RGBA til `r`, klemt til bildet. Rå bytes gjør at det frosne bildet
/// kan deles uten kopi.
pub fn crop_raw(data: &[u8], w: u32, h: u32, r: Rect) -> Image {
    let r = clip_rect(r, w as i32, h as i32);
    let mut out = Image::new(r.w as u32, r.h as u32);
    let stride = w as usize * 4;
    let line = r.w as usize * 4;
    if line == 0 {
        return out;
    }
    for (row, dst) in out.data.chunks_exact_mut(line).enumerate() {
        let start = (r.y as usize + row) * stride + r.x as usize * 4;
        dst.copy_from_slice(&data[start..start + line]);
    }
    out
}

pub fn clip_rect(r: Rect, w: i32, h: i32) -> Rect {
    let x0 = r.x.clamp(0, w);
    let y0 = r.y.clamp(0, h);
    let x1 = r.right().clamp(x0, w);
    let y1 = r.bottom().clamp(y0, h);
    Rect::new(x0, y0, x1 - x0, y1 - y0)
}

/// Beskjær til polygonet, med myk kant og gjennomsiktige piksler utenfor formen.
/// `pts` er i kildebildets koordinater og `r` er formens omsluttende rektangel.
pub fn crop_polygon_raw(data: &[u8], w: u32, h: u32, r: Rect, pts: &[Pt]) -> Image {
    let mut out = crop_raw(data, w, h, r);
    let mask = polygon_mask(out.w as usize, out.h as usize, r, pts);
    for (px, m) in out.data.chunks_exact_mut(4).zip(mask.iter()) {
        px[3] = ((u32::from(px[3]) * u32::from(*m) + 127) / 255) as u8;
        if px[3] == 0 {
            px.fill(0);
        }
    }
    out
}

/// Dekningsmaske med ikke-null-regel, fire delrader per piksel.
fn polygon_mask(w: usize, h: usize, r: Rect, pts: &[Pt]) -> Vec<u8> {
    const SUB: usize = 4;
    let mut mask = vec![0u8; w * h];
    if pts.len() < 3 || w == 0 || h == 0 {
        return mask;
    }
    let poly: Vec<(f64, f64)> = pts
        .iter()
        .map(|p| (f64::from(p.x - r.x), f64::from(p.y - r.y)))
        .collect();
    let mut cover = vec![0f32; w];
    let mut xs: Vec<(f64, i32)> = Vec::new();
    for y in 0..h {
        cover.fill(0.0);
        for sub in 0..SUB {
            let sy = y as f64 + (sub as f64 + 0.5) / SUB as f64;
            xs.clear();
            for i in 0..poly.len() {
                let (x0, y0) = poly[i];
                let (x1, y1) = poly[(i + 1) % poly.len()];
                if (y0 <= sy) != (y1 <= sy) {
                    let t = (sy - y0) / (y1 - y0);
                    xs.push((x0 + t * (x1 - x0), if y1 > y0 { 1 } else { -1 }));
                }
            }
            xs.sort_by(|a, b| a.0.total_cmp(&b.0));
            let mut wind = 0;
            for pair in xs.windows(2) {
                wind += pair[0].1;
                if wind != 0 {
                    add_span(&mut cover, pair[0].0, pair[1].0);
                }
            }
        }
        for (m, c) in mask[y * w..(y + 1) * w].iter_mut().zip(cover.iter()) {
            *m = (c / SUB as f32 * 255.0).round().min(255.0) as u8;
        }
    }
    mask
}

fn add_span(cover: &mut [f32], x0: f64, x1: f64) {
    let w = cover.len() as f64;
    let (a, b) = (x0.clamp(0.0, w), x1.clamp(0.0, w));
    if b <= a {
        return;
    }
    let (ia, ib) = (a.floor() as usize, b.floor() as usize);
    if ia == ib {
        cover[ia.min(cover.len() - 1)] += (b - a) as f32;
        return;
    }
    cover[ia] += (ia as f64 + 1.0 - a) as f32;
    for c in &mut cover[ia + 1..ib] {
        *c += 1.0;
    }
    if ib < cover.len() {
        cover[ib] += (b - ib as f64) as f32;
    }
}

/// Størrelsen bildet får når det skal passe inni boksen. Forstørrer aldri.
pub fn fit_size(w: u32, h: u32, max_w: u32, max_h: u32) -> (u32, u32) {
    let f = (f64::from(max_w) / f64::from(w.max(1)))
        .min(f64::from(max_h) / f64::from(h.max(1)))
        .min(1.0);
    (
        ((f64::from(w) * f).round() as u32).max(1),
        ((f64::from(h) * f).round() as u32).max(1),
    )
}

/// Boksfilter med alfa-vekting. Raskt nok for 4K utenfor UI-tråden.
pub fn downscale(src: &Image, dw: u32, dh: u32) -> Image {
    if dw >= src.w && dh >= src.h {
        return src.clone();
    }
    let (sw, sh) = (src.w as usize, src.h as usize);
    let (dw, dh) = (dw as usize, dh as usize);
    let mut out = Image::new(dw as u32, dh as u32);
    let mut acc = vec![[0u64; 4]; dw];
    let ranges: Vec<(usize, usize)> = (0..dw)
        .map(|x| {
            let a = x * sw / dw;
            (a, ((x + 1) * sw).div_ceil(dw).max(a + 1).min(sw))
        })
        .collect();
    for dy in 0..dh {
        let y0 = dy * sh / dh;
        let y1 = ((dy + 1) * sh).div_ceil(dh).max(y0 + 1).min(sh);
        acc.iter_mut().for_each(|a| *a = [0; 4]);
        for sy in y0..y1 {
            let row = &src.data[sy * sw * 4..(sy + 1) * sw * 4];
            for (a, &(x0, x1)) in acc.iter_mut().zip(ranges.iter()) {
                for px in row[x0 * 4..x1 * 4].chunks_exact(4) {
                    let al = u64::from(px[3]);
                    a[0] += u64::from(px[0]) * al;
                    a[1] += u64::from(px[1]) * al;
                    a[2] += u64::from(px[2]) * al;
                    a[3] += al;
                }
            }
        }
        let row = &mut out.data[dy * dw * 4..(dy + 1) * dw * 4];
        for (x, (px, a)) in row.chunks_exact_mut(4).zip(acc.iter()).enumerate() {
            let n = ((ranges[x].1 - ranges[x].0) * (y1 - y0)) as u64;
            if a[3] > 0 {
                px[0] = ((a[0] + a[3] / 2) / a[3]) as u8;
                px[1] = ((a[1] + a[3] / 2) / a[3]) as u8;
                px[2] = ((a[2] + a[3] / 2) / a[3]) as u8;
                px[3] = ((a[3] + n / 2) / n) as u8;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gradient(w: u32, h: u32) -> Image {
        let mut img = Image::new(w, h);
        for (i, px) in img.data.chunks_exact_mut(4).enumerate() {
            let (x, y) = (i as u32 % w, i as u32 / w);
            px.copy_from_slice(&[x as u8, y as u8, 7, 255]);
        }
        img
    }

    #[test]
    fn png_round_trip_rgb_and_rgba() {
        let img = gradient(40, 30);
        let bytes = encode_png_bytes(&img, false).unwrap();
        assert_eq!(decode_png_bytes(&bytes).unwrap(), img);
        let mut alpha = img.clone();
        alpha.data[3] = 0;
        let bytes = encode_png_bytes(&alpha, true).unwrap();
        assert_eq!(decode_png_bytes(&bytes).unwrap(), alpha);
    }

    #[test]
    fn crop_is_pixel_exact() {
        let img = gradient(40, 30);
        let c = crop_raw(&img.data, img.w, img.h, Rect::new(10, 5, 3, 2));
        assert_eq!((c.w, c.h), (3, 2));
        assert_eq!(&c.data[0..4], &[10, 5, 7, 255]);
        assert_eq!(&c.data[20..24], &[12, 6, 7, 255]);
    }

    #[test]
    fn crop_clamps_to_image() {
        let img = gradient(10, 10);
        let c = crop_raw(&img.data, img.w, img.h, Rect::new(8, 8, 10, 10));
        assert_eq!((c.w, c.h), (2, 2));
    }

    #[test]
    fn polygon_png_keeps_only_pixels_from_the_selection() {
        let img = gradient(20, 20);
        let tri = [Pt { x: 0, y: 0 }, Pt { x: 20, y: 0 }, Pt { x: 0, y: 20 }];
        let crop = crop_polygon_raw(&img.data, img.w, img.h, Rect::new(0, 0, 20, 20), &tri);
        let png = encode_png_bytes(&crop, true).unwrap();
        let c = decode_png_bytes(&png).unwrap();
        let pixel = |x: usize, y: usize| &c.data[(y * 20 + x) * 4..][..4];
        assert_eq!(pixel(2, 2), [2, 2, 7, 255]);
        assert_eq!(pixel(18, 18), [0, 0, 0, 0]);
        let alpha = |x, y| pixel(x, y)[3];
        assert!(
            alpha(10, 9) > 0 && alpha(10, 9) < 255,
            "kanten skal være myk"
        );
    }

    #[test]
    fn fit_size_never_upscales() {
        assert_eq!(fit_size(100, 50, 448, 600), (100, 50));
        assert_eq!(fit_size(3840, 2160, 448, 600), (448, 252));
        assert_eq!(fit_size(300, 3000, 448, 600), (60, 600));
    }

    #[test]
    fn downscale_averages() {
        let mut img = Image::new(2, 2);
        img.data.copy_from_slice(&[
            0, 0, 0, 255, 200, 200, 200, 255, 0, 0, 0, 255, 200, 200, 200, 255,
        ]);
        let d = downscale(&img, 1, 1);
        assert_eq!(d.data, vec![100, 100, 100, 255]);
    }

    #[test]
    fn downscale_ignores_color_of_transparent_pixels() {
        let mut img = Image::new(2, 1);
        img.data.copy_from_slice(&[255, 0, 0, 0, 0, 255, 0, 255]);
        let d = downscale(&img, 1, 1);
        assert_eq!(&d.data[..3], &[0, 255, 0]);
        assert_eq!(d.data[3], 128);
    }
}
