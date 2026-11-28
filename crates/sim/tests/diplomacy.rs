//! Diplomatie in der laufenden Partie: Verträge von der Annahme bis zur Kündigung,
//! Vertragsbruch, Frieden zu Bedingungen, Schutz, Vereinigung, Embargo, Durchfahrt, und
//! dass alles über Speichern und Laden hinweg bitgleich weiterläuft.
//!
//! Beide Seiten eines Paares erhalten ihre Befehle vom Drehbuch (`Headless::issue`); die
//! KI käme mit ihrer Antwort erst nach `answer_days` Tagen und ist dann zu spät.

use sim::GameData;
use sim::ai::MilitaryMode;
use sim::clock::DAYS_PER_MONTH;
use sim::command::{CommandOutcome, SimCommand};
use sim::diplomacy::sight::Ground;
use sim::diplomacy::view::{RungState, overview, relation_view};
use sim::diplomacy::{Deal, Diplomacy, PeaceTerms, ProposalStatus, TreatyKind, WarAim};
use sim::endgame::{Endgame, Path};
use sim::finance::Finance;
use sim::fleet::Fleets;
use sim::harness::{Headless, check_invariants, nearest_neighbour};
use sim::ids::{PlanetId, PolityId};
use sim::logistics::Logistics;
use sim::state::{SaveGame, SimConfig, money_drift};
use sim::tech::Research;
use std::sync::OnceLock;

fn data() -> &'static GameData {
    static DATA: OnceLock<GameData> = OnceLock::new();
    DATA.get_or_init(|| GameData::load_workspace().unwrap())
}

/// Eine Partie ohne Spieler, deren Flotten ruhen.
fn game(seed: u64) -> Headless {
    let mut sim = Headless::new(data().clone(), SimConfig { seed, player: None });
    sim.set_military_mode(None, MilitaryMode::Resting);
    sim
}

fn command(sim: &mut Headless, polity: PolityId, command: SimCommand) -> CommandOutcome {
    sim.issue(polity, command);
    sim.tick();
    sim.command_log().entries.last().unwrap().outcome.clone()
}

fn applied(sim: &mut Headless, polity: PolityId, issued: SimCommand) {
    let outcome = command(sim, polity, issued.clone());
    assert_eq!(outcome, CommandOutcome::Applied, "{issued:?}");
}

fn rejection(sim: &mut Headless, polity: PolityId, issued: SimCommand) -> String {
    match command(sim, polity, issued.clone()) {
        CommandOutcome::Rejected(reason) => reason,
        CommandOutcome::Applied => panic!("{issued:?} hätte abgelehnt werden müssen"),
    }
}

fn diplomacy(sim: &Headless) -> &Diplomacy {
    sim.world().resource::<Diplomacy>()
}

fn endgame(sim: &Headless) -> &Endgame {
    sim.world().resource::<Endgame>()
}

fn checked(sim: &Headless) {
    check_invariants(sim.world()).unwrap();
    assert_eq!(money_drift(sim.world()), Some(0));
}

/// Läuft bis zum fünften Tag eines Monats: Zwischen zwei Befehlen liegt dann kein
/// Monatsabschluss.
fn mid_month(sim: &mut Headless) {
    while sim.clock().day() % DAYS_PER_MONTH != 5 {
        sim.run_days(1);
    }
}

/// Schlägt vor und lässt den Empfänger sofort annehmen.
fn agree(sim: &mut Headless, from: PolityId, to: PolityId, deal: Deal) {
    applied(
        sim,
        from,
        SimCommand::Propose {
            to,
            deal: deal.clone(),
        },
    );
    let id = diplomacy(sim)
        .proposals
        .iter()
        .find(|open| open.from == from && open.to == to && open.deal == deal)
        .expect("der Vorschlag ist offen")
        .id;
    applied(
        sim,
        to,
        SimCommand::AnswerProposal {
            proposal: id,
            accept: true,
        },
    );
}

/// Zwei Nachbarn, von denen der zweite eine Welt außer seiner Hauptwelt besitzt.
fn neighbours(sim: &Headless) -> (PolityId, PolityId) {
    for polity in sim.polities().alive() {
        let Some(other) = nearest_neighbour(sim.galaxy(), sim.polities(), polity.id) else {
            continue;
        };
        if !colonies(sim, other).is_empty() {
            return (polity.id, other);
        }
    }
    panic!("keine zwei Nachbarn mit Kolonie");
}

fn colonies(sim: &Headless, polity: PolityId) -> Vec<PlanetId> {
    let home = sim.polities().get(polity).capital;
    sim.galaxy()
        .planets_of(polity)
        .filter(|planet| Some(planet.id) != home)
        .map(|planet| planet.id)
        .collect()
}

fn ground(sim: &Headless) -> Ground<'_> {
    let world = sim.world();
    Ground {
        data: world.resource::<GameData>(),
        galaxy: sim.galaxy(),
        polities: sim.polities(),
        diplomacy: diplomacy(sim),
        fleets: world.resource::<Fleets>(),
        research: world.resource::<Research>(),
        finance: world.resource::<Finance>(),
        seed: world.resource::<SimConfig>().seed,
        day: sim.clock().day(),
    }
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
fn treaties_climb_the_ladder_and_end_after_notice() {
    let mut sim = game(4);
    sim.run_days(90);
    let (a, b) = neighbours(&sim);
    // Ohne Handel kein Nichtangriff.
    let reason = rejection(
        &mut sim,
        a,
        SimCommand::Propose {
            to: b,
            deal: Deal::Treaty(TreatyKind::NonAggression),
        },
    );
    assert!(reason.contains("Handel"), "{reason}");
    agree(&mut sim, a, b, Deal::Treaty(TreatyKind::Trade));
    agree(&mut sim, b, a, Deal::Treaty(TreatyKind::NonAggression));
    agree(&mut sim, a, b, Deal::Treaty(TreatyKind::Research));
    assert_eq!(diplomacy(&sim).rungs(a, b), 3);
    // Doppelt geht nicht, und unter Handel kein Embargo.
    let again = rejection(
        &mut sim,
        a,
        SimCommand::Propose {
            to: b,
            deal: Deal::Treaty(TreatyKind::Trade),
        },
    );
    assert!(again.contains("besteht bereits"), "{again}");
    let embargo = rejection(
        &mut sim,
        a,
        SimCommand::SetEmbargo {
            target: b,
            on: true,
        },
    );
    assert!(embargo.contains("Handelsvertrag"), "{embargo}");

    // Die Stufenleiter im Lesemodell: Investition offen, Verteidigung offen, Union
    // gesperrt mit Grund.
    let view = relation_view(&ground(&sim), a, b);
    let state = |kind: TreatyKind| {
        view.ladder
            .iter()
            .find(|rung| rung.kind == kind)
            .unwrap()
            .state
            .clone()
    };
    assert!(matches!(state(TreatyKind::Trade), RungState::Active { .. }));
    assert_eq!(state(TreatyKind::Investment), RungState::Available);
    assert!(
        matches!(state(TreatyKind::Union), RungState::Locked(reason) if reason.contains("Investition"))
    );

    // Kündigung des Handels: Mit ihm enden nach der Frist alles, was darauf aufbaut.
    applied(
        &mut sim,
        b,
        SimCommand::CancelTreaty {
            with: a,
            kind: TreatyKind::Trade,
        },
    );
    let notice = data().diplomacy.levers.notice_days;
    sim.run_days(notice - 2);
    assert_eq!(
        diplomacy(&sim).rungs(a, b),
        3,
        "während der Frist gilt alles"
    );
    sim.run_days(3);
    assert_eq!(diplomacy(&sim).rungs(a, b), 0);
    checked(&sim);
}

#[test]
fn war_under_a_pact_is_a_breach() {
    let mut sim = game(6);
    sim.run_days(60);
    let (a, b) = neighbours(&sim);
    agree(&mut sim, a, b, Deal::Treaty(TreatyKind::Trade));
    agree(&mut sim, a, b, Deal::Treaty(TreatyKind::NonAggression));
    applied(
        &mut sim,
        a,
        SimCommand::DeclareWarWithAim {
            target: b,
            aim: WarAim::Subjugation,
        },
    );
    let relation = diplomacy(&sim).relation(a, b).unwrap();
    assert_eq!(relation.rungs(), 0, "der Bruch beendet alle Verträge");
    assert_eq!(
        relation.grievance[sim::diplomacy::relations::side(b, a)],
        1.0
    );
    let war = relation.war.as_ref().unwrap();
    assert_eq!((war.attacker, war.aim), (a, WarAim::Subjugation));
    assert!(war.pop0.iter().all(|pop| *pop > 0.0));
    // Kriegserklärung und Ziel sind öffentliche Taten: Am nächsten Tag stehen sie in den
    // Dossiers aller anderen über den Angreifer.
    sim.run_days(1);
    use sim::diplomacy::ActKind;
    let kinds = |dossier: &sim::diplomacy::Dossier| -> Vec<ActKind> {
        dossier.clues.iter().map(|clue| clue.kind).collect()
    };
    let dossier = diplomacy(&sim).dossier(b, a);
    // Der Antrag des Handels (das erste Binden im Jahr), dann Erklärung und Ziel.
    assert_eq!(
        kinds(&dossier),
        vec![ActKind::Bind, ActKind::Strike, ActKind::Aim]
    );
    assert!(dossier.weight > 0.0);
    let face = diplomacy(&sim).public_face_of(a);
    assert_eq!(kinds(&face), kinds(&dossier));
    // Das Bild des Angreifers rückt vom Kooperativen weg.
    assert!(face.belief()[0] < 0.25);
    checked(&sim);
}

#[test]
fn peace_terms_follow_the_lead() {
    let mut sim = game(8);
    sim.run_days(2 * 360);
    let (a, b) = neighbours(&sim);
    applied(&mut sim, a, SimCommand::DeclareWar { target: b });
    let colony = colonies(&sim, b)[0];
    let harsh = PeaceTerms {
        yielder: Some(b),
        cede: vec![colony],
        tribute_years: 3,
        subjugate: false,
    };
    // Ohne Vorsprung darf der Angreifer nichts fordern …
    let reason = rejection(
        &mut sim,
        a,
        SimCommand::Propose {
            to: b,
            deal: Deal::Peace(harsh.clone()),
        },
    );
    assert!(reason.contains("Vorsprung"), "{reason}");
    // … aber der Unterlegene darf alles anbieten.
    applied(
        &mut sim,
        b,
        SimCommand::Propose {
            to: a,
            deal: Deal::Peace(harsh.clone()),
        },
    );
    let offered = diplomacy(&sim).proposals.last().unwrap().id;
    applied(
        &mut sim,
        b,
        SimCommand::WithdrawProposal { proposal: offered },
    );
    assert!(matches!(
        diplomacy(&sim).history.back().unwrap().status,
        ProposalStatus::Withdrawn
    ));

    // Mit Vorsprung: Abtretung und Tribut. Eine Unterwerfung verlangt das Kriegsziel.
    lead(&mut sim, a, b, 0.6);
    let subjugate = rejection(
        &mut sim,
        a,
        SimCommand::Propose {
            to: b,
            deal: Deal::Peace(PeaceTerms {
                subjugate: true,
                ..harsh.clone()
            }),
        },
    );
    assert!(subjugate.contains("Kriegsziel"), "{subjugate}");
    let population = sim.galaxy().planet(colony).population;
    agree(&mut sim, a, b, Deal::Peace(harsh));
    assert!(!diplomacy(&sim).at_war(a, b));
    assert!(
        diplomacy(&sim)
            .truce_until(a, b, sim.clock().day())
            .is_some()
    );
    assert_eq!(sim.galaxy().planet(colony).owner, Some(a));
    assert_eq!(endgame(&sim).path_of(colony), Path::Forced);
    assert!((endgame(&sim).absorbed_pop[colony.index()] - population).abs() < 1e-6);
    let tribute = diplomacy(&sim).relation(a, b).unwrap().tribute.unwrap();
    assert_eq!(tribute.payer, b);
    // Tribut wird gezahlt, ohne dass Geld entsteht oder vergeht.
    sim.run_days(2 * DAYS_PER_MONTH);
    checked(&sim);
}

#[test]
fn subjugation_and_protection_make_wards() {
    let mut sim = game(8);
    sim.run_days(2 * 360);
    let (a, b) = neighbours(&sim);
    applied(
        &mut sim,
        a,
        SimCommand::DeclareWarWithAim {
            target: b,
            aim: WarAim::Subjugation,
        },
    );
    lead(&mut sim, a, b, 1.2);
    agree(
        &mut sim,
        a,
        b,
        Deal::Peace(PeaceTerms {
            yielder: Some(b),
            cede: Vec::new(),
            tribute_years: 0,
            subjugate: true,
        }),
    );
    let standing = diplomacy(&sim).standing[b.index()];
    assert_eq!(standing.overlord, Some(a));
    assert!(!standing.voluntary);
    for planet in sim.galaxy().planets_of(b) {
        assert_eq!(endgame(&sim).path_of(planet.id), Path::Forced);
    }
    // Ein Vasall schlägt nichts vor.
    let other = sim
        .polities()
        .alive()
        .map(|polity| polity.id)
        .find(|id| *id != a && *id != b)
        .unwrap();
    let reason = rejection(
        &mut sim,
        b,
        SimCommand::Propose {
            to: other,
            deal: Deal::Treaty(TreatyKind::Trade),
        },
    );
    assert!(reason.contains("untersteht"), "{reason}");
    sim.run_days(DAYS_PER_MONTH + 1);
    checked(&sim);

    // Eine Dritte erbittet Schutz bei A.
    agree(&mut sim, other, a, Deal::Protectorate { ward: other });
    let standing = diplomacy(&sim).standing[other.index()];
    assert_eq!(standing.overlord, Some(a));
    assert!(standing.voluntary);
    for planet in sim.galaxy().planets_of(other) {
        assert_ne!(endgame(&sim).path_of(planet.id), Path::Own);
    }
    // Die Schutzmacht entlässt den Vasallen.
    applied(&mut sim, a, SimCommand::ReleaseWard { ward: b });
    assert!(diplomacy(&sim).is_free(b));
    sim.run_days(DAYS_PER_MONTH + 1);
    checked(&sim);
}

#[test]
fn a_union_held_long_enough_ends_in_a_merger() {
    let mut sim = game(4);
    sim.run_days(120);
    let (a, b) = neighbours(&sim);
    for kind in [
        TreatyKind::Trade,
        TreatyKind::NonAggression,
        TreatyKind::Investment,
        TreatyKind::Defence,
        TreatyKind::Union,
    ] {
        agree(&mut sim, a, b, Deal::Treaty(kind));
    }
    let reason = rejection(
        &mut sim,
        a,
        SimCommand::Propose {
            to: b,
            deal: Deal::Merge { lead: a },
        },
    );
    assert!(reason.contains("gehalten"), "{reason}");
    mid_month(&mut sim);
    let hold = data().diplomacy.levers.merge_hold_months;
    sim.world_mut()
        .resource_mut::<Diplomacy>()
        .relation_mut(a, b)
        .merge_months = hold;
    let worlds: Vec<PlanetId> = sim.galaxy().planets_of(b).map(|planet| planet.id).collect();
    agree(&mut sim, a, b, Deal::Merge { lead: a });
    assert!(!sim.polities().get(b).alive);
    for planet in worlds {
        assert_eq!(sim.galaxy().planet(planet).owner, Some(a));
        assert_eq!(endgame(&sim).path_of(planet), Path::Joined);
    }
    assert!(
        diplomacy(&sim)
            .relations
            .keys()
            .all(|(x, y)| *x != b && *y != b)
    );
    sim.run_days(DAYS_PER_MONTH + 1);
    checked(&sim);
}

/// Gelieferte Menge zwischen Systemen von `a` und von `b`.
fn trade_between(sim: &Headless, a: PolityId, b: PolityId) -> f64 {
    let galaxy = sim.galaxy();
    sim.world()
        .resource::<Logistics>()
        .flows
        .iter()
        .filter(|((from, to, _), _)| {
            let (x, y) = (galaxy.system_owner(*from), galaxy.system_owner(*to));
            (x == Some(a) && y == Some(b)) || (x == Some(b) && y == Some(a))
        })
        .map(|(_, quantity)| quantity)
        .sum()
}

#[test]
fn an_embargo_stops_trade_between_the_two() {
    let mut base = game(3);
    base.run_days(240);
    // Das Paar mit dem meisten Handel.
    let alive: Vec<PolityId> = base.polities().alive().map(|polity| polity.id).collect();
    let (a, b) = alive
        .iter()
        .flat_map(|a| alive.iter().map(move |b| (*a, *b)))
        .filter(|(a, b)| a < b)
        .max_by(|x, y| {
            trade_between(&base, x.0, x.1)
                .total_cmp(&trade_between(&base, y.0, y.1))
                .then(y.cmp(x))
        })
        .unwrap();
    assert!(
        trade_between(&base, a, b) > 0.0,
        "kein Handel zwischen Gemeinschaften"
    );
    let mut closed = Headless::from_save(data().clone(), base.save());
    applied(
        &mut closed,
        a,
        SimCommand::SetEmbargo {
            target: b,
            on: true,
        },
    );
    base.run_days(60);
    closed.run_days(60);
    // Kein Frachter der beiden läuft mehr in das Gebiet der anderen ein. Wer beim
    // Embargo dort lag, darf noch heraus. Handelshäuser Dritter dürfen weiter zwischen
    // ihnen fahren.
    let positions = |sim: &Headless| -> Vec<(bool, sim::ids::SystemId)> {
        let logistics = sim.world().resource::<Logistics>();
        logistics
            .freighters
            .iter()
            .map(|freighter| (freighter.alive, freighter.system))
            .collect()
    };
    let mut before = positions(&closed);
    for _ in 0..120 {
        closed.run_days(1);
        let now = positions(&closed);
        let logistics = closed.world().resource::<Logistics>();
        for (index, (alive, system)) in now.iter().enumerate() {
            let moved = before.get(index).is_none_or(|(_, old)| old != system);
            if !*alive || !moved {
                continue;
            }
            let house = logistics.houses[logistics.freighters[index].house.index()].polity;
            let host = closed.galaxy().system_owner(*system);
            for (mine, theirs) in [(a, b), (b, a)] {
                assert!(
                    !(house == mine && host == Some(theirs)),
                    "ein Frachter von {mine} läuft trotz Embargo in das Gebiet von {theirs} ein"
                );
            }
        }
        before = now;
    }
    base.run_days(120);
    let (open, shut) = (trade_between(&base, a, b), trade_between(&closed, a, b));
    assert!(shut < open, "Handel trotz Embargo: {shut} gegen {open}");
    let reason = rejection(
        &mut closed,
        a,
        SimCommand::Propose {
            to: b,
            deal: Deal::Treaty(TreatyKind::Trade),
        },
    );
    assert!(reason.contains("Embargo"), "{reason}");
    checked(&closed);
}

#[test]
fn fleets_need_passage_through_foreign_space() {
    let mut closed = data().clone();
    closed.diplomacy.levers.closed_borders = true;
    let mut sim = Headless::new(
        closed,
        SimConfig {
            seed: 5,
            player: None,
        },
    );
    sim.set_military_mode(None, MilitaryMode::Resting);
    sim.run_days(30);
    let (a, b) = neighbours(&sim);
    let fleet = sim
        .world()
        .resource::<Fleets>()
        .of(a)
        .next()
        .expect("Startflotte")
        .id;
    let target = sim
        .galaxy()
        .planet(sim.polities().get(b).capital.unwrap())
        .system;
    let reason = rejection(&mut sim, a, SimCommand::MoveFleet { fleet, to: target });
    assert!(reason.contains("Durchfahrt"), "{reason}");
    applied(
        &mut sim,
        b,
        SimCommand::SetTransit {
            target: a,
            open: true,
        },
    );
    applied(&mut sim, a, SimCommand::MoveFleet { fleet, to: target });
    applied(
        &mut sim,
        b,
        SimCommand::SetTransit {
            target: a,
            open: false,
        },
    );
    // Im Krieg braucht es keine Erlaubnis.
    applied(&mut sim, a, SimCommand::DeclareWar { target: b });
    applied(&mut sim, a, SimCommand::MoveFleet { fleet, to: target });
    checked(&sim);
}

#[test]
fn the_view_shows_what_the_viewer_may_know() {
    let mut sim = game(2);
    sim.run_days(400);
    let viewer = PolityId(0);
    let views = overview(&ground(&sim), viewer);
    assert_eq!(views.len(), sim.polities().alive().count() - 1);
    for view in &views {
        assert!(view.fleet_estimate.lo <= view.fleet_estimate.hi);
        assert!(view.intel > 0.0 && view.intel <= 1.0);
        assert_eq!(view.dossier.guesses.len(), 4);
        let sum: f32 = view.dossier.guesses.iter().map(|guess| guess.belief).sum();
        assert!((sum - 1.0).abs() < 1e-4);
        assert!((0.0..=100.0).contains(&view.tension));
        assert!(!view.tension_reasons.is_empty());
    }
}

#[test]
fn diplomacy_survives_save_and_load_bit_for_bit() {
    let mut sim = game(7);
    sim.run_days(200);
    let (a, b) = neighbours(&sim);
    let third = sim
        .polities()
        .alive()
        .map(|polity| polity.id)
        .find(|id| *id != a && *id != b)
        .unwrap();
    agree(&mut sim, a, b, Deal::Treaty(TreatyKind::Trade));
    applied(
        &mut sim,
        a,
        SimCommand::SetTransit {
            target: b,
            open: true,
        },
    );
    applied(&mut sim, third, SimCommand::DeclareWar { target: b });
    // Ein offener Vorschlag über die Speicherung hinweg.
    applied(
        &mut sim,
        third,
        SimCommand::Propose {
            to: b,
            deal: Deal::Peace(PeaceTerms::white()),
        },
    );
    sim.run_days(1);
    let text = sim.save().to_ron().unwrap();
    let mut loaded = Headless::from_save(data().clone(), SaveGame::from_ron(&text).unwrap());
    assert_eq!(loaded.hash(), sim.hash());
    assert_eq!(diplomacy(&loaded).proposals, diplomacy(&sim).proposals);
    sim.run_days(400);
    loaded.run_days(400);
    assert_eq!(loaded.hash(), sim.hash());
    checked(&loaded);
}

#[test]
fn same_commands_same_course() {
    let run = || {
        let mut sim = Headless::new(
            data().clone(),
            SimConfig {
                seed: 11,
                player: None,
            },
        );
        sim.run_days(100);
        let (a, b) = neighbours(&sim);
        agree(&mut sim, a, b, Deal::Treaty(TreatyKind::Trade));
        applied(&mut sim, b, SimCommand::DeclareWar { target: a });
        sim.run_days(3 * 360);
        checked(&sim);
        (sim.hash(), sim.command_log().ai_rejected)
    };
    assert_eq!(run(), run());
}

#[test]
fn ai_answers_every_proposal_in_time() {
    let mut sim = game(9);
    sim.run_days(100);
    let (a, b) = neighbours(&sim);
    applied(
        &mut sim,
        a,
        SimCommand::Propose {
            to: b,
            deal: Deal::Treaty(TreatyKind::Trade),
        },
    );
    let id = diplomacy(&sim).proposals.last().unwrap().id;
    let (_, late) = data().diplomacy.levers.answer_days;
    sim.run_days(late + 1);
    assert!(diplomacy(&sim).proposals.iter().all(|open| open.id != id));
    let record = diplomacy(&sim)
        .history
        .iter()
        .find(|record| record.proposal.id == id)
        .unwrap();
    assert!(matches!(
        record.status,
        ProposalStatus::Accepted | ProposalStatus::Declined(Some(_))
    ));
    // Das Ergebnis steht im Dossier von A über B (die Antwort galt A).
    assert!(!diplomacy(&sim).dossier(a, b).clues.is_empty());
}

#[test]
fn the_view_never_reads_the_hidden_minds() {
    let mut sim = game(12);
    sim.run_days(200);
    let before: Vec<_> = overview(&ground(&sim), PolityId(1))
        .into_iter()
        .map(|view| format!("{view:?}"))
        .collect();
    // Andere Haltungen, sonst nichts geändert: Das Lesemodell bleibt gleich.
    let count = sim.polities().list.len();
    sim.world_mut().resource_mut::<sim::ai::AiState>().minds =
        sim::ai::mind::Minds::deal(data(), 999, count);
    let after: Vec<_> = overview(&ground(&sim), PolityId(1))
        .into_iter()
        .map(|view| format!("{view:?}"))
        .collect();
    assert_eq!(before, after);
    // Zu Beginn weiß niemand etwas: Jede Einschätzung ist gleichverteilt.
    let fresh = game(12);
    for view in overview(&ground(&fresh), PolityId(0)) {
        for guess in &view.dossier.guesses {
            assert!((guess.belief - 0.25).abs() < 1e-6);
        }
    }
}
