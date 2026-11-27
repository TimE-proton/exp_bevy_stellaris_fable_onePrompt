//! Die militärische Grundhaltung der KI-Gemeinschaften in ganzen Partien: Rüstung im
//! Rahmen des Staatshaushalts, Stilllegen bei dauerhaftem Soldrückstand, Führung der
//! Flotten in einem Krieg, den ein Drehbuch erklärt, und Determinismus mit KI-Kriegen.

use sim::GameData;
use sim::ai::{AiState, MilitaryMode};
use sim::chronicle::Chronicle;
use sim::clock::DAYS_PER_YEAR;
use sim::command::{CommandOutcome, SimCommand};
use sim::diplomacy::Diplomacy;
use sim::economy::{Claim, Economy};
use sim::fleet::{Battles, Fleets};
use sim::harness::{
    Headless, ScriptedAct, ScriptedEvent, WarScript, check_invariants, run_scripted, summarize,
};
use sim::ids::{FleetId, PolityId, ShipId};
use sim::state::{SaveGame, SimConfig, money_drift};
use std::sync::OnceLock;

fn data() -> &'static GameData {
    static DATA: OnceLock<GameData> = OnceLock::new();
    DATA.get_or_init(|| GameData::load_workspace().unwrap())
}

/// Schiffe der Startflotte.
fn starting_ships() -> usize {
    data()
        .balance
        .military
        .starting_fleet_ids
        .iter()
        .map(|(_, count)| *count as usize)
        .sum()
}

#[test]
fn in_peace_the_ai_arms_with_the_economy_and_inside_its_budget() {
    let levers = &data().balance.military.ai;
    for seed in [3, 7] {
        let (sim, violation) =
            run_scripted(data(), seed, 30 * DAYS_PER_YEAR, &WarScript::default());
        assert_eq!(violation, None, "Seed {seed}");
        let summary = summarize(&sim);
        let fleets = sim.world().resource::<Fleets>();
        let day = sim.clock().day();
        let mut built = 0;
        for (index, polity) in summary.polities.iter().enumerate().take(6) {
            let name = &polity.name;
            // Die Flotte wächst mit der Wirtschaft: nach dreißig Jahren mehr Schiffe als
            // zu Beginn, alle aus eigenen Werften, keines stillgelegt oder verloren.
            assert!(
                polity.ships > starting_ships(),
                "Seed {seed}, {name}: {} Schiffe",
                polity.ships
            );
            assert_eq!(
                polity.ships,
                starting_ships() + polity.ships_built as usize,
                "Seed {seed}, {name}"
            );
            assert_eq!(polity.ships_laid_up, 0, "Seed {seed}, {name}");
            built += polity.ships_built;
            // Im Rahmen des Haushalts: Sold und Wartung bleiben unter der Obergrenze des
            // Zielanteils, die Flotte bekam, was sie verlangte, und in dreißig Jahren
            // blieb nie Sold aus.
            let fleet = polity.budget.line(Claim::Fleet);
            let share = fleet.paid as f64 / polity.budget.revenue.max(1) as f64;
            assert!(
                share > 0.5 * levers.peace_share && share < levers.max_share,
                "Seed {seed}, {name}: Flotte {:.1} % der Einnahmen",
                100.0 * share
            );
            assert_eq!(polity.months_in_arrears, 0, "Seed {seed}, {name}");
            assert_eq!(polity.arrears, 0.0, "Seed {seed}, {name}");
            // Kein Auftrag hängt: Was offen ist, stockt nicht und ist jünger als zwei Jahre.
            for order in fleets.orders_of(PolityId(index as u32)) {
                assert!(
                    !order.stalled() && day - order.started < 2 * DAYS_PER_YEAR,
                    "Seed {seed}, {name}: {}",
                    order.status_text(data())
                );
            }
            // Im Frieden liegt alles vereinigt in einer Flotte, versorgt und im
            // Geleitschutz; ein eben ausgeliefertes Schiff darf noch unterwegs sein.
            assert!(
                polity.fleets <= 2,
                "Seed {seed}, {name}: {} Flotten",
                polity.fleets
            );
        }
        assert!(built >= 12, "Seed {seed}: nur {built} Schiffe gebaut");
        assert!(
            summary.ai_rejected_share() < 0.02,
            "Seed {seed}: {} von {} KI-Befehlen abgelehnt",
            summary.ai_rejected,
            summary.ai_applied + summary.ai_rejected
        );
        assert_eq!(summary.battles, 0);
    }
}

/// Dieselben Daten, aber jede Gemeinschaft beginnt mit dem Vierfachen der Startflotte:
/// mehr, als ihr Haushalt ohne höhere Steuer trägt.
fn with_four_fleets() -> GameData {
    let mut data = data().clone();
    for entry in &mut data.balance.military.starting_fleet_ids {
        entry.1 *= 4;
    }
    for entry in &mut data.balance.military.starting_fleet {
        entry.1 *= 4;
    }
    data
}

#[test]
fn a_fleet_the_budget_cannot_carry_is_laid_up_until_the_pay_is_met() {
    let heavy = with_four_fleets();
    let start = 4 * starting_ships();
    let mut sim = Headless::new(
        heavy.clone(),
        SimConfig {
            seed: 3,
            player: None,
        },
    );
    // Die ersten Jahre: Der Haushalt trägt die Flotte nicht, Sold bleibt aus.
    let mut worst = vec![0.0f32; 6];
    for _ in 0..3 * 12 {
        sim.run_days(30);
        let fleets = sim.world().resource::<Fleets>();
        for (index, worst) in worst.iter_mut().enumerate() {
            *worst = worst.max(fleets.arrears_of(PolityId(index as u32)));
        }
    }
    let struck: Vec<usize> = (0..6).filter(|index| worst[*index] > 0.05).collect();
    assert!(
        struck.len() >= 3,
        "die vierfache Flotte müsste die meisten in Rückstand bringen: {worst:?}"
    );
    sim.run_days(9 * DAYS_PER_YEAR);
    assert_eq!(check_invariants(sim.world()), Ok(()));
    assert_eq!(money_drift(sim.world()), Some(0));
    let summary = summarize(&sim);
    let chronicle = sim.world().resource::<Chronicle>();
    for index in struck {
        let polity = &summary.polities[index];
        let name = &polity.name;
        // Wer Sold schuldig blieb, hat Schiffe stillgelegt, bis der Haushalt die Flotte
        // trug: Am Ende ist der Rückstand abgetragen, und die Flotte ist kleiner.
        assert!(
            polity.ships_laid_up >= 2,
            "{name}: {} stillgelegt",
            polity.ships_laid_up
        );
        assert_eq!(
            polity.ships,
            start + polity.ships_built as usize - polity.ships_laid_up as usize,
            "{name}"
        );
        assert!(polity.ships < start, "{name}: {} Schiffe", polity.ships);
        assert_eq!(polity.arrears, 0.0, "{name}");
        // Aber nicht mehr als nötig: Stillgelegt wird nur bis zum Rüstungsziel, und das
        // liegt in diesen Jahren bei rund einer Startflotte.
        assert!(
            polity.fleet_value >= 0.6 * heavy.balance.military.starting_fleet_value,
            "{name}: nur noch {} Schiffe im Bauwert von {:.0}",
            polity.ships,
            polity.fleet_value
        );
    }
    assert!(
        chronicle
            .entries
            .iter()
            .any(|entry| entry.text.contains("still;"))
            || chronicle.total as usize > Chronicle::CAPACITY,
        "das Stilllegen steht in der Chronik"
    );
    assert!(summary.ai_rejected_share() < 0.02);
}

#[test]
fn the_player_can_lay_up_ships_at_an_own_yard() {
    let me = PolityId(0);
    let mut sim = Headless::new(
        data().clone(),
        SimConfig {
            seed: 11,
            player: Some(me),
        },
    );
    sim.run_days(60);
    let fleet = FleetId(0);
    let ships: Vec<ShipId> = sim.world().resource::<Fleets>().list[0]
        .ships
        .iter()
        .map(|ship| ship.id)
        .collect();
    let home = sim.world().resource::<Fleets>().list[0].system;
    let treasury = |sim: &Headless| sim.world().resource::<Economy>().treasury(me);
    let market = |sim: &Headless| sim.world().resource::<Economy>().market(home).cash;
    let outcome = |sim: &Headless| {
        sim.command_log()
            .entries
            .last()
            .map(|entry| entry.outcome.clone())
            .unwrap()
    };
    // Ein fremdes Schiff, ein fremdes Kommando: abgelehnt, nichts ändert sich.
    sim.issue(
        me,
        SimCommand::LayUpShips {
            fleet,
            ships: vec![ShipId(9999)],
        },
    );
    sim.tick();
    assert!(matches!(outcome(&sim), CommandOutcome::Rejected(_)));
    sim.issue(
        PolityId(1),
        SimCommand::LayUpShips {
            fleet,
            ships: vec![ships[0]],
        },
    );
    sim.tick();
    assert!(matches!(outcome(&sim), CommandOutcome::Rejected(_)));
    assert_eq!(
        sim.world().resource::<Fleets>().list[0].ships.len(),
        ships.len()
    );

    // Im Heimatsystem mit seiner Werft: Zwei Schiffe gehen, ihr Sold endet, und der Erlös
    // der verkauften Bauteile wandert vom Kontor in die Staatskasse, Cent für Cent.
    let upkeep = sim::fleet::monthly_upkeep(data(), sim.world().resource::<Fleets>(), me);
    let (state_before, market_before) = (treasury(&sim), market(&sim));
    sim.issue(
        me,
        SimCommand::LayUpShips {
            fleet,
            ships: ships[..2].to_vec(),
        },
    );
    sim.tick();
    assert_eq!(outcome(&sim), CommandOutcome::Applied);
    let fleets = sim.world().resource::<Fleets>();
    assert_eq!(fleets.list[0].ships.len(), ships.len() - 2);
    assert_eq!(fleets.tally_of(me).laid_up, 2);
    assert!(sim::fleet::monthly_upkeep(data(), fleets, me) < upkeep);
    let proceeds = treasury(&sim) - state_before;
    assert!(proceeds > 0, "der Verkauf der Bauteile bringt etwas ein");
    assert_eq!(market(&sim) - market_before, -proceeds);
    assert_eq!(money_drift(sim.world()), Some(0));
    assert_eq!(check_invariants(sim.world()), Ok(()));

    // Fern jeder eigenen Werft geht es nicht: unterwegs nicht, und nicht im Nachbarsystem.
    let away = sim
        .galaxy()
        .neighbors(home)
        .iter()
        .map(|neighbor| neighbor.system)
        .find(|system| !sim.galaxy().owns_planet_in(me, *system))
        .expect("ein Nachbarsystem ohne eigene Welt");
    sim.issue(me, SimCommand::MoveFleet { fleet, to: away });
    sim.run_days(2);
    sim.issue(
        me,
        SimCommand::LayUpShips {
            fleet,
            ships: vec![ships[2]],
        },
    );
    sim.tick();
    assert!(matches!(outcome(&sim), CommandOutcome::Rejected(_)));
    while !sim.world().resource::<Fleets>().list[0].route.is_empty() {
        sim.run_days(1);
    }
    sim.issue(
        me,
        SimCommand::LayUpShips {
            fleet,
            ships: vec![ships[2]],
        },
    );
    sim.tick();
    assert!(matches!(outcome(&sim), CommandOutcome::Rejected(_)));
    assert_eq!(
        sim.world().resource::<Fleets>().list[0].ships.len(),
        ships.len() - 2
    );
}

/// Ein Krieg nach Drehbuch zwischen der ersten Gemeinschaft und ihrem nächsten Nachbarn,
/// vom Jahr `from` bis zum Jahr `to`. Die Flotten führt die KI.
fn border_war(from: u64, to: u64) -> WarScript {
    WarScript::new(vec![
        ScriptedEvent {
            day: from * DAYS_PER_YEAR,
            act: ScriptedAct::BorderWar { a: PolityId(0) },
        },
        ScriptedEvent {
            day: to * DAYS_PER_YEAR,
            act: ScriptedAct::PeaceWithAll { a: PolityId(0) },
        },
    ])
}

#[test]
fn the_ai_fights_a_war_it_did_not_choose_and_returns_to_peace() {
    // Zwei Partien, in denen die Nachbarn nah genug liegen, dass sie einander erreichen,
    // und in denen eine Seite der anderen überlegen genug ist, um anzugreifen.
    let mut battles = 0;
    for seed in [21, 37] {
        let script = border_war(20, 32);
        let mut sim = Headless::new(data().clone(), SimConfig { seed, player: None });
        script.run_until(&mut sim, 20 * DAYS_PER_YEAR, false);
        let built_before: Vec<u32> = summarize(&sim)
            .polities
            .iter()
            .map(|polity| polity.ships_built)
            .collect();
        script.run_until(&mut sim, 34 * DAYS_PER_YEAR, false);
        assert_eq!(check_invariants(sim.world()), Ok(()), "Seed {seed}");
        let summary = summarize(&sim);
        let a = PolityId(0);
        // Der Gegner: wer außer der ersten Gemeinschaft im Krieg stand.
        let b = PolityId(
            summary
                .polities
                .iter()
                .skip(1)
                .position(|polity| polity.months_at_war > 0)
                .expect("der Krieg wurde erklärt") as u32
                + 1,
        );
        let sides = [&summary.polities[a.index()], &summary.polities[b.index()]];
        for (side, polity) in sides.iter().zip([a, b]) {
            // Zwölf Jahre Krieg, in denen beide Seiten weitergebaut haben (wenig: Seit die
            // stehende Verteidigung der Welten ihren Sold aus dem Haushalt bekommt, bleibt
            // den Werften etwas weniger).
            assert_eq!(side.months_at_war, 12 * 12, "Seed {seed}, {}", side.name);
            let built = side.ships_built - built_before[polity.index()];
            assert!(
                built >= 1,
                "Seed {seed}, {}: seit Kriegsbeginn {built} gebaut, {} verloren",
                side.name,
                side.ships_lost
            );
            // Keine Flotte ging in einem aussichtslosen Treffen ganz verloren.
            assert_eq!(side.fleets_lost, 0, "Seed {seed}, {}", side.name);
        }
        // Mindestens eine Seite ging zum Angriff über und hielt ein System gesperrt.
        assert!(
            sides.iter().any(|side| side.offensives > 0),
            "Seed {seed}: kein Angriff"
        );
        assert!(
            sides.iter().map(|side| side.blockade_days).sum::<u32>() >= 60,
            "Seed {seed}: keine Blockade"
        );
        battles += summary.battles;
        // Alle Gefechte sind zu Ende, und der Friede hält: keine Unternehmung läuft
        // weiter, die Flotten stehen wieder im eigenen Gebiet.
        let state = sim.world().resource::<Battles>();
        assert!(state.active.is_empty(), "Seed {seed}");
        assert_eq!(
            summary.battle_outcomes.iter().sum::<u32>() as u64,
            summary.battles,
            "Seed {seed}"
        );
        assert!(!sim.world().resource::<Diplomacy>().is_at_war(a));
        let ai = sim.world().resource::<AiState>();
        let fleets = sim.world().resource::<Fleets>();
        for polity in [a, b] {
            assert!(ai.plan(polity).operations.is_empty(), "Seed {seed}");
            for fleet in fleets.of(polity).filter(|fleet| fleet.route.is_empty()) {
                assert!(
                    sim.galaxy().owns_planet_in(polity, fleet.system),
                    "Seed {seed}: {} steht noch bei {}",
                    fleet.name,
                    sim.galaxy().system(fleet.system).name
                );
            }
        }
        assert!(
            summary.ai_rejected_share() < 0.02,
            "Seed {seed}: {} von {} KI-Befehlen abgelehnt",
            summary.ai_rejected,
            summary.ai_applied + summary.ai_rejected
        );
    }
    assert!(battles >= 2, "in zwei Kriegen nur {battles} Gefechte");
}

#[test]
fn a_fleet_led_by_the_ai_beats_one_that_stays_passive() {
    // Derselbe Krieg zweimal: Einmal bleibt die Flotte der einen Seite ohne Befehle,
    // einmal die der anderen. Beide rüsten in beiden Partien gleich. Wer geführt wird,
    // sperrt Systeme und gewinnt die Gefechte; wer untätig bleibt, tut keines von beidem.
    // (Eine Partie, in der die geführte Seite auf beiden Plätzen stark genug ist, um
    // anzugreifen: Gegen eine gleich starke Flotte in Reichweite greift die KI nicht an,
    // auch wenn die untätig bleibt.)
    let seed = 7;
    let a = PolityId(0);
    let play = |passive_is_a: bool| {
        let events = vec![
            ScriptedEvent {
                day: 20 * DAYS_PER_YEAR,
                act: ScriptedAct::BorderWar { a },
            },
            ScriptedEvent {
                day: 0,
                act: if passive_is_a {
                    ScriptedAct::Mode {
                        who: Some(a),
                        mode: MilitaryMode::PassiveFleet,
                    }
                } else {
                    ScriptedAct::NeighbourMode {
                        of: a,
                        mode: MilitaryMode::PassiveFleet,
                    }
                },
            },
        ];
        let (sim, violation) =
            run_scripted(data(), seed, 30 * DAYS_PER_YEAR, &WarScript::new(events));
        assert_eq!(violation, None);
        let summary = summarize(&sim);
        let b = PolityId(
            summary
                .polities
                .iter()
                .skip(1)
                .position(|polity| polity.months_at_war > 0)
                .expect("der Krieg wurde erklärt") as u32
                + 1,
        );
        let (first, second) = (
            summary.polities[a.index()].clone(),
            summary.polities[b.index()].clone(),
        );
        if passive_is_a {
            (second, first)
        } else {
            (first, second)
        }
    };
    for passive_is_a in [true, false] {
        let (led, passive) = play(passive_is_a);
        let case = format!("untätig: {}", passive.name);
        assert_eq!(passive.offensives, 0, "{case}");
        assert_eq!(passive.blockade_days, 0, "{case}");
        assert!(led.offensives > 0, "{case}");
        assert!(
            led.blockade_days >= 180,
            "{case}: {} Blockadetage",
            led.blockade_days
        );
        assert!(
            led.battles_won >= passive.battles_won && led.ships_lost <= passive.ships_lost,
            "{case}: geführt {} Siege und {} Verluste, untätig {} und {}",
            led.battles_won,
            led.ships_lost,
            passive.battles_won,
            passive.ships_lost
        );
    }
}

#[test]
fn wars_led_by_the_ai_are_deterministic() {
    // Ein früher Krieg, damit jeder Takt gerechnet werden kann. Mit gleichen Startflotten
    // griffe niemand an; deshalb verdoppelt die erste Gemeinschaft ihre Flotte vorher.
    let mut events = vec![
        ScriptedEvent {
            day: 0,
            act: ScriptedAct::Navy {
                who: Some(PolityId(0)),
                factor: 2.0,
            },
        },
        ScriptedEvent {
            day: 3 * DAYS_PER_YEAR,
            act: ScriptedAct::BorderWar { a: PolityId(0) },
        },
    ];
    events.push(ScriptedEvent {
        day: 6 * DAYS_PER_YEAR,
        act: ScriptedAct::PeaceWithAll { a: PolityId(0) },
    });
    let script = WarScript::new(events);
    let config = SimConfig {
        seed: 6,
        player: None,
    };
    let days = 5 * DAYS_PER_YEAR;
    let mut fast = Headless::new(data().clone(), config.clone());
    script.run_until(&mut fast, days, false);
    // Der Krieg läuft, und die KI hat ihre Flotten geführt.
    assert!(
        sim::needs_fine_ticks(fast.world())
            || fast.world().resource::<Diplomacy>().is_at_war(PolityId(0))
    );
    let summary = summarize(&fast);
    assert!(
        summary
            .polities
            .iter()
            .any(|polity| polity.offensives > 0 && polity.blockade_days > 0),
        "im Krieg ist nichts geschehen"
    );

    // Derselbe Seed noch einmal; jeden Takt gerechnet; in der Mitte des Kriegs gespeichert
    // und geladen: immer derselbe Zustand.
    let mut again = Headless::new(data().clone(), config.clone());
    script.run_until(&mut again, days, false);
    assert_eq!(again.hash(), fast.hash());

    let mut exact = Headless::new(data().clone(), config.clone());
    script.run_until(&mut exact, days, true);
    assert_eq!(exact.clock(), fast.clock());
    assert_eq!(exact.hash(), fast.hash());

    let mut first_half = Headless::new(data().clone(), config);
    let middle = 4 * DAYS_PER_YEAR + 100;
    script.run_until(&mut first_half, middle, false);
    assert!(
        first_half
            .world()
            .resource::<Diplomacy>()
            .is_at_war(PolityId(0))
    );
    let text = first_half.save().to_ron().unwrap();
    let mut resumed = Headless::from_save(data().clone(), SaveGame::from_ron(&text).unwrap());
    assert_eq!(resumed.hash(), first_half.hash());
    script.run_until(&mut resumed, days, false);
    assert_eq!(resumed.hash(), fast.hash());
}

#[test]
fn wars_led_by_the_ai_do_not_repeat_the_same_empty_battle_or_sortie() {
    // Zwei Grenzkriege über zwanzig Jahre, in denen sich früher dieselben zwei Flotten
    // Dutzende Male ohne Verluste trafen und Angriffe im Wochentakt begonnen und
    // abgebrochen wurden.
    for seed in [13, 20] {
        let script = border_war(25, 45);
        let mut sim = Headless::new(data().clone(), SimConfig { seed, player: None });
        script.run_until(&mut sim, 46 * DAYS_PER_YEAR, false);
        assert_eq!(check_invariants(sim.world()), Ok(()), "Seed {seed}");
        let reports = &sim.world().resource::<Battles>().reports;
        // Gefechte, in denen niemand etwas verlor: Eine Seite wich der anderen nur aus.
        let empty = reports
            .iter()
            .filter(|report| report.sides.iter().all(|side| side.lost_value <= 0.0))
            .count();
        assert!(
            empty <= 6,
            "Seed {seed}: {empty} von {} Gefechten ohne jeden Verlust",
            reports.len()
        );
        // Wer angreift, hält im Mittel wenigstens zwei Wochen ein System gesperrt: Ein
        // Angriff ist kein Ausfall, der am nächsten Tag wieder abgebrochen wird.
        let summary = summarize(&sim);
        for polity in summary
            .polities
            .iter()
            .filter(|polity| polity.offensives >= 10)
        {
            assert!(
                polity.blockade_days >= 14 * polity.offensives,
                "Seed {seed}, {}: {} Angriffe für {} Blockadetage",
                polity.name,
                polity.offensives,
                polity.blockade_days
            );
        }
        assert!(summary.ai_rejected_share() < 0.02, "Seed {seed}");
    }
}

#[test]
fn after_laying_up_ships_the_ai_does_not_order_new_ones_for_years() {
    // Eine Partie, in der nach dem Grenzkrieg eine Gemeinschaft ihre Flotte nicht mehr
    // trägt und Schiffe stilllegt. Monat für Monat: Nach einer Stilllegung kommt im
    // Frieden layup_quiet_months lang kein neuer Auftrag hinzu.
    let quiet = data().balance.military.ai.layup_quiet_months as u64;
    let script = border_war(25, 45);
    let mut sim = Headless::new(
        data().clone(),
        SimConfig {
            seed: 19,
            player: None,
        },
    );
    script.run_until(&mut sim, 40 * DAYS_PER_YEAR, false);
    // Je Gemeinschaft: bisher stillgelegt, ausgeliefert plus offene Aufträge, und der
    // Monat der letzten Stilllegung.
    let mut seen: Vec<(u32, u32, Option<u64>)> = Vec::new();
    let (mut laid_up, mut checked) = (0, 0);
    for month in 40 * 12..60 * 12 {
        script.run_until(&mut sim, (month + 1) * sim::clock::DAYS_PER_MONTH, false);
        let fleets = sim.world().resource::<Fleets>();
        let diplomacy = sim.world().resource::<Diplomacy>();
        let count = sim.polities().list.len();
        seen.resize(count, (0, 0, None));
        for (index, entry) in seen.iter_mut().enumerate() {
            let id = PolityId::from_index(index);
            let tally = fleets.tally_of(id);
            let committed = tally.built + fleets.orders_of(id).count() as u32;
            if tally.laid_up > entry.0 {
                laid_up += tally.laid_up - entry.0;
                entry.2 = Some(month);
            } else if let Some(since) = entry.2
                && month - since < quiet
                && !diplomacy.is_at_war(id)
            {
                checked += 1;
                assert!(
                    committed <= entry.1,
                    "{}: neuer Auftrag {} Monate nach der Stilllegung",
                    sim.polities().get(id).name,
                    month - since
                );
            }
            entry.0 = tally.laid_up;
            entry.1 = committed;
        }
    }
    assert!(
        laid_up >= 2,
        "Szenario neu wählen: nur {laid_up} Schiffe stillgelegt"
    );
    assert!(
        checked >= 24,
        "nur {checked} Monate nach einer Stilllegung geprüft"
    );
    assert_eq!(check_invariants(sim.world()), Ok(()));
}
