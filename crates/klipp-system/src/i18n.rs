//! Språkvalg og innebygde gettext-kataloger. GTK initialiseres ikke av oppslag.

use gtk::glib;
use std::collections::HashMap;
use std::sync::OnceLock;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    #[default]
    System,
    Nb,
    En,
    Sv,
    Da,
    De,
    Fr,
    Es,
    It,
    PtBr,
    Nl,
}

impl Language {
    pub const ALL: [Self; 11] = [
        Self::System,
        Self::Nb,
        Self::En,
        Self::Sv,
        Self::Da,
        Self::De,
        Self::Fr,
        Self::Es,
        Self::It,
        Self::PtBr,
        Self::Nl,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Nb => "nb",
            Self::En => "en",
            Self::Sv => "sv",
            Self::Da => "da",
            Self::De => "de",
            Self::Fr => "fr",
            Self::Es => "es",
            Self::It => "it",
            Self::PtBr => "pt_BR",
            Self::Nl => "nl",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|lang| lang.id() == id)
    }

    pub fn endonym(self) -> &'static str {
        match self {
            Self::System => system_language().endonym(),
            Self::Nb => "Norsk bokmål",
            Self::En => "English",
            Self::Sv => "Svenska",
            Self::Da => "Dansk",
            Self::De => "Deutsch",
            Self::Fr => "Français",
            Self::Es => "Español",
            Self::It => "Italiano",
            Self::PtBr => "Português (Brasil)",
            Self::Nl => "Nederlands",
        }
    }

    pub fn resolve(self) -> Self {
        if self == Self::System {
            system_language()
        } else {
            self
        }
    }

    fn catalog(self) -> &'static str {
        match self {
            Self::System | Self::Nb => "",
            Self::En => include_str!("../../../po/en.po"),
            Self::Sv => include_str!("../../../po/sv.po"),
            Self::Da => include_str!("../../../po/da.po"),
            Self::De => include_str!("../../../po/de.po"),
            Self::Fr => include_str!("../../../po/fr.po"),
            Self::Es => include_str!("../../../po/es.po"),
            Self::It => include_str!("../../../po/it.po"),
            Self::PtBr => include_str!("../../../po/pt_BR.po"),
            Self::Nl => include_str!("../../../po/nl.po"),
        }
    }

    fn plural_form(self, n: usize) -> usize {
        usize::from(match self {
            Self::Fr | Self::PtBr => n > 1,
            _ => n != 1,
        })
    }
}

pub fn system_language() -> Language {
    resolve_names(glib::language_names().iter().map(|name| name.as_str()))
}

fn resolve_names<'a>(names: impl IntoIterator<Item = &'a str>) -> Language {
    names
        .into_iter()
        .filter_map(|name| match name.split(['.', '@', '_', '-']).next()? {
            "nb" | "nn" | "no" => Some(Language::Nb),
            "pt" => Some(Language::PtBr),
            "system" => None,
            id => Language::from_id(id),
        })
        .next()
        .unwrap_or(Language::En)
}

static ACTIVE: OnceLock<Translations> = OnceLock::new();

/// Kalles før kommandolinjen og GTK leser tekst. Oppslag kan gjøres fra alle tråder.
pub fn init(language: Language) {
    ACTIVE.get_or_init(|| {
        let language = language.resolve();
        Translations {
            language,
            selected: parse_po(language.catalog()),
            english: if matches!(language, Language::Nb | Language::En) {
                Catalog::default()
            } else {
                parse_po(Language::En.catalog())
            },
        }
    });
}

pub fn active_language() -> Language {
    ACTIVE.get().map_or(Language::Nb, |active| active.language)
}

pub fn tr(msgid: &'static str) -> &'static str {
    trc("", msgid)
}

pub fn trc(context: &str, msgid: &'static str) -> &'static str {
    ACTIVE
        .get()
        .and_then(|active| active.lookup(context, msgid, None))
        .unwrap_or(msgid)
}

pub fn trn(singular: &'static str, plural: &'static str, n: usize) -> String {
    let template = ACTIVE
        .get()
        .and_then(|active| active.lookup("", singular, Some(n)))
        .unwrap_or(if n == 1 { singular } else { plural });
    fill(template, &[("n", &n.to_string())])
}

pub fn tr_fmt(msgid: &'static str, values: &[(&str, &str)]) -> String {
    fill(tr(msgid), values)
}

/// Sett bare inn verdier i malen, også når en verdi selv inneholder klammer.
fn fill(mut rest: &str, values: &[(&str, &str)]) -> String {
    let mut result = String::with_capacity(rest.len());
    while let Some((before, after)) = rest.split_once('{') {
        result.push_str(before);
        rest = after;
        if let Some((name, tail)) = after.split_once('}')
            && let Some((_, value)) = values.iter().find(|(key, _)| *key == name)
        {
            result.push_str(value);
            rest = tail;
        } else {
            result.push('{');
        }
    }
    result.push_str(rest);
    result
}

#[derive(Default)]
struct Message {
    plural: Option<String>,
    forms: [String; 2],
}

type Catalog = HashMap<String, HashMap<String, Message>>;

struct Translations {
    language: Language,
    selected: Catalog,
    english: Catalog,
}

impl Translations {
    fn lookup(&self, context: &str, msgid: &str, n: Option<usize>) -> Option<&str> {
        [
            (&self.selected, self.language),
            (&self.english, Language::En),
        ]
        .into_iter()
        .find_map(|(catalog, language)| {
            let message = catalog.get(context)?.get(msgid)?;
            let index = n.map_or(0, |n| language.plural_form(n));
            Some(message.forms[index].as_str()).filter(|s| !s.is_empty())
        })
    }
}

#[derive(Clone, Copy)]
enum Field {
    Context,
    Id,
    Plural,
    Form(usize),
}

#[derive(Default)]
struct Entry {
    context: String,
    id: Option<String>,
    message: Message,
    fuzzy: bool,
}

impl Entry {
    fn insert(self, catalog: &mut Catalog, include_fuzzy: bool) {
        if (include_fuzzy || !self.fuzzy)
            && let Some(id) = self.id.filter(|id| !id.is_empty())
        {
            catalog
                .entry(self.context)
                .or_default()
                .insert(id, self.message);
        }
    }

    fn append(&mut self, field: Field, text: &str) -> Option<()> {
        let dest = match field {
            Field::Context => &mut self.context,
            Field::Id => self.id.get_or_insert_with(String::new),
            Field::Plural => self.message.plural.get_or_insert_with(String::new),
            Field::Form(i) => self.message.forms.get_mut(i)?,
        };
        dest.push_str(&unquote(text)?);
        Some(())
    }
}

fn parse_po(text: &str) -> Catalog {
    parse_entries(text, false)
}

fn parse_entries(text: &str, include_fuzzy: bool) -> Catalog {
    let mut catalog = Catalog::new();
    let mut entry = Entry::default();
    let mut current = None;
    for line in text.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') {
            if line.is_empty() || entry.id.is_some() {
                std::mem::take(&mut entry).insert(&mut catalog, include_fuzzy);
            }
            if let Some(flags) = line.strip_prefix("#,") {
                entry.fuzzy |= flags.split(',').any(|flag| flag.trim() == "fuzzy");
            }
            current = None;
            continue;
        }
        if line.starts_with('"') {
            if let Some(field) = current {
                entry.append(field, line);
            }
            continue;
        }
        current = None;
        let Some((keyword, value)) = line.split_once(char::is_whitespace) else {
            continue;
        };
        let field = match keyword {
            "msgctxt" => Field::Context,
            "msgid" => Field::Id,
            "msgid_plural" => Field::Plural,
            "msgstr" => Field::Form(0),
            "msgstr[0]" => Field::Form(0),
            "msgstr[1]" => Field::Form(1),
            _ => continue,
        };
        if matches!(field, Field::Context | Field::Id) && entry.id.is_some() {
            std::mem::take(&mut entry).insert(&mut catalog, include_fuzzy);
        }
        if entry.append(field, value.trim()).is_some() {
            current = Some(field);
        }
    }
    entry.insert(&mut catalog, include_fuzzy);
    catalog
}

fn unquote(text: &str) -> Option<String> {
    let mut chars = text.strip_prefix('"')?.strip_suffix('"')?.chars();
    let mut result = String::new();
    while let Some(c) = chars.next() {
        result.push(if c == '\\' {
            match chars.next()? {
                'n' => '\n',
                't' => '\t',
                'r' => '\r',
                '"' => '"',
                '\\' => '\\',
                _ => return None,
            }
        } else {
            c
        });
    }
    Some(result)
}

#[cfg(test)]
mod tests;
