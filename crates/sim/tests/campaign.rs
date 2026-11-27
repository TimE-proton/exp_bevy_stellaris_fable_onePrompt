//! Belagerung und Verteidigung durch die KI in ganzen Partien: Kriege erklärt ein
//! Drehbuch, Flotten, Feldzüge, Belagerungen und Verteidigungspläne führt auf allen
//! Seiten die KI.

use sim::GameData;
use sim::ai::siege::{defense_budget, plan_upkeep};
use sim::ai::{AiState, MilitaryMode};
use sim::clock::{DAYS_PER_MONTH, DAYS_PER_YEAR};
use sim::diplomacy::Diplomacy;
use sim::economy::{Claim, Economy, to_credits};
use sim::fleet::Fleets;
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

// Grenzen für den Lauf über Seed 2, gemessen an ihm (Stand dieses Tests): 50 Flottentage
// mit leerem Tank fern jedem eigenen Hafen, die längste Folge 25 Tage, kein Flottentag
// mit müßigen Truppen an Bord. Der Spielraum lässt Änderungen an anderer Stelle Platz;
// ohne die Heimkehr nach dem Feldzug sind es über 130 Tanktage, ohne das Ausschiffen im
// Krieg Tausende Truppentage.
const DRY_LONGEST: u32 = 60;
const DRY_DAYS: u32 = 100;
const IDLE_TROOP_DAYS: u32 = 30;

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
    // Flottentage im Krieg mit Truppen an Bord, ohne Feldzug und ohne Belagerung, länger
    // als `land_after_days` und einen Monat für den Weg in den Hafen nach dem Ende des
    // letzten Feldzugs: Diese Truppen fehlen der Verteidigung.
    let levers = &data().balance.military.ai.siege;
    let mut idle_troop_days = 0u32;
    for day in war + 1..=war + 12 * DAYS_PER_YEAR {
        script.run_until(&mut sim, day, false);
        assert_eq!(money_drift(sim.world()), Some(0), "Tag {day}");
        assert_eq!(check_invariants(sim.world()), Ok(()), "Tag {day}");
        let ai = sim.world().resource::<AiState>();
        let at_war = sim.world().resource::<sim::diplomacy::Diplomacy>();
        for polity in sim.polities().alive() {
            let plan = ai.plan(polity.id);
            if let Some(campaign) = plan.campaign {
                stages[campaign.stage as usize] += 1;
                // Die Flotte eines Feldzugs gehört nicht der Grundhaltung.
                assert!(ai.is_reserved(campaign.fleet), "Tag {day}");
            }
            if !at_war.is_at_war(polity.id)
                || plan.campaign.is_some()
                || day < plan.campaign_end + levers.land_after_days + 30
            {
                continue;
            }
            idle_troop_days += sim
                .world()
                .resource::<sim::fleet::Fleets>()
                .of(polity.id)
                .filter(|fleet| {
                    !sieges(&sim).assign.contains_key(&fleet.id)
                        && sieges(&sim)
                            .embarked
                            .get(&fleet.id)
                            .is_some_and(|force| force.troops > 0.0)
                })
                .count() as u32;
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
    script.run_until(&mut sim, 60 * DAYS_PER_YEAR, false);
    assert_eq!(check_invariants(sim.world()), Ok(()));
    assert_eq!(money_drift(sim.world()), Some(0));

    let summary = summarize(&sim);
    let stats = &summary.sieges;
    println!(
        "Seed {seed}: {stats:?}; Abschnitte {stages:?}; Truppen müßig an Bord {idle_troop_days} \
         Flottentage; leerer Tank fern jedem Hafen {} Flottentage, längste Folge {} Tage",
        summary.dry_fleet_days, summary.dry_longest
    );
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
    // Keine Belagerung der KI dauert länger als die Frist jeder Einschließung.
    let bound = data().balance.siege.max_siege_days as u64;
    assert!(stats.longest <= bound && longest_card <= bound, "{stats:?}");
    // Über die ganze Partie gezählt, nicht nur am letzten Tag: Keine Flotte liegt lange
    // mit leerem Tank fern jedem eigenen Hafen, solange ihre Gemeinschaft einen hat; was
    // vorkommt, ist der Weg dorthin, den sie mit leerem Tank nur langsam schafft.
    assert!(
        summary.dry_longest <= DRY_LONGEST && summary.dry_fleet_days <= DRY_DAYS,
        "leerer Tank fern jedem Hafen: {} Flottentage, längste Folge {} Tage",
        summary.dry_fleet_days,
        summary.dry_longest
    );
    // Nach einem Feldzug gehen Truppen auch im Krieg wieder an Land.
    assert!(
        idle_troop_days <= IDLE_TROOP_DAYS,
        "{idle_troop_days} Flottentage mit müßigen Truppen an Bord"
    );
    // Eine Gemeinschaft hat mit ihrer letzten Welt geendet, über den gemeinsamen Pfad
    // der Nachfolge: Sie hält nichts mehr, führt keinen Krieg und keine Flotte. Dass
    // Gemeinschaften enden, urteilt der Test über mehrere Seeds; hier zählt, wie.
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

/// Was eine Partie des Drehbuchs „Feldzug" über die Zeit zeigt, für
/// [`over_seeds_the_ai_ends_communities_and_its_fleets_troops_and_purse_come_home`].
#[derive(Debug, Default)]
struct Watched {
    seed: u64,
    begun: u32,
    fallen: u32,
    ended: u32,
    dry_days: u32,
    dry_longest: u32,
    /// Flottenmonate im Krieg mit müßigen Truppen an Bord, am Monatsersten gezählt wie im
    /// Test über Seed 2 Tag für Tag.
    idle_troop_months: u32,
    /// Monate einer Gemeinschaft, deren Einnahmen der letzten drei Monate unter 70 % des
    /// langen Mittels liegen, und davon die, in denen der Unterhalt aller ihrer Pläne
    /// mehr als das Anderthalbfache des Etats ([`defense_budget`]) kostet.
    shrunk_months: u32,
    excess_months: u32,
    /// Die höchsten Kosten der Verteidigung einer Gemeinschaft in einem Jahr, gemessen an
    /// ihren Einnahmen desselben Jahres.
    defense_peak: f64,
}

fn watch_campaign_game(seed: u64, years: u64) -> Watched {
    let script = WarScript::new(campaign_wars());
    let levers = &data().balance.military.ai;
    let mut sim = Headless::new(data().clone(), SimConfig { seed, player: None });
    let mut watched = Watched {
        seed,
        ..Default::default()
    };
    let mut day = 0;
    while day < years * DAYS_PER_YEAR {
        day += DAYS_PER_MONTH;
        script.run_until(&mut sim, day, false);
        let ai = sim.world().resource::<AiState>();
        let diplomacy = sim.world().resource::<Diplomacy>();
        let economy = sim.world().resource::<Economy>();
        let fleets = sim.world().resource::<Fleets>();
        let state = sieges(&sim);
        for polity in sim.polities().alive() {
            let plan = ai.plan(polity.id);
            let war = diplomacy.is_at_war(polity.id);
            if war
                && plan.campaign.is_none()
                && day >= plan.campaign_end + levers.siege.land_after_days + 30
            {
                watched.idle_troop_months += fleets
                    .of(polity.id)
                    .filter(|fleet| {
                        !state.assign.contains_key(&fleet.id)
                            && state
                                .embarked
                                .get(&fleet.id)
                                .is_some_and(|force| force.troops > 0.0)
                    })
                    .count() as u32;
            }
            let Some(budget) = economy.budget(polity.id) else {
                continue;
            };
            if plan.usual_revenue > 0.0 && budget.history.len() >= 3 {
                let recent: Vec<f64> = budget
                    .history
                    .iter()
                    .rev()
                    .take(3)
                    .map(|month| to_credits(month.revenue))
                    .collect();
                if recent.iter().sum::<f64>() / 3.0 < 0.7 * plan.usual_revenue {
                    watched.shrunk_months += 1;
                    let allowed = defense_budget(data(), plan.usual_revenue, &recent, war);
                    let planned: f64 = sim
                        .galaxy()
                        .planets_of(polity.id)
                        .filter(|planet| state.siege(planet.id).is_none())
                        .map(|planet| plan_upkeep(data(), &state.defense(planet.id).plan))
                        .sum();
                    if planned > 1.5 * allowed {
                        watched.excess_months += 1;
                    }
                }
            }
            if day.is_multiple_of(DAYS_PER_YEAR) && budget.history.len() >= 12 {
                let year = budget.year();
                let revenue = to_credits(year.revenue);
                if revenue > 0.0 {
                    let share = to_credits(year.line(Claim::Defense).paid) / revenue;
                    watched.defense_peak = watched.defense_peak.max(share);
                }
            }
        }
    }
    assert_eq!(check_invariants(sim.world()), Ok(()), "Seed {seed}");
    assert_eq!(money_drift(sim.world()), Some(0), "Seed {seed}");
    let summary = summarize(&sim);
    watched.begun = summary.sieges.begun;
    watched.fallen = summary.sieges.fallen;
    watched.ended = summary.sieges.ended;
    watched.dry_days = summary.dry_fleet_days;
    watched.dry_longest = summary.dry_longest;
    watched
}

/// Die Seeds, über die geurteilt wird (ohne Seed 2, den der Test Tag für Tag prüft), und
/// die Jahre: Die Kriege des Drehbuchs enden mit dem Frieden im Jahr 50.
const SEEDS: [u64; 4] = [0, 1, 3, 4];
const YEARS: u64 = 52;

#[test]
fn over_seeds_the_ai_ends_communities_and_its_fleets_troops_and_purse_come_home() {
    // Jede Partie in einem eigenen Faden; geurteilt wird über alle zusammen, damit kein
    // einzelner Seed trägt, was die KI im Allgemeinen tun soll. Gemessen (Stand dieses
    // Tests): je Seed 16 bis 43 Belagerungen und 13 bis 40 gefallene Welten, zusammen 3
    // beendete Gemeinschaften; höchstens 2 Flottenmonate mit müßigen Truppen; 68 von
    // 1285 geschrumpften Monaten über dem Etat, die teuerste Verteidigung 45 % der
    // Einnahmen eines Jahres; zusammen 474 Flottentage mit leerem Tank fern jedem Hafen,
    // die längste Folge 65 Tage.
    let runs: Vec<Watched> = std::thread::scope(|scope| {
        let handles: Vec<_> = SEEDS
            .iter()
            .map(|seed| scope.spawn(move || watch_campaign_game(*seed, YEARS)))
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect()
    });
    for run in &runs {
        println!("Seed {}: {run:?}", run.seed);
    }
    // In jeder Partie belagert die KI und nimmt Welten; über alle enden Gemeinschaften.
    for run in &runs {
        assert!(run.begun >= 10 && run.fallen >= 8, "{run:?}");
    }
    let ended: u32 = runs.iter().map(|run| run.ended).sum();
    assert!(ended >= 2, "{ended} beendete Gemeinschaften: {runs:?}");
    // Wer schrumpft, kürzt die Verteidigung: Der Etat folgt den Einnahmen der letzten
    // Monate (ohne das: über ein Drittel der geschrumpften Monate über dem Etat und
    // Verteidigung für mehr als das Doppelte der Einnahmen eines Jahres).
    let shrunk: u32 = runs.iter().map(|run| run.shrunk_months).sum();
    let excess: u32 = runs.iter().map(|run| run.excess_months).sum();
    assert!(shrunk >= 200, "{runs:?}");
    assert!(
        excess * 8 <= shrunk,
        "{excess} von {shrunk} geschrumpften Monaten über dem Etat: {runs:?}"
    );
    for run in &runs {
        assert!(run.defense_peak <= 1.0, "{run:?}");
    }
    // Nach einem Feldzug gehen Truppen auch im Krieg wieder an Land (ohne das Ausschiffen
    // im Krieg: über 90 Flottenmonate je Seed).
    let idle: u32 = runs.iter().map(|run| run.idle_troop_months).sum();
    assert!(
        idle <= 12,
        "{idle} Flottenmonate mit müßigen Truppen: {runs:?}"
    );
    // Keine Flotte liegt lange mit leerem Tank fern jedem eigenen Hafen.
    let dry: u32 = runs.iter().map(|run| run.dry_days).sum();
    assert!(dry <= 1000, "{dry} Flottentage mit leerem Tank: {runs:?}");
    for run in &runs {
        assert!(run.dry_longest <= 120, "{run:?}");
    }
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
}
