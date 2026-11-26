//! Das Drehbuch auf der Kommandozeile: Kriege, Friedensschlüsse, Bestellungen und
//! Flottenprogramme für `run`, `sweep`, `hash` und `fleets`. Gespielt wird es von
//! `sim::harness::WarScript`.

use clap::Args;
use sim::GameData;
use sim::battle::matchup::Side;
use sim::clock::DAYS_PER_YEAR;
use sim::harness::{ScriptedAct, ScriptedEvent, WarScript, YardChoice};
use sim::ids::PolityId;

#[derive(Args, Clone, Default)]
pub struct ScriptArgs {
    /// Krieg nach Drehbuch, beliebig oft: Zu Beginn des Jahres erklärt Gemeinschaft A
    /// (Nummer ab 0) der Gemeinschaft B den Krieg und schickt ihre Flotten im
    /// Blockadeauftrag in deren Heimatsystem.
    #[arg(long, value_name = "JAHR:A:B", value_parser = parse_war)]
    pub war: Vec<ScriptedEvent>,
    /// Frieden nach Drehbuch, beliebig oft: Zu Beginn des Jahres schließt A mit B Frieden.
    #[arg(long, value_name = "JAHR:A:B", value_parser = parse_peace)]
    pub peace: Vec<ScriptedEvent>,
    /// Bestellung nach Drehbuch, beliebig oft: Zu Beginn des Jahres bestellt Gemeinschaft
    /// WER (Nummer ab 0, `alle` für jede) die Schiffe, z. B. `11:1:speer:1,dolch:2`. Mit
    /// `@kolonie` am Ende geht die Bestellung an die Kolonie mit den meisten Bauplätzen
    /// statt an die Hauptwelt.
    #[arg(long, value_name = "JAHR:WER:SCHIFFE[@ORT]")]
    pub order: Vec<String>,
    /// Flottenprogramm nach Drehbuch, beliebig oft: Vom Beginn des Jahres an hält WER
    /// (Nummer ab 0, `alle` für jede) seine Marine auf dem FAKTOR-fachen der Startflotte
    /// und bestellt jeden Monat nach, was fehlt, z. B. `10:alle:1` für Ersatz aller
    /// Verluste. Faktor 0 beendet das Programm.
    #[arg(long, value_name = "JAHR:WER:FAKTOR")]
    pub navy: Vec<String>,
}

impl ScriptArgs {
    /// Das Drehbuch aus allen Angaben. Entwürfe werden gegen die Spieldaten geprüft.
    pub fn script(&self, data: &GameData) -> Result<WarScript, String> {
        let mut events: Vec<ScriptedEvent> = self.war.iter().chain(&self.peace).cloned().collect();
        for text in &self.order {
            events
                .push(parse_order(data, text).map_err(|error| format!("--order {text}: {error}"))?);
        }
        for text in &self.navy {
            events.push(parse_navy(text).map_err(|error| format!("--navy {text}: {error}"))?);
        }
        Ok(WarScript::new(events))
    }
}

fn number(text: &str, what: &str) -> Result<u32, String> {
    text.trim()
        .parse::<u32>()
        .map_err(|_| format!("{what} „{}“ ist keine Zahl", text.trim()))
}

/// Liest `JAHR:A:B`, z. B. `12:0:3`, als Tag und die beiden Gemeinschaften.
fn parse_pair(text: &str) -> Result<(u64, PolityId, PolityId), String> {
    let parts: Vec<&str> = text.split(':').collect();
    let [year, a, b] = parts.as_slice() else {
        return Err("erwartet wird JAHR:A:B, z. B. 12:0:3".into());
    };
    let (a, b) = (
        PolityId(number(a, "Gemeinschaft")?),
        PolityId(number(b, "Gemeinschaft")?),
    );
    if a == b {
        return Err("A und B müssen verschiedene Gemeinschaften sein".into());
    }
    Ok((number(year, "Jahr")? as u64 * DAYS_PER_YEAR, a, b))
}

pub fn parse_war(text: &str) -> Result<ScriptedEvent, String> {
    parse_pair(text).map(|(day, a, b)| ScriptedEvent::war(day, a, b))
}

pub fn parse_peace(text: &str) -> Result<ScriptedEvent, String> {
    parse_pair(text).map(|(day, a, b)| ScriptedEvent::peace(day, a, b))
}

/// Liest `JAHR:WER:REST`: den Tag, die Gemeinschaft (`alle` oder `*` für jede) und den Rest.
fn parse_head(text: &str) -> Result<(u64, Option<PolityId>, &str), String> {
    let mut parts = text.splitn(3, ':');
    let (Some(year), Some(who), Some(rest)) = (parts.next(), parts.next(), parts.next()) else {
        return Err("erwartet wird JAHR:WER:…, z. B. 11:1:… oder 11:alle:…".into());
    };
    let who = match who.trim() {
        "alle" | "*" => None,
        other => Some(PolityId(number(other, "Gemeinschaft")?)),
    };
    Ok((number(year, "Jahr")? as u64 * DAYS_PER_YEAR, who, rest))
}

/// Liest `JAHR:WER:SCHIFFE[@ORT]`, z. B. `11:1:speer:1,dolch:2@kolonie`.
pub fn parse_order(data: &GameData, text: &str) -> Result<ScriptedEvent, String> {
    let (day, who, rest) = parse_head(text)?;
    let (ships, yard) = match rest.split_once('@') {
        None => (rest, YardChoice::Capital),
        Some((ships, place)) => (
            ships,
            match place.trim() {
                "hauptwelt" => YardChoice::Capital,
                "kolonie" => YardChoice::Colony,
                other => {
                    return Err(format!(
                        "unbekannter Ort „{other}“; möglich sind hauptwelt und kolonie"
                    ));
                }
            },
        ),
    };
    Ok(ScriptedEvent {
        day,
        act: ScriptedAct::Order {
            who,
            ships: Side::parse(data, ships)?.ships,
            yard,
        },
    })
}

/// Liest `JAHR:WER:FAKTOR`, z. B. `10:alle:2`.
pub fn parse_navy(text: &str) -> Result<ScriptedEvent, String> {
    let (day, who, rest) = parse_head(text)?;
    let factor: f32 = rest
        .trim()
        .parse()
        .map_err(|_| format!("Faktor „{}“ ist keine Zahl", rest.trim()))?;
    if !(0.0..=16.0).contains(&factor) {
        return Err("der Faktor muss zwischen 0 und 16 liegen".into());
    }
    Ok(ScriptedEvent {
        day,
        act: ScriptedAct::Navy { who, factor },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_are_parsed_and_checked() {
        assert_eq!(
            parse_war("12:0:3"),
            Ok(ScriptedEvent::war(
                12 * DAYS_PER_YEAR,
                PolityId(0),
                PolityId(3)
            ))
        );
        assert_eq!(
            parse_peace(" 2 : 1 : 0 "),
            Ok(ScriptedEvent::peace(
                2 * DAYS_PER_YEAR,
                PolityId(1),
                PolityId(0)
            ))
        );
        assert!(parse_war("12:0").is_err());
        assert!(parse_war("zwölf:0:3").is_err());
        assert!(parse_war("12:2:2").is_err());
        let data = GameData::load_workspace().unwrap();
        assert!(ScriptArgs::default().script(&data).unwrap().is_empty());
        let args = ScriptArgs {
            war: vec![parse_war("1:0:1").unwrap()],
            ..Default::default()
        };
        assert!(!args.script(&data).unwrap().is_empty());
    }

    #[test]
    fn orders_and_programmes_are_parsed_against_the_game_data() {
        let data = GameData::load_workspace().unwrap();
        let speer = data.design_index("speer").unwrap();
        let dolch = data.design_index("dolch").unwrap();
        assert_eq!(
            parse_order(&data, "11:1:speer:1,dolch:2"),
            Ok(ScriptedEvent {
                day: 11 * DAYS_PER_YEAR,
                act: ScriptedAct::Order {
                    who: Some(PolityId(1)),
                    ships: vec![(speer, 1), (dolch, 2)],
                    yard: YardChoice::Capital,
                },
            })
        );
        assert_eq!(
            parse_order(&data, "3:alle:dolch@kolonie"),
            Ok(ScriptedEvent {
                day: 3 * DAYS_PER_YEAR,
                act: ScriptedAct::Order {
                    who: None,
                    ships: vec![(dolch, 1)],
                    yard: YardChoice::Colony,
                },
            })
        );
        assert!(parse_order(&data, "3:alle:kahn").is_err());
        assert!(parse_order(&data, "3:alle:dolch@mond").is_err());
        assert!(parse_order(&data, "dolch:2").is_err());
        assert_eq!(
            parse_navy("10:*:2.5"),
            Ok(ScriptedEvent {
                day: 10 * DAYS_PER_YEAR,
                act: ScriptedAct::Navy {
                    who: None,
                    factor: 2.5
                },
            })
        );
        assert!(parse_navy("10:0:viel").is_err());
        assert!(parse_navy("10:0:-1").is_err());
        // Fehler nennen die Angabe, an der es liegt.
        let args = ScriptArgs {
            order: vec!["3:alle:kahn".into()],
            ..Default::default()
        };
        assert!(
            args.script(&data)
                .unwrap_err()
                .starts_with("--order 3:alle:kahn: ")
        );
    }
}
