//! Kommandozeile des Spiels.

use bevy::prelude::Resource;
use std::path::PathBuf;

pub const USAGE: &str = "\
KONFLUENZ

Aufruf: game [Optionen]

  --seed N            Sofort eine neue Partie mit diesem Seed starten (ohne Hauptmenü)
  --load DATEI        Sofort diesen Spielstand laden
  --practice          Sofort eine Gefechtsübung starten; --seed wählt dann ihren Verlauf
  --fleet-a LISTE     Flotte des blauen Verbands, z. B. speer:4,waechter:6
  --fleet-b LISTE     Flotte des roten Verbands
  --lineup            Mit --practice: je ein Schiff jeder Klasse nebeneinander zeigen
  --data-dir PFAD     Verzeichnis der RON-Spieldaten (Standard: assets/data)
  --selftest [N]      Selbsttest: Partie starten, N Frames laufen lassen (Standard 240),
                      beenden; mit --practice eine Gefechtsübung, höchstens N Frames.
                      Endet mit einem Fehler, wenn die Kamera der Ansicht dabei nicht
                      zeichnete, nichts von Karte oder Gefecht in ihrem Sichtfeld lag oder
                      ein verlangtes Foto nicht entstand
  --screenshot DATEI  Im Selbsttest Bildschirmfotos speichern; die Gefechtsübung
                      nummeriert sie (DATEI_1.png, DATEI_2.png, …)
  --help              Diese Hilfe";

#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct Args {
    pub seed: Option<u64>,
    pub load: Option<PathBuf>,
    pub data_dir: Option<PathBuf>,
    pub selftest: Option<u32>,
    pub screenshot: Option<PathBuf>,
    /// Gefechtsübung statt Partie.
    pub practice: bool,
    pub fleet_a: Option<String>,
    pub fleet_b: Option<String>,
    pub lineup: bool,
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
                "--practice" => parsed.practice = true,
                "--fleet-a" => parsed.fleet_a = Some(value("--fleet-a")?),
                "--fleet-b" => parsed.fleet_b = Some(value("--fleet-b")?),
                "--lineup" => parsed.lineup = true,
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
        if !parsed.practice {
            for (given, name) in [
                (parsed.fleet_a.is_some(), "--fleet-a"),
                (parsed.fleet_b.is_some(), "--fleet-b"),
                (parsed.lineup, "--lineup"),
            ] {
                if given {
                    return Err(format!("{name} gilt nur zusammen mit --practice"));
                }
            }
        } else if parsed.load.is_some() {
            return Err("--practice und --load schließen einander aus".into());
        }
        Ok(parsed)
    }
}

/// Dateiname eines nummerierten Bildschirmfotos: aus `bild.png` wird `bild_2.png`.
pub fn numbered(path: &std::path::Path, number: u32) -> PathBuf {
    let stem = path
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| "bild".into());
    let extension = path
        .extension()
        .map(|extension| extension.to_string_lossy().into_owned())
        .unwrap_or_else(|| "png".into());
    path.with_file_name(format!("{stem}_{number}.{extension}"))
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
        assert!(!args.practice);
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

    #[test]
    fn parses_practice_options() {
        let args =
            parse("--practice --fleet-a speer:4,waechter:6 --fleet-b bastion:2 --seed 9").unwrap();
        assert!(args.practice);
        assert_eq!(args.fleet_a.as_deref(), Some("speer:4,waechter:6"));
        assert_eq!(args.fleet_b.as_deref(), Some("bastion:2"));
        assert_eq!(args.seed, Some(9));
        assert!(parse("--practice --lineup --selftest").unwrap().lineup);
        // Flottenlisten ohne Übung sind ein Versehen und werden gemeldet.
        assert!(parse("--fleet-a speer:4").is_err());
        assert!(parse("--lineup").is_err());
        assert!(parse("--fleet-b").is_err());
        assert!(parse("--practice --load spiel.ron").is_err());
    }

    #[test]
    fn numbers_screenshot_files() {
        assert_eq!(
            numbered(std::path::Path::new("/tmp/x/gefecht.png"), 2),
            PathBuf::from("/tmp/x/gefecht_2.png")
        );
        assert_eq!(
            numbered(std::path::Path::new("bild"), 1),
            PathBuf::from("bild_1.png")
        );
    }
}
