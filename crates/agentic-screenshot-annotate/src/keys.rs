//! Tastaturet i annoteringsvinduet, som `annotateKeys` i prototypen. Uten GTK, så det kan
//! testes. Vinduet sender ikke tastene hit mens man skriver tekst.

use crate::ops::Tool;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KeyPress {
    /// Tegnet tasten gir, med små bokstaver.
    pub ch: Option<char>,
    pub enter: bool,
    pub escape: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    /// Tasten holdes nede og gjentas.
    pub repeat: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyAction {
    Escape,
    Undo,
    Redo,
    Copy,
    Done,
    Tool(Tool),
    Color(usize),
    Thicker,
    Thinner,
}

pub fn action(k: KeyPress) -> Option<KeyAction> {
    if k.escape {
        return Some(KeyAction::Escape);
    }
    if k.ctrl && !k.alt {
        return match k.ch {
            _ if k.enter => Some(KeyAction::Done),
            Some('z') if k.shift => Some(KeyAction::Redo),
            Some('z') => Some(KeyAction::Undo),
            Some('y') => Some(KeyAction::Redo),
            Some('c') => Some(KeyAction::Copy),
            _ => None,
        };
    }
    if k.ctrl || k.alt || k.repeat {
        return None;
    }
    let ch = k.ch?;
    if let Some(tool) = Tool::from_key(ch).filter(|_| !k.shift) {
        return Some(KeyAction::Tool(tool));
    }
    match ch {
        '1'..='9' => Some(KeyAction::Color(ch as usize - '1' as usize)),
        '+' => Some(KeyAction::Thicker),
        '-' => Some(KeyAction::Thinner),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ch(c: char) -> KeyPress {
        KeyPress {
            ch: Some(c),
            ..KeyPress::default()
        }
    }

    #[test]
    fn tool_keys_select_tools() {
        assert_eq!(action(ch('p')), Some(KeyAction::Tool(Tool::Arrow)));
        assert_eq!(action(ch('s')), Some(KeyAction::Tool(Tool::Redact)));
        assert_eq!(action(ch('e')), Some(KeyAction::Tool(Tool::Eraser)));
        assert_eq!(action(ch('h')), Some(KeyAction::Tool(Tool::Highlight)));
        let shifted = KeyPress {
            shift: true,
            ..ch('p')
        };
        assert_eq!(action(shifted), None);
    }

    #[test]
    fn digits_pick_colours_one_to_nine() {
        assert_eq!(action(ch('1')), Some(KeyAction::Color(0)));
        assert_eq!(action(ch('8')), Some(KeyAction::Color(7)));
        assert_eq!(action(ch('9')), Some(KeyAction::Color(8)), "egen farge");
        assert_eq!(action(ch('0')), None);
    }

    #[test]
    fn plus_and_minus_change_thickness_but_do_not_repeat() {
        assert_eq!(action(ch('+')), Some(KeyAction::Thicker));
        assert_eq!(action(ch('-')), Some(KeyAction::Thinner));
        let held = KeyPress {
            repeat: true,
            ..ch('+')
        };
        assert_eq!(action(held), None);
    }

    #[test]
    fn control_chords() {
        let ctrl = |c| KeyPress {
            ctrl: true,
            ..ch(c)
        };
        assert_eq!(action(ctrl('z')), Some(KeyAction::Undo));
        assert_eq!(
            action(KeyPress {
                shift: true,
                ..ctrl('z')
            }),
            Some(KeyAction::Redo)
        );
        assert_eq!(action(ctrl('y')), Some(KeyAction::Redo));
        assert_eq!(action(ctrl('c')), Some(KeyAction::Copy));
        assert_eq!(action(ctrl('p')), None);
        let enter = KeyPress {
            ctrl: true,
            enter: true,
            ..KeyPress::default()
        };
        assert_eq!(action(enter), Some(KeyAction::Done));
    }

    #[test]
    fn undo_may_repeat_while_held() {
        let held = KeyPress {
            ctrl: true,
            repeat: true,
            ..ch('z')
        };
        assert_eq!(action(held), Some(KeyAction::Undo));
    }

    #[test]
    fn alt_blocks_everything_but_escape() {
        let alt = KeyPress {
            alt: true,
            ..ch('p')
        };
        assert_eq!(action(alt), None);
        let esc = KeyPress {
            escape: true,
            alt: true,
            ..KeyPress::default()
        };
        assert_eq!(action(esc), Some(KeyAction::Escape));
    }

    #[test]
    fn plain_enter_does_nothing() {
        let enter = KeyPress {
            enter: true,
            ..KeyPress::default()
        };
        assert_eq!(action(enter), None);
    }
}
