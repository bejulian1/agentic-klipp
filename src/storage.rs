//! Filer: bildemappen, filnavn, tilstandsfila for hylla og thumbnail-cachen.

use crate::imaging::{self, Image};
use gtk::{gio, glib};
use std::fs::{self, File, OpenOptions};
use std::hash::{Hash, Hasher};
use std::io;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

/// Skriv via en midlertidig fil, så en krasj aldri etterlater en halv fil.
pub fn write_atomic(path: &Path, data: &[u8]) -> io::Result<()> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(format!(".tmp{}", std::process::id()));
    let tmp = PathBuf::from(tmp);
    fs::write(&tmp, data)?;
    fs::rename(&tmp, path).inspect_err(|_| {
        let _ = fs::remove_file(&tmp);
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stamp {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
}

impl Stamp {
    pub fn now() -> Stamp {
        match glib::DateTime::now_local() {
            Ok(d) => Stamp {
                year: d.year(),
                month: d.month() as u32,
                day: d.day_of_month() as u32,
                hour: d.hour() as u32,
                minute: d.minute() as u32,
                second: d.second() as u32,
            },
            Err(_) => Stamp {
                year: 1970,
                month: 1,
                day: 1,
                hour: 0,
                minute: 0,
                second: 0,
            },
        }
    }
}

pub fn file_stem(s: Stamp) -> String {
    format!(
        "klipp-{:04}-{:02}-{:02}_{:02}-{:02}-{:02}",
        s.year, s.month, s.day, s.hour, s.minute, s.second
    )
}

/// Navn nummer `n` for et tidsstempel. Det første har ikke suffiks.
pub fn numbered_name(stem: &str, n: u32) -> String {
    if n <= 1 {
        format!("{stem}.png")
    } else {
        format!("{stem}-{n}.png")
    }
}

/// Opprett en ny fil uten å overskrive noe. `create_new` gjør det trygt mot kappløp.
pub fn create_unique(dir: &Path, stem: &str) -> io::Result<(PathBuf, File)> {
    fs::create_dir_all(dir)?;
    let mut n = 1;
    loop {
        let path = dir.join(numbered_name(stem, n));
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(f) => return Ok((path, f)),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => n += 1,
            Err(e) => return Err(e),
        }
    }
}

/// Ett skjermbilde på hylla, slik det står i tilstandsfila.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShotMeta {
    pub path: PathBuf,
    /// Unix-tid i sekunder.
    pub ts: i64,
    pub w: u32,
    pub h: u32,
    pub alpha: bool,
}

impl ShotMeta {
    pub fn name(&self) -> String {
        self.path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    }
    pub fn dims(&self) -> String {
        format!("{} × {} px", self.w, self.h)
    }
}

fn escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('\t', "\\t")
        .replace('\n', "\\n")
}

fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c == '\\' {
            match it.next() {
                Some('t') => out.push('\t'),
                Some('n') => out.push('\n'),
                Some(o) => out.push(o),
                None => {}
            }
        } else {
            out.push(c);
        }
    }
    out
}

pub fn serialize_shelf(shots: &[ShotMeta]) -> String {
    let mut s = String::from("# klipp 1\n");
    for m in shots {
        s.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\n",
            m.ts,
            m.w,
            m.h,
            u8::from(m.alpha),
            escape(&m.path.to_string_lossy())
        ));
    }
    s
}

pub fn parse_shelf(text: &str) -> Vec<ShotMeta> {
    let mut out = Vec::new();
    for line in text.lines() {
        if line.starts_with('#') {
            continue;
        }
        let mut f = line.splitn(5, '\t');
        let parsed = (|| {
            Some(ShotMeta {
                ts: f.next()?.parse().ok()?,
                w: f.next()?.parse().ok()?,
                h: f.next()?.parse().ok()?,
                alpha: f.next()? == "1",
                path: PathBuf::from(unescape(f.next()?)),
            })
        })();
        out.extend(parsed);
    }
    out.sort_by(|a, b| b.ts.cmp(&a.ts));
    out
}

/// XDG state. Bindingene har ikke `g_get_user_state_dir` på vår GLib-funksjon.
pub fn state_dir() -> PathBuf {
    std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| glib::home_dir().join(".local").join("state"))
        .join("klipp")
}

pub fn shelf_path() -> PathBuf {
    state_dir().join("shelf.tsv")
}

/// Les hylla. Filer som er borte fjernes fra lista.
pub fn load_shelf(path: &Path) -> Vec<ShotMeta> {
    let text = fs::read_to_string(path).unwrap_or_default();
    parse_shelf(&text)
        .into_iter()
        .filter(|m| m.path.is_file())
        .collect()
}

pub fn save_shelf(path: &Path, shots: &[ShotMeta]) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    write_atomic(path, serialize_shelf(shots).as_bytes())
}

fn cache_dir() -> PathBuf {
    glib::user_cache_dir().join("klipp").join("thumbs")
}

fn thumb_cache_path(path: &Path, max_w: u32, max_h: u32) -> Option<PathBuf> {
    let md = fs::metadata(path).ok()?;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (path, md.size(), md.mtime(), md.mtime_nsec(), max_w, max_h).hash(&mut h);
    Some(cache_dir().join(format!("{:016x}.png", h.finish())))
}

/// Thumbnail fra cachen, ellers generert fra fila. Skal kjøres utenfor UI-tråden.
pub fn load_thumb(path: &Path, max_w: u32, max_h: u32) -> Result<Image, String> {
    let cached = thumb_cache_path(path, max_w, max_h);
    if let Some(img) = cached.as_ref().and_then(|c| imaging::decode_png(c).ok()) {
        return Ok(img);
    }
    let full = imaging::decode_png(path)?;
    let thumb = make_thumb(&full, max_w, max_h);
    drop(full);
    if let Some(c) = cached {
        store_thumb(&c, &thumb);
    }
    Ok(thumb)
}

pub fn make_thumb(full: &Image, max_w: u32, max_h: u32) -> Image {
    let (w, h) = imaging::fit_size(full.w, full.h, max_w, max_h);
    imaging::downscale(full, w, h)
}

fn store_thumb(path: &Path, thumb: &Image) {
    if let Some(dir) = path.parent()
        && fs::create_dir_all(dir).is_ok()
        && let Ok(bytes) = imaging::encode_png_bytes(thumb, true)
    {
        let _ = write_atomic(path, &bytes);
    }
}

/// Legg et ferdig thumbnail i cachen for en fil vi nettopp skrev.
pub fn cache_thumb(path: &Path, thumb: &Image, max_w: u32, max_h: u32) {
    if let Some(c) = thumb_cache_path(path, max_w, max_h) {
        store_thumb(&c, thumb);
    }
}

/// Lagre et nytt skjermbilde og lag thumbnail. Skal kjøres utenfor UI-tråden.
pub fn save_new_shot(
    dir: &Path,
    img: &Image,
    alpha: bool,
    max_w: u32,
    max_h: u32,
) -> Result<(ShotMeta, Image), String> {
    let (path, file) = create_unique(dir, &file_stem(Stamp::now()))
        .map_err(|e| format!("Kunne ikke lagre i {}: {e}", klipp_system::pretty_dir(dir)))?;
    if let Err(e) = imaging::encode_png_file(img, alpha, file) {
        let _ = fs::remove_file(&path);
        return Err(e);
    }
    let ts = glib::real_time() / 1_000_000;
    let thumb = make_thumb(img, max_w, max_h);
    cache_thumb(&path, &thumb, max_w, max_h);
    Ok((
        ShotMeta {
            path,
            ts,
            w: img.w,
            h: img.h,
            alpha,
        },
        thumb,
    ))
}

/// Stien til fila portalen nettopp returnerte, om det er en lokal fil.
pub fn local_path_of_uri(uri: &str) -> Option<PathBuf> {
    if !uri.starts_with("file:") {
        return None;
    }
    use gio::prelude::FileExt;
    gio::File::for_uri(uri).path()
}

/// Slett bare fila portalen ga oss, og aldri noe i Klipp-mappa vår.
pub fn remove_portal_file(path: &Path, own_dir: &Path) -> io::Result<()> {
    if path.starts_with(own_dir) {
        return Ok(());
    }
    fs::remove_file(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("klipp-st-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    const S: Stamp = Stamp {
        year: 2026,
        month: 3,
        day: 7,
        hour: 9,
        minute: 5,
        second: 2,
    };

    #[test]
    fn file_name_format() {
        assert_eq!(file_stem(S), "klipp-2026-03-07_09-05-02");
        assert_eq!(numbered_name("klipp-x", 1), "klipp-x.png");
        assert_eq!(numbered_name("klipp-x", 2), "klipp-x-2.png");
    }

    #[test]
    fn collisions_get_a_number() {
        let d = tmp("coll");
        let stem = file_stem(S);
        let (a, _f1) = create_unique(&d, &stem).unwrap();
        let (b, _f2) = create_unique(&d, &stem).unwrap();
        let (c, _f3) = create_unique(&d, &stem).unwrap();
        assert!(a.ends_with("klipp-2026-03-07_09-05-02.png"));
        assert!(b.ends_with("klipp-2026-03-07_09-05-02-2.png"));
        assert!(c.ends_with("klipp-2026-03-07_09-05-02-3.png"));
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn creates_missing_folder() {
        let root = tmp("mk");
        let d = root.join("a").join("b");
        let (p, _f) = create_unique(&d, "x").unwrap();
        assert!(p.exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn shelf_round_trip_sorts_newest_first_and_escapes() {
        let shots = vec![
            ShotMeta {
                path: "/a/b.png".into(),
                ts: 10,
                w: 1,
                h: 2,
                alpha: false,
            },
            ShotMeta {
                path: "/a/med\ttab og\nlinje\\.png".into(),
                ts: 20,
                w: 3,
                h: 4,
                alpha: true,
            },
        ];
        let got = parse_shelf(&serialize_shelf(&shots));
        assert_eq!(got[0], shots[1]);
        assert_eq!(got[1], shots[0]);
    }

    #[test]
    fn bad_shelf_lines_are_skipped() {
        let got = parse_shelf("# klipp 1\nsøppel\n1\t2\n5\t1\t1\t0\t/ok.png\n");
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].path, PathBuf::from("/ok.png"));
    }

    #[test]
    fn missing_files_leave_the_shelf() {
        let d = tmp("miss");
        let keep = d.join("keep.png");
        fs::write(&keep, b"x").unwrap();
        let shots = vec![
            ShotMeta {
                path: keep.clone(),
                ts: 2,
                w: 1,
                h: 1,
                alpha: false,
            },
            ShotMeta {
                path: d.join("borte.png"),
                ts: 1,
                w: 1,
                h: 1,
                alpha: false,
            },
        ];
        let state = d.join("state").join("shelf.tsv");
        save_shelf(&state, &shots).unwrap();
        let got = load_shelf(&state);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].path, keep);
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn portal_uri_is_decoded() {
        assert_eq!(
            local_path_of_uri("file:///home/a/Bilder/Skjermbilde%20fra.png"),
            Some(PathBuf::from("/home/a/Bilder/Skjermbilde fra.png"))
        );
        assert_eq!(local_path_of_uri("https://x/y.png"), None);
    }

    #[test]
    fn portal_cleanup_never_touches_our_folder() {
        let d = tmp("port");
        let own = d.join("Klipp");
        fs::create_dir_all(&own).unwrap();
        let ours = own.join("klipp-1.png");
        let theirs = d.join("Skjermbilde.png");
        let other = d.join("annet.png");
        for f in [&ours, &theirs, &other] {
            fs::write(f, b"x").unwrap();
        }
        remove_portal_file(&ours, &own).unwrap();
        remove_portal_file(&theirs, &own).unwrap();
        assert!(ours.exists());
        assert!(!theirs.exists());
        assert!(other.exists());
        let _ = fs::remove_dir_all(d);
    }
}
