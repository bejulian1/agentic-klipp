//! Kommandolinja. Ren parsing, slik at `--help` og `--version` aldri rører GTK.

use crate::capture::geometry::Mode;

pub const HELP: &str = "\
Agentic Klipp: skjermbilder til AI-kodeagenter

Bruk:
  klipp                  Vis vinduet (starter appen om den ikke kjører)
  klipp --capture        Ta et skjermbilde med sist brukte modus
  klipp --capture --mode omrade|frihand|vindu|skjerm
  klipp --hidden         Start skjult, for autostart
  klipp --quit           Avslutt appen
  klipp --version        Vis versjonen
  klipp --help           Vis denne hjelpen
";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    Show,
    Capture(Option<Mode>),
    Hidden,
    Quit,
    Version,
    Help,
}

pub fn mode_arg(s: &str) -> Option<Mode> {
    match s {
        "omrade" | "område" => Some(Mode::Area),
        "frihand" | "frihånd" => Some(Mode::Freehand),
        "vindu" => Some(Mode::Window),
        "skjerm" => Some(Mode::Screen),
        _ => None,
    }
}

pub fn parse<I, S>(args: I) -> Result<Command, String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let (mut capture, mut hidden) = (false, false);
    let mut mode = None;
    let mut it = args.into_iter();
    while let Some(arg) = it.next() {
        let arg = arg.as_ref();
        match arg {
            "--version" | "-V" => return Ok(Command::Version),
            "--help" | "-h" => return Ok(Command::Help),
            "--quit" => return Ok(Command::Quit),
            "--capture" => capture = true,
            "--hidden" => hidden = true,
            "--mode" => {
                let v = it.next().ok_or("--mode trenger en verdi")?;
                mode = Some(parse_mode(v.as_ref())?);
            }
            other => {
                if let Some(v) = other.strip_prefix("--mode=") {
                    mode = Some(parse_mode(v)?);
                } else {
                    return Err(format!("Ukjent argument: {other}"));
                }
            }
        }
    }
    if mode.is_some() && !capture {
        return Err("--mode kan bare brukes sammen med --capture".to_string());
    }
    if capture && hidden {
        return Err("--capture og --hidden kan ikke brukes sammen".to_string());
    }
    Ok(match (capture, hidden) {
        (true, _) => Command::Capture(mode),
        (_, true) => Command::Hidden,
        _ => Command::Show,
    })
}

fn parse_mode(v: &str) -> Result<Mode, String> {
    mode_arg(v)
        .ok_or_else(|| format!("Ukjent modus: {v}. Bruk omrade, frihand, vindu eller skjerm"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(a: &[&str]) -> Result<Command, String> {
        parse(a.iter().copied())
    }

    #[test]
    fn plain_start_shows_window() {
        assert_eq!(p(&[]), Ok(Command::Show));
    }

    #[test]
    fn capture_with_and_without_mode() {
        assert_eq!(p(&["--capture"]), Ok(Command::Capture(None)));
        assert_eq!(
            p(&["--capture", "--mode", "omrade"]),
            Ok(Command::Capture(Some(Mode::Area)))
        );
        assert_eq!(
            p(&["--capture", "--mode", "frihand"]),
            Ok(Command::Capture(Some(Mode::Freehand)))
        );
        assert_eq!(
            p(&["--capture", "--mode=vindu"]),
            Ok(Command::Capture(Some(Mode::Window)))
        );
        assert_eq!(
            p(&["--mode", "skjerm", "--capture"]),
            Ok(Command::Capture(Some(Mode::Screen)))
        );
    }

    #[test]
    fn info_flags_win() {
        assert_eq!(p(&["--version"]), Ok(Command::Version));
        assert_eq!(p(&["--capture", "--help"]), Ok(Command::Help));
    }

    #[test]
    fn hidden_flag() {
        assert_eq!(p(&["--hidden"]), Ok(Command::Hidden));
    }

    #[test]
    fn quit_flag() {
        assert_eq!(p(&["--quit"]), Ok(Command::Quit));
    }

    #[test]
    fn bad_input_is_an_error() {
        assert!(p(&["--capture", "--mode"]).is_err());
        assert!(p(&["--capture", "--mode", "tull"]).is_err());
        assert!(p(&["--mode", "skjerm"]).is_err());
        assert!(p(&["--tull"]).is_err());
        assert!(p(&["--capture", "--hidden"]).is_err());
    }
}
