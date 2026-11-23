//! Den samme versjonsloggen som følger kildekoden, vist fra nyest til eldst.
//! Norsk bokmål får den norske loggen, alle andre språk den engelske.

use crate::i18n::{self, Language, tr, tr_fmt};
use adw::prelude::*;
use gtk::{gdk, glib};
use std::cell::RefCell;

use crate::settings::About;

const CHANGELOG: &str = include_str!("../../../CHANGELOG.md");
const CHANGELOG_NB: &str = include_str!("../../../CHANGELOG.nb.md");

fn changelog(language: Language) -> &'static str {
    match language {
        Language::Nb => CHANGELOG_NB,
        _ => CHANGELOG,
    }
}

thread_local! {
    static OPEN: RefCell<glib::WeakRef<adw::Window>> = RefCell::new(glib::WeakRef::new());
}

#[derive(Debug, PartialEq, Eq)]
enum Block {
    Paragraph(String),
    Bullet(String),
    Heading(String),
}

#[derive(Debug)]
struct Release {
    heading: String,
    blocks: Vec<Block>,
}

fn version_key(heading: &str) -> Option<(u32, u32, u32)> {
    let mut parts = heading
        .split_whitespace()
        .next()?
        .trim_start_matches('v')
        .split('.');
    let version = (
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
    );
    parts.next().is_none().then_some(version)
}

/// Begrenset Markdown: versjonsoverskrifter, underoverskrifter, avsnitt og
/// punktlister. Fortsettelseslinjer samles til tekst som GTK kan linjebryte selv.
fn parse(source: &str) -> (Vec<Block>, Vec<Release>) {
    let mut intro = Vec::new();
    let mut releases: Vec<Release> = Vec::new();
    let mut continuation = false;
    for line in source.lines().map(str::trim) {
        if line.is_empty() {
            continuation = false;
            continue;
        }
        if let Some(heading) = line.strip_prefix("## ") {
            releases.push(Release {
                heading: heading.to_owned(),
                blocks: Vec::new(),
            });
            continuation = false;
            continue;
        }
        // Dokumenttittelen står allerede i vinduets tittellinje.
        if line.starts_with("# ") {
            continuation = false;
            continue;
        }
        let blocks = releases
            .last_mut()
            .map_or(&mut intro, |release| &mut release.blocks);
        if let Some(heading) = line.strip_prefix("### ") {
            blocks.push(Block::Heading(heading.to_owned()));
            continuation = false;
        } else if let Some(text) = line.strip_prefix("- ").or_else(|| line.strip_prefix("* ")) {
            blocks.push(Block::Bullet(text.to_owned()));
            continuation = true;
        } else if continuation {
            match blocks.last_mut() {
                Some(Block::Paragraph(text) | Block::Bullet(text)) => {
                    text.push(' ');
                    text.push_str(line);
                }
                _ => unreachable!("fortsettelseslinjer følger bare tekst"),
            }
        } else {
            blocks.push(Block::Paragraph(line.to_owned()));
            continuation = true;
        }
    }
    releases.sort_by_key(|release| std::cmp::Reverse(version_key(&release.heading)));
    (intro, releases)
}

fn label(text: &str) -> gtk::Label {
    gtk::Label::builder()
        .label(text)
        .use_markup(false)
        .selectable(false)
        .xalign(0.0)
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::WordChar)
        .hexpand(true)
        .build()
}

fn append_blocks(container: &gtk::Box, blocks: &[Block]) {
    for block in blocks {
        match block {
            Block::Paragraph(text) => container.append(&label(text)),
            Block::Heading(text) => {
                let heading = label(text);
                heading.add_css_class("heading");
                heading.set_margin_top(6);
                container.append(&heading);
            }
            Block::Bullet(text) => {
                let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
                let bullet = gtk::Label::builder()
                    .label("•")
                    .valign(gtk::Align::Start)
                    .build();
                row.append(&bullet);
                row.append(&label(text));
                container.append(&row);
            }
        }
    }
}

pub(crate) fn open(parent: &adw::Window, about: About) {
    if let Some(window) = OPEN.with_borrow(glib::WeakRef::upgrade) {
        window.present();
        return;
    }

    let (intro, releases) = parse(changelog(i18n::active_language()));
    let log = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(28)
        .margin_top(24)
        .margin_bottom(24)
        .margin_start(24)
        .margin_end(24)
        .build();
    if !intro.is_empty() {
        let introduction = gtk::Box::new(gtk::Orientation::Vertical, 8);
        append_blocks(&introduction, &intro);
        log.append(&introduction);
    }
    for release in releases {
        let section = gtk::Box::new(gtk::Orientation::Vertical, 10);
        let heading = label(&release.heading);
        heading.add_css_class("title-2");
        section.append(&heading);
        append_blocks(&section, &release.blocks);
        log.append(&section);
    }
    let clamp = adw::Clamp::builder()
        .maximum_size(680)
        .tightening_threshold(560)
        .child(&log)
        .build();
    let scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .vexpand(true)
        .child(&clamp)
        .build();

    // Versjon og bygg er synlige også når man blar langt ned i historikken.
    let current = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .margin_top(18)
        .margin_bottom(18)
        .margin_start(24)
        .margin_end(24)
        .build();
    let version = label(&tr_fmt(
        "Agentic Screenshot {version}",
        &[("version", about.version)],
    ));
    version.add_css_class("title-1");
    let build = label(&tr_fmt("Bygg {build}", &[("build", about.build)]));
    build.add_css_class("dim-label");
    current.append(&version);
    current.append(&build);

    let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
    content.append(&current);
    content.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    content.append(&scroll);

    let header = adw::HeaderBar::new();
    header.set_title_widget(Some(&adw::WindowTitle::new(tr("Versjonslogg"), "")));
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&content));

    let window = adw::Window::builder()
        .title(tr("Versjonslogg"))
        .transient_for(parent)
        .modal(true)
        .destroy_with_parent(true)
        .default_width(640)
        .default_height(680)
        .width_request(360)
        .height_request(320)
        .content(&toolbar)
        .build();
    window.set_application(parent.application().as_ref());

    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    let weak = window.downgrade();
    keys.connect_key_pressed(move |_, key, _, _| {
        if key == gdk::Key::Escape {
            if let Some(window) = weak.upgrade() {
                window.close();
            }
            return glib::Propagation::Stop;
        }
        glib::Propagation::Proceed
    });
    window.add_controller(keys);
    OPEN.with_borrow(|open| open.set(Some(&window)));
    window.present();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn versions(source: &str) -> Vec<Option<(u32, u32, u32)>> {
        let (_, releases) = parse(source);
        releases.iter().map(|r| version_key(&r.heading)).collect()
    }

    #[test]
    fn bokmal_gets_the_norwegian_log_and_every_other_language_english() {
        assert!(changelog(Language::Nb).starts_with("# Versjonslogg\n"));
        for language in Language::ALL.into_iter().filter(|l| *l != Language::Nb) {
            let text = changelog(language);
            assert!(text.starts_with("# Changelog\n"), "{}", language.id());
        }
    }

    #[test]
    fn both_logs_cover_the_same_versions_up_to_this_one() {
        let english = versions(CHANGELOG);
        assert_eq!(english, versions(CHANGELOG_NB));
        assert!(english.iter().all(Option::is_some));
        let current = env!("CARGO_PKG_VERSION");
        let (_, releases) = parse(CHANGELOG);
        assert!(releases[0].heading.starts_with(&format!("{current} — ")));
        let (_, releases) = parse(CHANGELOG_NB);
        assert!(releases[0].heading.starts_with(&format!("{current} — ")));
    }

    #[test]
    fn release_order_is_newest_first_and_preserves_intro() {
        let (intro, releases) = parse(
            "# Versjonslogg\n\nFra nyest til eldst.\n\n## 1.2.0 — Tidlig lokal versjon\n- Først\n\n## 1.10.0 — 2026-11-14\n- Nytt\n",
        );
        assert_eq!(intro, vec![Block::Paragraph("Fra nyest til eldst.".into())]);
        assert_eq!(releases[0].heading, "1.10.0 — 2026-11-14");
        assert_eq!(releases[1].heading, "1.2.0 — Tidlig lokal versjon");
    }

    #[test]
    fn renders_wrapped_text_and_subheadings_without_markdown_prefixes() {
        let (_, releases) = parse(
            "## 1.0.0\n### Endringer\n- Første linje\n  og fortsettelsen.\n\nEt avsnitt\nmed to linjer.\n\n* Neste punkt\n",
        );
        assert_eq!(
            releases[0].blocks,
            vec![
                Block::Heading("Endringer".into()),
                Block::Bullet("Første linje og fortsettelsen.".into()),
                Block::Paragraph("Et avsnitt med to linjer.".into()),
                Block::Bullet("Neste punkt".into()),
            ]
        );
    }
}
