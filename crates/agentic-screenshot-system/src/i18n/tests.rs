use super::*;
use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

const COMPLETE_LANGUAGES: &[Language] = &[
    Language::En,
    Language::Sv,
    Language::Da,
    Language::De,
    Language::Fr,
    Language::Es,
    Language::It,
    Language::PtBr,
    Language::Nl,
];
const TEMPLATE: &str = include_str!("../../../../po/agentic-screenshot.pot");

#[test]
fn parses_context_plural_continuation_escapes_and_fuzzy() {
    let po = r#"
# Catalog header
msgid ""
msgstr ""
"Language: en\n"

#. Translator note
#: src/main.rs:123
msgctxt ""
"dialog"
msgid ""
"Trykk "
"her"
msgstr "Click \"here\"\\\n"
"\tthen continue"

msgid "{n} bilde"
msgid_plural ""
"{n} bilder"
msgstr[0] "{n} image"
msgstr[1] ""
"{n} images"
#, no-c-format, fuzzy
msgid "Usikker"
msgstr "Guess"

#~ msgid "Gammel"
#~ msgstr "Old"

msgid "Trykk her"
msgstr "Press here"
"#;
    let catalog = parse_po(po);
    assert_eq!(
        catalog["dialog"]["Trykk her"].forms[0],
        "Click \"here\"\\\n\tthen continue"
    );
    let count = &catalog[""]["{n} bilde"];
    assert_eq!(count.plural.as_deref(), Some("{n} bilder"));
    assert_eq!(count.forms, ["{n} image", "{n} images"]);
    assert_eq!(catalog[""]["Trykk her"].forms[0], "Press here");
    assert!(!catalog[""].contains_key("Usikker"));
    assert!(!catalog[""].contains_key("Gammel"));
    assert!(!catalog[""].contains_key(""));
}

#[test]
fn falls_back_to_english_per_form_then_to_bokmal_source() {
    let selected = parse_po(
        r#"
msgid "Hei"
msgstr "Salut"

msgctxt "button"
msgid "Lukk"
msgstr ""

msgid "{n} bilde"
msgid_plural "{n} bilder"
msgstr[0] ""
msgstr[1] "{n} images sélectionnées"
"#,
    );
    let translations = Translations {
        language: Language::Fr,
        selected,
        english: parse_po(
            r#"
msgid "Hei"
msgstr "Hi"

msgctxt "button"
msgid "Lukk"
msgstr "Close"

msgid "{n} bilde"
msgid_plural "{n} bilder"
msgstr[0] "{n} image"
msgstr[1] "{n} images"
"#,
        ),
    };
    assert_eq!(translations.lookup("", "Hei", None), Some("Salut"));
    assert_eq!(translations.lookup("button", "Lukk", None), Some("Close"));
    assert_eq!(translations.lookup("", "Lukk", None), None);
    assert_eq!(
        translations.lookup("", "{n} bilde", Some(0)),
        Some("{n} images")
    );
    assert_eq!(
        translations.lookup("", "{n} bilde", Some(1)),
        Some("{n} image")
    );
    assert_eq!(
        translations.lookup("", "{n} bilde", Some(2)),
        Some("{n} images sélectionnées")
    );
    let (hello, close, context) = ("Hei", "Lukk", "button");
    let (singular, plural) = ("{n} bilde", "{n} bilder");
    assert_eq!(tr(hello), "Hei");
    assert_eq!(trc(context, close), "Lukk");
    assert_eq!(trn(singular, plural, 0), "0 bilder");
    assert_eq!(trn(singular, plural, 1), "1 bilde");
}

#[test]
fn plural_rules_follow_the_catalog_language() {
    for language in Language::ALL.into_iter().skip(1) {
        let zero = if matches!(language, Language::Fr | Language::PtBr) {
            0
        } else {
            1
        };
        assert_eq!(
            [0, 1, 2, 20].map(|n| language.plural_form(n)),
            [zero, 0, 1, 1]
        );
    }
}

#[test]
fn placeholders_can_move_repeat_and_contain_braces() {
    let values = [("path", "/tmp/{name}"), ("name", "Agentic Screenshot")];
    assert_eq!(
        fill("{name}: {path} ({name}) {unknown}", &values),
        "Agentic Screenshot: /tmp/{name} (Agentic Screenshot) {unknown}"
    );
    assert_eq!(fill("no placeholders {", &values), "no placeholders {");
}

#[test]
fn first_supported_system_locale_wins() {
    assert_eq!(resolve_names(["zh_CN", "sv_SE.UTF-8", "de"]), Language::Sv);
    assert_eq!(resolve_names(["nn_NO@euro", "en"]), Language::Nb);
    assert_eq!(resolve_names(["nb_NO.UTF-8"]), Language::Nb);
    assert_eq!(resolve_names(["no"]), Language::Nb);
    assert_eq!(resolve_names(["pt_BR.UTF-8"]), Language::PtBr);
    assert_eq!(resolve_names(["pt-PT"]), Language::PtBr);
    assert_eq!(resolve_names(["C", "POSIX", "ja_JP"]), Language::En);
    assert_eq!(resolve_names([]), Language::En);
}

#[test]
fn checked_in_catalogs_match_the_rust_sources() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let result = Command::new("python3")
        .args(["-I", "scripts/update-po.py", "--check"])
        .current_dir(root)
        .output()
        .expect("could not run the catalog extractor");
    assert!(
        result.status.success(),
        "{}{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
}

fn placeholders(text: &str) -> BTreeSet<&str> {
    text.split('{')
        .skip(1)
        .filter_map(|rest| rest.split_once('}').map(|(name, _)| name))
        .filter(|name| {
            name.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
                && name.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_')
        })
        .collect()
}

#[test]
fn catalog_completeness_and_placeholders() {
    let template = parse_po(TEMPLATE);
    assert!(!template.is_empty());
    for language in Language::ALL
        .into_iter()
        .filter(|l| !l.catalog().is_empty())
    {
        let catalog = parse_po(language.catalog());
        let with_fuzzy = parse_entries(language.catalog(), true);
        for (context, messages) in &template {
            for (msgid, source) in messages {
                let translation = catalog.get(context).and_then(|m| m.get(msgid));
                for (form, source) in std::iter::once(msgid.as_str())
                    .chain(source.plural.as_deref())
                    .enumerate()
                {
                    let value = translation.map_or("", |entry| entry.forms[form].as_str());
                    let message = || format!("{} {context:?} {msgid:?} form {form}", language.id());
                    if COMPLETE_LANGUAGES.contains(&language) {
                        assert!(!value.trim().is_empty(), "Missing {}", message());
                    }
                    if let Some(value) = with_fuzzy
                        .get(context)
                        .and_then(|m| m.get(msgid))
                        .map(|entry| &entry.forms[form])
                        .filter(|s| !s.is_empty())
                    {
                        assert_eq!(placeholders(source), placeholders(value), "{}", message());
                    }
                }
            }
        }
    }
}
