//! Kommandozeile des Spiels.

use bevy::prelude::Resource;
use std::path::PathBuf;

pub const USAGE: &str = "\
KONFLUENZ

Aufruf: game [Optionen]

  --seed N            Sofort eine neue Partie mit diesem Seed starten (ohne Hauptmenü)
  --load DATEI        Sofort diesen Spielstand laden
  --data-dir PFAD     Verzeichnis der RON-Spieldaten (Standard: assets/data)
  --selftest [N]      Selbsttest: Partie starten, N Frames laufen lassen (Standard 240), beenden
  --screenshot DATEI  Im Selbsttest kurz vor Ende ein Bildschirmfoto speichern
  --help              Diese Hilfe";

#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct Args {
    pub seed: Option<u64>,
    pub load: Option<PathBuf>,
    pub data_dir: Option<PathBuf>,
    pub selftest: Option<u32>,
    pub screenshot: Option<PathBuf>,
    pub help: bool,
}

impl Args {
    pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Self, String> {
        let mut parsed = Self::default();
        let mut args = args.into_iter().peekable();
        while let Some(arg) = args.next() {
            let mut value = |name: &str| args.next().ok_or(format!("{name} braucht einen Wert"));
            match arg.as_str() {
                "--seed" => {
                    let text = value("--seed")?;
                    parsed.seed = Some(
                        text.parse()
                            .map_err(|_| format!("--seed: `{text}` ist keine Zahl"))?,
                    );
                }
                "--load" => parsed.load = Some(PathBuf::from(value("--load")?)),
                "--data-dir" => parsed.data_dir = Some(PathBuf::from(value("--data-dir")?)),
                "--screenshot" => parsed.screenshot = Some(PathBuf::from(value("--screenshot")?)),
                "--selftest" => {
                    let frames = match args.peek() {
                        Some(next) if !next.starts_with("--") => {
                            let text = args.next().unwrap_or_default();
                            text.parse()
                                .map_err(|_| format!("--selftest: `{text}` ist keine Zahl"))?
                        }
                        _ => 240,
                    };
                    parsed.selftest = Some(frames);
                }
                "--help" | "-h" => parsed.help = true,
                other => return Err(format!("Unbekannte Option `{other}`")),
            }
        }
        Ok(parsed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Result<Args, String> {
        Args::parse(text.split_whitespace().map(String::from))
    }

    #[test]
    fn parses_all_options() {
        let args = parse("--seed 7 --data-dir d --selftest 30 --screenshot s.png").unwrap();
        assert_eq!(args.seed, Some(7));
        assert_eq!(args.data_dir, Some(PathBuf::from("d")));
        assert_eq!(args.selftest, Some(30));
        assert_eq!(args.screenshot, Some(PathBuf::from("s.png")));
    }

    #[test]
    fn selftest_has_default_frame_count() {
        assert_eq!(parse("--selftest").unwrap().selftest, Some(240));
        assert_eq!(parse("--selftest --seed 3").unwrap().selftest, Some(240));
    }

    #[test]
    fn rejects_unknown_and_incomplete_options() {
        assert!(parse("--bogus").is_err());
        assert!(parse("--seed").is_err());
        assert!(parse("--seed x").is_err());
    }
}
