//! Hält die Bedienungsanleitung im README bei dem, was das Spiel wirklich anbietet: Tasten
//! der Schlachtansicht, Einstellungen und Optionen der Gefechtsübung. Wer eine Taste, eine
//! Vorlage oder eine Option ändert, erfährt hier, dass das README nachzuziehen ist.

use crate::args::{Args, USAGE};
use crate::battle_view::practice::{MAX_SHIPS_PER_SIDE, PRESETS, PracticeSetup};
use crate::ui::fleets::{self, FleetState};
use crate::ui::panels::BATTLE_KEYS;
use crate::ui::tech;
use sim::GameData;
use sim::battle::{Doctrine, Formation};
use sim::data::TechBranch;
use sim::fleet::Stance;
use sim::state::SAVE_FORMAT;

const README: &str = include_str!("../../../README.md");

/// Text unter einer Überschrift, bis zur nächsten Überschrift gleicher oder höherer Ebene.
fn section(heading: &str) -> &'static str {
    let level = heading.chars().take_while(|sign| *sign == '#').count();
    let start = README
        .find(&format!("\n{heading}\n"))
        .unwrap_or_else(|| panic!("das README hat keinen Abschnitt „{heading}“"));
    let body = &README[start + heading.len() + 2..];
    let end = body
        .lines()
        .scan(0, |offset, line| {
            let at = *offset;
            *offset += line.len() + 1;
            Some((at, line))
        })
        .find(|(_, line)| {
            let depth = line.chars().take_while(|sign| *sign == '#').count();
            depth > 0 && depth <= level && line[depth..].starts_with(' ')
        })
        .map_or(body.len(), |(at, _)| at);
    &body[..end]
}

/// Was im Text in Rückstrichen steht.
fn code_spans(text: &str) -> Vec<&str> {
    text.split('`').skip(1).step_by(2).collect()
}

/// Sieht aus wie eine Flottenliste (`speer:4,waechter:6`)?
fn looks_like_a_fleet(word: &str) -> bool {
    word.split(',').all(|entry| {
        entry.split_once(':').is_some_and(|(key, count)| {
            !key.is_empty() && !count.is_empty() && count.chars().all(|sign| sign.is_ascii_digit())
        })
    })
}

#[test]
fn the_manual_has_its_parts() {
    let manual = section("## Bedienung");
    // Die Unterabschnitte gehören zur Bedienung und nicht hinter den nächsten Abschnitt.
    assert!(manual.contains("\n### Forschung\n"));
    assert!(manual.contains("\n### Diplomatie und Analyse\n"));
    assert!(manual.contains("\n### Flotten\n"));
    assert!(manual.contains("\n### Belagerung und Verteidigung\n"));
    assert!(manual.contains("\n### Gefechtsübung\n"));
    assert!(manual.contains("\n### Schlachtansicht\n"));
    assert!(!manual.contains("\n## "));
    assert!(section("### Gefechtsübung").len() < manual.len());
}

#[test]
fn the_manual_names_what_the_diplomacy_and_analysis_windows_offer() {
    use crate::ui::{analysis, diplomacy};
    let text = section("### Diplomatie und Analyse").replace('\n', " ");
    for label in [
        diplomacy::TITLE,
        analysis::TITLE,
        diplomacy::ACCEPT,
        diplomacy::DECLINE,
        diplomacy::WITHDRAW,
        diplomacy::PROPOSE,
        diplomacy::CANCEL,
        diplomacy::DECLARE,
        diplomacy::OFFER_PEACE,
        diplomacy::ASK_PROTECTION,
        diplomacy::OFFER_PROTECTION,
        diplomacy::RELEASE,
    ] {
        assert!(text.contains(&format!("„{label}“")), "„{label}“ fehlt");
    }
}

#[test]
fn the_manual_names_what_the_research_and_fleet_windows_offer() {
    // Knöpfe und Reiter stehen im README so, wie sie im Fenster beschriftet sind.
    let research = section("### Forschung");
    for label in [tech::TITLE, tech::RESEARCH, tech::SET_BUDGET] {
        assert!(research.contains(&format!("„{label}“")), "„{label}“ fehlt");
    }
    // Lange Beschriftungen dürfen im Fließtext umbrechen.
    let flowing = research.replace('\n', " ");
    assert!(flowing.contains(&format!("„{}“", tech::AUTOMATIC)));
    for tab in [tech::TAB_OWN, tech::TAB_OTHERS] {
        assert!(research.contains(&format!("**{tab}.**")), "Reiter {tab}");
    }
    for branch in TechBranch::ALL {
        assert!(research.contains(branch.label()), "{}", branch.label());
    }

    let fleet_part = section("### Flotten");
    for label in [
        fleets::TITLE,
        fleets::MARCH,
        fleets::RETREAT,
        fleets::SPLIT,
        fleets::LAY_UP,
        fleets::SHOW_ON_MAP,
        fleets::ORDER,
        fleets::CANCEL,
        crate::battle_view::campaign::WATCH,
    ] {
        assert!(
            fleet_part.contains(&format!("„{label}“")),
            "„{label}“ fehlt"
        );
    }
    for stance in Stance::ALL {
        assert!(fleet_part.contains(stance.label()), "{}", stance.label());
    }
    // Jeder Zustand, den die Liste zeigt, hat seine Zeile.
    for state in [
        FleetState::Stationed,
        FleetState::Marching,
        FleetState::Fighting,
    ] {
        assert!(fleet_part.contains(&state.label()), "{}", state.label());
    }
    assert!(fleet_part.contains("| sammelt sich bis"));
    assert!(
        FleetState::Regrouping { until: 0 }
            .label()
            .starts_with("sammelt sich bis")
    );

    // Das Speicherformat, das das README nennt, ist das des Kerns.
    assert!(section("## Bedienung").contains(&format!("Speicherformat unter {SAVE_FORMAT}")));
}

#[test]
fn the_manual_lists_every_key_of_the_battle_view() {
    let manual = section("### Schlachtansicht");
    for (keys, what) in BATTLE_KEYS {
        assert!(
            manual.contains(&format!("| {keys} | {what} |")),
            "im README fehlt die Zeile „{keys}: {what}“ der Hilfe"
        );
    }
    // Und umgekehrt: Die Tabelle nennt nichts, was die Hilfe im Spiel nicht kennt. Nur F1
    // steht dort nicht, weil es die Hilfe selbst öffnet.
    let rows: Vec<&str> = manual
        .lines()
        .filter(|line| line.starts_with("| ") && !line.starts_with("| Eingabe"))
        .collect();
    assert_eq!(rows.len(), BATTLE_KEYS.len() + 1, "{rows:#?}");
    assert!(rows.contains(&"| F1 | Hilfe |"));
}

#[test]
fn the_manual_names_the_options_of_the_practice_mode() {
    let manual = section("### Gefechtsübung");
    let spans = code_spans(manual);
    // Jede Option der Kommandozeile, die die Übung betrifft, hat ihre Zeile.
    for option in [
        "--practice",
        "--fleet-a",
        "--fleet-b",
        "--seed",
        "--lineup",
        "--selftest",
        "--screenshot",
    ] {
        assert!(USAGE.contains(option), "{option} steht nicht in der Hilfe");
        assert!(
            spans
                .iter()
                .any(|span| span.split(' ').next() == Some(option)),
            "im README fehlt {option}"
        );
    }
    // Und was das README an Optionen nennt, kennt die Kommandozeile.
    for span in &spans {
        if let Some(option) = span.split(' ').next().filter(|word| word.starts_with("--")) {
            assert!(USAGE.contains(&format!("  {option} ")), "{option}");
        }
    }

    for preset in &PRESETS {
        assert!(
            manual.contains(&format!("„{}“", preset.name)),
            "im README fehlt die Vorlage „{}“",
            preset.name
        );
    }
    for formation in Formation::ALL {
        assert!(manual.contains(formation.label()), "{}", formation.label());
    }
    for doctrine in Doctrine::ALL {
        assert!(manual.contains(doctrine.label()), "{}", doctrine.label());
    }
    assert!(manual.contains(&format!("höchstens {MAX_SHIPS_PER_SIDE} Schiffe")));
}

#[test]
fn the_examples_of_the_manual_work() {
    let manual = section("### Gefechtsübung");
    let data = GameData::load_workspace().expect("Spieldaten des Workspace");

    // Die genannten Entwürfe gibt es, mit der genannten Klasse.
    let fleet_row = manual
        .lines()
        .find(|line| line.starts_with("| Flottenliste"))
        .expect("Zeile zur Flottenliste");
    let named: Vec<&str> = code_spans(fleet_row)
        .into_iter()
        .filter(|span| !span.contains(':'))
        .collect();
    assert!(named.len() >= 6, "{named:?}");
    for key in named {
        let design = data
            .designs
            .iter()
            .find(|design| design.key == key)
            .unwrap_or_else(|| panic!("das README nennt den Entwurf `{key}`, den es nicht gibt"));
        let class = design.stats.class.label();
        assert!(
            fleet_row.contains(&format!("`{key}` ({class}")),
            "`{key}` ist laut Spieldaten: {class}"
        );
    }

    // Jede Flottenliste, die als Beispiel dasteht, ergibt eine gültige Aufstellung.
    let fleets: Vec<&str> = manual
        .split(|sign: char| sign.is_whitespace() || sign == '`')
        .filter(|word| looks_like_a_fleet(word))
        .collect();
    assert!(fleets.len() >= 3, "{fleets:?}");
    for fleet in fleets {
        let setup = PracticeSetup {
            fleets: [fleet.to_string(), fleet.to_string()],
            ..PracticeSetup::default()
        };
        if let Err(error) = setup.resolve(&data) {
            panic!("die Flottenliste `{fleet}` aus dem README gilt nicht: {error}");
        }
    }

    // Der Beispielaufruf startet eine Übung mit den genannten Flotten.
    let call = manual
        .lines()
        .find_map(|line| line.strip_prefix("cargo run -- "))
        .expect("Beispielaufruf");
    let args = Args::parse(call.split_whitespace().map(String::from))
        .unwrap_or_else(|error| panic!("der Beispielaufruf gilt nicht: {error}"));
    assert!(args.practice);
    let setup = PracticeSetup {
        fleets: [
            args.fleet_a.expect("--fleet-a"),
            args.fleet_b.expect("--fleet-b"),
        ],
        ..PracticeSetup::default()
    };
    assert!(setup.resolve(&data).is_ok());
}

#[test]
fn the_manual_names_what_the_siege_window_and_the_defence_panel_offer() {
    use crate::ui::siege::{self, defense};
    use sim::siege::arms::Works;
    use sim::siege::{Course, Fire, Phase, Posture};

    let part = section("### Belagerung und Verteidigung");
    let flowing = part.replace('\n', " ");
    for label in [
        siege::TITLE,
        siege::SHOW_SIEGE,
        siege::RESTING,
        siege::FORECAST,
        siege::BEGIN,
        siege::EMBARK,
        siege::DISEMBARK,
        siege::LAND,
        siege::LIFT,
        siege::CAPITULATE,
        defense::HEADING,
        defense::SET_PLAN,
        defense::DISCARD,
    ] {
        assert!(flowing.contains(&format!("„{label}“")), "„{label}“ fehlt");
    }
    for tab in [siege::TAB_SIEGES, siege::TAB_TROOPS] {
        assert!(part.contains(&format!("**{tab}.**")), "Reiter {tab}");
    }
    for phase in [
        Phase::Approach,
        Phase::Investment,
        Phase::Bombardment,
        Phase::Ground,
        Phase::Underground,
    ] {
        assert!(flowing.contains(phase.label()), "{}", phase.label());
    }
    for course in Course::ALL {
        assert!(flowing.contains(course.label()), "{}", course.label());
    }
    for fire in Fire::ALL {
        assert!(flowing.contains(fire.label()), "{}", fire.label());
    }
    for posture in [Posture::Forward, Posture::Deep] {
        assert!(flowing.contains(posture.label()), "{}", posture.label());
    }
    for works in Works::ALL {
        assert!(flowing.contains(works.label()), "{}", works.label());
    }
    // Karte, Orbitansicht und das Ende einer Gemeinschaft.
    {
        use crate::battle_view::campaign::WATCH as WATCH_BATTLE;
        use crate::battle_view::orbit;
        use crate::endings;
        use crate::ui::panels::ENDED;
        for label in [
            orbit::WATCH,
            orbit::BACK,
            orbit::TO_WINDOW,
            orbit::hud::RESULT_TITLE,
            orbit::hud::OVERVIEW,
            orbit::hud::HEADING_NUMBERS,
            orbit::hud::HEADING_EVENTS,
            WATCH_BATTLE,
            endings::TITLE_OTHER,
            endings::TITLE_PLAYER,
            endings::CONTINUE_AS_OBSERVER,
            ENDED,
            "3 Beschuss · ruht",
        ] {
            assert!(flowing.contains(&format!("„{label}“")), "„{label}“ fehlt");
        }
        for heading in [
            "**So belagert man.**",
            "**So verteidigt man.**",
            "**Marken auf der Karte.**",
            "**Orbitansicht.**",
            "**Ende einer Gemeinschaft.**",
        ] {
            assert!(part.contains(heading), "{heading}");
        }
    }
    // Der Knopf der Kopfleiste und die Zeile des Haushalts stehen in ihren Abschnitten.
    assert!(section("## Bedienung").contains("Belagerung (mit der Zahl"));
    assert!(section("### Staat und Haushalt").contains("Verteidigung"));
}
