//! PNG inn og ut. Alt her er blokkerende og skal kjøres utenfor GTK-hovedtråden.

use crate::pixels;
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufReader, BufWriter, ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

/// Cairos grense for bildesider.
const MAX_SIDE: u32 = 32_767;
/// Standardgrensa i png-kraten er 64 MiB, som en skjermfanging over to 4K-skjermer sprenger.
const MAX_DECODED: usize = 2 << 30;

/// Feil fra systemet på norsk. Teksten vises i appen, så ingen engelske systemmeldinger.
fn describe(e: &io::Error) -> &'static str {
    match e.kind() {
        ErrorKind::NotFound => "Filen eller mappen finnes ikke",
        ErrorKind::PermissionDenied | ErrorKind::ReadOnlyFilesystem => "Ingen tilgang",
        ErrorKind::StorageFull | ErrorKind::QuotaExceeded => "Disken er full",
        _ => "Ukjent feil ved lesing eller skriving",
    }
}

fn decode_error(e: png::DecodingError) -> String {
    match e {
        png::DecodingError::IoError(e) => describe(&e).to_string(),
        _ => "Filen er ikke en gyldig PNG".to_string(),
    }
}

fn encode_error(e: png::EncodingError) -> String {
    match e {
        png::EncodingError::IoError(e) => describe(&e).to_string(),
        _ => "Bildet kunne ikke kodes som PNG".to_string(),
    }
}

/// Et innlest bilde, klart for cairo og GDK.
#[derive(Debug, PartialEq, Eq)]
pub struct Loaded {
    pub width: u32,
    pub height: u32,
    pub alpha: bool,
    /// BGRA med forhåndsmultiplisert alfa, tett pakket.
    pub pixels: Vec<u8>,
}

pub fn load(path: &Path) -> Result<Loaded, String> {
    let file = File::open(path).map_err(|e| describe(&e).to_string())?;
    decode(BufReader::new(file))
}

pub fn decode<R: std::io::BufRead + std::io::Seek>(r: R) -> Result<Loaded, String> {
    let limits = png::Limits { bytes: MAX_DECODED };
    let mut dec = png::Decoder::new_with_limits(r, limits);
    dec.set_transformations(
        png::Transformations::normalize_to_color8() | png::Transformations::ALPHA,
    );
    let mut reader = dec.read_info().map_err(decode_error)?;
    let (width, height) = reader.info().size();
    if width > MAX_SIDE || height > MAX_SIDE {
        return Err(format!("Bildet er for stort: {width}×{height}"));
    }
    let size = reader
        .output_buffer_size()
        .ok_or_else(|| "Bildet er for stort".to_string())?;
    let mut buf = vec![0; size];
    let info = reader.next_frame(&mut buf).map_err(decode_error)?;
    buf.truncate(info.buffer_size());
    let mut rgba = match info.color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::GrayscaleAlpha => {
            let mut out = Vec::with_capacity(buf.len() * 2);
            for px in buf.chunks_exact(2) {
                out.extend_from_slice(&[px[0], px[0], px[0], px[1]]);
            }
            out
        }
        _ => return Err("Filen er ikke en gyldig PNG".to_string()),
    };
    let alpha = pixels::rgba_to_premul_bgra(&mut rgba);
    Ok(Loaded {
        width,
        height,
        alpha,
        pixels: rgba,
    })
}

/// Skriver bildet som PNG. Uten alfa lagres RGB, som gir mindre filer og raskere koding.
pub fn encode<W: Write>(
    bgra: &[u8],
    width: u32,
    height: u32,
    alpha: bool,
    out: W,
) -> Result<(), String> {
    let mut enc = png::Encoder::new(out, width, height);
    enc.set_color(if alpha {
        png::ColorType::Rgba
    } else {
        png::ColorType::Rgb
    });
    enc.set_depth(png::BitDepth::Eight);
    enc.set_compression(png::Compression::Fast);
    let data = if alpha {
        pixels::premul_bgra_to_rgba(bgra)
    } else {
        pixels::opaque_bgra_to_rgb(bgra)
    };
    let mut w = enc.write_header().map_err(encode_error)?;
    w.write_image_data(&data).map_err(encode_error)?;
    w.finish().map_err(encode_error)
}

pub fn encode_bytes(bgra: &[u8], width: u32, height: u32, alpha: bool) -> Result<Vec<u8>, String> {
    let mut out = Vec::with_capacity(bgra.len() / 4);
    encode(bgra, width, height, alpha, &mut out)?;
    Ok(out)
}

/// Temp-fil i samme mappe, så `rename` er atomisk. Skjult, slik at ingen ser den som et bilde.
fn temp_path(target: &Path) -> Option<PathBuf> {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let name = target.file_name()?;
    let mut tmp = OsString::from(".");
    tmp.push(name);
    tmp.push(format!(
        ".klipp-{}-{}.tmp",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    Some(target.with_file_name(tmp))
}

/// Skriv med `write` til en temp-fil i samme mappe og erstatt `path` atomisk. Enten står den
/// gamle filen urørt, eller så står hele den nye. En symbolsk lenke beholdes.
pub fn write_atomic(
    path: &Path,
    write: impl FnOnce(&mut BufWriter<File>) -> Result<(), String>,
) -> Result<(), String> {
    let target = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let tmp = temp_path(&target).ok_or_else(|| "Ugyldig filsti".to_string())?;
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&tmp)
        .map_err(|e| format!("Kunne ikke lagre bildet: {}", describe(&e)))?;
    let result = (|| {
        if let Ok(meta) = fs::metadata(&target) {
            fs::set_permissions(&tmp, meta.permissions()).map_err(|e| describe(&e).to_string())?;
        }
        let mut w = BufWriter::with_capacity(1 << 18, file);
        write(&mut w)?;
        let file = w
            .into_inner()
            .map_err(|e| describe(e.error()).to_string())?;
        file.sync_all().map_err(|e| describe(&e).to_string())?;
        fs::rename(&tmp, &target).map_err(|e| describe(&e).to_string())
    })();
    result.map_err(|e| {
        let _ = fs::remove_file(&tmp);
        format!("Kunne ikke lagre bildet: {e}")
    })
}

pub fn save_atomic(
    path: &Path,
    bgra: &[u8],
    width: u32,
    height: u32,
    alpha: bool,
) -> Result<(), String> {
    write_atomic(path, |w| encode(bgra, width, height, alpha, w))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use std::os::unix::fs::PermissionsExt;

    fn dir(name: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("klipp-annotate-test-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn entries(d: &Path) -> Vec<String> {
        fs::read_dir(d)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn round_trip_keeps_alpha() {
        // Rett RGBA inn, forhåndsmultiplisert i minnet, rett RGBA ut.
        let mut bgra = vec![
            10, 20, 30, 255, 0, 0, 0, 0, 100, 50, 25, 128, 255, 255, 255, 255,
        ];
        let has_alpha = pixels::rgba_to_premul_bgra(&mut bgra);
        assert!(has_alpha);
        let png = encode_bytes(&bgra, 2, 2, true).unwrap();
        let back = decode(Cursor::new(png)).unwrap();
        assert_eq!((back.width, back.height, back.alpha), (2, 2, true));
        assert_eq!(back.pixels, bgra);
    }

    #[test]
    fn opaque_images_become_rgb_files() {
        let bgra = vec![1, 2, 3, 255, 4, 5, 6, 255];
        let png = encode_bytes(&bgra, 2, 1, false).unwrap();
        let mut dec = png::Decoder::new(Cursor::new(&png)).read_info().unwrap();
        assert_eq!(dec.info().color_type, png::ColorType::Rgb);
        let mut buf = vec![0; dec.output_buffer_size().unwrap()];
        dec.next_frame(&mut buf).unwrap();
        assert_eq!(buf, vec![3, 2, 1, 6, 5, 4]);
    }

    #[test]
    fn grayscale_and_palette_are_normalised() {
        let mut out = Vec::new();
        let mut enc = png::Encoder::new(&mut out, 1, 1);
        enc.set_color(png::ColorType::Grayscale);
        enc.set_depth(png::BitDepth::Sixteen);
        let mut w = enc.write_header().unwrap();
        w.write_image_data(&[0x80, 0x00]).unwrap();
        w.finish().unwrap();
        let img = decode(Cursor::new(out)).unwrap();
        assert_eq!(img.pixels, vec![0x80, 0x80, 0x80, 255]);
        assert!(!img.alpha);
    }

    #[test]
    fn garbage_is_an_error_not_a_panic() {
        let err = decode(Cursor::new(b"ikke en png".to_vec())).unwrap_err();
        assert_eq!(err, "Filen er ikke en gyldig PNG");
    }

    #[test]
    fn errors_are_in_norwegian() {
        let err = load(Path::new("/finnes/ikke/a.png")).unwrap_err();
        assert_eq!(err, "Filen eller mappen finnes ikke");
        let err = write_atomic(Path::new("/finnes/ikke/a.png"), |_| Ok(())).unwrap_err();
        assert_eq!(
            err,
            "Kunne ikke lagre bildet: Filen eller mappen finnes ikke"
        );
        let full = io::Error::from(ErrorKind::StorageFull);
        assert_eq!(describe(&full), "Disken er full");
    }

    #[test]
    fn images_above_the_default_limit_decode() {
        // 4000×3000 RGBA er 48 MB. 5000×4000 er 80 MB, over png-kratens standard på 64 MiB.
        let (w, h) = (5000u32, 4000u32);
        let mut out = Vec::new();
        let mut enc = png::Encoder::new(&mut out, w, h);
        enc.set_color(png::ColorType::Rgba);
        enc.set_compression(png::Compression::Fast);
        let mut wr = enc.write_header().unwrap();
        wr.write_image_data(&vec![255; (w * h * 4) as usize])
            .unwrap();
        wr.finish().unwrap();
        assert_eq!(decode(Cursor::new(out)).unwrap().width, w);
    }

    #[test]
    fn atomic_write_replaces_and_leaves_no_temp_file() {
        let d = dir("replace");
        let p = d.join("a.png");
        fs::write(&p, b"gammel").unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o640)).unwrap();
        write_atomic(&p, |w| w.write_all(b"ny").map_err(|e| e.to_string())).unwrap();
        assert_eq!(fs::read(&p).unwrap(), b"ny");
        assert_eq!(entries(&d), vec!["a.png".to_string()]);
        let mode = fs::metadata(&p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o640);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn failed_write_keeps_the_old_file() {
        let d = dir("fail");
        let p = d.join("a.png");
        fs::write(&p, b"gammel").unwrap();
        let err = write_atomic(&p, |w| {
            w.write_all(b"halv").unwrap();
            Err("full disk".to_string())
        });
        assert!(err.unwrap_err().contains("full disk"));
        assert_eq!(fs::read(&p).unwrap(), b"gammel");
        assert_eq!(entries(&d), vec!["a.png".to_string()]);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn missing_directory_is_reported() {
        let p = Path::new("/finnes/ikke/a.png");
        assert!(write_atomic(p, |_| Ok(())).is_err());
    }

    #[test]
    fn symlink_stays_a_symlink() {
        let d = dir("link");
        let real = d.join("ekte.png");
        let link = d.join("lenke.png");
        fs::write(&real, b"gammel").unwrap();
        std::os::unix::fs::symlink(&real, &link).unwrap();
        write_atomic(&link, |w| w.write_all(b"ny").map_err(|e| e.to_string())).unwrap();
        assert!(
            fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(fs::read(&real).unwrap(), b"ny");
        let _ = fs::remove_dir_all(&d);
    }
}
