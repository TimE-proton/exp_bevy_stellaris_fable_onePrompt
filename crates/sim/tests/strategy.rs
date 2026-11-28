//! Die strategische Führung der KI: Kriegsziele, die zu Friedensbedingungen führen,
//! Grundhaltungen, die verschieden handeln, und das eine Ende jeder Partie.

use sim::GameData;
use sim::chronicle::{Chronicle, ChronicleKind};
use sim::clock::{DAYS_PER_MONTH, DAYS_PER_YEAR};
use sim::command::SimCommand;
use sim::diplomacy::occasion::{BIND, Occasion, Picture, STRIKE, SUBJUGATE, choice_probs};
use sim::diplomacy::{Archetype, Deal, Diplomacy, WarAim};
use sim::endgame::{Endgame, Path};
use sim::harness::{Headless, WarScript, nearest_neighbour, run_tracked};
use sim::ids::PolityId;
use sim::state::{SimConfig, money_drift};
use std::sync::OnceLock;

/// Spieldaten ohne Gründerzeit: Die KI führt ihre Beziehungen vom ersten Tag an.
fn data() -> &'static GameData {
    static DATA: OnceLock<GameData> = OnceLock::new();
    DATA.get_or_init(|| {
        let mut data = GameData::load_workspace().unwrap();
        data.diplomacy.levers.founding_years = 0;
        data
    })
}

fn diplomacy(sim: &Headless) -> &Diplomacy {
    sim.world().resource::<Diplomacy>()
}

fn end_entries(sim: &Headless) -> usize {
    sim.world()
        .resource::<Chronicle>()
        .entries
        .iter()
        .filter(|entry| entry.kind == ChronicleKind::End)
        .count()
}

/// Eine Lage, in der beide gleich stark sind und keiner mehr wachsen kann.
fn crowded() -> Picture {
    Picture {
        ratio: 0.3,
        share_me: 0.2,
        share_other: 0.15,
        own_fleet: 200.0,
        other_fleet: (90.0, 130.0),
        hops: 3.0,
        dep: 0.05,
        aff: 0.8,
        trust: 0.6,
        tension: 30.0,
        want: 1.0,
        ..Picture::default()
    }
}

#[test]
fn the_archetypes_choose_differently_in_the_same_situation() {
    let data = data();
    let levers = &data.diplomacy.levers;
    let profile = |archetype: Archetype| &data.diplomacy.archetypes[archetype.index()];
    let opening = |archetype: Archetype| {
        choice_probs(profile(archetype), &Occasion::Opening(crowded()), levers)
    };
    let aim = |archetype: Archetype| {
        choice_probs(profile(archetype), &Occasion::Aim(crowded()), levers)[SUBJUGATE]
    };
    let expansionist = opening(Archetype::Expansionist);
    let cooperative = opening(Archetype::Cooperative);
    // Die Expansionistische schlägt eher zu, die Kooperative bindet eher.
    assert!(
        expansionist[STRIKE] > cooperative[STRIKE] + 0.3,
        "{expansionist:?} {cooperative:?}"
    );
    assert!(
        cooperative[BIND] > expansionist[BIND] + 0.3,
        "{expansionist:?} {cooperative:?}"
    );
    // Wer zuschlägt, will unterwerfen, wenn er gierig ist, sonst begrenzt bleiben.
    assert!(aim(Archetype::Expansionist) > 0.9);
    assert!(aim(Archetype::Cooperative) < 0.5);
    // Eine vertragstreue Gemeinschaft schlägt gegen Partner kaum zu.
    let bound = Picture {
        rungs: 4.0,
        ..crowded()
    };
    let against_partner =
        choice_probs(profile(Archetype::Cooperative), &Occasion::Opening(bound), levers);
    assert!(against_partner[STRIKE] < 0.05, "{against_partner:?}");
}

/// Gibt der Seite `victor` im Krieg gegen `loser` einen Vorsprung, als hätte sie dem
/// Gegner einen Teil seiner Bevölkerung genommen.
fn lead(sim: &mut Headless, victor: PolityId, loser: PolityId, share: f64) {
    let mut diplomacy = sim.world_mut().resource_mut::<Diplomacy>();
    let war = diplomacy.relation_mut(victor, loser).war.as_mut().unwrap();
    let (us, them) = (
        sim::diplomacy::relations::side(victor, loser),
        sim::diplomacy::relations::side(loser, victor),
    );
    war.taken[us] = share * war.pop0[them];
}

#[test]
fn an_ai_war_aim_leads_to_its_peace_terms() {
    let mut sim = Headless::new(
        data().clone(),
        SimConfig {
            seed: 8,
            player: None,
        },
    );
    sim.run_days(2 * DAYS_PER_YEAR);
    let a = PolityId(0);
    let b = nearest_neighbour(sim.galaxy(), sim.polities(), a).unwrap();
    sim.issue(
        a,
        SimCommand::DeclareWarWithAim {
            target: b,
            aim: WarAim::Subjugation,
        },
    );
    sim.run_days(1);
    assert!(diplomacy(&sim).at_war(a, b));
    // Nach einigen Monaten liegt der Angreifer weit vorn: Er fordert, was sein Ziel ist.
    sim.run_days(7 * DAYS_PER_MONTH);
    if diplomacy(&sim).at_war(a, b) {
        lead(&mut sim, a, b, 0.9);
    }
    let mut demanded = false;
    for _ in 0..(3 * DAYS_PER_MONTH) {
        sim.run_days(1);
        let diplomacy = diplomacy(&sim);
        demanded |= diplomacy.proposals.iter().any(|proposal| {
            proposal.from == a
                && matches!(&proposal.deal, Deal::Peace(terms) if terms.subjugate)
        }) || diplomacy.history.iter().any(|record| {
            record.proposal.from == a
                && matches!(&record.proposal.deal, Deal::Peace(terms) if terms.subjugate)
        });
        if demanded {
            break;
        }
    }
    assert!(demanded, "der Sieger fordert die Unterwerfung");
    // Der Unterlegene nimmt an oder lehnt ab; nimmt er an, ist er Schützling.
    sim.run_days(DAYS_PER_MONTH);
    let diplomacy = diplomacy(&sim);
    if !diplomacy.at_war(a, b) {
        assert_eq!(diplomacy.overlord_of(b), Some(a));
        assert_eq!(diplomacy.tally.subjugations, 1);
    }
    assert_eq!(money_drift(sim.world()), Some(0));
}

#[test]
fn a_community_reduced_to_its_seat_ends_the_game_once_and_the_clock_stops() {
    // Klein gerechnet: Alle außer zwei gehen früh in einer auf, die zweite verliert
    // ihre Welten bis auf die Hauptwelt und fällt unter die Schwelle der Souveränität.
    let mut sim = Headless::new(
        data().clone(),
        SimConfig {
            seed: 5,
            player: None,
        },
    );
    sim.run_days(DAYS_PER_YEAR);
    let tick = sim.clock().tick;
    let ids: Vec<PolityId> = sim.polities().alive().map(|polity| polity.id).collect();
    let (lead_id, last) = (ids[0], ids[1]);
    for other in &ids[2..] {
        sim.succession(|s| s.absorb(*other, lead_id, Path::Joined, tick))
            .unwrap();
    }
    // Die Zweite unterstellt sich gewollt: Es bleibt ein Verbund.
    sim.succession(|s| s.subordinate(last, lead_id, true, tick))
        .unwrap();
    sim.run_days(2 * DAYS_PER_MONTH);
    let outcome = sim.outcome().expect("die Partie ist zu Ende");
    assert_eq!(outcome.survivor, Some(lead_id));
    assert_eq!(end_entries(&sim), 1);
    let (day, hash) = (sim.clock().day(), sim.hash());
    sim.run_days(5 * DAYS_PER_YEAR);
    assert_eq!(sim.clock().day(), day, "nach dem Ende steht die Uhr");
    assert_eq!(sim.hash(), hash);
    assert_eq!(end_entries(&sim), 1);
    // Der Ausgang steht im Spielstand und übersteht das Laden.
    let save = sim.save();
    let loaded = Headless::from_save(data().clone(), save);
    assert_eq!(loaded.outcome(), Some(outcome));
    assert_eq!(
        loaded.world().resource::<Endgame>().outcome,
        Some(outcome)
    );
}

/// Lange Läufe ohne Drehbuch: Jede Partie endet genau einmal, innerhalb von 250 Jahren,
/// und gleich gerechnet gleich. Dauert in der Prüfung ohne Optimierung lange.
#[test]
#[ignore]
fn every_game_ends_once_within_250_years() {
    let data = GameData::load_workspace().unwrap();
    for seed in 0..4 {
        let (sim, violation, _) =
            run_tracked(&data, seed, 250 * DAYS_PER_YEAR, &WarScript::default());
        assert_eq!(violation, None, "Seed {seed}");
        assert!(sim.outcome().is_some(), "Seed {seed} endet nicht");
        assert_eq!(end_entries(&sim), 1, "Seed {seed}");
        let (again, _, _) = run_tracked(&data, seed, 250 * DAYS_PER_YEAR, &WarScript::default());
        assert_eq!(again.hash(), sim.hash(), "Seed {seed}");
    }
}
