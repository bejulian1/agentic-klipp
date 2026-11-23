//! Filer: bildemappen, filnavn, tilstandsfila for hylla og thumbnail-cachen.

use crate::imaging::{self, Image};
use agentic_screenshot_system::i18n::tr_fmt;
use futures_channel::oneshot;
use gtk::{gio, glib};
use std::fs::{self, File, OpenOptions};
use std::hash::{Hash, Hasher};
use std::io;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread::{self, JoinHandle};

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
        "screenshot-{:04}-{:02}-{:02}_{:02}-{:02}-{:02}",
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
        tr_fmt(
            "{width} × {height} px",
            &[
                ("width", &self.w.to_string()),
                ("height", &self.h.to_string()),
            ],
        )
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
    let mut s = String::from("# agentic-screenshot 1\n");
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

pub fn state_dir() -> PathBuf {
    agentic_screenshot_system::user_state_dir().join(agentic_screenshot_system::DIR_NAME)
}

pub fn shelf_path() -> PathBuf {
    state_dir().join("shelf.tsv")
}

pub enum ShotFile {
    Available,
    Deleted,
    Unavailable,
}

pub fn shot_file(path: &Path) -> ShotFile {
    match path.metadata() {
        Ok(meta) if meta.is_file() => ShotFile::Available,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            let parent = path.parent().filter(|p| !p.as_os_str().is_empty());
            // Behold oppføringen når mappa eller lenkemålet er utilgjengelig.
            if parent.unwrap_or(Path::new(".")).is_dir()
                && path
                    .symlink_metadata()
                    .is_err_and(|e| e.kind() == io::ErrorKind::NotFound)
            {
                ShotFile::Deleted
            } else {
                ShotFile::Unavailable
            }
        }
        _ => ShotFile::Unavailable,
    }
}

/// Les hylla og behold oppføringer fra mapper som kan bli tilgjengelige igjen.
fn load_shelf(path: &Path) -> io::Result<Vec<ShotMeta>> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    Ok(parse_shelf(&text)
        .into_iter()
        .filter(|m| !matches!(shot_file(&m.path), ShotFile::Deleted))
        .collect())
}

fn save_shelf(path: &Path, shots: &[ShotMeta]) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    write_atomic(path, serialize_shelf(shots).as_bytes())
}

pub enum ShelfSnapshot {
    /// Endringene som allerede vises mens den gamle historikken lastes.
    BeforeLoad(Vec<ShotMeta>),
    Complete(Vec<ShotMeta>),
}

/// Én tråd leser historikken og skriver alle senere endringer i samme rekkefølge.
pub struct ShelfStorage {
    snapshots: mpsc::Sender<ShelfSnapshot>,
    worker: JoinHandle<()>,
}

impl ShelfStorage {
    pub fn start(path: PathBuf) -> (Self, oneshot::Receiver<Vec<ShotMeta>>) {
        Self::start_loading(path, load_shelf)
    }

    fn start_loading(
        path: PathBuf,
        load: impl FnOnce(&Path) -> io::Result<Vec<ShotMeta>> + Send + 'static,
    ) -> (Self, oneshot::Receiver<Vec<ShotMeta>>) {
        let (loaded, history) = oneshot::channel();
        let (snapshots, pending) = mpsc::channel();
        let worker = thread::spawn(move || {
            let mut initial = match load(&path) {
                Ok(shots) => shots,
                // En ødelagt fil blir ikke lesbar av seg selv. Ta vare på den og start på nytt.
                Err(e) if e.kind() == io::ErrorKind::InvalidData => {
                    let backup = path.with_extension("tsv.ugyldig");
                    eprintln!(
                        "agentic-screenshot: hylla var ødelagt og er flyttet til {}: {e}",
                        backup.display()
                    );
                    if let Err(e) = fs::rename(&path, &backup) {
                        eprintln!("agentic-screenshot: kunne ikke flytte den ødelagte hylla: {e}");
                        return;
                    }
                    Vec::new()
                }
                // Utilgjengelig fil: skriv ingenting, så historikken kan leses neste gang.
                Err(e) => {
                    eprintln!("agentic-screenshot: kunne ikke lese hylla: {e}");
                    return;
                }
            };
            let _ = loaded.send(initial.clone());
            let mut retry = None;
            while let Ok(first) = pending.recv() {
                let snapshot = pending.try_iter().last().unwrap_or(first);
                let shots = match snapshot {
                    ShelfSnapshot::BeforeLoad(mut shots) => {
                        shots.extend_from_slice(&initial);
                        shots
                    }
                    ShelfSnapshot::Complete(shots) => {
                        initial = Vec::new();
                        shots
                    }
                };
                retry = match save_shelf(&path, &shots) {
                    Ok(()) => None,
                    Err(e) => {
                        eprintln!("agentic-screenshot: kunne ikke lagre hylla: {e}");
                        Some(shots)
                    }
                };
            }
            // Gi den siste endringen en ny sjanse ved avslutning hvis skrivingen feilet.
            if let Some(shots) = retry
                && let Err(e) = save_shelf(&path, &shots)
            {
                eprintln!("agentic-screenshot: kunne ikke lagre hylla ved avslutning: {e}");
            }
        });
        (Self { snapshots, worker }, history)
    }

    pub fn save(&self, snapshot: ShelfSnapshot) {
        let _ = self.snapshots.send(snapshot);
    }

    pub fn finish(self) {
        drop(self.snapshots);
        if self.worker.join().is_err() {
            eprintln!("agentic-screenshot: lagringstråden for hylla stoppet uventet");
        }
    }
}

fn cache_dir() -> PathBuf {
    glib::user_cache_dir()
        .join(agentic_screenshot_system::DIR_NAME)
        .join("thumbs")
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
    let (path, file) = create_unique(dir, &file_stem(Stamp::now())).map_err(|e| {
        tr_fmt(
            "Kunne ikke lagre i {path}: {error}",
            &[
                ("path", &agentic_screenshot_system::pretty_dir(dir)),
                ("error", &e.to_string()),
            ],
        )
    })?;
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

/// Slett bare fila portalen ga oss, og aldri noe i bildemappa vår.
pub fn remove_portal_file(path: &Path, own_dir: &Path) -> io::Result<()> {
    if path.starts_with(own_dir) {
        return Ok(());
    }
    fs::remove_file(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shot(path: PathBuf, ts: i64) -> ShotMeta {
        ShotMeta {
            path,
            ts,
            w: 1,
            h: 1,
            alpha: false,
        }
    }

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "agentic-screenshot-st-{}-{name}",
            std::process::id()
        ));
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
        assert_eq!(file_stem(S), "screenshot-2026-03-07_09-05-02");
        assert_eq!(numbered_name("screenshot-x", 1), "screenshot-x.png");
        assert_eq!(numbered_name("screenshot-x", 2), "screenshot-x-2.png");
    }

    #[test]
    fn collisions_get_a_number() {
        let d = tmp("coll");
        let stem = file_stem(S);
        let (a, _f1) = create_unique(&d, &stem).unwrap();
        let (b, _f2) = create_unique(&d, &stem).unwrap();
        let (c, _f3) = create_unique(&d, &stem).unwrap();
        assert!(a.ends_with("screenshot-2026-03-07_09-05-02.png"));
        assert!(b.ends_with("screenshot-2026-03-07_09-05-02-2.png"));
        assert!(c.ends_with("screenshot-2026-03-07_09-05-02-3.png"));
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
        let got = parse_shelf("# agentic-screenshot 1\nsøppel\n1\t2\n5\t1\t1\t0\t/ok.png\n");
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].path, PathBuf::from("/ok.png"));
    }

    #[test]
    fn shelf_from_agentic_klipp_is_read() {
        let got = parse_shelf("# klipp 1\n5\t1\t1\t0\t/Bilder/Klipp/klipp-1.png\n");
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].path, PathBuf::from("/Bilder/Klipp/klipp-1.png"));
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
        let got = load_shelf(&state).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].path, keep);
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn clear_and_undo_survive_restart() {
        let dir = tmp("angre");
        let state = dir.join("shelf.tsv");
        let original = shot(dir.join("bilde.png"), 1);
        fs::write(&original.path, b"x").unwrap();
        save_shelf(&state, std::slice::from_ref(&original)).unwrap();

        let (store, history) = ShelfStorage::start(state.clone());
        let saved = glib::MainContext::new().block_on(history).unwrap();
        store.save(ShelfSnapshot::Complete(Vec::new()));
        store.save(ShelfSnapshot::Complete(saved));
        store.finish();

        assert_eq!(load_shelf(&state).unwrap(), vec![original]);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn capture_during_startup_is_saved_with_history() {
        let dir = tmp("forsinket");
        let state = dir.join("shelf.tsv");
        let old = shot(dir.join("før.png"), 1);
        let capture = shot(dir.join("ny.png"), 2);
        fs::write(&old.path, b"x").unwrap();
        fs::write(&capture.path, b"x").unwrap();
        save_shelf(&state, std::slice::from_ref(&old)).unwrap();

        let (resume, delayed) = mpsc::channel();
        let (store, _history) = ShelfStorage::start_loading(state.clone(), move |path| {
            delayed.recv().unwrap();
            load_shelf(path)
        });
        store.save(ShelfSnapshot::BeforeLoad(vec![capture.clone()]));
        assert_eq!(
            load_shelf(&state).unwrap().as_slice(),
            std::slice::from_ref(&old)
        );
        resume.send(()).unwrap();
        // Avslutt mens UI-et fortsatt venter på historikken.
        store.finish();

        assert_eq!(load_shelf(&state).unwrap(), vec![capture, old]);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn disconnected_folder_survives_reload_and_reconnect() {
        let dir = tmp("frakoblet");
        let state = dir.join("shelf.tsv");
        let pictures = dir.join("Bilder");
        let offline = dir.join("Frakoblet");
        fs::create_dir(&pictures).unwrap();
        let remote = shot(pictures.join("bilde.png"), 1);
        fs::write(&remote.path, b"x").unwrap();
        save_shelf(&state, std::slice::from_ref(&remote)).unwrap();

        fs::rename(&pictures, &offline).unwrap();
        assert!(matches!(shot_file(&remote.path), ShotFile::Unavailable));
        let (store, history) = ShelfStorage::start(state.clone());
        let mut shown = glib::MainContext::new().block_on(history).unwrap();
        assert_eq!(shown.as_slice(), std::slice::from_ref(&remote));

        let local = shot(dir.join("ny.png"), 2);
        fs::write(&local.path, b"x").unwrap();
        shown.insert(0, local.clone());
        store.save(ShelfSnapshot::Complete(shown));
        store.finish();

        fs::rename(&offline, &pictures).unwrap();
        assert!(matches!(shot_file(&remote.path), ShotFile::Available));
        assert_eq!(
            load_shelf(&state).unwrap(),
            vec![local.clone(), remote.clone()]
        );
        fs::remove_file(&remote.path).unwrap();
        assert!(matches!(shot_file(&remote.path), ShotFile::Deleted));
        assert_eq!(load_shelf(&state).unwrap(), vec![local]);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn filesystem_errors_do_not_delete_entries() {
        let dir = tmp("filfeil");
        let state = dir.join("shelf.tsv");
        let parent = dir.join("Bilder");
        fs::write(&parent, b"x").unwrap();
        let image = shot(parent.join("bilde.png"), 1);
        save_shelf(&state, std::slice::from_ref(&image)).unwrap();

        assert!(matches!(shot_file(&image.path), ShotFile::Unavailable));
        assert_eq!(load_shelf(&state).unwrap(), vec![image]);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn failed_history_read_does_not_overwrite_the_state_file() {
        let dir = tmp("lesefeil");
        // En mappe der fila skulle vært, gir en lesefeil som ikke er ødelagt innhold.
        let state = dir.join("shelf.tsv");
        fs::create_dir(&state).unwrap();
        let (store, _history) = ShelfStorage::start(state.clone());
        store.save(ShelfSnapshot::BeforeLoad(vec![shot(dir.join("ny.png"), 2)]));
        store.finish();

        assert!(state.is_dir());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn corrupt_history_is_kept_aside_and_replaced() {
        let dir = tmp("ugyldig");
        let state = dir.join("shelf.tsv");
        let invalid_utf8 = [0xff];
        fs::write(&state, invalid_utf8).unwrap();
        let capture = shot(dir.join("ny.png"), 2);
        fs::write(&capture.path, b"x").unwrap();
        let (store, _history) = ShelfStorage::start(state.clone());
        store.save(ShelfSnapshot::BeforeLoad(vec![capture.clone()]));
        store.finish();

        assert_eq!(
            fs::read(dir.join("shelf.tsv.ugyldig")).unwrap(),
            invalid_utf8
        );
        assert_eq!(load_shelf(&state).unwrap(), vec![capture]);
        fs::remove_dir_all(dir).unwrap();
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
        let own = d.join("Agentic Screenshot");
        fs::create_dir_all(&own).unwrap();
        let ours = own.join("screenshot-1.png");
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
