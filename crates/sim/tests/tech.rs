//! Technologie und Forschung als Ganzes: Tempo, Geld und Arbeit, Wirkungen, Baum, Einblick,
//! Wissenstransfer, Abspaltung, Determinismus.

use sim::GameData;
use sim::chronicle::{Chronicle, ChronicleKind};
use sim::clock::{DAYS_PER_MONTH, DAYS_PER_YEAR};
use sim::command::{CommandLog, CommandOutcome, SimCommand};
use sim::data::TechBranch;
use sim::economy::{
    Economy, EquityFlows, Money, expected_return, invest_and_depreciate, produce_and_consume,
    seed_colony, settle_public_accounts, state_reserve, steer_money_supply, to_credits, to_money,
    unit_economics_at,
};
use sim::finance::{Finance, Stage};
use sim::galaxy::Galaxy;
use sim::harness::{Headless, check_invariants, run_ai_game};
use sim::ids::{PlanetId, PolityId};
use sim::logistics::Logistics;
use sim::state::{SaveGame, SimConfig, SimState, money_drift};
use sim::tech::{
    Research, TechEffects, TierEstimate, contact_weights, estimate_width, research_month,
    update_beliefs, visible_spillover,
};

fn data() -> GameData {
    GameData::load_workspace().unwrap()
}

fn observer(seed: u64) -> Headless {
    Headless::new(data(), SimConfig { seed, player: None })
}

/// Partie, in der Gemeinschaft 0 vom Menschen geführt wird.
fn with_player(seed: u64) -> Headless {
    Headless::new(
        data(),
        SimConfig {
            seed,
            player: Some(PolityId(0)),
        },
    )
}

fn tech(data: &GameData, key: &str) -> u16 {
    data.tech_index(key).expect("Technologie in techs.ron")
}

/// Ausgangslage ohne laufende Partie, um einzelne Forschungsmonate gezielt zu rechnen.
fn start(data: &GameData, seed: u64) -> SimState {
    SimState::generate(data, SimConfig { seed, player: None })
}

/// Ein Forschungsmonat auf dem nackten Zustand, ohne den Rest der Simulation.
fn research_step(data: &GameData, world: &mut SimState) {
    research_month(
        &mut world.research,
        &mut world.economy,
        &world.galaxy,
        &world.polities,
        &world.finance,
        &world.logistics,
        &world.diplomacy,
        &mut world.chronicle,
        data,
        &world.config,
        world.clock.tick,
    );
}

/// Setzt die laufenden Staatseinnahmen des Vormonats, an denen sich der Etat bemisst.
fn set_income(world: &mut SimState, polity: usize, credits: f64) {
    world.economy.state_income[polity] = to_money(credits);
}

/// Stellt einen Befehl ein, führt ihn aus und gibt zurück, was aus ihm wurde.
fn command(sim: &mut Headless, polity: PolityId, command: SimCommand) -> CommandOutcome {
    sim.issue(polity, command);
    sim.tick();
    sim.command_log()
        .entries
        .last()
        .expect("Befehle von außen werden protokolliert")
        .outcome
        .clone()
}

fn rejected_with(outcome: CommandOutcome, needle: &str) {
    match outcome {
        CommandOutcome::Rejected(reason) => {
            assert!(reason.contains(needle), "unerwartete Begründung: {reason}")
        }
        CommandOutcome::Applied => panic!("Befehl hätte abgelehnt werden müssen ({needle})"),
    }
}

fn households_of(world: &SimState, polity: PolityId) -> Money {
    world
        .galaxy
        .planets_of(polity)
        .map(|planet| world.economy.planet(planet.id).households)
        .sum()
}

/// Tagesausstoß aller Werke einer Gemeinschaft, bewertet zu Richtpreisen: ein Maß ihrer
/// Wirtschaftsleistung, das weder von den Preisen des Tages abhängt noch in die
/// Forschungsrechnung eingeht.
fn output_of(sim: &Headless, data: &GameData, polity: PolityId) -> f64 {
    let economy = sim.world().resource::<Economy>();
    sim.galaxy()
        .planets_of(polity)
        .map(|planet| {
            economy
                .planet(planet.id)
                .output
                .iter()
                .zip(&data.recipes)
                .map(|(quantity, recipe)| quantity * data.good(recipe.output_id).base_price)
                .sum::<f64>()
        })
        .sum()
}

/// Setzt für alle Gemeinschaften denselben Etat, auch als den üblichen der KI.
fn set_all_shares(sim: &mut Headless, share: f64) {
    let mut research = sim.world_mut().resource_mut::<Research>();
    for entry in &mut research.polities {
        entry.budget_share = share;
        entry.usual_share = share;
    }
}

// --- 1. Tempo folgt Wirtschaftskraft und Bevölkerung ---

#[test]
fn more_money_and_more_people_mean_more_points() {
    let data = data();
    let mut base = start(&data, 2);
    let me = PolityId(0);
    let capital = base.polities.get(me).capital.unwrap();
    set_income(&mut base, 0, 10_000.0);
    base.research.polities[0].budget_share = 0.1;
    // Derselbe Monat, je einmal mit einer einzigen Abweichung.
    let points = |change: &dyn Fn(&mut SimState)| -> f64 {
        let mut world = base.clone();
        change(&mut world);
        research_step(&data, &mut world);
        world.research.polities[0].points_last_month
    };
    let normal = points(&|_| {});
    assert!(normal > 0.0);
    // Doppelte Einnahmen oder doppelter Etat: doppelte Punkte.
    let richer = points(&|world| set_income(world, 0, 20_000.0));
    let keener = points(&|world| world.research.polities[0].budget_share *= 2.0);
    assert!((richer / normal - 2.0).abs() < 1e-4, "{normal} → {richer}");
    assert!((keener / normal - 2.0).abs() < 1e-4, "{normal} → {keener}");
    // Doppelte Bevölkerung: mehr Punkte, aber deutlich weniger als das Doppelte, und der
    // Gewinn je zusätzlichem Kopf sinkt weiter.
    let larger = points(&|world| world.galaxy.planet_mut(capital).population *= 2.0);
    let largest = points(&|world| world.galaxy.planet_mut(capital).population *= 4.0);
    assert!(larger > normal * 1.1, "{normal} → {larger}");
    assert!(larger < normal * 1.9, "{normal} → {larger}");
    assert!(largest > larger);
    assert!((largest - larger) / 2.0 < larger - normal);
}

#[test]
fn the_budget_follows_income_not_the_movement_of_the_treasury() {
    let data = data();
    let mut base = start(&data, 2);
    set_income(&mut base, 0, 8_000.0);
    let share = base.research.polities[0].budget_share;
    let spent = |change: &dyn Fn(&mut SimState)| -> Money {
        let mut world = base.clone();
        change(&mut world);
        research_step(&data, &mut world);
        world.research.polities[0].spent_last_month
    };
    let planets = base.galaxy.planets_of(PolityId(0)).count() as Money;
    let normal = spent(&|_| {});
    let budget = to_money(share * 8_000.0);
    assert!(
        normal <= budget && normal >= budget - planets,
        "{normal} von {budget}"
    );
    // Ein einmaliger Zufluss (Anleihe, Verkauf, Geld der Notenbank) füllt die Kasse, hebt
    // aber den Etat nicht.
    let windfall = spent(&|world| world.economy.treasuries[0] += to_money(1_000_000.0));
    assert_eq!(windfall, normal);
    // Die geglättete Kassenbewegung, die Anleihen und Kriegskasse der KI bemisst, spielt
    // für die Forschung keine Rolle.
    for revenue in [0.0, 1.0e6] {
        let other_estimate = spent(&|world| world.finance.polities[0].revenue = revenue);
        assert_eq!(other_estimate, normal);
    }
    // Was zählt, sind die laufenden Einnahmen: ohne sie keine Forschung, auch bei voller
    // Kasse; mit doppelten das Doppelte.
    let idle = spent(&|world| {
        set_income(world, 0, 0.0);
        world.economy.treasuries[0] += to_money(1_000_000.0);
    });
    assert_eq!(idle, 0);
    let doubled = spent(&|world| set_income(world, 0, 16_000.0));
    assert!(
        (doubled - 2 * normal).abs() <= planets,
        "{normal} → {doubled}"
    );
}

#[test]
fn state_income_counts_what_the_state_takes_in_month_by_month() {
    let mut sim = observer(6);
    // Drei volle Monate, der Abschluss des dritten und der erste Tag des vierten.
    sim.run_days(3 * DAYS_PER_MONTH + 2);
    let count = sim.polities().list.len();
    let economy = sim.world().resource::<Economy>().clone();
    assert_eq!(economy.state_income.len(), count);
    assert_eq!(economy.state_income_running.len(), count);
    for polity in 0..count {
        // Ein voller Monat ist abgeschlossen, der neue hat eben begonnen: Er enthält die
        // Lohnsteuer eines Tages, der letzte die von dreißig und dazu Zölle und Dividenden.
        let last = economy.state_income[polity];
        let running = economy.state_income_running[polity];
        assert!(last > 0 && running > 0, "G{polity}: {last} / {running}");
        assert!(
            running * 12 < last,
            "G{polity}: der laufende Monat ({running}) ist kaum begonnen, der letzte war {last}"
        );
    }
    // Im Lauf des Monats wächst die laufende Rechnung Tag für Tag um die Lohnsteuer; der
    // abgeschlossene Monat bleibt stehen.
    sim.run_days(10);
    let later = sim.world().resource::<Economy>();
    for polity in 0..count {
        assert_eq!(later.state_income[polity], economy.state_income[polity]);
        assert!(later.state_income_running[polity] > 5 * economy.state_income_running[polity]);
    }
    assert_eq!(check_invariants(sim.world()), Ok(()));
}

#[test]
fn only_recurring_revenue_counts_as_state_income() {
    let data = data();
    let base = start(&data, 6);
    let me = PolityId(0);
    let capital = base.polities.get(me).capital.unwrap();
    let home = base.galaxy.planet(capital).system;
    let capitals: Vec<Option<PlanetId>> = base.polities.list.iter().map(|p| p.capital).collect();
    assert!(
        base.economy
            .state_income_running
            .iter()
            .all(|income| *income == 0)
    );

    // Lohnsteuer: Ein Produktionstag bucht genau das als Einnahme, was in die Kasse fließt.
    let mut world = base.clone();
    produce_and_consume(&mut world.galaxy, &mut world.economy, &data);
    for index in 0..base.polities.list.len() {
        let taxed = world.economy.treasuries[index] - base.economy.treasuries[index];
        assert!(taxed > 0, "G{index} nimmt keine Lohnsteuer ein");
        assert_eq!(world.economy.state_income_running[index], taxed);
    }

    // Zölle: Was ein Kontor über seine Betriebsmittel hinaus hat, führt es zur Hälfte ab.
    let mut world = base.clone();
    world.economy.markets[home.index()].cash += to_money(1_000_000.0);
    settle_public_accounts(
        &world.galaxy,
        &mut world.economy,
        &data,
        &mut EquityFlows::default(),
    );
    let tariff = world.economy.state_income_running[0];
    assert!(
        tariff > to_money(400_000.0) && tariff <= to_money(500_000.0),
        "{tariff}"
    );
    assert!(
        world.economy.state_income_running[1..]
            .iter()
            .all(|income| *income == 0),
        "die anderen haben nichts eingenommen"
    );

    // Geld der Notenbank füllt die Kasse, ist aber keine Einnahme.
    let mut world = base.clone();
    for price in &mut world.economy.markets[home.index()].price {
        *price *= 0.5;
    }
    steer_money_supply(&world.galaxy, &mut world.economy, &data, &capitals);
    assert!(
        world.economy.minted > base.economy.minted,
        "es wurde kein Geld geschöpft"
    );
    assert!(world.economy.treasuries[0] > base.economy.treasuries[0]);
    assert_eq!(world.economy.state_income_running[0], 0);

    // Anleihen ebenso: geliehenes Geld, keine Einnahme.
    let mut world = base.clone();
    let placed = sim::finance::issue_bonds(
        &mut world.finance,
        &mut world.economy,
        &world.galaxy,
        &world.diplomacy,
        &data,
        me,
        to_money(5_000.0),
        0,
    )
    .expect("am ersten Tag zeichnet jemand");
    assert!(placed > 0);
    assert_eq!(
        world.economy.treasuries[0],
        base.economy.treasuries[0] + placed
    );
    assert_eq!(world.economy.state_income_running[0], 0);

    // Der Monatsabschluss übernimmt die laufende Rechnung und beginnt eine neue.
    let mut world = base.clone();
    world.economy.book_state_income(me, 700);
    world.economy.book_state_income(me, 300);
    world.economy.book_state_income(me, -50);
    assert_eq!(world.economy.state_income_running[0], 1000);
    assert_eq!(world.economy.state_income_of(me), 0);
    world.economy.close_income_month();
    assert_eq!(world.economy.state_income_of(me), 1000);
    assert_eq!(world.economy.state_income_running[0], 0);
    world.economy.close_income_month();
    assert_eq!(world.economy.state_income_of(me), 0);
    assert_eq!(world.economy.find_anomaly(), None);
}

#[test]
fn hoarding_does_not_buy_research() {
    // Derselbe Staat, einmal wie er ist, einmal mit einer Rücklage, die er Monat für Monat
    // höher setzt. Die wachsende Kasse ist keine Einnahme: Geforscht wird damit nicht
    // mehr, sondern weniger, weil das Geld gebunden ist.
    let me = PolityId(0);
    let years = 8;
    let run = |hoard: bool| {
        let mut sim = with_player(3);
        let mut income = 0.0;
        for month in 0..years * 12 {
            if hoard {
                let treasury = to_credits(sim.world().resource::<Economy>().treasury(me));
                sim.issue(
                    me,
                    SimCommand::SetTreasuryTarget {
                        amount: treasury * 0.9 + 40.0 * month as f64,
                    },
                );
            }
            sim.run_days(DAYS_PER_MONTH);
            income += to_credits(sim.world().resource::<Economy>().state_income_of(me));
        }
        assert_eq!(check_invariants(sim.world()), Ok(()));
        let entry = sim.research().get(me).unwrap().clone();
        let treasury = to_credits(sim.world().resource::<Economy>().treasury(me));
        (entry, income, treasury)
    };
    let (plain, plain_income, plain_treasury) = run(false);
    let (hoarding, hoarding_income, hoarding_treasury) = run(true);
    assert!(
        hoarding_treasury > plain_treasury * 2.0,
        "die Rücklage wurde nicht aufgebaut: {plain_treasury} gegen {hoarding_treasury}"
    );
    // Die Einnahmen sind dieselbe Größenordnung: Horten macht keine.
    assert!(
        (0.7..1.3).contains(&(hoarding_income / plain_income)),
        "{plain_income} gegen {hoarding_income}"
    );
    assert!(plain.points_total > 0.0);
    assert!(
        hoarding.points_total < plain.points_total,
        "mit wachsender Rücklage {} Punkte, ohne {}",
        hoarding.points_total,
        plain.points_total
    );
    assert!(hoarding.spent_total < plain.spent_total);
    assert!(hoarding.completed <= plain.completed);
}

#[test]
fn larger_economies_research_faster_in_a_running_game() {
    // Gleicher Etat für alle, damit nur die Wirtschaft den Unterschied macht. Gemessen
    // wird sie an der Leistung der Betriebe und an der Bevölkerung, nicht an der Größe,
    // aus der die Forschung selbst rechnet.
    let data = data();
    let share = data.balance.research.budget_share;
    let mut clear_pairs = 0;
    for seed in [1, 2, 3] {
        let mut sim = observer(seed);
        set_all_shares(&mut sim, share);
        let count = sim.polities().list.len();
        let mut output = vec![0.0f64; count];
        let mut people = vec![0.0f64; count];
        let mut first_done = vec![None; count];
        for month in 1..=30 * 12 {
            sim.run_days(DAYS_PER_MONTH);
            for index in 0..count {
                let id = PolityId::from_index(index);
                output[index] += output_of(&sim, &data, id);
                people[index] += sim.galaxy().population_of(id);
                if first_done[index].is_none() && sim.research().polities[index].completed > 0 {
                    first_done[index] = Some(month as f64 / 12.0);
                }
            }
        }
        let research = sim.research();
        let points = |index: usize| research.polities[index].points_total;
        // Wirtschaftsleistung mal dem Gewicht der Köpfe, wie es die Forschung kennt.
        let weight = |index: usize| {
            output[index] * people[index].powf(data.balance.research.population_exponent)
        };
        // Wissen einer Gemeinschaft: die Zahl ihrer Technologien und was sie nach Liste
        // gekostet haben.
        let known = |index: usize| research.known_count(PolityId::from_index(index));
        let worth = |index: usize| -> f64 {
            data.techs
                .iter()
                .zip(&research.polities[index].known)
                .filter(|(_, known)| **known)
                .map(|(tech, _)| tech.cost)
                .sum()
        };
        for a in 0..count {
            for b in 0..count {
                if weight(a) < weight(b) * 1.3 {
                    continue;
                }
                clear_pairs += 1;
                assert!(
                    points(a) > points(b) * 1.1,
                    "Seed {seed}: G{a} leistet deutlich mehr als G{b} ({:.0} gegen {:.0}), \
                     forscht aber nicht deutlich mehr ({:.0} gegen {:.0} Punkte)",
                    weight(a),
                    weight(b),
                    points(a),
                    points(b)
                );
                // Wer deutlich mehr leistet, weiß nicht weniger. Die Zahl allein sagt das
                // nicht: Eine Technologie der dritten Stufe kostet so viel wie zwei der
                // zweiten, und welche eine Gemeinschaft als Nächstes angeht, hängt an ihren
                // Vorlieben. Im Rückstand ist erst, wer weniger Technologien kennt und
                // dafür auch weniger aufgewendet hat.
                assert!(
                    known(a) >= known(b) || worth(a) >= worth(b),
                    "Seed {seed}: G{a} leistet deutlich mehr als G{b}, weiß aber weniger \
                     ({} Technologien im Wert von {:.0} gegen {} im Wert von {:.0})",
                    known(a),
                    worth(a),
                    known(b),
                    worth(b)
                );
            }
        }
        // Das Tempo liegt im gewollten Rahmen: Die erste Technologie braucht einige Jahre,
        // und nach dreißig Jahren ist niemand über die dritte Stufe hinaus.
        for index in 0..count {
            let entry = &research.polities[index];
            let first = first_done[index].expect("in dreißig Jahren wird etwas erforscht");
            assert!(
                (2.5..=7.0).contains(&first),
                "Seed {seed}, G{index}: erste Technologie nach {first:.1} Jahren"
            );
            assert!(
                (3..=10).contains(&entry.completed),
                "Seed {seed}, G{index}: {} Technologien in dreißig Jahren",
                entry.completed
            );
            for branch in TechBranch::ALL {
                assert!(research.tier(&data, PolityId::from_index(index), branch) <= 3);
            }
        }
    }
    assert!(
        clear_pairs >= 6,
        "nur {clear_pairs} Paare mit deutlichem Abstand: die Prüfung sagt zu wenig"
    );
}

/// Stufen je Zweig aller lebenden Gemeinschaften.
fn all_tiers(sim: &Headless, data: &GameData) -> Vec<(PolityId, [u8; 5])> {
    sim.polities()
        .alive()
        .map(|polity| {
            (
                polity.id,
                TechBranch::ALL.map(|branch| sim.research().tier(data, polity.id, branch)),
            )
        })
        .collect()
}

#[test]
fn technology_leads_arise_and_follow_what_a_community_values() {
    // Gemeinschaften forschen verschieden viel und verschieden gerichtet: Nach einigen
    // Jahrzehnten liegen sie in einzelnen Zweigen um Stufen auseinander, nicht im
    // Gleichschritt.
    let data = data();
    let mut snapshots = 0;
    let mut wide = 0;
    // Paare aus der Gemeinschaft, die einen Zweig am höchsten schätzt, und einer, die ihn
    // höchstens halb so hoch schätzt: Wie oft liegt die erste dort vorn, wie oft gleichauf?
    let (mut ahead, mut level) = (0, 0);
    for seed in [1, 2, 3] {
        let mut sim = observer(seed);
        let mut widest_gap = 0;
        for year in [50, 60, 70] {
            sim.run_days(year * DAYS_PER_YEAR - sim.clock().day());
            let tiers = all_tiers(&sim, &data);
            for branch in 0..5 {
                let best = tiers.iter().map(|(_, tiers)| tiers[branch]).max().unwrap();
                let worst = tiers.iter().map(|(_, tiers)| tiers[branch]).min().unwrap();
                snapshots += 1;
                if best - worst >= 2 {
                    wide += 1;
                }
                widest_gap = widest_gap.max(best - worst);
            }
        }
        assert!(
            widest_gap >= 2,
            "Seed {seed}: nach siebzig Jahren liegt in keinem Zweig mehr als eine Stufe \
             zwischen der besten und der schwächsten Gemeinschaft"
        );
        // Der Vorsprung folgt den Vorlieben: Wer einen Zweig am höchsten schätzt, liegt
        // dort nie hinter einer Gemeinschaft, die ihn höchstens halb so hoch schätzt, es
        // sei denn, diese forscht insgesamt deutlich mehr.
        let tiers = all_tiers(&sim, &data);
        let research = sim.research();
        for branch in TechBranch::ALL {
            let focus = |id: PolityId| research.get(id).unwrap().focus.of(branch);
            let keenest = tiers
                .iter()
                .map(|(id, _)| *id)
                .max_by(|a, b| focus(*a).total_cmp(&focus(*b)).then(b.cmp(a)))
                .unwrap();
            let tier_of = |id: PolityId| {
                tiers
                    .iter()
                    .find(|(other, _)| *other == id)
                    .map(|(_, tiers)| tiers[branch.index()])
                    .unwrap()
            };
            let points = |id: PolityId| research.get(id).unwrap().points_total;
            for (other, _) in &tiers {
                if focus(*other) * 2.0 > focus(keenest) || points(*other) > points(keenest) * 1.5 {
                    continue;
                }
                assert!(
                    tier_of(keenest) >= tier_of(*other),
                    "Seed {seed}, {}: G{} schätzt den Zweig mehr als doppelt so hoch wie G{}, \
                     liegt aber zurück ({} gegen {})",
                    branch.label(),
                    keenest.index(),
                    other.index(),
                    tier_of(keenest),
                    tier_of(*other)
                );
                if tier_of(keenest) > tier_of(*other) {
                    ahead += 1;
                } else {
                    level += 1;
                }
            }
        }
    }
    assert!(
        wide * 5 >= snapshots,
        "nur in {wide} von {snapshots} Zweigständen liegen zwei Stufen oder mehr zwischen \
         der besten und der schwächsten Gemeinschaft"
    );
    assert!(
        ahead >= 2 * level && ahead >= 6,
        "wer einen Zweig doppelt so hoch schätzt, liegt nur in {ahead} Fällen vorn und in \
         {level} gleichauf"
    );
}

#[test]
#[ignore = "vier Partien über 150 Jahre; ausführen mit `cargo test -p sim --test tech -- --ignored`"]
fn the_long_run_pace_holds_at_the_usual_and_at_the_highest_budget() {
    // Das Tempo, auf das techs.ron ausgelegt ist: nach hundert Jahren die vierte bis fünfte
    // Stufe in den bevorzugten Zweigen, die sechste erst danach und nur vereinzelt, und
    // bis zum Jahr 150 vollendet niemand den Baum, auch nicht mit dem höchsten Etat.
    let data = data();
    let total = data.techs.len();
    let max = data.balance.research.max_budget_share;
    for seed in [0, 4] {
        let known = |sim: &Headless| -> Vec<usize> {
            sim.polities()
                .alive()
                .map(|polity| sim.research().known_count(polity.id))
                .collect()
        };
        // Mit den Etats der Vorlagen.
        let mut usual = observer(seed);
        usual.run_days(100 * DAYS_PER_YEAR);
        for (id, tiers) in all_tiers(&usual, &data) {
            let name = &usual.polities().get(id).name;
            assert!(
                tiers.iter().any(|tier| *tier >= 4),
                "Seed {seed}, {name}: nach hundert Jahren in keinem Zweig auf Stufe 4: {tiers:?}"
            );
            assert!(
                tiers.iter().all(|tier| *tier <= 5),
                "Seed {seed}, {name}: Stufe 6 schon nach hundert Jahren: {tiers:?}"
            );
        }
        for count in known(&usual) {
            assert!(
                (14..=24).contains(&count),
                "Seed {seed}: {count} nach 100 Jahren"
            );
        }
        usual.run_days(50 * DAYS_PER_YEAR);
        let at_end = known(&usual);
        for count in &at_end {
            assert!(
                (19..total).contains(count),
                "Seed {seed}: {count} von {total} Technologien nach 150 Jahren"
            );
        }
        // Die sechste Stufe ist ein Jahrhundertwerk: Wer sie erreicht, hat ein, zwei
        // Technologien davon, nicht den Zweigsatz.
        for (id, tiers) in all_tiers(&usual, &data) {
            let sixth = tiers.iter().filter(|tier| **tier >= 6).count();
            assert!(
                sixth <= 2,
                "Seed {seed}, {}: {sixth} Technologien der sechsten Stufe",
                usual.polities().get(id).name
            );
        }
        assert_eq!(check_invariants(usual.world()), Ok(()));

        // Alle mit dem höchsten Etat, vom ersten Tag an.
        let mut keen = observer(seed);
        set_all_shares(&mut keen, max);
        keen.run_days(150 * DAYS_PER_YEAR);
        let keen_end = known(&keen);
        for count in &keen_end {
            assert!(
                *count < total,
                "Seed {seed}: mit dem höchsten Etat ist der Baum nach 150 Jahren vollendet"
            );
        }
        // Mehr Etat bringt mehr Forschung, aber kein Vielfaches.
        let mean = |counts: &[usize]| counts.iter().sum::<usize>() as f64 / counts.len() as f64;
        assert!(
            mean(&keen_end) >= mean(&at_end) && mean(&keen_end) <= mean(&at_end) + 4.0,
            "Seed {seed}: {:.1} Technologien mit dem höchsten Etat, {:.1} mit dem üblichen",
            mean(&keen_end),
            mean(&at_end)
        );
        assert_eq!(check_invariants(keen.world()), Ok(()));
    }
}

#[test]
fn every_template_begins_with_a_project_of_the_first_tier() {
    // Wählte die Automatik für eine Vorlage zuerst eine Technologie der zweiten Stufe,
    // dauerte ihr erstes Ergebnis doppelt so lang wie das der anderen.
    let data = data();
    for template in &data.polities {
        let mut entry = sim::tech::PolityResearch::new(&data);
        for tech in &template.start_tech_ids {
            entry.known[*tech as usize] = true;
        }
        entry.focus = template.research_focus;
        let first = sim::tech::choose_target(&data, &entry, false).expect("etwas zu erforschen");
        assert_eq!(
            data.techs[first as usize].tier, 1,
            "{} beginnt mit {}",
            template.name, data.techs[first as usize].name
        );
    }
}

// --- 2. Forschung bewegt Geld, sie schafft und vernichtet keines ---

#[test]
fn research_pay_moves_money_from_the_treasury_to_households() {
    let data = data();
    let mut world = start(&data, 3);
    for index in 0..world.polities.list.len() {
        set_income(&mut world, index, 20_000.0);
    }
    let before = world.clone();
    research_step(&data, &mut world);
    for polity in &world.polities.list {
        let index = polity.id.index();
        let entry = &world.research.polities[index];
        let planets = world.galaxy.planets_of(polity.id).count() as Money;
        // Der Etat ist der Anteil an den Einnahmen; je Welt kann ein Tausendstel fehlen.
        let budget = to_money(entry.budget_share * 20_000.0);
        assert!(
            entry.spent_last_month <= budget && entry.spent_last_month >= budget - planets,
            "{} von {budget} gezahlt",
            entry.spent_last_month
        );
        assert!(entry.points_last_month > 0.0);
        // Die Kasse sinkt um genau das, was die Haushalte erhalten.
        assert_eq!(
            before.economy.treasuries[index] - world.economy.treasuries[index],
            entry.spent_last_month
        );
        assert_eq!(
            households_of(&world, polity.id) - households_of(&before, polity.id),
            entry.spent_last_month
        );
        assert_eq!(entry.spent_total, entry.spent_last_month);
    }
    assert_eq!(
        world.economy.money_in_accounts(),
        before.economy.money_in_accounts()
    );

    // Leere Kasse: keine Gehälter, keine Punkte, kein Fortschritt.
    let mut broke = before.clone();
    broke.economy.treasuries[0] = 0;
    research_step(&data, &mut broke);
    let entry = &broke.research.polities[0];
    assert_eq!(entry.spent_last_month, 0);
    assert_eq!(entry.points_last_month, 0.0);
    assert!(entry.progress.iter().all(|points| *points == 0.0));
    assert_eq!(broke.economy.treasuries[0], 0);
    assert_eq!(
        households_of(&broke, PolityId(0)),
        households_of(&before, PolityId(0))
    );
    assert!(
        broke.research.polities[1].points_last_month > 0.0,
        "die anderen forschen weiter"
    );

    // Die Rücklage ist tabu: Liegt die Kasse nur knapp darüber, wird nur dieser Rest
    // gezahlt und entsprechend weniger geforscht.
    let me = PolityId(0);
    let reserve = state_reserve(&before.economy, &data, me, before.galaxy.population_of(me));
    assert!(reserve > 0 && reserve < before.economy.treasuries[0]);
    let mut tight = before.clone();
    tight.economy.treasuries[0] = reserve + to_money(7.0);
    research_step(&data, &mut tight);
    let entry = &tight.research.polities[0];
    assert!(entry.spent_last_month > 0 && entry.spent_last_month <= to_money(7.0));
    assert_eq!(
        tight.economy.treasuries[0],
        reserve + to_money(7.0) - entry.spent_last_month
    );
    assert!(entry.points_last_month < world.research.polities[0].points_last_month / 10.0);
    let mut at_reserve = before.clone();
    at_reserve.economy.treasuries[0] = reserve;
    research_step(&data, &mut at_reserve);
    assert_eq!(at_reserve.research.polities[0].spent_last_month, 0);
    assert_eq!(at_reserve.economy.treasuries[0], reserve);

    // Ein befohlener Kriegsschatz bindet das Geld ebenso: Erst was darüber liegt, fließt.
    let mut hoarding = before.clone();
    hoarding.economy.treasury_targets[0] = before.economy.treasuries[0] + to_money(1.0);
    research_step(&data, &mut hoarding);
    assert_eq!(hoarding.research.polities[0].spent_last_month, 0);
    assert_eq!(hoarding.research.polities[0].points_last_month, 0.0);
    assert_eq!(hoarding.economy.treasuries[0], before.economy.treasuries[0]);
}

#[test]
fn the_money_sum_holds_while_everyone_researches() {
    let data = data();
    let mut sim = observer(9);
    for year in 1..=12 {
        sim.run_days(DAYS_PER_YEAR);
        assert_eq!(money_drift(sim.world()), Some(0), "Jahr {year}");
        assert_eq!(check_invariants(sim.world()), Ok(()), "Jahr {year}");
    }
    let research = sim.research();
    for polity in &sim.polities().list {
        let entry = research.get(polity.id).unwrap();
        assert!(entry.spent_total > 0, "{} gab nichts aus", polity.name);
        assert!(entry.points_total > 0.0);
        assert!(entry.completed >= 1, "{} erforschte nichts", polity.name);
    }
    assert!(research.is_consistent(&data, sim.polities().list.len()));
}

// --- 3. Wirkungen greifen ---

#[test]
fn a_yield_technology_raises_output_by_exactly_its_factor() {
    let data = data();
    let yield_tech = tech(&data, "wirtschaft_1");
    let mut plain = observer(8);
    let mut advanced = observer(8);
    plain.run_days(45);
    advanced.run_days(45);
    assert_eq!(plain.hash(), advanced.hash());
    // Eine Gemeinschaft, die die Technologie nicht schon von Haus aus hat.
    let owner = plain
        .polities()
        .list
        .iter()
        .map(|polity| polity.id)
        .find(|id| !plain.research().knows(*id, yield_tech))
        .expect("nicht jede Vorlage beginnt mit Fertigungsstraßen");
    let bystander = plain
        .polities()
        .list
        .iter()
        .map(|polity| polity.id)
        .find(|id| *id != owner)
        .unwrap();
    let capital = plain.polities().get(owner).capital.unwrap();
    assert!(
        advanced
            .world_mut()
            .resource_mut::<Research>()
            .grant(&data, owner, yield_tech)
    );
    let factor = advanced.research().effects(owner).yield_factor;
    assert!(factor > 1.01, "die Technologie hebt den Ertrag: {factor}");
    assert_eq!(plain.research().effects(owner).yield_factor, 1.0);

    plain.run_days(1);
    advanced.run_days(1);
    let before = plain.world().resource::<Economy>().planet(capital).clone();
    let after = advanced
        .world()
        .resource::<Economy>()
        .planet(capital)
        .clone();
    let mut compared = 0;
    for recipe in 0..data.recipes.len() {
        let (base, more) = (before.output[recipe], after.output[recipe]);
        if base <= 0.0 {
            continue;
        }
        // Mehr Ertrag senkt den Ausstoß nie.
        assert!(
            more >= base * (1.0 - 1e-12),
            "{}",
            data.recipes[recipe].name
        );
        // Läuft das Werk in beiden Welten gleich ausgelastet, unterscheidet sie nur die
        // Technik: Der Ausstoß ist um genau ihren Faktor höher.
        if before.utilization[recipe] == after.utilization[recipe] {
            assert!(
                (more / base - factor).abs() < 1e-9,
                "{}: {base} → {more}",
                data.recipes[recipe].name
            );
            compared += 1;
        }
    }
    assert!(compared >= 3, "nur {compared} Werke vergleichbar");
    assert_eq!(
        advanced.world().resource::<Economy>().yield_factor(owner),
        factor
    );
    // Wer die Technologie nicht hat, produziert wie zuvor.
    let other_capital = plain.polities().get(bystander).capital.unwrap();
    assert_eq!(
        plain
            .world()
            .resource::<Economy>()
            .planet(other_capital)
            .output,
        advanced
            .world()
            .resource::<Economy>()
            .planet(other_capital)
            .output
    );
    assert_eq!(check_invariants(advanced.world()), Ok(()));
}

#[test]
fn investors_reckon_with_the_yield_of_their_community() {
    let data = data();
    let base = start(&data, 3);
    let owner = PolityId(1);
    let capital = base.polities.get(owner).capital.unwrap();
    let capitals: Vec<Option<PlanetId>> = base.polities.list.iter().map(|p| p.capital).collect();
    let returns = |factor: f64| {
        let mut world = base.clone();
        world.economy.tech_yield[owner.index()] = factor;
        let mut funds = vec![0; capitals.len()];
        let mut flows = EquityFlows::default();
        invest_and_depreciate(
            &world.galaxy,
            &mut world.economy,
            &data,
            &capitals,
            &mut funds,
            0,
            &mut flows,
        );
        world.economy.planet(capital).roc.clone()
    };
    let factor = 1.2;
    let (plain, advanced) = (returns(1.0), returns(factor));
    let planet = base.galaxy.planet(capital);
    let local = base.economy.planet(capital);
    let market = base.economy.market(planet.system);
    let mut checked = 0;
    for (index, recipe) in data.recipes.iter().enumerate() {
        if local.capital[index] <= 0.0 {
            continue;
        }
        // Dieselbe Rechnung wie in der Simulation, mit dem Ertrag der Technik.
        let mut unit = unit_economics_at(&data, planet, local.wage, market, recipe, factor);
        let utilization = local.utilization_trend[index].clamp(0.0, 1.0);
        unit.revenue *= utilization;
        unit.inputs *= utilization;
        unit.energy *= utilization;
        unit.lift *= utilization;
        let expected = expected_return(&data, market, &unit);
        assert!(
            (advanced[index] - expected).abs() < 1e-9,
            "{}: {} statt {expected}",
            recipe.name,
            advanced[index]
        );
        // Mehr Ertrag bei gleichem Einsatz: Die Rendite steigt.
        assert!(advanced[index] > plain[index], "{}", recipe.name);
        checked += 1;
    }
    assert!(checked >= 3, "nur {checked} Rezepte geprüft");
    // Auf den Welten der anderen ändert sich nichts.
    let other = base.polities.get(PolityId(2)).capital.unwrap();
    let elsewhere = |factor: f64| {
        let mut world = base.clone();
        world.economy.tech_yield[owner.index()] = factor;
        let mut funds = vec![0; capitals.len()];
        invest_and_depreciate(
            &world.galaxy,
            &mut world.economy,
            &data,
            &capitals,
            &mut funds,
            0,
            &mut EquityFlows::default(),
        );
        world.economy.planet(other).roc.clone()
    };
    assert_eq!(elsewhere(1.0), elsewhere(factor));
}

/// Gründet für `owner` eine Kolonie auf `colony`, wie es der Befehl täte, und gibt die
/// Rezepte zurück, in die das mitgebrachte Kapital ging.
fn seeded_recipes(
    data: &GameData,
    base: &SimState,
    owner: PolityId,
    colony: PlanetId,
    factor: f64,
) -> Vec<usize> {
    let mut world = base.clone();
    world.economy.tech_yield[owner.index()] = factor;
    let capital = world.polities.get(owner).capital.unwrap();
    let settlers = data.balance.colonization.settlers;
    let before = world.galaxy.planet(capital).population;
    world.galaxy.planet_mut(capital).population -= settlers;
    let target = world.galaxy.planet_mut(colony);
    target.owner = Some(owner);
    target.population = settlers;
    seed_colony(
        data,
        &world.galaxy,
        &mut world.economy,
        colony,
        capital,
        settlers,
        before,
        0,
    );
    let seeded = &world.economy.planet(colony).capital;
    (0..data.recipes.len())
        .filter(|index| seeded[*index] > 0.0)
        .collect()
}

#[test]
fn a_new_colony_is_equipped_for_the_yield_of_its_founders() {
    let data = data();
    let mut differing = 0;
    let mut checked = 0;
    for seed in [1, 4, 7] {
        let base = start(&data, seed);
        // Vom ersten Takt an rechnet die Wirtschaft mit dem Startwissen, also auch eine
        // Gründung, die noch vor dem ersten Tag befohlen wird.
        for polity in &base.polities.list {
            assert_eq!(
                base.economy.yield_factor(polity.id),
                base.research.effects(polity.id).yield_factor,
                "Seed {seed}: {}",
                polity.name
            );
        }
        for polity in &base.polities.list {
            let owner = polity.id;
            let options = sim::polity::claimable_planets(
                &base.galaxy,
                &base.polities,
                &data.balance.colonization,
                to_money(1e9),
                owner,
                0,
            );
            for colony in options {
                let planet = base.galaxy.planet(colony);
                let market = base.economy.market(planet.system);
                // Die Rangfolge, wie sie mit dem Ertragsfaktor ausfallen muss.
                let ranking = |factor: f64| -> Vec<usize> {
                    let mut ranked: Vec<(usize, f64)> = data
                        .recipes
                        .iter()
                        .enumerate()
                        .filter(|(_, recipe)| recipe.input_ids.is_empty())
                        .filter_map(|(index, recipe)| {
                            let unit = unit_economics_at(
                                &data,
                                planet,
                                data.balance.economy.base_wage,
                                market,
                                recipe,
                                factor,
                            );
                            (unit.local_yield > 0.0)
                                .then(|| (index, expected_return(&data, market, &unit)))
                        })
                        .collect();
                    ranked.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
                    let mut top: Vec<usize> =
                        ranked.iter().take(2).map(|(index, _)| *index).collect();
                    top.sort_unstable();
                    top
                };
                for factor in [1.0, 1.04, 1.25, 3.0] {
                    assert_eq!(
                        seeded_recipes(&data, &base, owner, colony, factor),
                        ranking(factor),
                        "Seed {seed}, {} auf {}: Faktor {factor}",
                        polity.name,
                        planet.name
                    );
                }
                checked += 1;
                if ranking(1.0) != ranking(3.0) {
                    differing += 1;
                }
            }
        }
    }
    assert!(checked >= 20, "nur {checked} mögliche Kolonien geprüft");
    assert!(
        differing >= 1,
        "auf keiner Welt ändert der Ertragsfaktor die Wahl: die Prüfung sagt nichts"
    );
}

#[test]
fn freighters_of_an_advanced_community_travel_faster() {
    let data = data();
    let speed_tech = tech(&data, "wirtschaft_2");
    let mut plain = observer(8);
    let mut advanced = observer(8);
    plain.run_days(725);
    advanced.run_days(725);
    assert_eq!(plain.hash(), advanced.hash());
    let start = plain.world().resource::<Logistics>().clone();
    let underway = |polity: PolityId| {
        start
            .freighters
            .iter()
            .filter(|freighter| freighter.alive && !freighter.route.is_empty())
            .filter(|freighter| start.houses[freighter.house.index()].polity == polity)
            .count()
    };
    // Die Gemeinschaft ohne diese Technologie, von der die meisten Frachter unterwegs sind.
    let owner = plain
        .polities()
        .list
        .iter()
        .map(|polity| polity.id)
        .filter(|id| !plain.research().knows(*id, speed_tech))
        .max_by_key(|id| (underway(*id), std::cmp::Reverse(*id)))
        .expect("nach zwei Jahren hat niemand die zweite Stufe");
    advanced
        .world_mut()
        .resource_mut::<Research>()
        .grant(&data, owner, speed_tech);
    let factor = advanced.research().effects(owner).freighter_speed as f32;
    assert!(
        factor > 1.05,
        "die Technologie beschleunigt Frachter: {factor}"
    );

    plain.run_days(1);
    advanced.run_days(1);
    let slow = plain.world().resource::<Logistics>();
    let fast = advanced.world().resource::<Logistics>();
    let mut compared = 0;
    for (index, before) in start.freighters.iter().enumerate() {
        if !before.alive || before.route.is_empty() {
            continue;
        }
        let (a, b) = (&slow.freighters[index], &fast.freighters[index]);
        // Nur Frachter, die den ganzen Tag auf derselben Route unterwegs waren.
        let same_leg = |now: &sim::logistics::Freighter| {
            now.alive && now.system == before.system && now.route == before.route
        };
        if !same_leg(a) || !same_leg(b) {
            continue;
        }
        let (step_a, step_b) = (a.progress - before.progress, b.progress - before.progress);
        assert!(step_a > 0.0);
        if start.houses[before.house.index()].polity == owner {
            assert!(
                (step_b / step_a - factor).abs() < 1e-3,
                "Frachter {index}: {step_a} gegen {step_b}"
            );
            compared += 1;
        } else {
            assert_eq!(a.progress, b.progress, "fremder Frachter {index}");
        }
    }
    assert!(
        compared >= 1,
        "kein Frachter der Gemeinschaft war unterwegs"
    );
    let base = data.balance.logistics.speed;
    assert_eq!(slow.freighter_speed(owner, base), base);
    assert_eq!(fast.freighter_speed(owner, base), base * factor);
}

// --- 4. Regeln des Baums und der Etat ---

#[test]
fn research_commands_enforce_the_tree() {
    let data = data();
    let mut sim = with_player(4);
    let me = PolityId(0);
    // Ein Zweig, in dem die Gemeinschaft noch nichts kann.
    let branch = TechBranch::ALL
        .into_iter()
        .find(|branch| sim.research().tier(&data, me, *branch) == 0)
        .unwrap();
    let in_branch = |tier: u32| {
        data.techs
            .iter()
            .position(|def| def.branch == branch && def.tier == tier)
            .unwrap() as u16
    };
    let (first, second) = (in_branch(1), in_branch(2));

    // Die zweite Stufe ohne die erste: abgelehnt, mit Nennung dessen, was fehlt.
    let outcome = command(
        &mut sim,
        me,
        SimCommand::SetResearchTarget { tech: Some(second) },
    );
    rejected_with(outcome.clone(), "Voraussetzung fehlt");
    rejected_with(outcome, &data.techs[first as usize].name);
    rejected_with(
        command(
            &mut sim,
            me,
            SimCommand::SetResearchTarget { tech: Some(9_999) },
        ),
        "Technologie unbekannt",
    );
    rejected_with(
        command(
            &mut sim,
            PolityId(99),
            SimCommand::SetResearchTarget { tech: Some(first) },
        ),
        "besteht nicht mehr",
    );
    // Was schon erforscht ist, kann kein Ziel sein.
    let known = (0..data.techs.len() as u16)
        .find(|tech| sim.research().knows(me, *tech))
        .expect("jede Vorlage bringt Startwissen mit");
    rejected_with(
        command(
            &mut sim,
            me,
            SimCommand::SetResearchTarget { tech: Some(known) },
        ),
        "bereits erforscht",
    );
    assert!(
        sim.research().get(me).unwrap().auto,
        "abgelehnte Befehle ändern nichts"
    );

    // Die erste Stufe ist erlaubt.
    assert_eq!(
        command(
            &mut sim,
            me,
            SimCommand::SetResearchTarget { tech: Some(first) }
        ),
        CommandOutcome::Applied
    );
    let entry = sim.research().get(me).unwrap();
    assert_eq!(entry.target, Some(first));
    assert!(!entry.auto);

    // Ist sie abgeschlossen, steht die zweite offen und ihre Wirkung gilt.
    let cost = entry.cost(&data, first);
    sim.world_mut().resource_mut::<Research>().polities[0].progress[first as usize] = cost;
    sim.run_days(DAYS_PER_MONTH + 1);
    assert!(sim.research().knows(me, first));
    assert_eq!(sim.research().tier(&data, me, branch), 1);
    assert_eq!(
        sim.research().effects(me),
        TechEffects::of(&data, sim.research().known(me))
    );
    assert_eq!(
        command(
            &mut sim,
            me,
            SimCommand::SetResearchTarget { tech: Some(second) }
        ),
        CommandOutcome::Applied
    );
    assert_eq!(sim.research().get(me).unwrap().target, Some(second));
}

#[test]
fn a_new_design_unlocks_once_its_technology_is_researched() {
    let data = data();
    let mut sim = with_player(4);
    let me = PolityId(0);
    let (basic, improved) = (
        data.design_index("speer").unwrap(),
        data.design_index("speer_2").unwrap(),
    );
    let class = data.designs[basic as usize].stats.class;
    let (first, second) = (tech(&data, "waffen_1"), tech(&data, "waffen_2"));
    // Den Weg dorthin geht die Forschung selbst: Ziel setzen, forschen, abschließen.
    for step in [first, second] {
        if sim.research().knows(me, step) {
            continue;
        }
        assert_eq!(
            data.best_design(class, sim.research().known(me)),
            Some(basic),
            "ohne Spulenbeschleuniger bleibt es beim alten Entwurf"
        );
        assert_eq!(
            command(
                &mut sim,
                me,
                SimCommand::SetResearchTarget { tech: Some(step) }
            ),
            CommandOutcome::Applied
        );
        let cost = sim.research().cost(&data, me, step);
        sim.world_mut().resource_mut::<Research>().polities[0].progress[step as usize] = cost;
        sim.run_days(DAYS_PER_MONTH + 1);
        assert!(sim.research().knows(me, step));
    }
    assert_eq!(
        data.best_design(class, sim.research().known(me)),
        Some(improved)
    );
    // Andere Gemeinschaften bauen weiter, was sie selbst beherrschen.
    let other = PolityId(1);
    if !sim.research().knows(other, second) {
        assert_eq!(
            data.best_design(class, sim.research().known(other)),
            Some(basic)
        );
    }
}

#[test]
fn progress_is_kept_when_the_target_changes() {
    let data = data();
    let mut sim = with_player(4);
    let me = PolityId(0);
    let available = sim.research().available(&data, me);
    let (a, b) = (available[0], available[1]);
    let progress =
        |sim: &Headless, tech: u16| sim.research().get(me).unwrap().progress[tech as usize];
    let target = |tech: u16| SimCommand::SetResearchTarget { tech: Some(tech) };

    assert_eq!(command(&mut sim, me, target(a)), CommandOutcome::Applied);
    sim.run_days(DAYS_PER_YEAR);
    let on_a = progress(&sim, a);
    assert!(on_a > 0.0 && !sim.research().knows(me, a));
    assert_eq!(progress(&sim, b), 0.0);

    assert_eq!(command(&mut sim, me, target(b)), CommandOutcome::Applied);
    sim.run_days(DAYS_PER_YEAR / 2);
    let on_b = progress(&sim, b);
    assert_eq!(progress(&sim, a), on_a, "Liegengelassenes verfällt nicht");
    assert!(on_b > 0.0 && !sim.research().knows(me, b));

    assert_eq!(command(&mut sim, me, target(a)), CommandOutcome::Applied);
    sim.run_days(DAYS_PER_YEAR / 2);
    assert!(
        progress(&sim, a) > on_a,
        "die Arbeit geht dort weiter, wo sie stand"
    );
    assert_eq!(progress(&sim, b), on_b);
}

#[test]
fn the_automatic_rule_continues_and_a_manual_target_ends_in_a_pause() {
    let data = data();
    let mut sim = with_player(4);
    let (me, other) = (PolityId(0), PolityId(1));
    // Vorlauf, bis die Staaten Einnahmen haben.
    sim.run_days(DAYS_PER_YEAR);
    let mine = sim.research().available(&data, me)[0];
    assert_eq!(
        command(
            &mut sim,
            me,
            SimCommand::SetResearchTarget { tech: Some(mine) }
        ),
        CommandOutcome::Applied
    );
    let theirs = sim
        .research()
        .get(other)
        .unwrap()
        .target
        .expect("die Automatik hat ein Ziel gewählt");
    // Beide stehen einen Punkt vor dem Abschluss.
    {
        let mut research = sim.world_mut().resource_mut::<Research>();
        for (index, tech) in [(0, mine), (1, theirs)] {
            let cost = research.polities[index].cost(&data, tech);
            research.polities[index].progress[tech as usize] = cost - 1.0;
        }
    }
    let completed_before = sim.research().get(other).unwrap().completed;
    sim.run_days(DAYS_PER_MONTH + 1);

    // Die Automatik schließt ab, wählt sofort das nächste Ziel und forscht weiter.
    let entry = sim.research().get(other).unwrap();
    assert!(sim.research().knows(other, theirs));
    assert_eq!(entry.completed, completed_before + 1);
    assert!(entry.auto);
    let next = entry.target.expect("nächstes Ziel");
    assert_ne!(next, theirs);
    assert!(entry.prerequisites_met(&data, next) && !entry.known[next as usize]);
    assert!(
        entry.progress[next as usize] > 0.0,
        "der Überschuss fließt ins nächste Ziel"
    );

    // Das befohlene Ziel ist erforscht; ohne neuen Befehl ruht die Forschung, das Geld
    // bleibt in der Kasse, und der Überschuss wartet.
    let entry = sim.research().get(me).unwrap();
    assert!(sim.research().knows(me, mine));
    assert_eq!(entry.target, None);
    assert!(!entry.auto);
    assert!(entry.carry > 0.0);
    assert_eq!(entry.effects, TechEffects::of(&data, &entry.known));
    let name = &data.techs[mine as usize].name;
    let chronicle = sim.world().resource::<Chronicle>();
    assert!(
        chronicle.entries.iter().any(|entry| {
            entry.kind == ChronicleKind::Tech
                && entry.polity == Some(me)
                && entry.text.contains(name.as_str())
        }),
        "der Abschluss steht in der Chronik"
    );
    let (spent, carry) = (entry.spent_total, entry.carry);
    sim.run_days(3 * DAYS_PER_MONTH);
    let entry = sim.research().get(me).unwrap();
    assert_eq!(entry.spent_total, spent);
    assert_eq!(entry.spent_last_month, 0);
    assert_eq!(entry.carry, carry);

    // Der Spieler gibt die Wahl zurück: Die Automatik wählt sofort und forscht weiter.
    assert_eq!(
        command(&mut sim, me, SimCommand::SetResearchTarget { tech: None }),
        CommandOutcome::Applied
    );
    let entry = sim.research().get(me).unwrap();
    assert!(entry.auto);
    assert!(entry.target.is_some(), "die Automatik wählt sofort");
    sim.run_days(DAYS_PER_MONTH + 1);
    let entry = sim.research().get(me).unwrap();
    assert!(entry.spent_total > spent);
    assert_eq!(entry.carry, 0.0);
    let current = entry.target.expect("Ziel der Automatik");
    assert!(
        entry.progress[current as usize] >= carry,
        "der wartende Überschuss fließt ins neue Ziel"
    );
}

#[test]
fn a_scripted_target_for_an_ai_community_holds_until_done() {
    let data = data();
    let mut sim = observer(4);
    let other = PolityId(2);
    sim.run_days(DAYS_PER_YEAR);
    // Von außen (Skript, früherer Spieler) gesetzt: nicht das, was die Automatik wählte.
    let automatic = sim.research().get(other).unwrap().target;
    let pick = *sim
        .research()
        .available(&data, other)
        .iter()
        .rev()
        .find(|tech| Some(**tech) != automatic)
        .unwrap();
    assert_eq!(
        command(
            &mut sim,
            other,
            SimCommand::SetResearchTarget { tech: Some(pick) }
        ),
        CommandOutcome::Applied
    );
    sim.run_days(DAYS_PER_YEAR);
    let entry = sim.research().get(other).unwrap();
    assert_eq!(
        entry.target,
        Some(pick),
        "das gesetzte Ziel übersteht die Monate"
    );
    assert!(!entry.auto && entry.progress[pick as usize] > 0.0);

    // Nach dem Abschluss übernimmt wieder die Automatik: Eine KI-Gemeinschaft ruht nie.
    let cost = entry.cost(&data, pick);
    sim.world_mut().resource_mut::<Research>().polities[other.index()].progress[pick as usize] =
        cost;
    sim.run_days(2 * DAYS_PER_MONTH + 1);
    let entry = sim.research().get(other).unwrap();
    assert!(sim.research().knows(other, pick));
    assert!(entry.auto);
    assert!(entry.target.is_some());
    assert!(entry.spent_last_month > 0);
}

#[test]
fn the_research_budget_is_validated_and_sets_the_spending() {
    let data = data();
    let mut sim = with_player(4);
    let me = PolityId(0);
    sim.run_days(DAYS_PER_YEAR);
    let max = data.balance.research.max_budget_share;
    let usual = sim.research().get(me).unwrap().budget_share;
    for share in [-0.01, max + 0.01, 1.5, f64::NAN, f64::INFINITY] {
        rejected_with(
            command(&mut sim, me, SimCommand::SetResearchBudget { share }),
            "Forschungsanteil",
        );
        assert_eq!(sim.research().get(me).unwrap().budget_share, usual);
    }

    // Kein Etat: keine Gehälter, keine Punkte.
    assert_eq!(
        command(&mut sim, me, SimCommand::SetResearchBudget { share: 0.0 }),
        CommandOutcome::Applied
    );
    let spent = sim.research().get(me).unwrap().spent_total;
    sim.run_days(6 * DAYS_PER_MONTH);
    let entry = sim.research().get(me).unwrap();
    assert_eq!(entry.spent_total, spent);
    assert_eq!(entry.points_last_month, 0.0);
    // Die Führung des Spielers bleibt unangetastet: Keine KI stellt seinen Etat zurück.
    assert_eq!(entry.budget_share, 0.0);

    // Ein Etat im erlaubten Rahmen: Gezahlt wird genau dieser Anteil der laufenden
    // Einnahmen des Vormonats.
    let share = (usual * 1.5).min(max);
    assert_eq!(
        command(&mut sim, me, SimCommand::SetResearchBudget { share }),
        CommandOutcome::Applied
    );
    // Bis kurz vor dem nächsten Monatsersten: Die Einnahmen, aus denen er rechnen wird,
    // stehen dann fest, nur die Dividenden des Abschlusses fehlen noch.
    let to_month_end = DAYS_PER_MONTH - sim.clock().day() % DAYS_PER_MONTH;
    sim.run_days(to_month_end);
    let entry = sim.research().get(me).unwrap();
    assert_eq!(entry.budget_share, share);
    let income = sim.world().resource::<Economy>().state_income_of(me);
    let planets = sim.galaxy().planets_of(me).count() as Money;
    assert!(income > 0);
    let budget = (share * income as f64).round() as Money;
    assert!(
        entry.spent_last_month <= budget && entry.spent_last_month >= budget - planets,
        "{} von {budget}",
        entry.spent_last_month
    );
    // Eine KI-Gemeinschaft daneben zahlt den Anteil ihrer Vorlage.
    let other = PolityId(1);
    let theirs = sim.research().get(other).unwrap();
    assert_eq!(theirs.budget_share, theirs.usual_share);
    let their_income = sim.world().resource::<Economy>().state_income_of(other);
    let their_budget = (theirs.budget_share * their_income as f64).round() as Money;
    let their_planets = sim.galaxy().planets_of(other).count() as Money;
    assert!(
        theirs.spent_last_month <= their_budget
            && theirs.spent_last_month >= their_budget - their_planets,
        "{} von {their_budget}",
        theirs.spent_last_month
    );
}

#[test]
fn ai_communities_set_their_budget_by_command_and_raise_it_in_war() {
    let data = data();
    let balance = &data.balance.research;
    let mut sim = with_player(4);
    let (me, fighter, bystander) = (PolityId(0), PolityId(2), PolityId(3));
    sim.run_days(2 * DAYS_PER_MONTH + 3);
    let usual = sim.research().get(fighter).unwrap().usual_share;
    assert!(usual > 0.0 && usual < balance.max_budget_share);
    // Im Frieden gilt der Etat der Vorlage; dafür braucht es keinen Befehl.
    assert_eq!(sim.research().get(fighter).unwrap().budget_share, usual);
    let applied = |sim: &Headless| sim.world().resource::<CommandLog>().ai_applied;
    // Der Spieler stellt seinen Etat selbst ein, auch auf einen ungewöhnlichen Wert.
    let own = usual / 3.0;
    assert_eq!(
        command(&mut sim, me, SimCommand::SetResearchBudget { share: own }),
        CommandOutcome::Applied
    );

    // Der Spieler erklärt einer KI-Gemeinschaft den Krieg. Am nächsten Monatsersten setzt
    // sie ihren Etat herauf, und zwar durch einen Befehl wie jeder Spieler.
    assert_eq!(
        command(&mut sim, me, SimCommand::DeclareWar { target: fighter }),
        CommandOutcome::Applied
    );
    let before = applied(&sim);
    sim.run_days(DAYS_PER_MONTH + 1);
    let wartime = (usual * balance.ai_war_factor).min(balance.max_budget_share);
    assert!(
        wartime > usual,
        "im Krieg ändert sich nichts: {usual} → {wartime}"
    );
    let entry = sim.research().get(fighter).unwrap();
    assert_eq!(entry.budget_share, wartime);
    assert_eq!(entry.usual_share, usual);
    assert!(applied(&sim) > before, "die Erhöhung kam nicht als Befehl");
    // Wer nicht im Krieg steht, bleibt bei seinem Etat; der Spieler behält den seinen,
    // auch wenn er selbst Krieg führt.
    let neighbour = sim.research().get(bystander).unwrap();
    assert_eq!(neighbour.budget_share, neighbour.usual_share);
    assert_eq!(sim.research().get(me).unwrap().budget_share, own);

    // Nach dem Friedensschluss kehrt sie zum üblichen Etat zurück.
    assert_eq!(
        command(&mut sim, me, SimCommand::MakePeace { target: fighter }),
        CommandOutcome::Applied
    );
    sim.run_days(DAYS_PER_MONTH + 1);
    assert_eq!(sim.research().get(fighter).unwrap().budget_share, usual);
    assert_eq!(sim.research().get(me).unwrap().budget_share, own);

    // Ein Etat, den jemand von außen verstellt hat (Skript, früherer Spieler), wird von
    // der KI-Führung am nächsten Monatsersten wieder auf den ihrer Vorlage gesetzt.
    assert_eq!(
        command(
            &mut sim,
            bystander,
            SimCommand::SetResearchBudget { share: 0.0 }
        ),
        CommandOutcome::Applied
    );
    assert_eq!(sim.research().get(bystander).unwrap().budget_share, 0.0);
    sim.run_days(DAYS_PER_MONTH + 1);
    let entry = sim.research().get(bystander).unwrap();
    assert_eq!(entry.budget_share, entry.usual_share);
    assert_eq!(check_invariants(sim.world()), Ok(()));
}

// --- 5. Schätzungen ---

/// Setzt den Einblick eines Betrachters und lässt die Bilder folgen, wie es der
/// Forschungsschritt am Monatsersten tut.
fn see(
    research: &mut Research,
    data: &GameData,
    viewer: PolityId,
    target: PolityId,
    intel: f32,
    month: u64,
) {
    research.intel[viewer.index()][target.index()] = intel;
    update_beliefs(research, data, 5, month);
}

#[test]
fn estimates_bracket_the_truth_and_sharpen_with_insight() {
    let mut data = data();
    // Ohne Sichtungen: Hier geht es allein darum, was der Einblick zeigt.
    data.balance.research.sighting_per_month = 0.0;
    let build = || {
        let mut sim = Headless::new(
            data.clone(),
            SimConfig {
                seed: 5,
                player: None,
            },
        );
        sim.run_days(3 * DAYS_PER_YEAR);
        // Unterschiedlich weit entwickelte Gemeinschaften, auch an den Rändern des Baums.
        {
            let mut research = sim.world_mut().resource_mut::<Research>();
            research.grant(&data, PolityId(1), tech(&data, "waffen_4"));
            research.grant(&data, PolityId(2), tech(&data, "wirtschaft_6"));
            research.grant(&data, PolityId(3), tech(&data, "boden_3"));
            research.grant(&data, PolityId(3), tech(&data, "verteidigung_5"));
        }
        sim
    };
    let sim = build();
    let count = sim.polities().list.len();
    let start = sim.research().clone();
    let spread = data.balance.research.estimate_spread;
    let truth = |research: &Research, target: PolityId| {
        TechBranch::ALL.map(|branch| research.tier(&data, target, branch))
    };
    assert_eq!(start.find_anomaly(&data, count), None);

    for viewer in (0..count).map(PolityId::from_index) {
        // Sich selbst kennt jede Gemeinschaft genau.
        let own = start.estimate(&data, viewer, viewer);
        assert_eq!(own, truth(&start, viewer).map(TierEstimate::exact));
        for target in (0..count).map(PolityId::from_index) {
            if viewer == target {
                continue;
            }
            let actual = truth(&start, target);
            let mut research = start.clone();
            let mut wider: Option<[TierEstimate; 5]> = None;
            for step in 0..=20 {
                let intel = step as f32 / 20.0;
                see(&mut research, &data, viewer, target, intel, step);
                let estimate = research.estimate(&data, viewer, target);
                let width = estimate_width(intel, spread);
                for (range, tier) in estimate.iter().zip(actual) {
                    assert!(
                        range.contains(tier),
                        "Stufe {tier} außerhalb von {range} bei Einblick {intel}"
                    );
                    assert!(range.width() <= width);
                }
                // Dieselbe Frage: dieselbe Antwort, gleich wie oft und wann sie gestellt
                // wird. Auch ein weiterer Monat ohne neuen Einblick ändert nichts.
                assert_eq!(estimate, research.estimate(&data, viewer, target));
                update_beliefs(&mut research, &data, 5, 1000 + step);
                assert_eq!(estimate, research.estimate(&data, viewer, target));
                // Mehr Einblick macht keine Spanne weiter und widerspricht nie dem
                // früheren Bild.
                if let Some(wider) = wider {
                    for (narrow, wide) in estimate.iter().zip(wider) {
                        assert!(narrow.width() <= wide.width());
                        assert!(wide.lo <= narrow.lo && narrow.hi <= wide.hi);
                    }
                }
                if step == 20 {
                    assert_eq!(estimate, actual.map(TierEstimate::exact));
                }
                wider = Some(estimate);
            }
            // Der Einblick lässt wieder nach: Was bestätigt war, bleibt bestätigt; nach
            // oben wird das Bild unsicher.
            see(&mut research, &data, viewer, target, 0.0, 77);
            for (range, tier) in research.estimate(&data, viewer, target).iter().zip(actual) {
                assert_eq!(range.lo, tier);
                assert!(range.contains(tier));
            }
        }
    }

    // Zwei Betrachter mit gleichem Einblick sehen nicht zwingend dasselbe, und der Seed
    // entscheidet mit: Wo die Spanne liegt, hängt an der Geschichte jedes Paares.
    let narrowed = |seed: u64, viewer: usize| {
        let mut research = start.clone();
        for other in 0..count {
            if other != viewer {
                research.intel[viewer][other] = 0.8;
            }
        }
        update_beliefs(&mut research, &data, seed, 3);
        (0..count)
            .filter(|other| *other != viewer)
            .map(|other| research.beliefs[viewer][other])
            .collect::<Vec<_>>()
    };
    assert_ne!(
        narrowed(5, 0),
        narrowed(6, 0),
        "der Seed spielt keine Rolle"
    );
    assert_eq!(narrowed(5, 0), narrowed(5, 0));

    // Eine zweite Partie mit demselben Seed zeigt jedem dasselbe Bild.
    let twin = build();
    assert_eq!(twin.hash(), sim.hash());
    assert_eq!(twin.research().beliefs, sim.research().beliefs);
}

#[test]
fn a_notebook_kept_for_decades_shows_no_more_than_the_latest_picture() {
    // Eine ganze Partie lang schreibt jede Gemeinschaft jeden Monat mit, was sie von jeder
    // anderen sieht. Sicher schließen lässt sich daraus: mindestens die höchste je
    // gesehene untere Grenze (Wissen geht nicht verloren), höchstens die aktuelle obere.
    // Das darf nie enger sein als das aktuelle Bild.
    let data = data();
    let spread = data.balance.research.estimate_spread;
    let years = 45;
    let mut sim = observer(2);
    let count = sim.polities().list.len();
    let pairs: Vec<(usize, usize)> = (0..count)
        .flat_map(|viewer| (0..count).map(move |target| (viewer, target)))
        .filter(|(viewer, target)| viewer != target)
        .collect();
    let tiers = |sim: &Headless, target: usize| {
        TechBranch::ALL.map(|branch| {
            sim.research()
                .tier(&data, PolityId::from_index(target), branch)
        })
    };
    let mut best_floor = vec![[0u8; 5]; pairs.len()];
    let mut last_picture: Vec<[TierEstimate; 5]> = pairs
        .iter()
        .map(|(viewer, target)| {
            sim.research().estimate(
                &data,
                PolityId::from_index(*viewer),
                PolityId::from_index(*target),
            )
        })
        .collect();
    let mut last_truth: Vec<[u8; 5]> = pairs
        .iter()
        .map(|(_, target)| tiers(&sim, *target))
        .collect();
    // Einblick, wie er vor dem Schritt bestand: Ihm folgt das Bild dieses Monats.
    let mut last_intel: Vec<f32> = pairs
        .iter()
        .map(|(viewer, target)| {
            sim.research()
                .intel(PolityId::from_index(*viewer), PolityId::from_index(*target))
        })
        .collect();
    let top = sim::tech::top_tier(&data, TechBranch::Economy);
    assert!(
        TechBranch::ALL
            .iter()
            .all(|branch| sim::tech::top_tier(&data, *branch) == top)
    );
    // Aufstiege, die das Bild nicht bewegt haben, und Bewegungen ohne Aufstieg.
    let (mut unnoticed, mut noticed, mut confirmations) = (0u32, 0u32, 0u32);
    for month in 0..years * 12 {
        sim.run_days(DAYS_PER_MONTH);
        for (slot, (viewer, target)) in pairs.iter().enumerate() {
            let (viewer_id, target_id) =
                (PolityId::from_index(*viewer), PolityId::from_index(*target));
            let picture = sim.research().estimate(&data, viewer_id, target_id);
            let truth = tiers(&sim, *target);
            let intel = sim.research().intel(viewer_id, target_id);
            assert!(intel < 1.0, "in dieser Partie hat niemand vollen Einblick");
            let width = estimate_width(last_intel[slot], spread);
            last_intel[slot] = intel;
            for branch in 0..5 {
                let (now, before) = (picture[branch], last_picture[slot][branch]);
                let case = format!(
                    "Monat {month}, G{viewer} über G{target}, Zweig {branch}: {before} → {now}, \
                     wahr {}",
                    truth[branch]
                );
                assert!(now.contains(truth[branch]), "{case}");
                best_floor[slot][branch] = best_floor[slot][branch].max(now.lo);
                // Das Notizbuch: [höchste untere Grenze, aktuelle obere] ist das Bild selbst.
                assert_eq!(best_floor[slot][branch], now.lo, "{case}");
                // Nie genau, und genau so weit, wie es der Einblick erlaubt, mit dem der
                // Monat begann; enger nur am oberen Rand des Baums.
                assert!(!now.is_exact(), "{case}");
                assert!(now.width() <= width, "{case}");
                assert!(now.width() == width || now.hi == top, "{case}");
                // Ohne neuen Einblick bewegt sich das Bild um höchstens eine Stufe.
                if now.width() == before.width() {
                    assert!(now.lo - before.lo <= 1, "{case}");
                }
                let rose = truth[branch] > last_truth[slot][branch];
                let moved = now.lo > before.lo && now.width() == before.width();
                match (rose, moved) {
                    (true, false) => unnoticed += 1,
                    (true, true) => noticed += 1,
                    (false, true) => confirmations += 1,
                    (false, false) => {}
                }
            }
            last_picture[slot] = picture;
            last_truth[slot] = truth;
        }
    }
    // Die meisten Aufstiege bleiben unbemerkt, und ein Schritt des Bildes heißt nicht,
    // dass die andere Gemeinschaft aufgestiegen ist: Meist hat sich nur etwas bestätigt.
    assert!(
        unnoticed > 5 * noticed,
        "{unnoticed} unbemerkt, {noticed} bemerkt"
    );
    assert!(
        confirmations > noticed,
        "{confirmations} Bestätigungen, {noticed} bemerkte Aufstiege"
    );
    assert!(unnoticed >= 100, "nur {unnoticed} Aufstiege beobachtet");
    assert_eq!(check_invariants(sim.world()), Ok(()));
}

#[test]
fn insight_follows_ownership_and_events_and_fades() {
    let data = data();
    let balance = &data.balance.research;
    let mut sim = observer(5);
    let (viewer, target) = (PolityId(0), PolityId(1));
    assert_eq!(sim.research().intel(viewer, target), balance.intel_base);
    assert_eq!(sim.research().intel(viewer, viewer), 1.0);
    sim.run_days(10);
    let wide = sim.research().estimate(&data, viewer, target);
    assert!(wide.iter().all(|range| !range.is_exact()));

    // Ein beobachtetes Gefecht: voller Einblick. Der Wert gilt sofort; das Bild folgt am
    // nächsten Monatsersten und ist dann genau.
    sim.world_mut()
        .resource_mut::<Research>()
        .add_intel(viewer, target, 1.0);
    assert_eq!(sim.research().intel(viewer, target), 1.0);
    assert_eq!(sim.research().estimate(&data, viewer, target), wide);
    sim.run_days(DAYS_PER_MONTH);
    let exact = sim.research().estimate(&data, viewer, target);
    let truth = TechBranch::ALL.map(|branch| sim.research().tier(&data, target, branch));
    assert_eq!(exact, truth.map(TierEstimate::exact));
    // Danach verblasst der Einblick über die Jahre, und mit ihm die Schärfe: Die untere
    // Grenze bleibt, nach oben wird das Bild wieder offen.
    sim.run_days(DAYS_PER_YEAR);
    let after_a_year = sim.research().intel(viewer, target);
    assert!(after_a_year < 1.0 && after_a_year > 0.6, "{after_a_year}");
    let faded = sim.research().estimate(&data, viewer, target);
    for (range, known) in faded.iter().zip(exact) {
        assert!(range.lo >= known.lo);
        assert!(!range.is_exact());
    }
    assert!(
        sim.research().intel(target, viewer) < 0.5,
        "Einblick ist gerichtet: Der Beobachtete erfährt nichts"
    );

    // Besitz an den Betrieben der anderen: Der Einblick steigt und hält sich.
    let capital = sim.polities().get(viewer).capital.unwrap();
    {
        let galaxy = sim.galaxy().clone();
        let mut finance = sim.world_mut().resource_mut::<Finance>();
        for (id, issuer) in finance.issuers.iter_mut() {
            let sim::ids::IssuerId::Firm { planet, .. } = id else {
                continue;
            };
            if galaxy.planet(*planet).owner == Some(target) {
                // Die Elite des Betrachters hält fortan ein Fünftel jeder Firma.
                let stake = issuer.shares / 4;
                issuer.shares += stake;
                issuer
                    .holdings
                    .insert(sim::ids::HolderId::Elite(capital), stake);
            }
        }
    }
    sim.world_mut().resource_mut::<Research>().intel[0][1] = balance.intel_base;
    sim.run_days(DAYS_PER_MONTH + 1);
    let tied = sim.research().intel(viewer, target);
    assert!(
        tied > balance.intel_base + 0.2 && tied <= balance.intel_ties_max,
        "Einblick aus Besitz: {tied}"
    );
    assert!(
        sim.research().intel(target, viewer) < tied,
        "wer nichts besitzt, sieht weniger"
    );
    // Das Bild folgt dem Einblick mit dem nächsten Schritt.
    sim.run_days(DAYS_PER_MONTH);
    let estimate = sim.research().estimate(&data, viewer, target);
    assert!(
        estimate
            .iter()
            .all(|range| range.width() < 2 * balance.estimate_spread)
    );
    assert_eq!(check_invariants(sim.world()), Ok(()));
}

// --- 6. Wissenstransfer ---

/// Bestätigt dem Betrachter, was das Ziel wirklich kann: voller Einblick für einen Monat.
fn reveal(world: &mut SimState, data: &GameData, viewer: PolityId, target: PolityId) {
    world.research.add_intel(viewer, target, 1.0);
    update_beliefs(&mut world.research, data, world.config.seed, 0);
}

#[test]
fn visible_knowledge_of_neighbours_makes_a_technology_cheaper_the_nearer_the_more() {
    let mut data = data();
    // Ohne zufällige Sichtungen: Bestätigt ist nur, was die Prüfung bestätigen lässt.
    data.balance.research.sighting_per_month = 0.0;
    let balance = data.balance.research.clone();
    let mut base = start(&data, 3);
    let learner = PolityId(0);
    let wanted = tech(&data, "boden_2");
    let required = tech(&data, "boden_1");
    // Zu Beginn kennt niemand die zweite Stufe; die Lernende bringt die erste mit.
    assert!(
        base.polities
            .list
            .iter()
            .all(|p| !base.research.knows(p.id, wanted))
    );
    base.research.grant(&data, learner, required);
    for index in 0..base.polities.list.len() {
        set_income(&mut base, index, 9_000.0);
    }
    let home = |polity: PolityId| {
        base.galaxy
            .planet(base.polities.get(polity).capital.unwrap())
            .system
    };
    let hops = base.galaxy.hops_from(home(learner));
    let others: Vec<PolityId> = base.polities.list.iter().skip(1).map(|p| p.id).collect();
    let distance = |polity: &PolityId| hops[home(*polity).index()];
    let nearest = *others.iter().min_by_key(|p| (distance(p), **p)).unwrap();
    let farthest = *others.iter().max_by_key(|p| (distance(p), **p)).unwrap();
    assert!(distance(&nearest) < distance(&farthest));

    // `knowers` beherrschen die Technologie, bei `seen` hat die Lernende das auch erkannt.
    let world_with = |knowers: &[PolityId], seen: &[PolityId], at_war: bool| {
        let mut world = base.clone();
        for knower in knowers {
            world.research.grant(&data, *knower, wanted);
        }
        for target in seen {
            reveal(&mut world, &data, learner, *target);
        }
        if at_war {
            let pair = (learner.min(nearest), learner.max(nearest));
            world.diplomacy.wars.insert(pair);
        }
        world
    };
    let cost_with = |knowers: &[PolityId], seen: &[PolityId], at_war: bool| {
        let mut world = world_with(knowers, seen, at_war);
        research_step(&data, &mut world);
        world.research.cost(&data, learner, wanted)
    };
    let full = data.techs[wanted as usize].cost;
    assert_eq!(
        cost_with(&[], &[], false),
        full,
        "was niemand weiß, kostet den vollen Preis"
    );
    // Was alle wissen, die Lernende aber bei niemandem erkannt hat, kostet ebenfalls den
    // vollen Preis: Der Nachlass verrät nicht, wer was kann.
    assert_eq!(cost_with(&others, &[], false), full);
    // Erkanntes Wissen verbilligt, das der Nachbarn mehr als das der Fernen.
    let near = cost_with(&others, &[nearest], false);
    let far = cost_with(&others, &[farthest], false);
    let both = cost_with(&others, &[nearest, farthest], false);
    let everyone = cost_with(&others, &others, false);
    assert!(near < far, "Nähe zählt: {near} gegen {far}");
    assert!(far < full);
    assert!(both < near);
    assert!(everyone < both);
    assert!(
        everyone > full * (1.0 - balance.spillover_max),
        "der Nachlass bleibt unter seiner Obergrenze"
    );
    // Erkennen allein genügt nicht: Die andere muss die Technologie auch beherrschen.
    assert_eq!(cost_with(&[], &others, false), full);
    // Im Krieg ruht der Austausch mit dem Gegner.
    assert_eq!(cost_with(&others, &[nearest], true), full);
    assert!(cost_with(&others, &[nearest, farthest], true) > both);
    assert_eq!(cost_with(&others, &[nearest, farthest], true), far);

    // Der Nachlass beschleunigt die Forschung: bei gleichen Einnahmen früher fertig.
    let months_until_known = |seen: &[PolityId]| {
        let mut world = world_with(&others, seen, false);
        // Ein von außen gesetztes Ziel gilt bis zu seinem Abschluss.
        world.research.polities[0].auto = false;
        world.research.polities[0].target = Some(wanted);
        let mut months = 0;
        while !world.research.knows(learner, wanted) {
            // Die Kasse bleibt gefüllt: Hier zählt allein der Preis.
            world.economy.treasuries[0] = base.economy.treasuries[0];
            research_step(&data, &mut world);
            months += 1;
            assert!(months < 600, "die Technologie wird nie fertig");
        }
        months
    };
    let (slow, quick) = (months_until_known(&[]), months_until_known(&others));
    assert!(quick < slow, "{quick} gegen {slow} Monate");
    assert!(months_until_known(&[nearest]) <= months_until_known(&[farthest]));
}

#[test]
fn own_costs_reveal_nothing_the_estimates_do_not_show() {
    let mut data = data();
    // Ohne Sichtungen ändert sich ein Bild nur, wenn die Wahrheit es verlässt. So lässt
    // sich prüfen, dass verborgenes Wissen der anderen am eigenen Stand nichts ändert.
    data.balance.research.sighting_per_month = 0.0;
    let learner = PolityId(0);
    let mut plain = start(&data, 3);
    for index in 0..plain.polities.list.len() {
        set_income(&mut plain, index, 12_000.0);
    }
    // In der zweiten Welt sind die anderen weit voraus, in jedem Zweig verschieden weit.
    // Alles liegt aber innerhalb der Spannen, die die Lernende ohnehin für möglich hält.
    let mut hidden = plain.clone();
    let widest = estimate_width(
        data.balance.research.intel_base,
        data.balance.research.estimate_spread,
    ) as u32;
    assert!(widest >= 3);
    let others: Vec<PolityId> = plain.polities.list.iter().skip(1).map(|p| p.id).collect();
    let mut granted = 0;
    for (position, other) in others.iter().enumerate() {
        for branch in TechBranch::ALL {
            let tier = 1 + (position as u32 + branch.index() as u32) % widest;
            let advanced = data
                .techs
                .iter()
                .position(|def| def.branch == branch && def.tier == tier)
                .unwrap() as u16;
            if hidden.research.grant(&data, *other, advanced) {
                granted += 1;
            }
        }
    }
    assert!(granted >= 20, "die Welten unterscheiden sich kaum");
    assert_eq!(hidden.research.beliefs[0], plain.research.beliefs[0]);

    let treasuries = plain.economy.treasuries.clone();
    for month in 0..48 {
        for world in [&mut plain, &mut hidden] {
            world.clock.tick += 600;
            // Die Kassen bleiben gefüllt: Ohne laufende Wirtschaft kämen keine Steuern nach.
            world.economy.treasuries.clone_from(&treasuries);
        }
        research_step(&data, &mut plain);
        research_step(&data, &mut hidden);
        // Voraussetzung der Prüfung: Die Lernende sieht in beiden Welten dasselbe.
        assert_eq!(
            hidden.research.beliefs[0], plain.research.beliefs[0],
            "Monat {month}: die Bilder gehen auseinander"
        );
        // Dann ist auch ihr ganzer Forschungsstand derselbe: Kosten, Nachlass, Ziel,
        // Fortschritt, Abschlüsse. Nichts davon hängt am wahren Stand der anderen.
        assert_eq!(
            hidden.research.polities[0], plain.research.polities[0],
            "Monat {month}"
        );
        for world in [&plain, &hidden] {
            // Der Nachlass folgt allein aus dem eigenen Bild und der Karte.
            let weights = contact_weights(
                &world.galaxy,
                &world.polities,
                &world.diplomacy,
                &data.balance.research,
            );
            for (index, entry) in world.research.polities.iter().enumerate() {
                assert_eq!(
                    entry.spillover,
                    visible_spillover(
                        &world.research,
                        &data,
                        PolityId::from_index(index),
                        &weights[index]
                    ),
                    "Monat {month}, G{index}"
                );
            }
        }
    }
    // In beiden Welten wurde wirklich geforscht, und die anderen sind in der zweiten weiter.
    assert!(plain.research.polities[0].completed >= 1);
    for other in &others {
        assert!(hidden.research.known_count(*other) > plain.research.known_count(*other));
    }
    // Gegenprobe: Sobald die Lernende etwas davon erkennt, wirkt es, und nur dann.
    let before = hidden.research.polities[0].spillover.clone();
    assert!(before.iter().all(|discount| *discount == 0.0));
    reveal(&mut hidden, &data, learner, others[0]);
    research_step(&data, &mut hidden);
    let after = &hidden.research.polities[0].spillover;
    assert!(after.iter().any(|discount| *discount > 0.0));
    for (index, def) in data.techs.iter().enumerate() {
        let known_there = hidden.research.knows(others[0], index as u16);
        assert_eq!(
            after[index] > 0.0,
            known_there,
            "{}: Nachlass {} bei bekanntem Stand {known_there}",
            def.name,
            after[index]
        );
    }
}

#[test]
fn a_war_reveals_nothing_about_the_enemy() {
    let mut data = data();
    data.balance.research.sighting_per_month = 0.0;
    let learner = PolityId(0);
    let mut world = start(&data, 3);
    for index in 0..world.polities.list.len() {
        set_income(&mut world, index, 9_000.0);
    }
    let enemy = PolityId(1);
    let witness = PolityId(2);
    // Der spätere Gegner ist weit voraus; erkannt hat die Lernende davon nichts. Bei einer
    // dritten Gemeinschaft hat sie zwei Stufen Waffentechnik erkannt.
    for key in ["waffen_4", "verteidigung_3", "antrieb_2", "wirtschaft_4"] {
        world.research.grant(&data, enemy, tech(&data, key));
    }
    world
        .research
        .grant(&data, witness, tech(&data, "waffen_2"));
    reveal(&mut world, &data, learner, witness);
    research_step(&data, &mut world);
    let costs = |world: &SimState| -> Vec<f64> {
        (0..data.techs.len() as u16)
            .map(|tech| world.research.cost(&data, learner, tech))
            .collect()
    };
    let peace = costs(&world);
    assert!(
        peace[tech(&data, "waffen_2") as usize] < data.techs[tech(&data, "waffen_2") as usize].cost
    );

    // Kriegserklärung und sofortiger Frieden: Die eigenen Kosten bewegen sich nicht, also
    // lässt sich aus ihnen nicht ablesen, was der Gegner kann.
    let pair = (learner.min(enemy), learner.max(enemy));
    let mut at_war = world.clone();
    at_war.diplomacy.wars.insert(pair);
    research_step(&data, &mut at_war);
    assert_eq!(costs(&at_war), peace);
    at_war.diplomacy.wars.remove(&pair);
    research_step(&data, &mut at_war);
    assert_eq!(costs(&at_war), peace);

    // Ein Krieg gegen die Gemeinschaft, bei der etwas erkannt ist, nimmt genau diesen
    // Nachlass und sonst nichts: Auch das wusste die Lernende schon.
    let pair = (learner.min(witness), learner.max(witness));
    let mut cut_off = world.clone();
    cut_off.diplomacy.wars.insert(pair);
    research_step(&data, &mut cut_off);
    let mut lost = 0;
    for (index, def) in data.techs.iter().enumerate() {
        // Erkannt war alles, was sie beherrscht: Der Einblick war für einen Monat voll.
        let seen = world.research.knows(witness, index as u16);
        let cost = cut_off.research.cost(&data, learner, index as u16);
        if seen {
            assert_eq!(cost, def.cost, "{}", def.name);
            assert!(peace[index] < def.cost, "{}", def.name);
            lost += 1;
        } else {
            assert_eq!(cost, peace[index], "{}", def.name);
        }
    }
    assert!(lost >= 2, "der Nachlass hing an mehr als einer Technologie");
}

#[test]
fn in_a_running_game_the_better_placed_community_learns_more_from_what_it_sees() {
    // Eine laufende Partie, in der jede Gemeinschaft jede andere genau kennt (jeden Monat
    // voller Einblick): Dann entscheidet allein die Lage, wie viel der Wissenstransfer
    // bringt, und der Nachlass lässt sich aus Karte und Wissensstand nachrechnen.
    let data = data();
    let balance = &data.balance.research;
    let mut sim = observer(3);
    let reveal_all = |sim: &mut Headless| {
        let count = sim.polities().list.len();
        let mut research = sim.world_mut().resource_mut::<Research>();
        for viewer in 0..count {
            for target in 0..count {
                research.add_intel(
                    PolityId::from_index(viewer),
                    PolityId::from_index(target),
                    1.0,
                );
            }
        }
    };
    // Einen Takt versetzt: Jeder Abschnitt endet unmittelbar nach einem Monatsersten.
    sim.tick();
    let mut checked = 0;
    for month in 0..25 * 12 {
        let before = sim.polities().list.len();
        reveal_all(&mut sim);
        sim.run_days(DAYS_PER_MONTH);
        let count = sim.polities().list.len();
        if count != before {
            // Eine eben gegründete Gemeinschaft kennt erst ab dem nächsten Monat alle genau.
            continue;
        }
        let research = sim.research();
        let weights = contact_weights(
            sim.galaxy(),
            sim.polities(),
            sim.world().resource::<sim::diplomacy::Diplomacy>(),
            balance,
        );
        for learner in 0..count {
            for (index, _) in data.techs.iter().enumerate() {
                let sum: f64 = (0..count)
                    .filter(|other| *other != learner)
                    .filter(|other| research.knows(PolityId::from_index(*other), index as u16))
                    .map(|other| weights[learner][other])
                    .sum();
                let expected = sim::tech::spillover_discount(balance, sum);
                let actual = research.polities[learner].spillover[index];
                assert!(
                    (actual - expected).abs() < 1e-12,
                    "Monat {month}, G{learner}, {}: Nachlass {actual} statt {expected}",
                    data.techs[index].name
                );
                checked += 1;
            }
        }
    }
    assert!(checked > 10_000);

    // Nach 25 Jahren: Für eine Technologie, die alle anderen beherrschen, hängt der
    // Nachlass nur noch an der Lage. Wer mehr Kontakt hat, zahlt weniger.
    let research = sim.research();
    let count = sim.polities().list.len();
    let weights = contact_weights(
        sim.galaxy(),
        sim.polities(),
        sim.world().resource::<sim::diplomacy::Diplomacy>(),
        balance,
    );
    let contact = |index: usize| weights[index].iter().sum::<f64>();
    let common: Vec<u16> = (0..data.techs.len() as u16)
        .filter(|tech| (0..count).all(|index| research.knows(PolityId::from_index(index), *tech)))
        .collect();
    assert!(
        !common.is_empty(),
        "nach 25 Jahren kennt jede die erste Stufe der Wirtschaft"
    );
    let mut compared = 0;
    for a in 0..count {
        for b in 0..count {
            if contact(a) <= contact(b) * 1.05 {
                continue;
            }
            for tech in &common {
                let (near, far) = (
                    research.polities[a].spillover[*tech as usize],
                    research.polities[b].spillover[*tech as usize],
                );
                assert!(
                    near > far,
                    "G{a} (Kontakt {:.2}) gegen G{b} ({:.2})",
                    contact(a),
                    contact(b)
                );
                compared += 1;
            }
        }
    }
    assert!(compared >= 3, "die Lagen unterscheiden sich kaum");

    // Der Nachlass ist echt: Jede Gemeinschaft hat für ihre Abschlüsse weniger Punkte
    // gebraucht, als sie zum vollen Preis gekostet hätten, und kein Punkt ging verloren.
    let start = SimState::generate(
        &data,
        SimConfig {
            seed: 3,
            player: None,
        },
    );
    let mut saved_somewhere = false;
    for (index, entry) in research.polities.iter().enumerate() {
        // Nur die Gemeinschaften des Spielbeginns: Ein Freistaat erbt Wissen, das er nicht
        // selbst bezahlt hat.
        let Some(inherited) = start.research.polities.get(index).map(|entry| &entry.known) else {
            continue;
        };
        let full_price: f64 = data
            .techs
            .iter()
            .enumerate()
            .filter(|(tech, _)| entry.known[*tech] && !inherited[*tech])
            .map(|(_, def)| def.cost)
            .sum();
        let in_progress: f64 = entry.progress.iter().sum::<f64>() + entry.carry;
        let used = entry.points_total - in_progress;
        assert!(used > 0.0);
        assert!(
            used <= full_price + 1e-6,
            "G{index}: {used:.0} Punkte verbraucht, zum vollen Preis wären es {full_price:.0}"
        );
        if used < full_price * 0.98 {
            saved_somewhere = true;
        }
    }
    assert!(
        saved_somewhere,
        "niemand hat durch den Wissenstransfer gespart"
    );
    assert_eq!(check_invariants(sim.world()), Ok(()));
}

// --- 7. Abspaltung ---

#[test]
fn a_seceding_region_takes_the_knowledge_of_its_old_realm_along() {
    let data = data();
    let mut sim = observer(7);
    sim.run_days(6 * DAYS_PER_YEAR);
    let parent = PolityId(0);
    let before = sim.polities().list.len();
    let capital = sim.polities().get(parent).capital.unwrap();
    // Die geräumigste Kolonie des Reichs wird groß genug für einen eigenen Staat.
    let needed = data.balance.finance.cohesion.independence_min_population + 50.0;
    let colony: PlanetId = sim
        .galaxy()
        .planets_of(parent)
        .filter(|planet| planet.id != capital)
        .max_by(|a, b| a.capacity.total_cmp(&b.capacity).then(b.id.cmp(&a.id)))
        .map(|planet| planet.id)
        .expect("nach sechs Jahren gibt es Kolonien");
    assert!(sim.galaxy().planet(colony).capacity >= needed);
    {
        let world = sim.world_mut();
        let mut galaxy = world.resource_mut::<Galaxy>();
        let planet = galaxy.planet_mut(colony);
        planet.population = planet.population.max(needed);
    }
    // Das alte Reich ist den anderen voraus und mitten in einer Arbeit.
    let begun = tech(&data, "antrieb_3");
    {
        let mut research = sim.world_mut().resource_mut::<Research>();
        research.grant(&data, parent, tech(&data, "waffen_2"));
        research.grant(&data, parent, tech(&data, "wirtschaft_3"));
        research.grant(&data, parent, tech(&data, "antrieb_2"));
        research.polities[0].progress[begun as usize] = 4321.0;
        research.polities[0].budget_share = 0.2;
        research.polities[0].usual_share = 0.2;
        research.add_intel(parent, PolityId(1), 0.5);
    }
    let inherited = sim.research().known(parent).to_vec();
    let focus = sim.research().get(parent).unwrap().focus;
    // Die Region ist seit Jahren autonom und sagt sich im nächsten Monatsabschluss los.
    {
        let months = data.balance.finance.cohesion.secession_months;
        let mut finance = sim.world_mut().resource_mut::<Finance>();
        let region = &mut finance.regions[colony.index()];
        region.stage = Stage::Autonomous;
        region.cohesion = 0.0;
        region.months_below = months - 1;
        region.months_above = 0;
    }
    let view_of_others = sim.research().estimate(&data, parent, PolityId(1));
    sim.run_days(1);

    let polities = sim.polities();
    assert_eq!(polities.list.len(), before + 1, "ein Freistaat entsteht");
    let child = PolityId::from_index(before);
    assert_eq!(sim.galaxy().planet(colony).owner, Some(child));
    let research = sim.research();
    // Jede Liste hat für jede Gemeinschaft einen Eintrag, noch im Takt der Gründung.
    assert!(research.is_consistent(&data, before + 1));
    assert_eq!(research.polities.len(), polities.list.len());
    assert_eq!(research.intel.len(), polities.list.len());
    assert_eq!(research.beliefs.len(), polities.list.len());
    assert_eq!(check_invariants(sim.world()), Ok(()));
    let economy = sim.world().resource::<Economy>();
    let logistics = sim.world().resource::<Logistics>();
    assert_eq!(economy.tech_yield.len(), before + 1);
    assert_eq!(economy.state_income.len(), before + 1);
    assert_eq!(economy.state_income_running.len(), before + 1);
    assert_eq!(logistics.tech_speed.len(), before + 1);
    // Der Freistaat weiß, was das alte Reich wusste, und forscht auf eigene Rechnung weiter.
    let new = research.get(child).unwrap();
    assert_eq!(new.known, inherited);
    assert!(research.knows(child, tech(&data, "waffen_2")));
    assert_eq!(new.effects, TechEffects::of(&data, &inherited));
    assert!(new.effects.yield_factor > 1.0);
    assert_eq!(new.progress[begun as usize], 4321.0);
    assert_eq!(new.budget_share, 0.2);
    assert_eq!(new.usual_share, 0.2);
    assert_eq!(new.focus, focus);
    assert!(new.auto);
    assert_eq!(new.completed, 0);
    // Seine Werke und Frachter rechnen vom Tag der Gründung an mit der geerbten Technik.
    assert_eq!(economy.yield_factor(child), new.effects.yield_factor);
    assert_eq!(
        logistics.freighter_speed(child, 10.0),
        10.0 * new.effects.freighter_speed as f32
    );
    assert_eq!(
        data.best_design(
            data.designs[data.design_index("speer").unwrap() as usize]
                .stats
                .class,
            research.known(child)
        ),
        data.design_index("speer_2")
    );
    // Einblick: Beide kennen einander fast genau (der erste Monat hat schon etwas
    // verblassen lassen), und der Freistaat erbt das Bild, das das Reich von anderen hatte.
    assert!(research.intel(child, parent) > 0.95);
    assert!(research.intel(parent, child) > 0.95);
    assert!(research.intel(child, PolityId(1)) > data.balance.research.intel_base + 0.3);
    let truth = TechBranch::ALL.map(|branch| research.tier(&data, parent, branch));
    assert_eq!(
        research.estimate(&data, child, parent),
        truth.map(TierEstimate::exact)
    );
    for (inherited, old) in research
        .estimate(&data, child, PolityId(1))
        .iter()
        .zip(view_of_others)
    {
        assert!(inherited.lo >= old.lo, "Bestätigtes geht mit");
    }

    // Die Partie läuft weiter: Der Freistaat nimmt Steuern ein, bezahlt Forschende und
    // seine Werke nutzen die geerbte Technik.
    for _ in 0..4 {
        sim.run_days(DAYS_PER_YEAR);
        assert_eq!(check_invariants(sim.world()), Ok(()));
    }
    let research = sim.research();
    let entry = research.get(child).unwrap();
    assert!(entry.spent_total > 0 && entry.points_total > 0.0);
    assert!(entry.target.is_some());
    assert!(research.is_consistent(&data, before + 1));
    let economy = sim.world().resource::<Economy>();
    assert_eq!(economy.yield_factor(child), entry.effects.yield_factor);
    assert!(economy.state_income_of(child) > 0);
    assert!(
        research.intel(child, parent) < 1.0,
        "Vertrautheit verblasst"
    );
    assert_eq!(money_drift(sim.world()), Some(0));
}

// --- 8. Determinismus ---

#[test]
fn the_same_seed_gives_the_same_research() {
    let data = data();
    let a = run_ai_game(&data, 7, 12 * DAYS_PER_YEAR);
    let b = run_ai_game(&data, 7, 12 * DAYS_PER_YEAR);
    let c = run_ai_game(&data, 8, 12 * DAYS_PER_YEAR);
    assert_eq!(a.hash(), b.hash());
    assert_eq!(a.research().polities, b.research().polities);
    assert_eq!(a.research().intel, b.research().intel);
    assert_eq!(a.research().beliefs, b.research().beliefs);
    assert_ne!(a.research().polities, c.research().polities);
    assert_ne!(a.research().beliefs, c.research().beliefs);
    // Es wurde wirklich geforscht.
    assert!(
        a.research()
            .polities
            .iter()
            .all(|entry| entry.completed > 0)
    );
}

#[test]
fn skipping_idle_ticks_changes_no_research() {
    let config = SimConfig {
        seed: 21,
        player: None,
    };
    let mut fast = Headless::new(data(), config.clone());
    let mut exact = Headless::new(data(), config);
    fast.run_days(450);
    exact.run_days_exact(450);
    assert_eq!(fast.clock(), exact.clock());
    assert_eq!(fast.research().polities, exact.research().polities);
    assert_eq!(fast.research().beliefs, exact.research().beliefs);
    assert_eq!(fast.hash(), exact.hash());
    assert!(
        fast.research()
            .polities
            .iter()
            .all(|entry| entry.spent_total > 0 && entry.target.is_some())
    );
}

#[test]
fn a_save_in_the_middle_of_a_project_continues_identically() {
    let data = data();
    let config = SimConfig {
        seed: 13,
        player: None,
    };
    let mut straight = Headless::new(data.clone(), config.clone());
    straight.run_days(1400);

    let mut first_half = Headless::new(data.clone(), config);
    first_half.run_days(700);
    // Mitten in der Arbeit: Jede Gemeinschaft hat ein angefangenes Ziel.
    for entry in &first_half.research().polities {
        let target = entry.target.expect("Ziel gewählt");
        let progress = entry.progress[target as usize];
        assert!(progress > 0.0 && progress < entry.cost(&data, target));
    }
    let text = first_half.save().to_ron().unwrap();
    let save = SaveGame::from_ron(&text).unwrap();
    assert!(save.matches_data(&data));
    let mut resumed = Headless::from_save(data, save);
    assert_eq!(resumed.research().polities, first_half.research().polities);
    assert_eq!(resumed.research().intel, first_half.research().intel);
    assert_eq!(resumed.research().beliefs, first_half.research().beliefs);
    assert_eq!(
        resumed.hash(),
        first_half.hash(),
        "Laden muss den Zustand exakt herstellen"
    );
    resumed.run_days(700);
    assert_eq!(resumed.research().polities, straight.research().polities);
    assert_eq!(resumed.research().beliefs, straight.research().beliefs);
    assert_eq!(resumed.hash(), straight.hash());
}

#[test]
fn research_commands_replay_from_the_log() {
    let data = data();
    let me = PolityId(0);
    let config = SimConfig {
        seed: 5,
        player: Some(me),
    };
    let mut original = Headless::new(data.clone(), config.clone());
    original.run_days(200);
    let pick = original.research().available(&data, me)[1];
    original.issue(me, SimCommand::SetResearchTarget { tech: Some(pick) });
    original.issue(
        me,
        SimCommand::SetResearchBudget {
            share: data.balance.research.max_budget_share,
        },
    );
    original.run_days(400);
    original.issue(me, SimCommand::SetResearchTarget { tech: None });
    original.issue(me, SimCommand::SetResearchBudget { share: 7.0 });
    original.run_days(200);
    let log = original.command_log().clone();
    assert_eq!(log.entries.len(), 4);
    assert_eq!(
        log.entries
            .iter()
            .filter(|entry| entry.outcome == CommandOutcome::Applied)
            .count(),
        3,
        "{:?}",
        log.entries
    );

    let mut replay = Headless::new(data.clone(), config.clone());
    replay.set_replay(&log);
    replay.run_days(800);
    assert_eq!(replay.research().polities, original.research().polities);
    assert_eq!(replay.hash(), original.hash());

    // Ohne die Befehle verläuft die Forschung der Gemeinschaft anders.
    let mut passive = Headless::new(data, config);
    passive.run_days(800);
    assert_ne!(
        passive.research().polities[0],
        original.research().polities[0]
    );
}

// --- Sichtbarkeit in der Chronik ---

#[test]
fn a_human_player_reads_only_their_own_breakthroughs() {
    let breakthroughs = |sim: &Headless| -> Vec<Option<PolityId>> {
        sim.world()
            .resource::<Chronicle>()
            .entries
            .iter()
            .filter(|entry| entry.kind == ChronicleKind::Tech)
            .map(|entry| entry.polity)
            .collect()
    };
    let completed =
        |sim: &Headless, polity: usize| sim.research().polities[polity].completed as usize;

    // Ohne Menschen am Tisch steht jeder Abschluss in der Chronik.
    let mut open = observer(6);
    open.run_days(8 * DAYS_PER_YEAR);
    let entries = breakthroughs(&open);
    let count = open.polities().list.len();
    assert_eq!(
        entries.len(),
        (0..count)
            .map(|polity| completed(&open, polity))
            .sum::<usize>()
    );
    assert!(entries.iter().any(|polity| *polity != entries[0]));

    // Spielt ein Mensch mit, erfährt er aus der Chronik nur von der eigenen Forschung;
    // die anderen forschen trotzdem.
    let mut played = with_player(6);
    played.run_days(8 * DAYS_PER_YEAR);
    let entries = breakthroughs(&played);
    assert!(
        completed(&played, 0) > 0,
        "auch ohne Befehl forscht die Automatik"
    );
    assert_eq!(entries.len(), completed(&played, 0));
    assert!(entries.iter().all(|polity| *polity == Some(PolityId(0))));
    assert!((1..count).all(|polity| completed(&played, polity) > 0));
}
