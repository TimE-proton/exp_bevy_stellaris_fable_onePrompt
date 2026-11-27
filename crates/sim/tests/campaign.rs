//! Belagerung und Verteidigung durch die KI in ganzen Partien: Kriege erklärt ein
//! Drehbuch, Flotten, Feldzüge, Belagerungen und Verteidigungspläne führt auf allen
//! Seiten die KI.

use sim::GameData;
use sim::ai::{AiState, CampaignStage, MilitaryMode};
use sim::clock::DAYS_PER_YEAR;
use sim::command::SimCommand;
use sim::economy::{Claim, to_credits};
use sim::harness::{
    Headless, ScriptedAct, ScriptedEvent, WarScript, campaign_wars, check_invariants, summarize,
};
use sim::ids::PolityId;
use sim::siege::Sieges;
use sim::state::{SaveGame, SimConfig, money_drift};
use std::sync::OnceLock;

fn data() -> &'static GameData {
    static DATA: OnceLock<GameData> = OnceLock::new();
    DATA.get_or_init(|| GameData::load_workspace().unwrap())
}

fn sieges(sim: &Headless) -> &Sieges {
    sim.world().resource::<Sieges>()
}

#[test]
fn the_ai_besieges_takes_worlds_and_ends_a_community_in_wars_it_leads_itself() {
    // Das Drehbuch „Feldzug" in einer ganzen Partie: Es erklärt nur die Kriege. Vom
    // ersten Krieg an Tag für Tag geprüft, zwölf Jahre lang; danach bis zum Jahr 60.
    let script = WarScript::new(campaign_wars());
    let seed = 2;
    let mut sim = Headless::new(data().clone(), SimConfig { seed, player: None });
    let war = 20 * DAYS_PER_YEAR;
    script.run_until(&mut sim, war, false);
    assert_eq!(check_invariants(sim.world()), Ok(()));
    assert_eq!(sieges(&sim).stats.begun, 0, "im Frieden belagert niemand");
    let alive_before = sim.polities().alive().count();
    // Belagernde Flotten und Tage, an denen ein Feldzug in jedem seiner Abschnitte stand.
    let mut stages = [0u32; 3];
    let mut longest_card = 0;
    for day in war + 1..=war + 12 * DAYS_PER_YEAR {
        script.run_until(&mut sim, day, false);
        assert_eq!(money_drift(sim.world()), Some(0), "Tag {day}");
        assert_eq!(check_invariants(sim.world()), Ok(()), "Tag {day}");
        let ai = sim.world().resource::<AiState>();
        for polity in sim.polities().alive() {
            if let Some(campaign) = ai.plan(polity.id).campaign {
                stages[campaign.stage as usize] += 1;
                // Die Flotte eines Feldzugs gehört nicht der Grundhaltung.
                assert!(ai.is_reserved(campaign.fleet), "Tag {day}");
            }
        }
        longest_card = longest_card.max(
            sim.siege_cards()
                .iter()
                .map(|card| card.days)
                .max()
                .unwrap_or(0),
        );
    }
    assert!(
        stages.iter().all(|days| *days > 0),
        "Sammeln, Marsch und Belagerung kommen vor: {stages:?}"
    );
    let _ = (
        CampaignStage::Muster,
        CampaignStage::March,
        CampaignStage::Siege,
    );
    script.run_until(&mut sim, 60 * DAYS_PER_YEAR, false);
    assert_eq!(check_invariants(sim.world()), Ok(()));
    assert_eq!(money_drift(sim.world()), Some(0));

    let summary = summarize(&sim);
    let stats = &summary.sieges;
    println!("Seed {seed}: {stats:?}; Abschnitte {stages:?}");
    // Die KI hat Feldzüge begonnen, belagert und Welten genommen, auf mehr als einem Weg.
    let campaigns: u32 = summary.polities.iter().map(|polity| polity.campaigns).sum();
    assert!(campaigns >= 5, "{campaigns} Feldzüge");
    assert!(stats.begun >= 5 && stats.fallen >= 5, "{stats:?}");
    assert_eq!(stats.fallen, stats.conquests());
    let ways = [stats.stormed, stats.starved, stats.capitulated]
        .iter()
        .filter(|count| **count > 0)
        .count();
    assert!(ways >= 2, "nur ein Weg führte zum Ziel: {stats:?}");
    assert!(
        stats.capitulated >= 1,
        "keine Welt wurde übergeben: {stats:?}"
    );
    // Keine Belagerung der KI dauert länger als die Frist jeder Einschließung, und keine
    // Flotte liegt am Ende mit leerem Tank in fremdem Gebiet.
    let bound = data().balance.siege.max_siege_days as u64;
    assert!(stats.longest <= bound && longest_card <= bound, "{stats:?}");
    assert_eq!(summary.fleets_dry_abroad, 0);
    // Eine Gemeinschaft hat mit ihrer letzten Welt geendet, über den gemeinsamen Pfad
    // der Nachfolge: Sie hält nichts mehr, führt keinen Krieg und keine Flotte.
    assert!(stats.ended >= 1, "{stats:?}");
    assert!(sim.polities().list.len() >= alive_before);
    let ended: Vec<PolityId> = sim
        .polities()
        .list
        .iter()
        .filter(|polity| !polity.alive)
        .map(|polity| polity.id)
        .collect();
    assert!(!ended.is_empty());
    for polity in ended {
        assert_eq!(sim.galaxy().planets_of(polity).count(), 0);
        assert!(
            !sim.world()
                .resource::<sim::diplomacy::Diplomacy>()
                .is_at_war(polity)
        );
        assert_eq!(
            sim.world()
                .resource::<sim::fleet::Fleets>()
                .of(polity)
                .count(),
            0
        );
        assert_eq!(
            sim.world().resource::<AiState>().plan(polity).campaign,
            None
        );
    }
    // Befehle der KI werden fast immer angenommen: Sie prüft, bevor sie befiehlt.
    assert!(
        summary.ai_rejected_share() < 0.02,
        "{} von {} KI-Befehlen abgelehnt",
        summary.ai_rejected,
        summary.ai_applied + summary.ai_rejected
    );
}

#[test]
fn in_peace_every_world_of_the_ai_gets_stores_and_the_defence_stays_cheap() {
    let levers = &data().balance.military.ai.siege;
    let seed = 3;
    let mut sim = Headless::new(data().clone(), SimConfig { seed, player: None });
    sim.run_days(30 * DAYS_PER_YEAR);
    assert_eq!(check_invariants(sim.world()), Ok(()));
    let summary = summarize(&sim);
    // Jede Welt hat einen Plan mit Depot, und fast alle Depots sind gefüllt: Keine
    // Kolonie stünde in einer Einschließung vom ersten Tag an ohne Vorräte da.
    let state = sieges(&sim);
    let mut worlds = 0;
    for planet in sim
        .galaxy()
        .planets
        .iter()
        .filter(|planet| planet.owner.is_some())
    {
        worlds += 1;
        let defense = state.defense(planet.id);
        assert!(
            defense.plan.depot_days >= levers.defense_interior.depot_days,
            "{}",
            planet.name
        );
    }
    assert!(worlds > 20);
    assert!(summary.colonies > 20);
    assert!(
        summary.colonies_without_stores * 10 <= summary.colonies,
        "{} von {} Kolonien ohne Vorräte",
        summary.colonies_without_stores,
        summary.colonies
    );
    // Im Frieden bleibt die Verteidigung billig: unter defense_share der Einnahmen, und
    // niemand hält Truppen für einen Feldzug.
    for polity in summary.polities.iter().filter(|polity| polity.alive) {
        let paid = to_credits(polity.budget.line(Claim::Defense).paid);
        let revenue = to_credits(polity.budget.revenue).max(1.0);
        assert!(
            paid / revenue < levers.defense_share,
            "{}: Verteidigung {:.1} % der Einnahmen",
            polity.name,
            100.0 * paid / revenue
        );
        assert_eq!(polity.campaigns, 0);
    }
    assert!(state.embarked.is_empty() && state.active.is_empty());
    assert_eq!(summary.sieges.begun, 0);
}

#[test]
fn a_plan_set_by_script_and_a_resting_community_are_left_alone() {
    let mut sim = Headless::new(
        data().clone(),
        SimConfig {
            seed: 5,
            player: None,
        },
    );
    let (held, resting) = (PolityId(0), PolityId(1));
    sim.set_military_mode(Some(resting), MilitaryMode::Resting);
    let script = WarScript::new(vec![ScriptedEvent {
        day: 30,
        act: ScriptedAct::Garrison {
            who: held,
            regulars: 21.0,
        },
    }]);
    script.run_until(&mut sim, 3 * DAYS_PER_YEAR, false);
    let state = sieges(&sim);
    let home = |polity: PolityId| sim.polities().get(polity).capital.unwrap();
    // Der Plan des Drehbuchs steht nach drei Jahren noch, obwohl die KI die übrigen
    // Welten der Gemeinschaft plant.
    assert_eq!(state.defense(home(held)).plan.regulars, 21.0);
    let colony = sim
        .galaxy()
        .planets_of(held)
        .find(|planet| planet.id != home(held))
        .expect("eine Kolonie");
    assert!(state.defense(colony.id).plan.depot_days > 0);
    // Wo die militärische Führung ruht, plant die KI nichts.
    for planet in sim
        .galaxy()
        .planets_of(resting)
        .filter(|planet| planet.id != home(resting))
    {
        assert_eq!(
            state.defense(planet.id).plan.depot_days,
            0,
            "{}",
            planet.name
        );
    }
    // Die dritte Gemeinschaft führt die KI ganz: Auch ihre Kolonien haben Depots.
    let third = PolityId(2);
    assert!(
        sim.galaxy()
            .planets_of(third)
            .filter(|planet| planet.id != home(third))
            .all(|planet| state.defense(planet.id).plan.depot_days > 0)
    );
}

#[test]
fn ai_sieges_replay_identically_with_every_tick_and_across_saving() {
    // Ein Feldzug-Krieg von zwei Jahren ab dem Jahr 20, schnell, mit jedem Takt, und
    // über Speichern und Laden mitten im Krieg: dieselbe Partie.
    let script = WarScript::new(campaign_wars());
    let config = SimConfig {
        seed: 7,
        player: None,
    };
    let start = 20 * DAYS_PER_YEAR;
    let days = start + 2 * DAYS_PER_YEAR;
    let mut fast = Headless::new(data().clone(), config.clone());
    script.run_until(&mut fast, start, false);
    let text = fast.save().to_ron().unwrap();
    script.run_until(&mut fast, days, false);
    let campaigns: u32 = summarize(&fast)
        .polities
        .iter()
        .map(|polity| polity.campaigns)
        .sum();
    assert!(campaigns >= 1, "die Partie zeigt einen Feldzug der KI");

    let mut exact = Headless::from_save(data().clone(), SaveGame::from_ron(&text).unwrap());
    script.run_until(&mut exact, days, true);
    assert_eq!(exact.hash(), fast.hash());

    let mut first = Headless::from_save(data().clone(), SaveGame::from_ron(&text).unwrap());
    script.run_until(&mut first, start + 400, false);
    let middle = first.save().to_ron().unwrap();
    let mut resumed = Headless::from_save(data().clone(), SaveGame::from_ron(&middle).unwrap());
    assert_eq!(resumed.hash(), first.hash());
    script.run_until(&mut resumed, days, false);
    assert_eq!(resumed.hash(), fast.hash());
    // Der Spieler führt seine Gemeinschaft selbst: Für sie plant die KI nichts.
    let me = PolityId(0);
    let mut own = Headless::new(
        data().clone(),
        SimConfig {
            seed: 7,
            player: Some(me),
        },
    );
    own.run_days(2 * DAYS_PER_YEAR);
    let home = own.polities().get(me).capital.unwrap();
    assert!(
        own.galaxy()
            .planets_of(me)
            .filter(|planet| planet.id != home)
            .all(|planet| sieges(&own).defense(planet.id).plan.depot_days == 0)
    );
    let _ = SimCommand::LiftSiege { planet: home };
}
