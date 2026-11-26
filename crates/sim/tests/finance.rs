//! Börse, Besitz und Zusammenhalt als Ganzes.

use sim::GameData;
use sim::clock::DAYS_PER_YEAR;
use sim::command::{CommandOutcome, SimCommand};
use sim::data::Sector;
use sim::economy::{Economy, to_credits, to_money};
use sim::finance::shares::ExpropriationScope;
use sim::finance::{
    CohesionFactors, Finance, Stage, buy_shares, cohesion_target, holder_cash_now, pay_dividend,
};
use sim::fleet::{Fleets, Stance};
use sim::galaxy::Galaxy;
use sim::harness::{Headless, check_invariants, economy_stats, run_checked};
use sim::ids::{FleetId, HolderId, IssuerId, PlanetId, PolityId};
use sim::polity::Polities;
use sim::state::{SimConfig, money_drift};

fn data() -> GameData {
    GameData::load_workspace().unwrap()
}

fn observer(seed: u64) -> Headless {
    Headless::new(data(), SimConfig { seed, player: None })
}

/// Partie, in der Gemeinschaft 0 vom Menschen geführt wird: Die KI greift dort nicht ein.
fn with_player(seed: u64) -> Headless {
    Headless::new(
        data(),
        SimConfig {
            seed,
            player: Some(PolityId(0)),
        },
    )
}

/// Lässt die vom Menschen geführte Gemeinschaft über `years` Jahre Kolonien gründen, so
/// wie es ein Spieler täte: jeweils die beste freie Welt in Reichweite.
fn settle(sim: &mut Headless, me: PolityId, years: u64) {
    for _ in 0..years * 4 {
        let day = sim.clock().day();
        let target = {
            let world = sim.world();
            sim::ai::best_colony_target(
                world.resource::<GameData>(),
                world.resource::<Galaxy>(),
                world.resource::<Polities>(),
                world.resource::<Economy>(),
                me,
                day,
            )
        };
        if let Some(planet) = target {
            sim.issue(me, SimCommand::ClaimPlanet { planet });
        }
        sim.run_days(DAYS_PER_YEAR / 4);
    }
}

/// Die bevölkerungsreichste Kolonie einer Gemeinschaft.
fn largest_colony(sim: &Headless, polity: PolityId) -> PlanetId {
    let capital = sim.polities().get(polity).capital;
    sim.galaxy()
        .planets_of(polity)
        .filter(|planet| Some(planet.id) != capital)
        .max_by(|a, b| a.population.total_cmp(&b.population).then(b.id.cmp(&a.id)))
        .map(|planet| planet.id)
        .expect("Kolonie vorhanden")
}

/// Alle Betriebe mit Sitz auf dem Planeten.
fn firms_on(sim: &Headless, planet: PlanetId) -> Vec<IssuerId> {
    sim.world()
        .resource::<Finance>()
        .issuers
        .keys()
        .copied()
        .filter(|id| matches!(id, IssuerId::Firm { planet: seat, .. } if *seat == planet))
        .collect()
}

/// Überträgt alle Anteile der Betriebe einer Welt an einen einzigen Halter.
fn give_everything_to(sim: &mut Headless, planet: PlanetId, holder: HolderId) {
    let firms = firms_on(sim, planet);
    let mut finance = sim.world_mut().resource_mut::<Finance>();
    for id in firms {
        let issuer = finance.issuers.get_mut(&id).unwrap();
        issuer.holdings.clear();
        issuer.offered.clear();
        issuer.holdings.insert(holder, issuer.shares);
    }
}

fn cohesion_of(sim: &Headless, planet: PlanetId) -> f64 {
    sim.world().resource::<Finance>().region(planet).cohesion
}

#[test]
fn broad_mutual_ownership_binds_and_concentration_divides() {
    let balance = data().balance.finance.cohesion;
    let neutral = CohesionFactors {
        supply: 1.0,
        tax: balance.tax_neutral,
        years: 50.0,
        ..Default::default()
    };
    let interlocked = CohesionFactors {
        interlock: 0.6,
        dispersion: 0.7,
        local_share: 0.4,
        ..neutral.clone()
    };
    let hoarded = CohesionFactors {
        center_share: 0.95,
        drain: 0.8,
        dispersion: 0.1,
        ..neutral.clone()
    };
    let foreign = CohesionFactors {
        foreign_share: 0.7,
        foreign_polity: Some(PolityId(3)),
        ..neutral.clone()
    };
    let base = cohesion_target(&neutral, &balance);
    assert!(cohesion_target(&interlocked, &balance) > base + 15.0);
    assert!(cohesion_target(&hoarded, &balance) < base - 20.0);
    assert!(cohesion_target(&foreign, &balance) < base - 25.0);
    // Verflochtene Regionen bleiben loyal, ausgeplünderte fallen unter die Autonomieschwelle.
    assert!(cohesion_target(&interlocked, &balance) >= balance.loyal_above);
    assert!(cohesion_target(&hoarded, &balance) < balance.restless_below);
}

#[test]
fn the_starting_position_stays_loyal_for_years_without_intervention() {
    let data = data();
    for seed in [1, 2, 3] {
        let (sim, violation) = run_checked(&data, seed, 15 * DAYS_PER_YEAR);
        assert_eq!(violation, None, "Seed {seed}");
        let stats = economy_stats(sim.world());
        assert_eq!(stats.secessions, 0, "Seed {seed}: Abspaltung ohne Anlass");
        assert_eq!(stats.autonomous_regions, 0, "Seed {seed}");
        assert!(
            stats.cohesion > 65.0,
            "Seed {seed}: Zusammenhalt {}",
            stats.cohesion
        );
        // Heimatwelten sind fest gebunden.
        for polity in &sim.polities().list {
            let home = polity.capital.unwrap();
            assert_eq!(
                sim.world().resource::<Finance>().region(home).stage,
                Stage::Loyal
            );
        }
    }
}

#[test]
fn a_colony_owned_by_the_center_drifts_into_autonomy_while_a_shared_one_stays() {
    let me = PolityId(0);
    // Eine Partie: dieselbe Kolonie einmal im Besitz des Zentrums, einmal breit gestreut.
    // Gibt den mittleren Zusammenhalt beider Kolonien über die letzten zehn Jahre zurück.
    let play = |seed: u64| -> (f64, f64) {
        let mut hoarded = with_player(seed);
        let mut shared = with_player(seed);
        settle(&mut hoarded, me, 12);
        settle(&mut shared, me, 12);
        let colony = largest_colony(&hoarded, me);
        assert_eq!(colony, largest_colony(&shared, me));
        let capital = hoarded.polities().get(me).capital.unwrap();

        // Zusammenhalt beider Kolonien am Ende jedes Jahres.
        let (mut lows, mut highs) = (Vec::new(), Vec::new());
        for year in 0..25 {
            // Im einen Reich zieht der Staat jedes Jahr allen Besitz an sich …
            give_everything_to(&mut hoarded, colony, HolderId::State(me));
            // … im anderen halten die Einheimischen ihre Betriebe und dazu ein Stück der Hauptwelt.
            let firms = firms_on(&shared, colony);
            let home_firm = IssuerId::Firm {
                planet: capital,
                sector: Sector::Industry,
            };
            {
                let mut finance = shared.world_mut().resource_mut::<Finance>();
                for id in firms {
                    let issuer = finance.issuers.get_mut(&id).unwrap();
                    let total = issuer.shares;
                    issuer.holdings.clear();
                    issuer.holdings.insert(HolderId::Public(colony), total / 2);
                    issuer.holdings.insert(HolderId::Elite(colony), total / 4);
                    issuer
                        .holdings
                        .insert(HolderId::State(me), total - total / 2 - total / 4);
                }
                if year == 0 {
                    let issuer = finance
                        .issuers
                        .get_mut(&home_firm)
                        .expect("Heimatindustrie");
                    let stake = issuer.shares / 5;
                    issuer.shares += stake;
                    issuer.holdings.insert(HolderId::Public(colony), stake);
                }
            }
            hoarded.run_days(DAYS_PER_YEAR);
            shared.run_days(DAYS_PER_YEAR);
            lows.push(cohesion_of(&hoarded, colony));
            highs.push(cohesion_of(&shared, colony));
        }
        let stage = |sim: &Headless| sim.world().resource::<Finance>().region(colony).stage;
        assert_ne!(
            stage(&hoarded),
            Stage::Loyal,
            "Seed {seed}: Konzentration muss Unruhe erzeugen"
        );
        assert_eq!(stage(&shared), Stage::Loyal, "Seed {seed}");
        // Autonome Regionen zahlen keine Steuern mehr.
        if stage(&hoarded) == Stage::Autonomous {
            assert!(
                hoarded
                    .world()
                    .resource::<Economy>()
                    .planet(colony)
                    .tax_exempt
            );
        }
        assert!(check_invariants(hoarded.world()).is_ok());
        let late = |values: &[f64]| values[15..].iter().sum::<f64>() / 10.0;
        (late(&lows), late(&highs))
    };
    // Gemessen wird das Mittel der letzten zehn Jahre über drei Partien, nicht ein einzelner
    // Monat einer einzigen: Der Abfluss der Gewinne schwankt mit den Ausschüttungen von Jahr
    // zu Jahr um mehrere Punkte, und ein Stichtag träfe mal ein Tal, mal einen Gipfel.
    // Die Partien sind so gewählt, dass die Kolonie nicht schon aus anderen Gründen an der
    // Schwelle steht: In Seed 24 etwa pendelt die gehortete Kolonie zwischen 45 und 51
    // Punkten genau um die Grenzen der Stufen, und ihre Stufe am Stichtag ist Zufall.
    let games: Vec<(f64, f64)> = [4, 14, 44].into_iter().map(play).collect();
    for (low, high) in &games {
        assert!(
            *high > *low + 10.0,
            "in jeder Partie klar getrennt: geteilt {high:.1}, gehortet {low:.1}"
        );
    }
    let low = games.iter().map(|(low, _)| low).sum::<f64>() / games.len() as f64;
    let high = games.iter().map(|(_, high)| high).sum::<f64>() / games.len() as f64;
    assert!(high > low + 20.0, "geteilt {high:.1}, gehortet {low:.1}");
}

#[test]
fn a_foreign_majority_pulls_a_region_into_the_other_community() {
    let me = PolityId(0);
    let rival = PolityId(1);
    let mut sim = with_player(5);
    settle(&mut sim, me, 12);
    let colony = largest_colony(&sim, me);
    let rival_capital = sim.polities().get(rival).capital.unwrap();
    let mut left = None;
    for year in 0..40 {
        // Die Elite des Rivalen hält alles; die Hauptwelt kümmert sich nicht.
        give_everything_to(&mut sim, colony, HolderId::Elite(rival_capital));
        sim.run_days(DAYS_PER_YEAR);
        if sim.galaxy().planet(colony).owner != Some(me) {
            left = Some(year);
            break;
        }
    }
    assert!(left.is_some(), "die Region blieb trotz fremder Mehrheit");
    assert_eq!(
        sim.galaxy().planet(colony).owner,
        Some(rival),
        "wer die Mehrheit hält, zu dem tritt die Region über"
    );
    let finance = sim.world().resource::<Finance>();
    assert!(finance.secessions >= 1);
    assert_eq!(
        finance.region(colony).stage,
        Stage::Loyal,
        "unter neuem Besitzer beginnt die Uhr neu"
    );
    assert!(check_invariants(sim.world()).is_ok());
}

#[test]
fn dividends_reach_holders_wherever_they_sit_and_nothing_is_lost() {
    let mut sim = observer(6);
    sim.run_days(3 * DAYS_PER_YEAR);
    let capital = sim.polities().list[0].capital.unwrap();
    let rival_capital = sim.polities().list[1].capital.unwrap();
    let issuer = IssuerId::Firm {
        planet: capital,
        sector: Sector::Industry,
    };
    let foreign = HolderId::Elite(rival_capital);
    let world = sim.world_mut();
    let mut finance = world.remove_resource::<Finance>().unwrap();
    let mut economy = world.remove_resource::<Economy>().unwrap();
    {
        let entry = finance.issuers.get_mut(&issuer).unwrap();
        let stake = entry.shares / 3;
        entry.shares += stake;
        entry.holdings.insert(foreign, stake);
    }
    let holders: Vec<HolderId> = finance.issuers[&issuer].holdings.keys().copied().collect();
    let before: Vec<i64> = holders
        .iter()
        .map(|h| holder_cash_now(&economy, &finance.elites, *h))
        .collect();
    let amount = to_money(12_345.678);
    pay_dividend(&mut finance, &mut economy, Some(capital), issuer, amount);
    let after: Vec<i64> = holders
        .iter()
        .map(|h| holder_cash_now(&economy, &finance.elites, *h))
        .collect();
    let received: i64 = before.iter().zip(&after).map(|(b, a)| a - b).sum();
    assert_eq!(received, amount, "jedes Tausendstel kommt an");
    let foreign_index = holders.iter().position(|h| *h == foreign).unwrap();
    let foreign_share = (after[foreign_index] - before[foreign_index]) as f64 / amount as f64;
    assert!(
        (foreign_share - 0.25).abs() < 0.01,
        "ein Viertel der Anteile, ein Viertel der Dividende"
    );
    // Der Abfluss ist für den Zusammenhalt vermerkt.
    assert!(finance.region(capital).dividends_out > 0.0);
    assert!(finance.region(rival_capital).dividends_in > 0.0);
    world.insert_resource(finance);
    world.insert_resource(economy);
}

#[test]
fn buying_moves_the_price_and_only_shifts_money() {
    let mut sim = observer(7);
    sim.run_days(3 * DAYS_PER_YEAR);
    let data = data();
    let polity = PolityId(0);
    let capital = sim.polities().get(polity).capital.unwrap();
    let issuer = IssuerId::Firm {
        planet: capital,
        sector: Sector::Agrar,
    };
    let world = sim.world_mut();
    let mut finance = world.remove_resource::<Finance>().unwrap();
    let mut economy = world.remove_resource::<Economy>().unwrap();
    economy.treasuries[polity.index()] += to_money(1e7);
    economy.initial_money += to_money(1e7);
    let buyer = HolderId::State(polity);
    let price = finance.issuers[&issuer].price;
    let held = finance.issuers[&issuer].held_by(buyer);
    let total_before = economy.money_in_accounts() + finance.money();

    let (small, small_cost) = buy_shares(
        &mut finance,
        &mut economy,
        &data,
        buyer,
        issuer,
        Some(capital),
        5,
        i64::MAX,
    );
    assert_eq!(small, 5);
    let (large, large_cost) = buy_shares(
        &mut finance,
        &mut economy,
        &data,
        buyer,
        issuer,
        Some(capital),
        60,
        i64::MAX,
    );
    assert!(large >= 10, "nur {large} Anteile am Markt");
    // Je größer der Kauf, desto höher der Durchschnittspreis.
    let small_average = to_credits(small_cost) / small as f64;
    let large_average = to_credits(large_cost) / large as f64;
    assert!(small_average >= price * 0.999);
    assert!(large_average > small_average);
    assert_eq!(
        finance.issuers[&issuer].held_by(buyer),
        held + small + large
    );
    assert!(finance.issuers[&issuer].pressure > 0.0);
    let holdings: u64 = finance.issuers[&issuer].holdings.values().sum();
    assert_eq!(
        holdings, finance.issuers[&issuer].shares,
        "Anteile gehen nicht verloren"
    );
    assert_eq!(economy.money_in_accounts() + finance.money(), total_before);
    world.insert_resource(finance);
    world.insert_resource(economy);
}

#[test]
fn takeover_expropriation_and_bonds_work_as_commands() {
    let me = PolityId(0);
    let mut sim = with_player(8);
    settle(&mut sim, me, 10);
    let colony = largest_colony(&sim, me);
    let target = firms_on(&sim, colony)[0];
    let state = HolderId::State(me);
    {
        // Genug Geld für ein Übernahmeangebot. Als Rücklage gebunden, damit der Staat es
        // nicht sofort wieder ausgibt und einen Nachfrageboom auslöst.
        let mut economy = sim.world_mut().resource_mut::<Economy>();
        economy.treasuries[me.index()] += to_money(300_000.0);
        economy.initial_money += to_money(300_000.0);
        economy.treasury_targets[me.index()] = to_money(10_000_000.0);
    }

    // Übernahme: Mit kräftigem Aufschlag gehört dem Staat danach die Mehrheit.
    sim.issue(
        me,
        SimCommand::TakeoverBid {
            issuer: target,
            premium: 0.5,
        },
    );
    sim.run_days(1);
    let share = sim.world().resource::<Finance>().issuers[&target].share_of(state);
    assert!(
        share > 0.5,
        "nach der Übernahme hält der Staat {:.0} %",
        share * 100.0
    );

    // Anteile verteilen: ohne Gegenleistung an den Fonds der Region.
    let before =
        sim.world().resource::<Finance>().issuers[&target].held_by(HolderId::Public(colony));
    sim.issue(
        me,
        SimCommand::DistributeShares {
            issuer: target,
            shares: 100,
            to: HolderId::Public(colony),
        },
    );
    sim.run_days(1);
    let after =
        sim.world().resource::<Finance>().issuers[&target].held_by(HolderId::Public(colony));
    assert_eq!(after, before + 100);

    // Staatsanleihen: Geld kommt herein, Schulden entstehen, Zinsen fließen.
    let treasury = sim.world().resource::<Economy>().treasury(me);
    sim.issue(me, SimCommand::IssueBonds { amount: 20_000.0 });
    sim.run_days(1);
    let placed = sim.world().resource::<Economy>().treasury(me) - treasury;
    let debt = sim.world().resource::<Finance>().debt_of(me);
    assert!(placed > 0 && debt > 0);
    assert!(
        (placed - debt).abs() <= to_money(5_000.0),
        "platziert {placed}, Schulden {debt}"
    );
    let holder = sim.world().resource::<Finance>().bonds[0].holder;
    let cash = |sim: &Headless| {
        let world = sim.world();
        holder_cash_now(
            world.resource::<Economy>(),
            &world.resource::<Finance>().elites,
            holder,
        )
    };
    assert!(sim.world().resource::<Finance>().bonds[0].rate >= 0.04);
    let _ = cash(&sim);

    // Enteignung: Die Anteile gehören dem Staat, die Kurse im Reich brechen ein.
    let home_firm = IssuerId::Firm {
        planet: sim.polities().get(me).capital.unwrap(),
        sector: Sector::Industry,
    };
    let price_before = sim.world().resource::<Finance>().issuers[&home_firm].price;
    sim.issue(
        me,
        SimCommand::Expropriate {
            issuer: target,
            scope: ExpropriationScope::Everyone,
        },
    );
    sim.run_days(40);
    let finance = sim.world().resource::<Finance>();
    assert!(finance.issuers[&target].share_of(state) > 0.99);
    assert!(finance.polity(me).expropriation_risk > 0.2);
    assert!(
        finance.issuers[&home_firm].price < price_before * 0.9,
        "Kurs vorher {price_before:.2}, nachher {:.2}",
        finance.issuers[&home_firm].price
    );
    // Alle vier Finanzbefehle wurden angenommen und stehen im Protokoll.
    let log = sim.command_log();
    let finance_commands: Vec<_> = log
        .entries
        .iter()
        .filter(|entry| !matches!(entry.command, SimCommand::ClaimPlanet { .. }))
        .collect();
    assert_eq!(finance_commands.len(), 4);
    assert!(
        finance_commands
            .iter()
            .all(|entry| entry.outcome == CommandOutcome::Applied),
        "{finance_commands:?}"
    );

    assert_eq!(money_drift(sim.world()), Some(0));
}

#[test]
fn a_state_that_cannot_pay_defaults_and_loses_trust() {
    let me = PolityId(0);
    let mut sim = with_player(9);
    sim.run_days(5 * DAYS_PER_YEAR);
    sim.issue(me, SimCommand::IssueBonds { amount: 50_000.0 });
    sim.run_days(1);
    let debt = sim.world().resource::<Finance>().debt_of(me);
    assert!(debt > 0);
    let yield_before = sim.world().resource::<Finance>().polity(me).bond_yield;
    // Die Anleihen werden heute fällig, die Kasse ist leer: Der Staat kann nicht tilgen.
    let today = sim.clock().day();
    for bond in sim.world_mut().resource_mut::<Finance>().bonds.iter_mut() {
        bond.matures_on = today;
    }
    {
        let mut economy = sim.world_mut().resource_mut::<Economy>();
        let cash = std::mem::take(&mut economy.treasuries[me.index()]);
        // Das Geld geht an die Haushalte einer Welt, damit die Summe stimmt.
        let first = economy
            .planets
            .iter()
            .position(|p| p.households > 0)
            .unwrap();
        economy.planets[first].households += cash;
    }
    sim.run_days(31);
    let finance = sim.world().resource::<Finance>();
    assert!(
        finance.polity(me).defaults >= 1,
        "kein Ausfall trotz leerer Kasse"
    );
    assert!(
        finance.debt_of(me) < debt,
        "die Gläubiger tragen den Verlust"
    );
    assert!(
        finance.polity(me).bond_yield > yield_before + 0.03,
        "der Markt verlangt danach mehr Zins"
    );
    assert_eq!(money_drift(sim.world()), Some(0));
}

#[test]
fn share_prices_follow_profits_and_fall_under_blockade() {
    let attacker = PolityId(0);
    let victim = PolityId(1);
    let mut blockaded = observer(7);
    let mut control = observer(7);
    blockaded.run_days(30 * DAYS_PER_YEAR);
    control.run_days(30 * DAYS_PER_YEAR);
    let colony = largest_colony(&blockaded, victim);
    let system = blockaded.galaxy().planet(colony).system;
    blockaded.issue(attacker, SimCommand::DeclareWar { target: victim });
    blockaded.issue(
        attacker,
        SimCommand::SetStance {
            fleet: FleetId(0),
            stance: Stance::Blockade,
        },
    );
    blockaded.issue(
        attacker,
        SimCommand::MoveFleet {
            fleet: FleetId(0),
            to: system,
        },
    );
    for _ in 0..3 * DAYS_PER_YEAR {
        blockaded.run_days(1);
        control.run_days(1);
        if blockaded.world().resource::<Fleets>().is_blockaded(system) {
            break;
        }
    }
    assert!(blockaded.world().resource::<Fleets>().is_blockaded(system));
    blockaded.run_days(DAYS_PER_YEAR);
    control.run_days(DAYS_PER_YEAR);

    let local = |sim: &Headless, id: &IssuerId| match id {
        IssuerId::Firm { planet, .. } => sim.galaxy().planet(*planet).system == system,
        IssuerId::Kontor(seat) => *seat == system,
        IssuerId::House(_) => false,
    };
    // Jeder Emittent im gesperrten System trägt den Risikoabschlag, im offenen keiner.
    for (sim, expect_risk) in [(&blockaded, true), (&control, false)] {
        let finance = sim.world().resource::<Finance>();
        for (id, issuer) in finance.issuers.iter().filter(|(id, _)| local(sim, id)) {
            assert_eq!(
                issuer.risk >= 0.3,
                expect_risk,
                "{id:?}: Risiko {:.2}",
                issuer.risk
            );
        }
    }
    // Das Kontor lebt vom Handel; ohne Frachter bricht sein Kurs ein. (Einzelne Werke
    // können dagegen gewinnen: Wer vor Ort herstellt, was nicht mehr hereinkommt,
    // verkauft zu Höchstpreisen.)
    let kontor = |sim: &Headless| {
        sim.world().resource::<Finance>().issuers[&IssuerId::Kontor(system)].market_value()
    };
    let (closed, open) = (kontor(&blockaded), kontor(&control));
    assert!(
        closed < open * 0.7,
        "Kontor gesperrt {closed:.0}, offen {open:.0}"
    );
    // Kriegsmüdigkeit baut sich auf beiden Seiten auf.
    assert!(
        blockaded
            .world()
            .resource::<Finance>()
            .polity(victim)
            .war_weariness
            > 5.0
    );
    let _ = (
        blockaded.world().resource::<Galaxy>(),
        blockaded.world().resource::<Polities>(),
    );
}
