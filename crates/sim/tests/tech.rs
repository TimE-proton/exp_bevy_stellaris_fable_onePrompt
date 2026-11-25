//! Technologie und Forschung als Ganzes: Tempo, Geld, Wirkungen, Baum, Einblick,
//! Wissenstransfer, Abspaltung, Determinismus.

use sim::GameData;
use sim::chronicle::{Chronicle, ChronicleKind};
use sim::clock::{DAYS_PER_MONTH, DAYS_PER_YEAR};
use sim::command::{CommandOutcome, SimCommand};
use sim::data::TechBranch;
use sim::economy::{Economy, Money, state_reserve, to_money};
use sim::finance::{Finance, Stage};
use sim::harness::{Headless, check_invariants, run_ai_game};
use sim::ids::{PlanetId, PolityId};
use sim::logistics::Logistics;
use sim::state::{SaveGame, SimConfig, SimState, money_drift};
use sim::tech::{Research, TechEffects, TierEstimate, research_month};

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
        world.config.player,
        world.clock.tick,
    );
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

// --- 1. Tempo folgt Wirtschaftskraft und Bevölkerung ---

#[test]
fn more_money_and_more_people_mean_more_points() {
    let data = data();
    let base = start(&data, 2);
    let me = PolityId(0);
    let capital = base.polities.get(me).capital.unwrap();
    // Derselbe Monat, je einmal mit einer einzigen Abweichung.
    let points = |change: &dyn Fn(&mut SimState)| -> f64 {
        let mut world = base.clone();
        world.finance.polities[0].revenue = 1000.0;
        change(&mut world);
        research_step(&data, &mut world);
        world.research.polities[0].points_last_month
    };
    let normal = points(&|_| {});
    assert!(normal > 0.0);
    // Doppelte Einnahmen oder doppelter Etat: doppelte Punkte.
    let richer = points(&|world| world.finance.polities[0].revenue = 2000.0);
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
fn richer_communities_know_at_least_as_much_after_thirty_years() {
    let data = data();
    for seed in [1, 2, 3] {
        let mut sim = observer(seed);
        let count = sim.polities().list.len();
        let mut revenue = vec![0.0f64; count];
        let mut first_done = vec![None; count];
        for month in 1..=30 * 12 {
            sim.run_days(DAYS_PER_MONTH);
            let finance = sim.world().resource::<Finance>();
            for index in 0..count {
                revenue[index] += finance.polities[index].revenue;
                if first_done[index].is_none() && sim.research().polities[index].completed > 0 {
                    first_done[index] = Some(month as f64 / 12.0);
                }
            }
        }
        let research = sim.research();
        let by_revenue = |a: &usize, b: &usize| revenue[*a].total_cmp(&revenue[*b]);
        let richest = (0..count).max_by(by_revenue).unwrap();
        let poorest = (0..count).min_by(by_revenue).unwrap();
        assert!(
            revenue[richest] > revenue[poorest] * 1.1,
            "Seed {seed}: die Einnahmen unterscheiden sich kaum"
        );
        let known = |index: usize| research.known_count(PolityId::from_index(index));
        assert!(
            known(richest) >= known(poorest),
            "Seed {seed}: die reichste Gemeinschaft kennt {} Technologien, die ärmste {}",
            known(richest),
            known(poorest)
        );
        assert!(
            research.polities[richest].points_total > research.polities[poorest].points_total,
            "Seed {seed}: mehr Einnahmen müssen mehr Forschung ergeben"
        );
        // Das Tempo liegt im gewollten Rahmen: Die erste Technologie braucht einige Jahre,
        // und nach dreißig Jahren ist niemand über die zweite, dritte Stufe hinaus.
        for index in 0..count {
            let entry = &research.polities[index];
            let first = first_done[index].expect("in dreißig Jahren wird etwas erforscht");
            assert!(
                (2.5..=7.0).contains(&first),
                "Seed {seed}, G{index}: erste Technologie nach {first:.1} Jahren"
            );
            assert!(
                (3..=9).contains(&entry.completed),
                "Seed {seed}, G{index}: {} Technologien in dreißig Jahren",
                entry.completed
            );
            for branch in TechBranch::ALL {
                assert!(research.tier(&data, PolityId::from_index(index), branch) <= 3);
            }
        }
    }
}

// --- 2. Forschung bewegt Geld, sie schafft und vernichtet keines ---

#[test]
fn research_pay_moves_money_from_the_treasury_to_households() {
    let data = data();
    let mut world = start(&data, 3);
    for state in &mut world.finance.polities {
        state.revenue = 2000.0;
    }
    let before = world.clone();
    research_step(&data, &mut world);
    let share = data.balance.research.budget_share;
    for polity in &world.polities.list {
        let index = polity.id.index();
        let entry = &world.research.polities[index];
        let planets = world.galaxy.planets_of(polity.id).count() as Money;
        // Der Etat ist der Anteil an den Einnahmen; je Welt kann ein Tausendstel fehlen.
        let budget = to_money(share * 2000.0);
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

// --- 4. Regeln des Baums ---

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
    sim.run_days(2 * DAYS_PER_YEAR);
    let on_a = progress(&sim, a);
    assert!(on_a > 0.0 && !sim.research().knows(me, a));
    assert_eq!(progress(&sim, b), 0.0);

    assert_eq!(command(&mut sim, me, target(b)), CommandOutcome::Applied);
    sim.run_days(DAYS_PER_YEAR);
    let on_b = progress(&sim, b);
    assert_eq!(progress(&sim, a), on_a, "Liegengelassenes verfällt nicht");
    assert!(on_b > 0.0);

    assert_eq!(command(&mut sim, me, target(a)), CommandOutcome::Applied);
    sim.run_days(DAYS_PER_YEAR);
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
    sim.run_days(2 * DAYS_PER_YEAR);
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
    let usual = data.balance.research.budget_share;
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

    // Der höchste Etat: Gezahlt wird genau dieser Anteil der geglätteten Einnahmen.
    assert_eq!(
        command(&mut sim, me, SimCommand::SetResearchBudget { share: max }),
        CommandOutcome::Applied
    );
    sim.run_days(DAYS_PER_MONTH);
    let entry = sim.research().get(me).unwrap();
    assert_eq!(entry.budget_share, max);
    let revenue = sim.world().resource::<Finance>().polity(me).revenue;
    let planets = sim.galaxy().planets_of(me).count() as Money;
    assert!(revenue > 0.0);
    let budget = to_money(max * revenue);
    assert!(
        entry.spent_last_month <= budget && entry.spent_last_month >= budget - planets,
        "{} von {budget}",
        entry.spent_last_month
    );
    // Eine KI-Gemeinschaft daneben zahlt den üblichen Anteil ihrer Einnahmen.
    let other = PolityId(1);
    let theirs = sim.research().get(other).unwrap();
    let their_budget = to_money(usual * sim.world().resource::<Finance>().polity(other).revenue);
    let their_planets = sim.galaxy().planets_of(other).count() as Money;
    assert!(
        theirs.spent_last_month <= their_budget
            && theirs.spent_last_month >= their_budget - their_planets
    );
}

// --- 5. Schätzungen ---

#[test]
fn estimates_bracket_the_truth_and_sharpen_with_insight() {
    let data = data();
    let seed = 5;
    let build = || {
        let mut sim = observer(seed);
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
    let mut research = sim.research().clone();
    let spread = data.balance.research.estimate_spread;
    let truth = |research: &Research, target: PolityId| {
        TechBranch::ALL.map(|branch| research.tier(&data, target, branch))
    };

    for viewer in (0..count).map(PolityId::from_index) {
        // Sich selbst kennt jede Gemeinschaft genau.
        let own = research.estimate(&data, viewer, viewer, seed, 3);
        assert_eq!(own, truth(&research, viewer).map(TierEstimate::exact));
        for target in (0..count).map(PolityId::from_index) {
            if viewer == target {
                continue;
            }
            let actual = truth(&research, target);
            for year in [0, 3, 57] {
                let mut wider: Option<[TierEstimate; 5]> = None;
                for step in 0..=20 {
                    let intel = step as f32 / 20.0;
                    research.intel[viewer.index()][target.index()] = intel;
                    let estimate = research.estimate(&data, viewer, target, seed, year);
                    for (range, tier) in estimate.iter().zip(actual) {
                        assert!(
                            range.contains(tier),
                            "Stufe {tier} außerhalb von {range} bei Einblick {intel}"
                        );
                        assert!(range.width() <= 2 * spread);
                    }
                    // Dieselbe Frage im selben Jahr: dieselbe Antwort.
                    assert_eq!(
                        estimate,
                        research.estimate(&data, viewer, target, seed, year)
                    );
                    // Mehr Einblick macht keine Spanne weiter und widerspricht nie dem
                    // früheren Bild.
                    if let Some(wider) = wider {
                        for (narrow, wide) in estimate.iter().zip(wider) {
                            assert!(narrow.width() <= wide.width());
                            assert!(wide.lo <= narrow.lo && narrow.hi <= wide.hi);
                        }
                    }
                    if step == 0 {
                        assert!(estimate.iter().all(|range| range.width() == 2 * spread));
                    }
                    if step == 20 {
                        assert_eq!(estimate, actual.map(TierEstimate::exact));
                    }
                    wider = Some(estimate);
                }
            }
        }
    }

    // Über die Jahre wandert die Lage der Spanne, und jede Betrachterin sieht ihr eigenes Bild.
    let (viewer, target) = (PolityId(0), PolityId(1));
    research.intel[0][1] = 0.5;
    research.intel[2][1] = 0.5;
    let windows: std::collections::BTreeSet<(u8, u8)> = (0..40)
        .map(|year| research.estimate(&data, viewer, target, seed, year)[2])
        .map(|range| (range.lo, range.hi))
        .collect();
    assert!(windows.len() > 1, "die Spanne liegt jedes Jahr gleich");
    assert!(
        (0..40).any(|year| {
            research.estimate(&data, viewer, target, seed, year)
                != research.estimate(&data, PolityId(2), target, seed, year)
        }),
        "zwei Betrachter sehen immer dasselbe"
    );
    assert!(
        (0..40).any(|year| {
            research.estimate(&data, viewer, target, seed, year)
                != research.estimate(&data, viewer, target, seed + 1, year)
        }),
        "der Seed spielt keine Rolle"
    );

    // Eine zweite Partie mit demselben Seed zeigt jedem dasselbe Bild.
    let twin = build();
    assert_eq!(twin.hash(), sim.hash());
    for viewer in (0..count).map(PolityId::from_index) {
        for target in (0..count).map(PolityId::from_index) {
            for year in 0..10 {
                assert_eq!(
                    twin.research().estimate(&data, viewer, target, seed, year),
                    sim.research().estimate(&data, viewer, target, seed, year)
                );
            }
        }
    }
}

#[test]
fn insight_follows_ownership_and_events_and_fades() {
    let data = data();
    let balance = &data.balance.research;
    let mut sim = observer(5);
    let (viewer, target) = (PolityId(0), PolityId(1));
    assert_eq!(sim.research().intel(viewer, target), balance.intel_base);
    assert_eq!(sim.research().intel(viewer, viewer), 1.0);

    // Ein beobachtetes Gefecht: voller Einblick, der über die Jahre verblasst.
    sim.world_mut()
        .resource_mut::<Research>()
        .add_intel(viewer, target, 1.0);
    assert_eq!(sim.research().intel(viewer, target), 1.0);
    let exact = sim.research().estimate(&data, viewer, target, 5, 0);
    assert!(exact.iter().all(|range| range.is_exact()));
    sim.run_days(DAYS_PER_YEAR);
    let after_a_year = sim.research().intel(viewer, target);
    assert!(after_a_year < 1.0 && after_a_year > 0.6, "{after_a_year}");
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
    let estimate = sim.research().estimate(&data, viewer, target, 5, 1);
    assert!(
        estimate
            .iter()
            .all(|range| range.width() < 2 * balance.estimate_spread)
    );
}

// --- 6. Wissenstransfer ---

#[test]
fn knowledge_of_neighbours_makes_a_technology_cheaper_the_nearer_the_more() {
    let data = data();
    let balance = &data.balance.research;
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

    let world_with = |knowers: &[PolityId], at_war: bool| {
        let mut world = base.clone();
        for knower in knowers {
            world.research.grant(&data, *knower, wanted);
        }
        if at_war {
            let pair = (learner.min(nearest), learner.max(nearest));
            world.diplomacy.wars.insert(pair);
        }
        world
    };
    let cost_with = |knowers: &[PolityId], at_war: bool| {
        let mut world = world_with(knowers, at_war);
        research_step(&data, &mut world);
        world.research.cost(&data, learner, wanted)
    };
    let full = data.techs[wanted as usize].cost;
    let alone = cost_with(&[], false);
    let near = cost_with(&[nearest], false);
    let far = cost_with(&[farthest], false);
    let both = cost_with(&[nearest, farthest], false);
    let everyone = cost_with(&others, false);
    assert_eq!(alone, full, "was niemand weiß, kostet den vollen Preis");
    assert!(near < far, "Nähe zählt: {near} gegen {far}");
    assert!(far < alone);
    assert!(both < near);
    assert!(everyone < both);
    assert!(
        everyone > full * (1.0 - balance.spillover_max),
        "der Nachlass bleibt unter seiner Obergrenze"
    );
    // Im Krieg ruht der Austausch mit dem Gegner.
    assert_eq!(cost_with(&[nearest], true), full);
    assert!(cost_with(&[nearest, farthest], true) > both);

    // Der Nachlass beschleunigt die Forschung: bei gleichen Einnahmen früher fertig.
    let months_until_known = |knowers: &[PolityId]| {
        let mut world = world_with(knowers, false);
        world.finance.polities[0].revenue = 3000.0;
        // Ein von außen gesetztes Ziel gilt bis zu seinem Abschluss.
        world.research.polities[0].auto = false;
        world.research.polities[0].target = Some(wanted);
        let mut months = 0;
        while !world.research.knows(learner, wanted) {
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
        let mut galaxy = world.resource_mut::<sim::galaxy::Galaxy>();
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
        research.add_intel(parent, PolityId(1), 0.5);
    }
    let inherited = sim.research().known(parent).to_vec();
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
    sim.run_days(1);

    let polities = sim.polities();
    assert_eq!(polities.list.len(), before + 1, "ein Freistaat entsteht");
    let child = PolityId::from_index(before);
    assert_eq!(sim.galaxy().planet(colony).owner, Some(child));
    let research = sim.research();
    // Jede Liste hat für jede Gemeinschaft einen Eintrag.
    assert!(research.is_consistent(&data, before + 1));
    assert_eq!(research.polities.len(), polities.list.len());
    assert_eq!(research.intel.len(), polities.list.len());
    assert_eq!(check_invariants(sim.world()), Ok(()));
    // Der Freistaat weiß, was das alte Reich wusste, und forscht auf eigene Rechnung weiter.
    let new = research.get(child).unwrap();
    assert_eq!(new.known, inherited);
    assert!(research.knows(child, tech(&data, "waffen_2")));
    assert_eq!(new.effects, TechEffects::of(&data, &inherited));
    assert!(new.effects.yield_factor > 1.0);
    assert_eq!(new.progress[begun as usize], 4321.0);
    assert_eq!(new.budget_share, 0.2);
    assert!(new.auto);
    assert_eq!(new.completed, 0);
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
    assert_eq!(
        sim.world().resource::<Economy>().yield_factor(child),
        entry.effects.yield_factor
    );
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
    assert_ne!(a.research().polities, c.research().polities);
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
    assert_eq!(
        resumed.hash(),
        first_half.hash(),
        "Laden muss den Zustand exakt herstellen"
    );
    resumed.run_days(700);
    assert_eq!(resumed.research().polities, straight.research().polities);
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
    original.issue(me, SimCommand::SetResearchBudget { share: 0.2 });
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
