//! Besitzwechsel, Gründung, Ende und Unterstellung von Gemeinschaften in der laufenden
//! Partie, und die beiden Enden. Die reinen Regeln (Entscheidung über das Ende, Häupter und
//! Verbünde, Flotten im Gefecht) prüfen die Tests neben dem Code; hier geht es darum, dass
//! in einer ganzen Welt nichts verloren geht und nichts auf eine beendete Gemeinschaft zeigt.
//!
//! Wo ein Vorgang gegen einen anderen gestellt wird, laufen Zwillinge: dieselbe Partie,
//! bis zum Eingriff bitgleich.

use sim::GameData;
use sim::chronicle::{Chronicle, ChronicleKind};
use sim::clock::{DAYS_PER_MONTH, DAYS_PER_YEAR, TICKS_PER_DAY};
use sim::command::{CommandOutcome, SimCommand};
use sim::data::ShipClass;
use sim::diplomacy::Diplomacy;
use sim::economy::{Economy, Money};
use sim::endgame::{Endgame, Ending, Path, PlayerFate};
use sim::finance::{
    Finance, Secession, Stage, apply_secession, ownership_of_region, polity_of_issuer,
};
use sim::fleet::{BattleOutcome, Battles, Fleets};
use sim::galaxy::Galaxy;
use sim::harness::{Headless, check_invariants};
use sim::ids::{FleetId, HolderId, IssuerId, PlanetId, PolityId};
use sim::logistics::Logistics;
use sim::polity::{Founding, Polities, TransferTerms};
use sim::rng::{Domain, Rng};
use sim::state::{SaveGame, SimConfig, money_drift};
use sim::tech::Research;
use std::collections::BTreeMap;
use std::sync::OnceLock;

/// Die Spieldaten des Workspace, einmal geladen.
fn data() -> &'static GameData {
    static DATA: OnceLock<GameData> = OnceLock::new();
    DATA.get_or_init(|| GameData::load_workspace().unwrap())
}

fn observer(seed: u64) -> Headless {
    Headless::new(data().clone(), SimConfig { seed, player: None })
}

fn with_player(seed: u64, player: PolityId) -> Headless {
    Headless::new(
        data().clone(),
        SimConfig {
            seed,
            player: Some(player),
        },
    )
}

/// Dieselbe Partie noch einmal, bitgleich.
fn twin(sim: &Headless) -> Headless {
    let copy = Headless::from_save(data().clone(), sim.save());
    assert_eq!(copy.hash(), sim.hash());
    copy
}

fn finance(sim: &Headless) -> &Finance {
    sim.world().resource::<Finance>()
}

fn economy(sim: &Headless) -> &Economy {
    sim.world().resource::<Economy>()
}

fn diplomacy(sim: &Headless) -> &Diplomacy {
    sim.world().resource::<Diplomacy>()
}

fn endgame(sim: &Headless) -> &Endgame {
    sim.world().resource::<Endgame>()
}

fn fleets(sim: &Headless) -> &Fleets {
    sim.world().resource::<Fleets>()
}

fn battles(sim: &Headless) -> &Battles {
    sim.world().resource::<Battles>()
}

fn logistics(sim: &Headless) -> &Logistics {
    sim.world().resource::<Logistics>()
}

fn player(sim: &Headless) -> Option<PolityId> {
    sim.world().resource::<SimConfig>().player
}

fn treasury(sim: &Headless, polity: PolityId) -> Money {
    economy(sim).treasury(polity)
}

fn capital(sim: &Headless, polity: PolityId) -> PlanetId {
    sim.polities()
        .get(polity)
        .capital
        .expect("die Gemeinschaft hat eine Hauptwelt")
}

/// Welten einer Gemeinschaft außer ihrer Hauptwelt, die bevölkerungsreichste zuerst.
fn colonies(sim: &Headless, polity: PolityId) -> Vec<PlanetId> {
    let home = sim.polities().get(polity).capital;
    let mut worlds: Vec<_> = sim
        .galaxy()
        .planets_of(polity)
        .filter(|planet| Some(planet.id) != home)
        .collect();
    worlds.sort_by(|a, b| b.population.total_cmp(&a.population).then(a.id.cmp(&b.id)));
    worlds.into_iter().map(|planet| planet.id).collect()
}

fn planets_of(sim: &Headless, polity: PolityId) -> Vec<PlanetId> {
    sim.galaxy()
        .planets_of(polity)
        .map(|planet| planet.id)
        .collect()
}

fn population_of_planet(sim: &Headless, planet: PlanetId) -> f64 {
    sim.galaxy().planet(planet).population
}

/// Bevölkerung aller Welten, in fester Reihenfolge summiert.
fn total_population(sim: &Headless) -> f64 {
    sim.galaxy()
        .planets
        .iter()
        .map(|planet| planet.population)
        .sum()
}

/// Ausgegebene Anteile je Emittent.
fn share_totals(sim: &Headless) -> BTreeMap<IssuerId, u64> {
    finance(sim)
        .issuers
        .iter()
        .map(|(id, issuer)| (*id, issuer.shares))
        .collect()
}

/// Was in jeder Lage gelten muss: Die Geldsumme stimmt, bei jedem Emittenten ergeben die
/// Bestände der Halter genau die ausgegebenen Anteile, und die Invarianten halten.
fn assert_sound(sim: &Headless, when: &str) {
    assert_eq!(money_drift(sim.world()), Some(0), "{when}: Geldsumme");
    for (id, issuer) in &finance(sim).issuers {
        let held: u64 = issuer.holdings.values().sum();
        assert_eq!(held, issuer.shares, "{when}: Anteile von {id:?}");
        for (holder, offered) in &issuer.offered {
            assert!(
                *offered <= issuer.held_by(*holder),
                "{when}: {holder:?} bietet mehr an, als er hält"
            );
        }
    }
    assert_eq!(check_invariants(sim.world()), Ok(()), "{when}");
}

/// Stand vor einem Eingriff, gegen den danach verglichen wird.
struct Before {
    population: f64,
    shares: BTreeMap<IssuerId, u64>,
    money: Money,
}

impl Before {
    fn of(sim: &Headless) -> Self {
        assert_sound(sim, "vor dem Eingriff");
        Self {
            population: total_population(sim),
            shares: share_totals(sim),
            money: all_money(sim),
        }
    }

    /// Der Eingriff hat nichts geschaffen und nichts vernichtet: kein Geld, keinen Anteil,
    /// keinen Menschen.
    fn assert_conserved(&self, sim: &Headless, when: &str) {
        assert_sound(sim, when);
        assert_eq!(all_money(sim), self.money, "{when}: Geld");
        assert_eq!(share_totals(sim), self.shares, "{when}: Anteile");
        assert_eq!(total_population(sim), self.population, "{when}: Menschen");
    }
}

/// Alles Geld der Partie: Konten der Wirtschaft, Handelshäuser, Eliten.
fn all_money(sim: &Headless) -> Money {
    economy(sim).money_in_accounts() + logistics(sim).money() + finance(sim).money()
}

fn end_entries(sim: &Headless) -> usize {
    sim.world()
        .resource::<Chronicle>()
        .entries
        .iter()
        .filter(|entry| entry.kind == ChronicleKind::End)
        .count()
}

/// Gibt einen Befehl, lässt ihn im nächsten Takt anwenden und liefert das Ergebnis.
fn command(sim: &mut Headless, polity: PolityId, command: SimCommand) -> CommandOutcome {
    sim.issue(polity, command);
    sim.tick();
    sim.command_log().entries.last().unwrap().outcome.clone()
}

fn rejection(sim: &mut Headless, polity: PolityId, issued: SimCommand) -> String {
    match command(sim, polity, issued.clone()) {
        CommandOutcome::Rejected(reason) => reason,
        CommandOutcome::Applied => panic!("{issued:?} hätte abgelehnt werden müssen"),
    }
}

/// Bedingungen einer erzwungenen Übernahme, wie sie eine Eroberung stellt.
fn forced_terms(to: PolityId) -> TransferTerms {
    TransferTerms {
        state_shares_to: HolderId::State(to),
        state_share: 1.0,
        cohesion: 30.0,
        stage: Stage::Restless,
        shock: 0.8,
        elite_flight: 0.5,
        path: Path::Forced,
    }
}

fn secede(
    sim: &mut Headless,
    planet: PlanetId,
    from: PolityId,
    joins: Option<PolityId>,
) -> Option<PolityId> {
    let tick = sim.clock().tick;
    sim.succession(|succession| {
        apply_secession(
            &Secession {
                planet,
                from,
                joins,
            },
            succession,
            tick,
        )
    })
}

/// Gründet aus einer Welt eine neue Gemeinschaft, ohne die Schwellen der Abspaltung.
fn found_free_state(sim: &mut Headless, planet: PlanetId) -> PolityId {
    let tick = sim.clock().tick;
    let parent = sim.galaxy().planet(planet).owner.unwrap();
    let founding = Founding::free_state(sim.polities().get(parent), sim.galaxy().planet(planet));
    let terms = TransferTerms::secession(planet, &data().balance.finance.cohesion, Path::Own);
    sim.succession(|succession| {
        let child = succession.spawn(parent, founding, tick)?;
        succession.transfer_planet(planet, child, terms, tick)?;
        Ok::<_, String>(child)
    })
    .unwrap()
}

// --- 2. Nichts geht verloren ----------------------------------------------------------------

/// Eine laufende Partie nach zwölf Jahren, in der eine Gemeinschaft alles hat, was bei
/// ihrem Ende zu regeln ist: einen Krieg, Schulden, einen Bauauftrag, eine Flotte auf dem
/// Marsch. Gibt die Partie, diese Gemeinschaft, ihre größte Kolonie, eine zweite und eine
/// dritte Gemeinschaft (die Kriegsgegnerin) zurück.
fn busy_game(seed: u64) -> (Headless, PolityId, PlanetId, PolityId, PolityId) {
    let mut sim = observer(seed);
    sim.run_days(12 * DAYS_PER_YEAR);
    let needed = data().balance.finance.cohesion.independence_min_population;
    // Die Gemeinschaft mit der größten Kolonie der Galaxie.
    let (from, colony) = sim
        .polities()
        .alive()
        .filter_map(|polity| colonies(&sim, polity.id).first().map(|c| (polity.id, *c)))
        .max_by(|a, b| {
            population_of_planet(&sim, a.1)
                .total_cmp(&population_of_planet(&sim, b.1))
                .then(b.0.cmp(&a.0))
        })
        .expect("nach zwölf Jahren gibt es Kolonien");
    assert!(
        population_of_planet(&sim, colony) >= needed,
        "Seed {seed}: die größte Kolonie ist zu klein für einen eigenen Staat"
    );
    let others: Vec<PolityId> = sim
        .polities()
        .alive()
        .map(|polity| polity.id)
        .filter(|id| *id != from)
        .collect();
    let (into, third) = (others[0], others[1]);

    sim.issue(from, SimCommand::DeclareWar { target: third });
    sim.issue(from, SimCommand::IssueBonds { amount: 20_000.0 });
    sim.issue(
        from,
        SimCommand::BuildClass {
            planet: capital(&sim, from),
            class: ShipClass::Corvette,
        },
    );
    let fleet = fleets(&sim).of(from).next().expect("Startflotte").id;
    let home = fleets(&sim).list[fleet.index()].system;
    let next_door = sim.galaxy().neighbors(home)[0].system;
    sim.issue(
        from,
        SimCommand::MoveFleet {
            fleet,
            to: next_door,
        },
    );
    sim.run_days(1);
    assert!(diplomacy(&sim).at_war(from, third), "Seed {seed}");
    assert!(finance(&sim).debt_of(from) > 0, "Seed {seed}");
    assert_eq!(fleets(&sim).orders_of(from).count(), 1, "Seed {seed}");
    assert!(treasury(&sim, from) > 0, "Seed {seed}");
    (sim, from, colony, into, third)
}

#[test]
fn succession_conserves() {
    for seed in [3, 11, 20] {
        let (base, from, colony, into, third) = busy_game(seed);
        let tick = base.clock().tick;
        let day = base.clock().day();
        let gap = 1.0 - diplomacy(&base).affinity(from, into);
        let count = base.polities().list.len();
        let two_years = 2 * DAYS_PER_YEAR;

        // Abspaltung in eine neue Gemeinschaft.
        {
            let mut sim = twin(&base);
            let before = Before::of(&sim);
            let child = secede(&mut sim, colony, from, None).expect("ein Freistaat entsteht");
            before.assert_conserved(&sim, "Abspaltung");
            assert_eq!(child, PolityId::from_index(count));
            assert_eq!(sim.polities().list.len(), count + 1);
            let state = sim.polities().get(child);
            assert!(state.alive && state.capital == Some(colony));
            assert_eq!(sim.galaxy().planet(colony).owner, Some(child));
            assert!(sim.polities().get(from).alive);
            // Steuersatz, Kultur und Wissen des Reichs, das sie verlässt; weder Kasse noch
            // Schulden, frei und im Frieden.
            assert_eq!(
                economy(&sim).tax_rates[child.index()],
                economy(&sim).tax_rates[from.index()]
            );
            assert_eq!(
                diplomacy(&sim).culture_of(child),
                diplomacy(&sim).culture_of(from)
            );
            assert_eq!(diplomacy(&sim).affinity(child, from), 1.0);
            let research = sim.world().resource::<Research>();
            assert_eq!(research.known(child), research.known(from));
            assert!(research.known_count(child) > 0);
            assert_eq!(treasury(&sim, child), 0);
            assert_eq!(finance(&sim).debt_of(child), 0);
            assert!(diplomacy(&sim).is_free(child) && !diplomacy(&sim).is_at_war(child));
            assert_eq!(fleets(&sim).of(child).count(), 0);
            // Die Gründungswelt ist der Neuen eigen; gezählt wird die Gründung.
            assert_eq!(endgame(&sim).path_of(colony), Path::Own);
            assert_eq!(endgame(&sim).free_states, 1);
            assert_eq!(finance(&sim).secessions, 1);
            assert_eq!(finance(&sim).region(colony).stage, Stage::Loyal);
            assert_eq!(finance(&sim).region(colony).owner_seen, Some(child));
            sim.run_days(two_years);
            assert_sound(&sim, "zwei Jahre nach der Abspaltung");
            assert!(sim.polities().get(child).alive);
        }

        // Übertritt einer Region zu einer bestehenden Gemeinschaft.
        {
            let mut sim = twin(&base);
            let before = Before::of(&sim);
            let people = population_of_planet(&sim, colony);
            assert_eq!(secede(&mut sim, colony, from, Some(into)), Some(into));
            before.assert_conserved(&sim, "Übertritt");
            assert_eq!(sim.polities().list.len(), count);
            assert_eq!(sim.galaxy().planet(colony).owner, Some(into));
            assert_eq!(endgame(&sim).path_of(colony), Path::Joined);
            assert_eq!(endgame(&sim).absorbed_pop[colony.index()], people);
            assert_eq!(endgame(&sim).gap[colony.index()], gap);
            assert_eq!(endgame(&sim).free_states, 0, "kein neuer Staat");
            assert!(sim.polities().get(from).alive && diplomacy(&sim).at_war(from, third));
            sim.run_days(two_years);
            assert_sound(&sim, "zwei Jahre nach dem Übertritt");
        }

        // Was beide Arten des Endes gemeinsam haben, gemessen vor dem Eingriff.
        let worlds = planets_of(&base, from);
        let seat = capital(&base, from);
        let cash = treasury(&base, from) + treasury(&base, into);
        let debt = finance(&base).debt_of(from) + finance(&base).debt_of(into);
        let own_fleets = fleets(&base).of(into).count();
        let passing_fleets = fleets(&base).of(from).count();
        let houses = |sim: &Headless, polity: PolityId| {
            logistics(sim)
                .houses
                .iter()
                .filter(|house| house.polity == polity)
                .count()
        };
        assert!(passing_fleets >= 1 && worlds.len() >= 2);
        let assert_ended = |sim: &Headless, when: &str| {
            let gone = sim.polities().get(from);
            assert!(!gone.alive && gone.capital.is_none(), "{when}");
            assert!(planets_of(sim, from).is_empty(), "{when}");
            for world in &worlds {
                assert_eq!(sim.galaxy().planet(*world).owner, Some(into), "{when}");
                // Die Regionen behalten ihren Stand: Der Monatsabschluss liest keinen
                // neuen Besitzer, der Stufe und Fristen zurücksetzte.
                let (then, now) = (finance(&base).region(*world), finance(sim).region(*world));
                assert_eq!(now.owner_seen, Some(into), "{when}");
                assert_eq!(
                    (now.stage, now.cohesion, now.since_day),
                    (then.stage, then.cohesion, then.since_day),
                    "{when}"
                );
            }
            // Kasse und Schulden sind bei der Erbin; die Kriege der Beendeten sind vorbei.
            assert_eq!(treasury(sim, into), cash, "{when}");
            assert_eq!(treasury(sim, from), 0, "{when}");
            assert_eq!(finance(sim).debt_of(into), debt, "{when}");
            assert_eq!(finance(sim).debt_of(from), 0, "{when}");
            assert!(!diplomacy(sim).is_at_war(third), "{when}");
            assert!(!diplomacy(sim).is_at_war(into), "{when}");
            assert_eq!((houses(sim, from), houses(sim, into)), (0, 2), "{when}");
            assert_eq!(fleets(sim).of(from).count(), 0, "{when}");
            assert_eq!(fleets(sim).orders_of(from).count(), 0, "{when}");
            // Die Erbin weiß, was die Beendete wusste.
            let research = sim.world().resource::<Research>();
            for (tech, known) in research.known(from).iter().enumerate() {
                assert!(!known || research.knows(into, tech as u16), "{when}");
            }
            // Die übernommene Kasse zählt nicht als Einnahme des Monats: Der nächste
            // Monatsabschluss misst die Einnahmen der Erbin an ihrer Kasse samt Erbe. Ihre
            // geschätzten Einnahmen sind die beider zusammen.
            let (then, now) = (finance(&base), finance(sim));
            assert_eq!(
                now.polity(into).last_treasury,
                then.polity(into).last_treasury + treasury(&base, from),
                "{when}"
            );
            assert_eq!(
                now.polity(into).revenue,
                then.polity(into).revenue + then.polity(from).revenue,
                "{when}"
            );
            assert_eq!(now.polity(from).revenue, 0.0, "{when}");
            assert_eq!(
                economy(sim).state_income_of(into),
                economy(&base).state_income_of(into) + economy(&base).state_income_of(from),
                "{when}"
            );
            assert_eq!(economy(sim).state_income_of(from), 0, "{when}");
        };

        // Friedliches Ende: Die Gemeinschaft geht in einer anderen auf.
        {
            let mut sim = twin(&base);
            let before = Before::of(&sim);
            let absorbed = sim
                .succession(|succession| succession.absorb(from, into, Path::Joined, tick))
                .unwrap();
            before.assert_conserved(&sim, "friedliches Ende");
            assert_ended(&sim, "friedliches Ende");
            assert_eq!(absorbed.planets, worlds);
            assert_eq!(absorbed.treasury, treasury(&base, from));
            assert_eq!(absorbed.fleets, passing_fleets);
            // Flotten und Bauauftrag gehen über; die Hauptwelt wird zum Sitz.
            assert_eq!(fleets(&sim).of(into).count(), own_fleets + passing_fleets);
            assert_eq!(fleets(&sim).orders_of(into).count(), 1);
            for world in &worlds {
                assert_eq!(endgame(&sim).path_of(*world), Path::Joined);
                assert_eq!(endgame(&sim).gap[world.index()], gap);
                assert_eq!(
                    endgame(&sim).absorbed_pop[world.index()],
                    population_of_planet(&base, *world)
                );
                assert_eq!(endgame(&sim).is_seat(*world), *world == seat);
            }
            sim.run_days(two_years);
            assert_sound(&sim, "zwei Jahre nach dem friedlichen Ende");
            assert_eq!(fleets(&sim).of(from).count(), 0);
        }

        // Erzwungenes Ende: Flotten und Aufträge sind fort.
        {
            let mut sim = twin(&base);
            let before = Before::of(&sim);
            let absorbed = sim
                .succession(|succession| succession.absorb(from, into, Path::Forced, tick))
                .unwrap();
            before.assert_conserved(&sim, "erzwungenes Ende");
            assert_ended(&sim, "erzwungenes Ende");
            assert_eq!(absorbed.fleets, passing_fleets);
            assert_eq!(fleets(&sim).of(into).count(), own_fleets);
            assert_eq!(fleets(&sim).orders_of(into).count(), 0);
            for world in &worlds {
                assert_eq!(endgame(&sim).path_of(*world), Path::Forced);
                assert!(!endgame(&sim).is_seat(*world));
            }
            sim.run_days(two_years);
            assert_sound(&sim, "zwei Jahre nach dem erzwungenen Ende");
        }

        // Unterstellung: Die Gemeinschaft bleibt bestehen und behält, was sie hat.
        {
            let mut sim = twin(&base);
            let before = Before::of(&sim);
            sim.succession(|succession| succession.subordinate(from, into, true, tick))
                .unwrap();
            before.assert_conserved(&sim, "Unterstellung");
            assert!(sim.polities().get(from).alive);
            assert_eq!(planets_of(&sim, from), worlds);
            assert_eq!(treasury(&sim, from), treasury(&base, from));
            assert_eq!(fleets(&sim).of(from).count(), passing_fleets);
            assert_eq!(fleets(&sim).orders_of(from).count(), 1);
            let standing = diplomacy(&sim).standing[from.index()];
            assert_eq!(
                (standing.overlord, standing.voluntary, standing.since_day),
                (Some(into), true, day)
            );
            assert_eq!(diplomacy(&sim).bloc(from), vec![into, from]);
            // Ihr eigener Krieg ist vorbei; ihre Welten gelten als beigetreten.
            assert!(!diplomacy(&sim).is_at_war(from) && !diplomacy(&sim).is_at_war(third));
            for world in &worlds {
                assert_eq!(endgame(&sim).path_of(*world), Path::Joined);
            }
            sim.run_days(two_years);
            assert_sound(&sim, "zwei Jahre nach der Unterstellung");
            assert!(sim.polities().get(from).alive);
        }
    }
}

// --- 3. Die Hauptwelt fällt -----------------------------------------------------------------

#[test]
fn capital_falls() {
    let mut sim = observer(7);
    sim.run_days(10 * DAYS_PER_YEAR);
    let (loser, gainer) = (PolityId(0), PolityId(1));
    let old_capital = capital(&sim, loser);
    let remaining = colonies(&sim, loser);
    assert!(remaining.len() >= 2, "das Szenario braucht zwei Kolonien");
    // Die bevölkerungsreichste verbleibende Welt wird Hauptwelt.
    let heir = remaining[0];
    let colony = remaining[1];
    let old_system = sim.galaxy().planet(old_capital).system;
    let new_system = sim.galaxy().planet(heir).system;
    {
        // Sie ist gerade autonom und zahlt keine Steuern: Als Hauptwelt darf sie das nicht.
        let world = sim.world_mut();
        let mut finance = world.resource_mut::<Finance>();
        finance.regions[heir.index()].stage = Stage::Autonomous;
        finance.regions[heir.index()].months_below = 7;
        world.resource_mut::<Economy>().planets[heir.index()].tax_exempt = true;
    }
    let house = logistics(&sim)
        .houses
        .iter()
        .find(|house| house.polity == loser)
        .expect("Handelshaus")
        .id;
    assert_eq!(logistics(&sim).houses[house.index()].home, old_system);

    // Was die Elite der alten Hauptwelt im verbleibenden Gebiet hält: alle Betriebe der
    // Verliererin außer denen, die mit der Hauptwelt selbst gehen.
    let (old_elite, new_elite) = (HolderId::Elite(old_capital), HolderId::Elite(heir));
    let capitals: Vec<Option<PlanetId>> = sim
        .polities()
        .list
        .iter()
        .map(|polity| polity.capital)
        .collect();
    let going: Vec<IssuerId> = sim::finance::issuers_of_region(
        finance(&sim),
        sim.galaxy(),
        logistics(&sim),
        &capitals,
        old_capital,
    )
    .map(|(id, _)| id)
    .collect();
    let staying: Vec<IssuerId> = finance(&sim)
        .issuers
        .keys()
        .copied()
        .filter(|id| polity_of_issuer(sim.galaxy(), logistics(&sim), *id) == Some(loser))
        .filter(|id| !going.contains(id))
        .collect();
    assert!(staying.contains(&IssuerId::House(house)));
    let held =
        |sim: &Headless, id: &IssuerId, holder: HolderId| finance(sim).issuers[id].held_by(holder);
    let stakes: Vec<(IssuerId, u64, u64)> = staying
        .iter()
        .map(|id| (*id, held(&sim, id, old_elite), held(&sim, id, new_elite)))
        .collect();
    assert!(
        stakes.iter().filter(|(_, old, _)| *old >= 2).count() >= 3,
        "die Elite der Hauptwelt ist im übrigen Reich beteiligt"
    );
    let going_stakes: Vec<(IssuerId, u64, u64)> = going
        .iter()
        .map(|id| (*id, held(&sim, id, old_elite), held(&sim, id, new_elite)))
        .collect();
    assert!(going_stakes.iter().any(|(_, old, _)| *old > 0));
    let (old_cash, new_cash) = (
        finance(&sim).elites[old_capital.index()],
        finance(&sim).elites[heir.index()],
    );
    assert!(old_cash > 0);
    let state_stakes: Vec<(IssuerId, u64, u64)> = going
        .iter()
        .map(|id| {
            (
                *id,
                held(&sim, id, HolderId::State(loser)),
                held(&sim, id, HolderId::State(gainer)),
            )
        })
        .collect();
    assert!(state_stakes.iter().any(|(_, state, _)| *state > 0));
    let people = population_of_planet(&sim, old_capital);
    let gap = 1.0 - diplomacy(&sim).affinity(loser, gainer);
    let before = Before::of(&sim);
    let (tick, day) = (sim.clock().tick, sim.clock().day());
    let terms = forced_terms(gainer);

    let transfer = sim
        .succession(|succession| succession.transfer_planet(old_capital, gainer, terms, tick))
        .unwrap();

    before.assert_conserved(&sim, "Fall der Hauptwelt");
    assert_eq!(transfer.from, loser);
    assert_eq!(transfer.new_capital, Some(heir));
    assert!(!transfer.eliminated);
    assert_eq!(sim.galaxy().planet(old_capital).owner, Some(gainer));
    assert_eq!(capital(&sim, loser), heir);
    assert!(sim.polities().get(loser).alive);
    // Das Handelshaus zieht mit der Hauptwelt um.
    assert_eq!(logistics(&sim).houses[house.index()].home, new_system);
    assert_eq!(logistics(&sim).houses[house.index()].polity, loser);
    // Die neue Hauptwelt ist höchstens unruhig und zahlt Steuern.
    let region = finance(&sim).region(heir);
    assert_eq!((region.stage, region.months_below), (Stage::Restless, 0));
    assert!(!economy(&sim).planet(heir).tax_exempt);
    // Die übertragene Welt beginnt unter der neuen Gemeinschaft, wie die Bedingungen sagen.
    let region = finance(&sim).region(old_capital);
    assert_eq!(
        (
            region.stage,
            region.cohesion,
            region.owner_seen,
            region.since_day
        ),
        (Stage::Restless, 30.0, Some(gainer), day)
    );
    assert!(!economy(&sim).planet(old_capital).tax_exempt);
    assert_eq!(endgame(&sim).path_of(old_capital), Path::Forced);
    assert_eq!(endgame(&sim).absorbed_pop[old_capital.index()], people);
    assert_eq!(endgame(&sim).gap[old_capital.index()], gap);
    // Die Anteile des alten Staates an den Betrieben der Welt gehören dem neuen.
    for (id, state, taker) in &state_stakes {
        assert_eq!(held(&sim, id, HolderId::State(loser)), 0, "{id:?}");
        assert_eq!(
            held(&sim, id, HolderId::State(gainer)),
            taker + state,
            "{id:?}"
        );
    }
    // Die Elite der neuen Hauptwelt gewinnt genau den genannten Anteil jeder Beteiligung
    // (auf ganze Anteile abgerundet) und des Geldes; der Rest bleibt, wo er war.
    let share = terms.elite_flight;
    let mut moved_any = false;
    for (id, old, new) in &stakes {
        let moved = (*old as f64 * share) as u64;
        assert_eq!(held(&sim, id, old_elite), old - moved, "{id:?}");
        assert_eq!(held(&sim, id, new_elite), new + moved, "{id:?}");
        moved_any |= moved > 0;
    }
    assert!(moved_any);
    let cash = (old_cash as f64 * share) as Money;
    assert!(cash > 0);
    assert_eq!(finance(&sim).elites[old_capital.index()], old_cash - cash);
    assert_eq!(finance(&sim).elites[heir.index()], new_cash + cash);
    // An den Betrieben, die mit der Hauptwelt gingen, ändert die Flucht nichts.
    for (id, old, new) in &going_stakes {
        assert_eq!(held(&sim, id, old_elite), *old, "{id:?}");
        assert_eq!(held(&sim, id, new_elite), *new, "{id:?}");
    }
    // Was der alten Elite bleibt, ist jetzt fremder Besitz: Die Gewinnerin ist an einer
    // verbliebenen Kolonie der Verliererin beteiligt.
    let capitals: Vec<Option<PlanetId>> = sim
        .polities()
        .list
        .iter()
        .map(|polity| polity.capital)
        .collect();
    let ownership = ownership_of_region(
        finance(&sim),
        sim.galaxy(),
        logistics(&sim),
        &capitals,
        colony,
    );
    let foreign = ownership.foreign.get(&gainer).copied().unwrap_or(0.0);
    assert!(foreign > 0.0, "fremder Anteil der Gewinnerin: {foreign}");

    sim.run_days(DAYS_PER_YEAR);
    assert_sound(&sim, "ein Jahr nach dem Fall der Hauptwelt");
    assert_eq!(capital(&sim, loser), heir);
    assert_ne!(finance(&sim).region(heir).stage, Stage::Autonomous);
}

// --- 4. Die letzte Welt ---------------------------------------------------------------------

#[test]
fn last_planet() {
    let (a, b, c) = (PolityId(0), PolityId(1), PolityId(2));
    // Die Gemeinschaft b führt der Mensch: Sie gründet keine Kolonie und hält nur ihre
    // Hauptwelt.
    let mut sim = with_player(5, b);
    let home = capital(&sim, b);
    let system = sim.galaxy().planet(home).system;
    sim.issue(a, SimCommand::DeclareWar { target: b });
    sim.issue(c, SimCommand::DeclareWar { target: b });
    sim.issue(b, SimCommand::IssueBonds { amount: 10_000.0 });
    sim.issue(
        b,
        SimCommand::BuildClass {
            planet: home,
            class: ShipClass::Corvette,
        },
    );
    // Die Flotte von a liegt im Heimatsystem von b: Es kommt zum Gefecht.
    let (ours, theirs) = (FleetId(a.0), FleetId(b.0));
    {
        let mut fleets = sim.world_mut().resource_mut::<Fleets>();
        assert_eq!(
            (
                fleets.list[ours.index()].owner,
                fleets.list[theirs.index()].owner
            ),
            (a, b)
        );
        fleets.list[ours.index()].system = system;
    }
    sim.run_days(3);
    assert_eq!(planets_of(&sim, b), vec![home]);
    assert!(battles(&sim).in_system(system).is_some());
    assert!(fleets(&sim).list[theirs.index()].in_battle);
    assert!(diplomacy(&sim).at_war(a, b) && diplomacy(&sim).at_war(c, b));
    let debt = finance(&sim).debt_of(b);
    let cash = treasury(&sim, b);
    assert!(debt > 0 && cash > 0);
    assert_eq!(fleets(&sim).orders_of(b).count(), 1);
    assert_eq!(finance(&sim).debt_of(a), 0);
    let taker_cash = treasury(&sim, a);
    let before = Before::of(&sim);
    let tick = sim.clock().tick;

    let transfer = sim
        .succession(|succession| succession.transfer_planet(home, a, forced_terms(a), tick))
        .unwrap();

    before.assert_conserved(&sim, "Verlust der letzten Welt");
    assert_eq!((transfer.from, transfer.new_capital), (b, None));
    assert!(transfer.eliminated);
    // Die Gemeinschaft hat geendet …
    let gone = sim.polities().get(b);
    assert!(!gone.alive && gone.capital.is_none());
    assert!(planets_of(&sim, b).is_empty());
    assert_eq!(sim.galaxy().planet(home).owner, Some(a));
    // … ihre Gegner führen keinen Krieg mehr mit ihr …
    assert!(!diplomacy(&sim).at_war(a, b) && !diplomacy(&sim).at_war(c, b));
    assert!(!diplomacy(&sim).is_at_war(a) && !diplomacy(&sim).is_at_war(c));
    assert!(diplomacy(&sim).wars.is_empty());
    // … Kasse und Anleihen sind bei der Erbin …
    assert_eq!(treasury(&sim, a), taker_cash + cash);
    assert_eq!(treasury(&sim, b), 0);
    assert_eq!(finance(&sim).debt_of(a), debt);
    assert!(finance(&sim).bonds.iter().all(|bond| bond.issuer != b));
    // … und Flotten gibt es keine mehr, auch nicht die im Gefecht und nicht die im Bau.
    assert_eq!(fleets(&sim).of(b).count(), 0);
    let dissolved = &fleets(&sim).list[theirs.index()];
    assert!(!dissolved.alive && dissolved.ships.is_empty() && !dissolved.in_battle);
    assert!(fleets(&sim).orders.is_empty());
    // Der Mensch ist ausgeschieden; die Partie läuft ohne ihn weiter.
    assert_eq!(player(&sim), None);
    assert_eq!(endgame(&sim).player, PlayerFate::Out);
    assert!(!endgame(&sim).ended());
    // Unter Zwang genommen, und kein Sitz.
    assert_eq!(endgame(&sim).path_of(home), Path::Forced);
    assert!(!endgame(&sim).is_seat(home));

    // Das Gefecht hat keine Gegner mehr und wird eingestellt; die Siegerin bleibt.
    sim.run_days(1);
    assert!(battles(&sim).active.is_empty());
    let report = battles(&sim).reports.last().expect("eine Bilanz");
    assert_eq!(report.outcome, BattleOutcome::CalledOff);
    // Die aufgelöste Partei steht noch in der Bilanz, der Erbin zugerechnet, aber ohne
    // Verlustwert: Fremde Verluste lastet das Ende des Gefechts niemandem an.
    let ghost = report
        .sides
        .iter()
        .find(|side| side.fleet == Some(theirs))
        .expect("die aufgelöste Flotte war Partei");
    assert_eq!((ghost.owner, ghost.lost_value), (a, 0.0));
    assert!(!ghost.held_field);
    assert!(report.sides.iter().all(|side| side.owner == a));
    assert!(fleets(&sim).list[ours.index()].alive);
    assert!(!fleets(&sim).list[ours.index()].in_battle);
    assert_sound(&sim, "am Tag nach dem Ende");
    // Befehle der Beendeten werden abgelehnt.
    let reason = rejection(&mut sim, b, SimCommand::SetTaxRate { rate: 0.3 });
    assert!(reason.contains("besteht nicht mehr"), "{reason}");
    sim.run_days(DAYS_PER_YEAR);
    assert_sound(&sim, "ein Jahr nach dem Ende");
    assert!(!sim.polities().get(b).alive);
}

/// Staatlicher Besitz einer Gemeinschaft, die endet: Anteile, Angebote und Anleihen gehen
/// an den Staat der Erbin; was sie sich danach selbst schuldete, ist getilgt.
#[test]
fn state_holdings_and_bonds_pass_to_the_heir() {
    let (from, into, third) = (PolityId(1), PolityId(2), PolityId(3));
    let mut sim = observer(16);
    sim.run_days(3 * DAYS_PER_YEAR);
    let (old_state, new_state) = (HolderId::State(from), HolderId::State(into));
    // Der alte Staat hält Anteile an Betrieben im ganzen Reich und bietet einen Teil an.
    let stakes = |sim: &Headless| -> Vec<(IssuerId, u64, u64)> {
        finance(sim)
            .issuers
            .iter()
            .filter(|(_, issuer)| issuer.held_by(old_state) > 0)
            .map(|(id, issuer)| (*id, issuer.held_by(old_state), issuer.held_by(new_state)))
            .collect()
    };
    let offered = stakes(&sim)[0].0;
    assert_eq!(
        command(
            &mut sim,
            from,
            SimCommand::OfferShares {
                issuer: offered,
                shares: 40,
            }
        ),
        CommandOutcome::Applied
    );
    // Gemessen nach dem Takt des Befehls: Am Monatsersten zeichnet der Staat neue Anteile.
    let held = stakes(&sim);
    assert!(held.len() >= 3);
    assert_eq!(
        finance(&sim).issuers[&offered].offered.get(&old_state),
        Some(&40)
    );
    // Anleihen in allen Lagen, von Hand ins Buch gestellt (Forderungen sind kein Geld):
    // die Erbin schuldet dem alten Staat, die Beendete schuldet dem Staat der Erbin, die
    // Beendete schuldet einer Elite, und eine Dritte schuldet dem alten Staat.
    let elite = HolderId::Elite(capital(&sim, third));
    let bond = |issuer: PolityId, holder: HolderId, principal: Money| sim::finance::Bond {
        issuer,
        holder,
        principal,
        rate: 0.05,
        matures_on: 99_999,
    };
    {
        let mut finance = sim.world_mut().resource_mut::<Finance>();
        assert!(finance.bonds.is_empty());
        finance.bonds.extend([
            bond(into, old_state, 1_000),
            bond(from, new_state, 2_000),
            bond(from, elite, 3_000),
            bond(third, old_state, 4_000),
        ]);
    }
    let before = Before::of(&sim);
    let tick = sim.clock().tick;
    sim.succession(|succession| succession.absorb(from, into, Path::Joined, tick))
        .unwrap();

    before.assert_conserved(&sim, "Erbe des Staates");
    for (id, old, new) in &held {
        let issuer = &finance(&sim).issuers[id];
        assert_eq!(issuer.held_by(old_state), 0, "{id:?}");
        assert_eq!(issuer.held_by(new_state), old + new, "{id:?}");
        assert!(!issuer.offered.contains_key(&old_state), "{id:?}");
    }
    assert_eq!(
        finance(&sim).issuers[&offered].offered.get(&new_state),
        Some(&40),
        "das Angebot bleibt bestehen"
    );
    // Zwei Anleihen schuldete die Erbin nun sich selbst: getilgt. Die beiden anderen
    // heißen neu.
    let bonds: Vec<(PolityId, HolderId, Money)> = finance(&sim)
        .bonds
        .iter()
        .map(|bond| (bond.issuer, bond.holder, bond.principal))
        .collect();
    assert_eq!(bonds, vec![(into, elite, 3_000), (third, new_state, 4_000)]);
    assert_eq!(finance(&sim).debt_of(into), 3_000);
    sim.run_days(DAYS_PER_YEAR);
    assert_sound(&sim, "ein Jahr nach dem Erbe");
}

// --- 5. Wege --------------------------------------------------------------------------------

#[test]
fn tags() {
    let (a, b, c) = (PolityId(0), PolityId(1), PolityId(2));
    let mut sim = observer(9);
    sim.run_days(6 * DAYS_PER_YEAR);
    let taken = colonies(&sim, a);
    assert!(taken.len() >= 2 && !colonies(&sim, b).is_empty());
    let (first, second) = (taken[0], taken[1]);
    let own = colonies(&sim, b)[0];
    let seat = capital(&sim, b);
    let tick = sim.clock().tick;
    let path = |sim: &Headless, planet: PlanetId| {
        let endgame = endgame(sim);
        (
            endgame.path_of(planet),
            endgame.absorbed_pop[planet.index()],
            endgame.gap[planet.index()],
        )
    };
    assert_eq!(path(&sim, first), (Path::Own, 0.0, 0.0));

    // Zwei Welten von a werden unter Zwang zu Welten von b. Die erste war autonom und
    // steuerfrei und beginnt unruhig: Dann zahlt sie wieder. Die zweite bleibt nach den
    // Bedingungen autonom, und nur dann steuerfrei.
    let forced_gap = 1.0 - diplomacy(&sim).affinity(a, b);
    sim.world_mut().resource_mut::<Economy>().planets[first.index()].tax_exempt = true;
    let autonomous = TransferTerms {
        stage: Stage::Autonomous,
        ..forced_terms(b)
    };
    for (planet, terms) in [(first, forced_terms(b)), (second, autonomous)] {
        sim.succession(|succession| succession.transfer_planet(planet, b, terms, tick))
            .unwrap();
        assert_eq!(finance(&sim).region(planet).stage, terms.stage);
        assert_eq!(
            economy(&sim).planet(planet).tax_exempt,
            terms.stage == Stage::Autonomous
        );
    }
    let as_taken = [path(&sim, first), path(&sim, second)];
    assert_eq!(
        as_taken[0],
        (Path::Forced, population_of_planet(&sim, first), forced_gap)
    );
    assert!(forced_gap > 0.0);

    // b geht friedlich in c auf: Eigene Welten gelten als beigetreten, die Hauptwelt wird
    // Sitz. Die genommenen bleiben genommen, mit dem, was damals festgehalten wurde.
    sim.run_days(DAYS_PER_MONTH);
    let joined_gap = 1.0 - diplomacy(&sim).affinity(b, c);
    let tick = sim.clock().tick;
    sim.succession(|succession| succession.absorb(b, c, Path::Joined, tick))
        .unwrap();
    assert_eq!(path(&sim, first), as_taken[0]);
    assert_eq!(path(&sim, second), as_taken[1]);
    assert_eq!(
        path(&sim, own),
        (Path::Joined, population_of_planet(&sim, own), joined_gap)
    );
    assert_eq!(path(&sim, seat).0, Path::Joined);
    assert!(endgame(&sim).is_seat(seat) && !endgame(&sim).is_seat(first));
    assert_sound(&sim, "nach der Vereinigung");

    // Eine Abspaltung in eine neue Gemeinschaft löscht die Spur: Die Welt ist ihr eigen.
    let child = found_free_state(&mut sim, first);
    assert_eq!(path(&sim, first), (Path::Own, 0.0, 0.0));
    assert_eq!(sim.galaxy().planet(first).owner, Some(child));
    assert_eq!(
        path(&sim, second),
        as_taken[1],
        "die andere bleibt genommen"
    );

    // Eine gewollte Unterstellung macht aus eigenen Welten beigetretene; genommene bleiben
    // genommen.
    let home = capital(&sim, c);
    let ward_gap = 1.0 - diplomacy(&sim).affinity(c, a);
    sim.succession(|succession| succession.subordinate(c, a, true, tick))
        .unwrap();
    assert_eq!(
        path(&sim, home),
        (Path::Joined, population_of_planet(&sim, home), ward_gap)
    );
    assert_eq!(
        path(&sim, own).2,
        ward_gap,
        "neu vermerkt mit dem Abstand zum Oberhaupt"
    );
    assert_eq!(path(&sim, second), as_taken[1]);
    assert!(endgame(&sim).is_seat(seat), "der Sitz bleibt Sitz");

    // Erzwungen aufgenommen, ist alles genommen, und Sitze gibt es nicht mehr.
    sim.succession(|succession| succession.absorb(c, a, Path::Forced, tick))
        .unwrap();
    for planet in [home, own, seat, second] {
        assert_eq!(path(&sim, planet).0, Path::Forced);
        assert_eq!(path(&sim, planet).2, ward_gap);
        assert!(!endgame(&sim).is_seat(planet));
    }
    assert_eq!(
        path(&sim, first).0,
        Path::Own,
        "der Freistaat bleibt unberührt"
    );
    assert_sound(&sim, "nach allen Wechseln");
    sim.run_days(DAYS_PER_YEAR);
    assert_sound(&sim, "ein Jahr später");
}

// --- 6. Unterstellte ------------------------------------------------------------------------

#[test]
fn wards() {
    let (lord, ward, minor, rival, bystander) = (
        PolityId(0),
        PolityId(1),
        PolityId(2),
        PolityId(3),
        PolityId(4),
    );
    let mut sim = with_player(4, ward);
    sim.run_days(30);
    // Zuerst unterstellt sich die Kleine der späteren Unterstellten; die führt gerade
    // Krieg gegen ihr künftiges Oberhaupt.
    let tick = sim.clock().tick;
    sim.succession(|succession| succession.subordinate(minor, ward, true, tick))
        .unwrap();
    assert_eq!(
        command(&mut sim, ward, SimCommand::DeclareWar { target: lord }),
        CommandOutcome::Applied
    );
    assert!(
        diplomacy(&sim).at_war(minor, lord),
        "sie teilt den Krieg ihres Haupts"
    );
    assert_eq!(player(&sim), Some(ward));

    // Dann wird diese selbst unterstellt, unter Zwang.
    let tick = sim.clock().tick;
    let before = Before::of(&sim);
    sim.succession(|succession| succession.subordinate(ward, lord, false, tick))
        .unwrap();
    before.assert_conserved(&sim, "Unterstellung");
    // Keine Ketten: Wer ihr unterstand, untersteht jetzt dem Oberhaupt.
    assert_eq!(diplomacy(&sim).overlord_of(ward), Some(lord));
    assert_eq!(diplomacy(&sim).overlord_of(minor), Some(lord));
    assert_eq!(diplomacy(&sim).bloc(minor), vec![lord, ward, minor]);
    assert!(!diplomacy(&sim).standing[ward.index()].voluntary);
    assert!(diplomacy(&sim).standing[minor.index()].voluntary);
    // Ihr Krieg gegen das Oberhaupt ist vorbei; im Verbund führt niemand Krieg gegeneinander.
    assert!(diplomacy(&sim).wars.is_empty());
    for (x, y) in [(lord, ward), (lord, minor), (ward, minor)] {
        assert!(!diplomacy(&sim).at_war(x, y));
    }
    // Ihre Welten gelten als genommen. Der Mensch, der sie führte, ist ausgeschieden.
    assert_eq!(endgame(&sim).path_of(capital(&sim, ward)), Path::Forced);
    assert_eq!(endgame(&sim).path_of(capital(&sim, minor)), Path::Joined);
    assert_eq!(player(&sim), None);
    assert_eq!(endgame(&sim).player, PlayerFate::Out);

    // Das Oberhaupt erklärt einen Krieg: Alle, die ihm unterstehen, führen ihn mit.
    assert_eq!(
        command(&mut sim, lord, SimCommand::DeclareWar { target: rival }),
        CommandOutcome::Applied
    );
    for member in [lord, ward, minor] {
        assert!(diplomacy(&sim).at_war(member, rival) && diplomacy(&sim).is_at_war(member));
        assert!(!diplomacy(&sim).at_war(member, bystander));
    }
    assert_eq!(
        diplomacy(&sim).enemies_of(rival).collect::<Vec<_>>(),
        vec![lord, ward, minor]
    );
    // Auch auf der Karte: Liegt eine Flotte der Unterstellten bei der Gegnerin, kommt es
    // zum Gefecht.
    let system = sim.galaxy().planet(capital(&sim, rival)).system;
    sim.world_mut().resource_mut::<Fleets>().list[ward.index()].system = system;
    sim.run_days(2);
    let battle = battles(&sim)
        .in_system(system)
        .expect("ein Gefecht beginnt");
    let owners: Vec<PolityId> = battle.combatants.iter().map(|side| side.owner).collect();
    assert!(owners.contains(&ward) && owners.contains(&rival));

    // Über Krieg und Frieden entscheidet das Oberhaupt: Unterstellte können beides nicht,
    // und niemand kann es mit ihnen.
    for (who, order) in [
        (ward, SimCommand::DeclareWar { target: bystander }),
        (ward, SimCommand::MakePeace { target: rival }),
        (minor, SimCommand::MakePeace { target: rival }),
        (rival, SimCommand::MakePeace { target: ward }),
        (bystander, SimCommand::DeclareWar { target: minor }),
    ] {
        let reason = rejection(&mut sim, who, order);
        assert!(reason.contains("Oberhaupt"), "{reason}");
    }
    assert!(!diplomacy(&sim).at_war(ward, bystander));
    assert!(diplomacy(&sim).at_war(ward, rival));
    // Schließt das Oberhaupt Frieden, gilt er für alle.
    assert_eq!(
        command(&mut sim, lord, SimCommand::MakePeace { target: rival }),
        CommandOutcome::Applied
    );
    for member in [lord, ward, minor] {
        assert!(!diplomacy(&sim).is_at_war(member));
    }

    // Wer untersteht, kann weder noch einmal unterstellt werden noch selbst führen.
    let tick = sim.clock().tick;
    let hash = sim.hash();
    for (lesser, greater) in [(ward, rival), (rival, ward), (minor, lord), (lord, lord)] {
        let error = sim
            .succession(|succession| succession.subordinate(lesser, greater, true, tick))
            .unwrap_err();
        assert!(error.contains("unter"), "{error}");
    }
    assert_eq!(sim.hash(), hash, "ein abgelehnter Aufruf ändert nichts");

    sim.run_days(DAYS_PER_YEAR);
    assert_sound(&sim, "ein Jahr unter dem Oberhaupt");
    // Geht die Unterstellte später im Oberhaupt auf, bleibt die Kleine dessen Unterstellte.
    let tick = sim.clock().tick;
    sim.succession(|succession| succession.absorb(ward, lord, Path::Joined, tick))
        .unwrap();
    assert_eq!(diplomacy(&sim).overlord_of(minor), Some(lord));
    assert_eq!(diplomacy(&sim).bloc(lord), vec![lord, minor]);
    assert_eq!(
        endgame(&sim).path_of(capital(&sim, lord)),
        Path::Own,
        "das Oberhaupt selbst bleibt unberührt"
    );
    assert_sound(&sim, "nach der Aufnahme der Unterstellten");
}

#[test]
fn wards_never_form_chains() {
    let id = PolityId;
    let mut sim = observer(10);
    sim.run_days(30);
    let tick = sim.clock().tick;
    let unite = |sim: &mut Headless, ward: u32, lord: u32| {
        sim.succession(|succession| succession.subordinate(id(ward), id(lord), true, tick))
            .unwrap();
    };
    // 1 untersteht 0; 3 untersteht 2; 5 untersteht 4.
    unite(&mut sim, 1, 0);
    unite(&mut sim, 3, 2);
    unite(&mut sim, 5, 4);

    // Ein Oberhaupt (2) geht in einer Unterstellten eines anderen (1) auf: Wer ihm
    // unterstand (3), untersteht nicht der Erbin, sondern deren Haupt (0).
    sim.succession(|succession| succession.absorb(id(2), id(1), Path::Joined, tick))
        .unwrap();
    assert_eq!(diplomacy(&sim).overlord_of(id(3)), Some(id(0)));
    assert_eq!(diplomacy(&sim).bloc(id(3)), vec![id(0), id(1), id(3)]);
    assert_eq!(sim.galaxy().planet(capital(&sim, id(1))).owner, Some(id(1)));
    assert_sound(&sim, "Erbin ist selbst unterstellt");

    // Ein Oberhaupt (4) geht in seiner eigenen Unterstellten (5) auf: Die ist danach frei.
    sim.succession(|succession| succession.absorb(id(4), id(5), Path::Forced, tick))
        .unwrap();
    assert!(diplomacy(&sim).is_free(id(5)));
    assert_eq!(diplomacy(&sim).bloc(id(5)), vec![id(5)]);
    assert_sound(&sim, "Erbin unterstand der Beendeten");

    // Die letzte Welt einer Unterstellten (3) wird übertragen: Sie endet, und ihre
    // Stellung endet mit ihr.
    let home = capital(&sim, id(3));
    let lost: Vec<PlanetId> = colonies(&sim, id(3));
    for planet in lost.into_iter().chain([home]) {
        sim.succession(|succession| {
            succession.transfer_planet(planet, id(5), forced_terms(id(5)), tick)
        })
        .unwrap();
    }
    assert!(!sim.polities().get(id(3)).alive);
    assert!(diplomacy(&sim).is_free(id(3)));
    assert_eq!(diplomacy(&sim).bloc(id(0)), vec![id(0), id(1)]);
    assert_sound(&sim, "Unterstellte verliert ihre letzte Welt");
    sim.run_days(DAYS_PER_YEAR);
    assert_sound(&sim, "ein Jahr später");
}

// --- Abgelehnte Aufrufe ---------------------------------------------------------------------

#[test]
fn impossible_calls_are_rejected_and_change_nothing() {
    let (a, b, c, d) = (PolityId(0), PolityId(1), PolityId(2), PolityId(3));
    let mut sim = observer(12);
    sim.run_days(4 * DAYS_PER_YEAR);
    let tick = sim.clock().tick;
    // Eine beendete Gemeinschaft (d) und eine unterstellte (c unter b) gehören zur Lage.
    sim.succession(|succession| succession.absorb(d, a, Path::Joined, tick))
        .unwrap();
    sim.succession(|succession| succession.subordinate(c, b, true, tick))
        .unwrap();
    let unknown = PolityId(99);
    let mine = colonies(&sim, a)[0];
    let free = sim
        .galaxy()
        .planets
        .iter()
        .find(|planet| planet.owner.is_none())
        .map(|planet| planet.id);
    let balance = &data().balance.finance.cohesion;
    let terms = TransferTerms::secession(mine, balance, Path::Joined);
    let founding = Founding::free_state(sim.polities().get(a), sim.galaxy().planet(mine));
    let hash = sim.hash();
    let mut rejected = 0;
    let mut expect = |sim: &mut Headless, result: Result<(), String>, part: &str| {
        let error = result.expect_err("der Aufruf hätte abgelehnt werden müssen");
        assert!(error.contains(part), "{error}");
        assert_eq!(sim.hash(), hash, "abgelehnt und doch verändert: {error}");
        rejected += 1;
    };

    // Übertragung einer Welt.
    let transfer = |sim: &mut Headless, planet: PlanetId, to: PolityId, terms: TransferTerms| {
        sim.succession(|succession| succession.transfer_planet(planet, to, terms, tick))
            .map(|_| ())
    };
    let outcome = transfer(&mut sim, PlanetId(9_999), b, terms);
    expect(&mut sim, outcome, "unbekannt");
    if let Some(free) = free {
        let outcome = transfer(&mut sim, free, b, terms);
        expect(&mut sim, outcome, "keinen Besitzer");
    }
    let outcome = transfer(&mut sim, mine, a, terms);
    expect(&mut sim, outcome, "gehört bereits");
    let outcome = transfer(&mut sim, mine, d, terms);
    expect(&mut sim, outcome, "besteht nicht mehr");
    let outcome = transfer(&mut sim, mine, unknown, terms);
    expect(&mut sim, outcome, "unbekannt");
    for broken in [
        TransferTerms {
            state_share: 1.5,
            ..terms
        },
        TransferTerms {
            elite_flight: -0.1,
            ..terms
        },
        TransferTerms {
            state_share: f64::NAN,
            ..terms
        },
    ] {
        let outcome = transfer(&mut sim, mine, b, broken);
        expect(&mut sim, outcome, "zwischen 0 und 1");
    }
    for shock in [0.0, -1.0, f64::INFINITY] {
        let outcome = transfer(&mut sim, mine, b, TransferTerms { shock, ..terms });
        expect(&mut sim, outcome, "Kurse");
    }
    let outcome = transfer(
        &mut sim,
        mine,
        b,
        TransferTerms {
            cohesion: f64::NAN,
            ..terms
        },
    );
    expect(&mut sim, outcome, "Zusammenhalt");
    let outcome = transfer(
        &mut sim,
        mine,
        b,
        TransferTerms {
            state_shares_to: HolderId::State(d),
            ..terms
        },
    );
    expect(&mut sim, outcome, "besteht nicht mehr");
    let outcome = transfer(
        &mut sim,
        mine,
        b,
        TransferTerms {
            state_shares_to: HolderId::Elite(PlanetId(9_999)),
            ..terms
        },
    );
    expect(&mut sim, outcome, "unbekannt");

    // Gründung.
    let spawn = |sim: &mut Headless, parent: PolityId, founding: Founding| {
        sim.succession(|succession| succession.spawn(parent, founding, tick))
            .map(|_| ())
    };
    let outcome = spawn(&mut sim, d, founding.clone());
    expect(&mut sim, outcome, "besteht nicht mehr");
    let outcome = spawn(&mut sim, unknown, founding.clone());
    expect(&mut sim, outcome, "unbekannt");
    let outcome = spawn(&mut sim, b, founding.clone());
    expect(&mut sim, outcome, "gehört nicht zu");
    let outcome = spawn(
        &mut sim,
        a,
        Founding {
            capital: PlanetId(9_999),
            ..founding.clone()
        },
    );
    expect(&mut sim, outcome, "unbekannt");
    let outcome = spawn(
        &mut sim,
        a,
        Founding {
            name: "  ".into(),
            ..founding.clone()
        },
    );
    expect(&mut sim, outcome, "Namen");

    // Ende einer Gemeinschaft.
    let absorb = |sim: &mut Headless, from: PolityId, into: PolityId, path: Path| {
        sim.succession(|succession| succession.absorb(from, into, path, tick))
            .map(|_| ())
    };
    let outcome = absorb(&mut sim, a, a, Path::Joined);
    expect(&mut sim, outcome, "sich selbst");
    let outcome = absorb(&mut sim, d, a, Path::Forced);
    expect(&mut sim, outcome, "besteht nicht mehr");
    let outcome = absorb(&mut sim, a, d, Path::Forced);
    expect(&mut sim, outcome, "besteht nicht mehr");
    let outcome = absorb(&mut sim, a, unknown, Path::Joined);
    expect(&mut sim, outcome, "unbekannt");
    let outcome = absorb(&mut sim, a, b, Path::Own);
    expect(&mut sim, outcome, "Beitritt oder unter Zwang");

    // Unterstellung.
    let subordinate = |sim: &mut Headless, ward: PolityId, lord: PolityId| {
        sim.succession(|succession| succession.subordinate(ward, lord, false, tick))
    };
    let outcome = subordinate(&mut sim, a, a);
    expect(&mut sim, outcome, "sich selbst");
    let outcome = subordinate(&mut sim, d, a);
    expect(&mut sim, outcome, "besteht nicht mehr");
    let outcome = subordinate(&mut sim, a, d);
    expect(&mut sim, outcome, "besteht nicht mehr");
    let outcome = subordinate(&mut sim, c, a);
    expect(&mut sim, outcome, "untersteht bereits");
    let outcome = subordinate(&mut sim, a, c);
    expect(&mut sim, outcome, "untersteht selbst");

    assert!(rejected >= 28, "{rejected} Fälle geprüft");
    // Eine Abspaltung, die nicht möglich ist, lässt ebenfalls alles, wie es war.
    assert_eq!(secede(&mut sim, mine, b, None), None, "falscher Besitzer");
    assert_eq!(
        secede(&mut sim, mine, a, Some(d)),
        None,
        "Ziel besteht nicht"
    );
    assert_eq!(
        secede(&mut sim, mine, a, Some(a)),
        None,
        "Ziel ist sie selbst"
    );
    assert_eq!(secede(&mut sim, PlanetId(9_999), a, None), None);
    assert_eq!(sim.hash(), hash);
    assert_sound(&sim, "nach allen abgelehnten Aufrufen");
}

#[test]
fn the_invariant_check_finds_what_it_should() {
    let (a, b, c, d) = (PolityId(0), PolityId(1), PolityId(2), PolityId(3));
    let mut base = observer(8);
    base.run_days(2 * DAYS_PER_YEAR);
    let tick = base.clock().tick;
    // Zur Lage gehören eine beendete Gemeinschaft (d) und eine unterstellte (c unter b).
    base.succession(|succession| succession.absorb(d, a, Path::Joined, tick))
        .unwrap();
    base.succession(|succession| succession.subordinate(c, b, true, tick))
        .unwrap();
    assert_sound(&base, "Ausgangslage");
    let home = capital(&base, a);
    let foreign = capital(&base, b);
    let free = base
        .galaxy()
        .planets
        .iter()
        .find(|planet| planet.owner.is_none())
        .expect("nach zwei Jahren sind noch Welten frei")
        .id;
    let firm = *finance(&base).issuers.keys().next().unwrap();

    // Jeder Eingriff bricht genau eine Regel; die Meldung nennt sie.
    let broken = |change: &dyn Fn(&mut Headless)| {
        let mut sim = twin(&base);
        change(&mut sim);
        check_invariants(sim.world()).expect_err("die Prüfung hätte anschlagen müssen")
    };
    let cases: Vec<(&str, Box<dyn Fn(&mut Headless)>)> = vec![
        // Listen je Gemeinschaft und je Planet.
        (
            "Liste je Gemeinschaft (Staatskassen)",
            Box::new(|sim| {
                sim.world_mut().resource_mut::<Economy>().treasuries.pop();
            }),
        ),
        (
            "Liste je Gemeinschaft (Kulturen)",
            Box::new(|sim| {
                sim.world_mut().resource_mut::<Diplomacy>().culture.pop();
            }),
        ),
        (
            "Liste je Gemeinschaft (Soldrückstand)",
            Box::new(|sim| {
                sim.world_mut().resource_mut::<Fleets>().arrears.pop();
            }),
        ),
        (
            "Liste je Gemeinschaft (verlorene Schiffe)",
            Box::new(|sim| {
                sim.world_mut().resource_mut::<Battles>().ships_lost.push(0);
            }),
        ),
        (
            "Liste je Gemeinschaft (Fahrtfaktoren)",
            Box::new(|sim| {
                sim.world_mut().resource_mut::<Logistics>().tech_speed.pop();
            }),
        ),
        (
            "Liste je Planet (kultureller Abstand)",
            Box::new(|sim| {
                sim.world_mut().resource_mut::<Endgame>().gap.pop();
            }),
        ),
        (
            "Spur eines Wechsels",
            Box::new(move |sim| {
                sim.world_mut().resource_mut::<Endgame>().path[free.index()] = Path::Forced;
            }),
        ),
        (
            "Spur eines Wechsels",
            Box::new(move |sim| {
                sim.world_mut().resource_mut::<Endgame>().absorbed_pop[home.index()] = 12.0;
            }),
        ),
        (
            "Sitz ohne Besitzer",
            Box::new(move |sim| {
                sim.world_mut().resource_mut::<Endgame>().seat[free.index()] = true;
            }),
        ),
        // Lebende Gemeinschaften.
        (
            "lebt ohne Hauptwelt",
            Box::new(move |sim| {
                sim.world_mut()
                    .resource_mut::<Polities>()
                    .get_mut(a)
                    .capital = None;
            }),
        ),
        (
            "besitzt seine Hauptwelt nicht",
            Box::new(move |sim| {
                sim.world_mut()
                    .resource_mut::<Polities>()
                    .get_mut(a)
                    .capital = Some(foreign);
            }),
        ),
        // Beendete Gemeinschaften haben nichts mehr.
        (
            "die geendet hat",
            Box::new(move |sim| {
                sim.world_mut()
                    .resource_mut::<Galaxy>()
                    .planet_mut(free)
                    .owner = Some(d);
            }),
        ),
        (
            "führt aber eine Hauptwelt",
            Box::new(move |sim| {
                sim.world_mut()
                    .resource_mut::<Polities>()
                    .get_mut(d)
                    .capital = Some(home);
            }),
        ),
        (
            "hält aber eine Staatskasse",
            Box::new(move |sim| {
                sim.world_mut().resource_mut::<Economy>().treasuries[d.index()] = 5;
            }),
        ),
        (
            "führt aber ein Handelshaus",
            Box::new(move |sim| {
                sim.world_mut().resource_mut::<Logistics>().houses[0].polity = d;
            }),
        ),
        (
            "die geendet hat",
            Box::new(move |sim| {
                sim.world_mut().resource_mut::<Fleets>().list[a.index()].owner = d;
            }),
        ),
        (
            "Bauauftrag",
            Box::new(move |sim| {
                let mut fleets = sim.world_mut().resource_mut::<Fleets>();
                let id = fleets.next_order;
                fleets.next_order += 1;
                fleets.orders.push(sim::fleet::BuildOrder {
                    id,
                    polity: d,
                    planet: home,
                    design: 0,
                    progress: 0.0,
                    spent: 0,
                    stalled: false,
                    started: 0,
                });
            }),
        ),
        (
            "Krieg zwischen",
            Box::new(move |sim| {
                sim.world_mut()
                    .resource_mut::<Diplomacy>()
                    .wars
                    .insert((a, d));
            }),
        ),
        (
            "Anleihe",
            Box::new(move |sim| {
                sim.world_mut()
                    .resource_mut::<Finance>()
                    .bonds
                    .push(sim::finance::Bond {
                        issuer: d,
                        holder: HolderId::Elite(home),
                        principal: 1000,
                        rate: 0.05,
                        matures_on: 9_999,
                    });
            }),
        ),
        (
            "Anleihe",
            Box::new(move |sim| {
                sim.world_mut()
                    .resource_mut::<Finance>()
                    .bonds
                    .push(sim::finance::Bond {
                        issuer: a,
                        holder: HolderId::State(d),
                        principal: 1000,
                        rate: 0.05,
                        matures_on: 9_999,
                    });
            }),
        ),
        (
            "einem Staat, der geendet hat",
            Box::new(move |sim| {
                let mut finance = sim.world_mut().resource_mut::<Finance>();
                let issuer = finance.issuers.get_mut(&firm).unwrap();
                issuer.shares += 3;
                issuer.holdings.insert(HolderId::State(d), 3);
            }),
        ),
        (
            "einem Staat, der geendet hat",
            Box::new(move |sim| {
                let mut finance = sim.world_mut().resource_mut::<Finance>();
                let issuer = finance.issuers.get_mut(&firm).unwrap();
                issuer.offered.insert(HolderId::State(d), 3);
            }),
        ),
        (
            "nennt eine Gemeinschaft, die geendet hat",
            Box::new(move |sim| {
                sim.world_mut().resource_mut::<Finance>().regions[free.index()].owner_seen =
                    Some(d);
            }),
        ),
        // Oberhäupter leben und sind frei; Kriege führen nur Freie.
        (
            "untersteht aber noch",
            Box::new(move |sim| {
                sim.world_mut().resource_mut::<Diplomacy>().standing[d.index()].overlord = Some(a);
            }),
        ),
        (
            "nicht lebt oder nicht frei ist",
            Box::new(move |sim| {
                sim.world_mut().resource_mut::<Diplomacy>().standing[a.index()].overlord = Some(d);
            }),
        ),
        (
            "nicht lebt oder nicht frei ist",
            Box::new(move |sim| {
                sim.world_mut().resource_mut::<Diplomacy>().standing[a.index()].overlord = Some(c);
            }),
        ),
        (
            "nicht lebt oder nicht frei ist",
            Box::new(move |sim| {
                sim.world_mut().resource_mut::<Diplomacy>().standing[a.index()].overlord = Some(a);
            }),
        ),
        (
            "Krieg zwischen",
            Box::new(move |sim| {
                sim.world_mut()
                    .resource_mut::<Diplomacy>()
                    .wars
                    .insert((a, c));
            }),
        ),
        // Der Spieler führt eine lebende, freie Gemeinschaft.
        (
            "der Spieler führt",
            Box::new(move |sim| {
                sim.world_mut().resource_mut::<SimConfig>().player = Some(d);
            }),
        ),
        (
            "der Spieler führt",
            Box::new(move |sim| {
                sim.world_mut().resource_mut::<SimConfig>().player = Some(c);
            }),
        ),
    ];
    for (expected, change) in &cases {
        let message = broken(change.as_ref());
        assert!(
            message.contains(expected),
            "erwartet „{expected}“: {message}"
        );
    }
    // Was erlaubt ist, schlägt nicht an: ein Spieler an der Spitze eines Verbunds, ein
    // Krieg zwischen Freien.
    let mut sim = twin(&base);
    sim.world_mut().resource_mut::<SimConfig>().player = Some(b);
    sim.world_mut()
        .resource_mut::<Diplomacy>()
        .wars
        .insert((a, b));
    assert_eq!(check_invariants(sim.world()), Ok(()));
}

#[test]
fn the_number_of_free_states_is_capped() {
    let mut sim = observer(3);
    sim.run_days(12 * DAYS_PER_YEAR);
    let levers = &data().balance.endgame;
    let needed = data().balance.finance.cohesion.independence_min_population;
    let a = PolityId(0);
    let big: Vec<PlanetId> = colonies(&sim, a)
        .into_iter()
        .filter(|planet| population_of_planet(&sim, *planet) >= needed)
        .collect();
    assert!(big.len() >= 2, "das Szenario braucht zwei große Kolonien");
    // Die Obergrenze ist erreicht: Die Region bleibt, wo sie ist, und nichts ändert sich.
    sim.world_mut().resource_mut::<Endgame>().free_states = levers.max_free_states;
    let hash = sim.hash();
    assert_eq!(secede(&mut sim, big[0], a, None), None);
    assert_eq!(sim.hash(), hash);
    // Ein Übertritt zu einer bestehenden Gemeinschaft ist davon nicht berührt und zählt
    // nicht als Gründung.
    assert_eq!(
        secede(&mut sim, big[0], a, Some(PolityId(1))),
        Some(PolityId(1))
    );
    assert_eq!(endgame(&sim).free_states, levers.max_free_states);
    // Einen Platz darunter entsteht der Freistaat, und er wird gezählt.
    sim.world_mut().resource_mut::<Endgame>().free_states = levers.max_free_states - 1;
    let child = secede(&mut sim, big[1], a, None).expect("ein Freistaat entsteht");
    assert_eq!(endgame(&sim).free_states, levers.max_free_states);
    assert!(sim.polities().get(child).alive);
    assert_sound(&sim, "nach der letzten möglichen Gründung");
}

// --- 7. Die Enden ---------------------------------------------------------------------------

/// Lässt die Partie bis zu ihrem Ende laufen und prüft, was für jedes Ende gilt: Der Lauf
/// kehrt vorzeitig zurück, der Zustand steht danach still, und die Chronik nennt das Ende
/// genau einmal. Gibt den Ausgang zurück.
fn run_to_the_end(sim: &mut Headless, at_most_days: u64) -> sim::endgame::Outcome {
    assert_eq!(end_entries(sim), 0);
    assert!(sim.outcome().is_none());
    let started = sim.clock().day();
    sim.run_days(at_most_days);
    let outcome = sim
        .outcome()
        .unwrap_or_else(|| panic!("nach {at_most_days} Tagen kein Ende"));
    let ended = sim.clock();
    assert!(
        ended.day() < started + at_most_days,
        "der Lauf kehrt mit dem Ende zurück"
    );
    // Entschieden wird am Monatsersten; die Uhr bleibt einen Takt danach stehen.
    assert_eq!(outcome.day % DAYS_PER_MONTH, 0);
    assert_eq!(ended.tick, outcome.day * TICKS_PER_DAY + 1);
    assert_eq!(endgame(sim).outcome, Some(outcome));
    assert_eq!(end_entries(sim), 1);
    assert_sound(sim, "am Ende");

    // Danach steht alles still: laufen, takten und Befehle ändern nichts mehr.
    let hash = sim.hash();
    sim.run_days(400);
    sim.run_days_exact(5);
    sim.tick();
    sim.issue(PolityId(0), SimCommand::SetTaxRate { rate: 0.3 });
    sim.run_days(40);
    assert_eq!(sim.hash(), hash, "nach dem Ende ändert sich nichts mehr");
    assert_eq!(sim.clock(), ended);
    assert_eq!(end_entries(sim), 1);
    assert_eq!(sim.outcome(), Some(outcome));
    outcome
}

/// Eine junge Partie, in der Gemeinschaft 0 schon vier der fünf anderen aufgenommen hat,
/// jede auf dem genannten Weg. Übrig ist Gemeinschaft 1.
fn nearly_united(seed: u64, paths: [Path; 4]) -> (Headless, PolityId, PolityId) {
    let (survivor, rival) = (PolityId(0), PolityId(1));
    let mut sim = observer(seed);
    sim.run_days(40);
    let tick = sim.clock().tick;
    for (index, path) in paths.into_iter().enumerate() {
        let from = PolityId(index as u32 + 2);
        sim.succession(|succession| succession.absorb(from, survivor, path, tick))
            .unwrap();
    }
    assert_eq!(sim.polities().alive().count(), 2);
    assert_sound(&sim, "vor dem letzten Schritt");
    // Zwei Souveräne: Noch ist nichts entschieden.
    sim.run_days(2 * DAYS_PER_MONTH);
    assert!(sim.outcome().is_none());
    (sim, survivor, rival)
}

#[test]
fn endings() {
    // Die letzte Rivalin wird unter Zwang unterstellt: Auslöschung, die Überlebende benannt.
    {
        let (mut sim, survivor, rival) = nearly_united(6, [Path::Forced; 4]);
        let tick = sim.clock().tick;
        sim.succession(|succession| succession.subordinate(rival, survivor, false, tick))
            .unwrap();
        let outcome = run_to_the_end(&mut sim, 5 * DAYS_PER_YEAR);
        assert_eq!(outcome.ending, Ending::Extinction);
        assert_eq!(outcome.survivor, Some(survivor));
        assert_eq!(outcome.joined, 0.0);
        // Genommen ist alles außer dem, was der Überlebenden von Anfang an eigen war: fünf
        // Heimatwelten und ihre Kolonien, gezählt im Augenblick des Wechsels.
        let taken: f64 = endgame(&sim).absorbed_pop.iter().sum();
        assert_eq!(outcome.forced, taken);
        assert!(taken > 10_000.0, "genommen {taken}");
        for world in sim
            .galaxy()
            .planets
            .iter()
            .filter(|world| world.owner.is_some())
        {
            let own = world.owner == Some(survivor);
            let path = endgame(&sim).path_of(world.id);
            assert!(
                path == Path::Forced || (own && path == Path::Own),
                "{}",
                world.name
            );
        }
        assert!(
            sim.polities().get(rival).alive,
            "die Unterstellte besteht fort"
        );
        let text = &sim
            .world()
            .resource::<Chronicle>()
            .entries
            .back()
            .unwrap()
            .text;
        assert!(
            text.contains("Auslöschung") && text.contains(&sim.polities().get(survivor).name),
            "{text}"
        );
    }
    // Die letzte Rivalin wird unter Zwang aufgenommen: dasselbe Ende.
    {
        let (mut sim, survivor, rival) = nearly_united(6, [Path::Forced; 4]);
        let tick = sim.clock().tick;
        sim.succession(|succession| succession.absorb(rival, survivor, Path::Forced, tick))
            .unwrap();
        let outcome = run_to_the_end(&mut sim, 5 * DAYS_PER_YEAR);
        assert_eq!(
            (outcome.ending, outcome.survivor),
            (Ending::Extinction, Some(survivor))
        );
        assert!(outcome.forced > outcome.joined);
        assert_eq!(sim.polities().alive().count(), 1);
    }
    // Eine friedliche Mehrheit: Vier sind beigetreten, eine wurde genommen. Metamorphose.
    {
        let (mut sim, survivor, rival) = nearly_united(6, [Path::Joined; 4]);
        let tick = sim.clock().tick;
        sim.succession(|succession| succession.absorb(rival, survivor, Path::Forced, tick))
            .unwrap();
        let outcome = run_to_the_end(&mut sim, 5 * DAYS_PER_YEAR);
        assert_eq!(
            (outcome.ending, outcome.survivor),
            (Ending::Metamorphosis, Some(survivor))
        );
        assert!(outcome.forced > 2000.0, "auch die Genommenen zählen");
        assert!(outcome.joined > 2.0 * outcome.forced);
        let text = &sim
            .world()
            .resource::<Chronicle>()
            .entries
            .back()
            .unwrap()
            .text;
        assert!(text.contains("Metamorphose"), "{text}");
    }
    // Und umgekehrt entscheidet die Mehrheit der Genommenen, auch wenn die letzte beitritt.
    {
        let (mut sim, survivor, rival) = nearly_united(6, [Path::Forced; 4]);
        let tick = sim.clock().tick;
        sim.succession(|succession| succession.subordinate(rival, survivor, true, tick))
            .unwrap();
        let outcome = run_to_the_end(&mut sim, 5 * DAYS_PER_YEAR);
        assert_eq!(
            (outcome.ending, outcome.survivor),
            (Ending::Extinction, Some(survivor))
        );
        assert!(outcome.joined > 0.0 && outcome.forced > outcome.joined);
    }
    // Ohne jeden Eingriff: Erschöpfen sich die Vorkommen ganz, verfällt die Wirtschaft,
    // die Bevölkerung bricht zusammen, und die Partie endet ohne Überlebende.
    {
        let mut sim = Headless::new(
            exhausting_data(),
            SimConfig {
                seed: 2,
                player: None,
            },
        );
        let outcome = run_to_the_end(&mut sim, 120 * DAYS_PER_YEAR);
        assert_eq!(
            (outcome.ending, outcome.survivor),
            (Ending::Extinction, None)
        );
        // Kein Krieg, keine Unterstellung: Der Zusammenbruch ist physisch.
        assert_eq!(sim.polities().alive().count(), 6);
        assert!(diplomacy(&sim).wars.is_empty());
        let levers = &data().balance.endgame;
        let state = endgame(&sim);
        assert!(state.months_below >= levers.collapse_months);
        assert!(total_population(&sim) < levers.collapse_share * state.peak_population);
        assert!(state.peak_population > 20_000.0);
        // Der Boden null gilt: Abgebaute Vorkommen sind restlos erschöpft.
        let exhausted = sim
            .galaxy()
            .planets
            .iter()
            .flat_map(|planet| planet.deposits.iter())
            .filter(|deposit| deposit.reserve == 0.0)
            .count();
        assert!(exhausted >= 6, "{exhausted} Vorkommen erschöpft");
        let text = &sim
            .world()
            .resource::<Chronicle>()
            .entries
            .back()
            .unwrap()
            .text;
        assert!(
            text.contains("Auslöschung") && text.contains("zusammengebrochen"),
            "{text}"
        );
    }
}

/// Spieldaten, in denen sich die Vorkommen tausendmal so schnell erschöpfen und nichts
/// von ihnen bleibt.
fn exhausting_data() -> GameData {
    let mut data = data().clone();
    data.balance.economy.depletion *= 1000.0;
    data.balance.economy.reserve_floor = 0.0;
    data
}

// --- Zufällige Folgen -----------------------------------------------------------------------

/// Ein zufälliger Eingriff in die Partie: Übertragung, Gründung, Ende, Unterstellung,
/// Krieg, Frieden oder ein Flottenmarsch zur Hauptwelt einer anderen. Abgelehnte Aufrufe
/// dürfen nichts verändern.
fn random_upheaval(sim: &mut Headless, rng: &mut Rng) -> String {
    let alive: Vec<PolityId> = sim.polities().alive().map(|polity| polity.id).collect();
    let a = alive[rng.range_usize(0, alive.len())];
    let others: Vec<PolityId> = alive.iter().copied().filter(|id| *id != a).collect();
    let b = others[rng.range_usize(0, others.len())];
    let tick = sim.clock().tick;
    let worlds = planets_of(sim, a);
    let planet = worlds[rng.range_usize(0, worlds.len())];
    let hash = sim.hash();
    let unchanged_if_rejected = |sim: &Headless, result: Result<(), String>| {
        if result.is_err() {
            assert_eq!(sim.hash(), hash, "abgelehnt und doch verändert: {result:?}");
        }
        result.is_ok()
    };
    match rng.range_usize(0, 12) {
        0..=3 => {
            let terms = TransferTerms {
                state_shares_to: [
                    HolderId::State(b),
                    HolderId::Public(planet),
                    HolderId::Elite(planet),
                ][rng.range_usize(0, 3)],
                state_share: rng.f64(),
                cohesion: rng.range_f64(5.0, 80.0),
                stage: [Stage::Loyal, Stage::Restless, Stage::Autonomous][rng.range_usize(0, 3)],
                shock: rng.range_f64(0.5, 1.0),
                elite_flight: rng.f64(),
                path: [Path::Joined, Path::Forced][rng.range_usize(0, 2)],
            };
            let transfer = sim
                .succession(|succession| succession.transfer_planet(planet, b, terms, tick))
                .expect("eine Welt einer lebenden Gemeinschaft lässt sich übertragen");
            format!("Übertragung {planet} von {a} an {b}: {transfer:?}")
        }
        4 | 5 => {
            // Eine Kolonie gründet einen neuen Staat, solange es nicht zu viele werden.
            let free = colonies(sim, a);
            if free.is_empty() || sim.polities().list.len() >= 14 {
                return format!("{a} hat keine Kolonie für einen neuen Staat");
            }
            let colony = free[rng.range_usize(0, free.len())];
            let child = found_free_state(sim, colony);
            format!("Gründung von {child} auf {colony} aus {a}")
        }
        6 => {
            let path = [Path::Joined, Path::Forced][rng.range_usize(0, 2)];
            // Nicht die vorletzte Gemeinschaft: Die Partie soll weiterlaufen.
            if alive.len() <= 3 {
                return "zu wenige Gemeinschaften für ein weiteres Ende".into();
            }
            sim.succession(|succession| succession.absorb(a, b, path, tick))
                .expect("zwei lebende Gemeinschaften");
            format!("Ende von {a} in {b} ({})", path.label())
        }
        7 | 8 => {
            let voluntary = rng.chance(0.5);
            let result = sim.succession(|succession| succession.subordinate(a, b, voluntary, tick));
            let done = unchanged_if_rejected(sim, result);
            format!("Unterstellung von {a} unter {b}: {done}")
        }
        9 => {
            sim.issue(a, SimCommand::MakePeace { target: b });
            format!("{a} bietet {b} Frieden")
        }
        _ => {
            // Krieg, und die Flotten marschieren zur Hauptwelt der anderen: Das gibt ein
            // Gefecht, in das die späteren Eingriffe hineinfallen. (Wer untersteht, darf
            // keinen Krieg erklären; dann marschiert die Flotte nur.)
            sim.issue(a, SimCommand::DeclareWar { target: b });
            let to = sim.galaxy().planet(capital(sim, b)).system;
            let own: Vec<FleetId> = fleets(sim).of(a).map(|fleet| fleet.id).collect();
            for fleet in &own {
                sim.issue(a, SimCommand::MoveFleet { fleet: *fleet, to });
            }
            format!(
                "{a} erklärt {b} den Krieg und schickt {} Flotten",
                own.len()
            )
        }
    }
}

#[test]
fn random_upheavals_keep_every_rule() {
    let mut battles_seen = 0;
    for seed in [21, 22, 23, 24] {
        let mut sim = observer(seed);
        sim.run_days(3 * DAYS_PER_YEAR);
        let mut rng = Rng::stream(seed, Domain::Polity, 4242, 0);
        let mut history: Vec<String> = Vec::new();
        for step in 0..60 {
            if sim.outcome().is_some() {
                break;
            }
            let before = Before::of(&sim);
            let event = random_upheaval(&mut sim, &mut rng);
            let when = format!("Seed {seed}, Schritt {step}: {event}");
            history.push(event);
            // Der Eingriff selbst schafft und vernichtet nichts …
            before.assert_conserved(&sim, &when);
            // … und die Partie läuft danach weiter, ohne eine Regel zu brechen.
            sim.run_days(20 + rng.range_u32(0, 80) as u64);
            assert_sound(&sim, &format!("{when}, danach"));
        }
        // Die Folge war nicht harmlos: Gemeinschaften haben geendet, andere sind entstanden.
        let ended = sim
            .polities()
            .list
            .iter()
            .filter(|polity| !polity.alive)
            .count();
        assert!(
            history.len() >= 20 || sim.outcome().is_some(),
            "Seed {seed}: {history:#?}"
        );
        assert!(ended >= 1, "Seed {seed}: niemand hat geendet: {history:#?}");
        assert!(
            sim.polities().list.len() > 6,
            "Seed {seed}: keine Gründung: {history:#?}"
        );
        battles_seen += battles(&sim).next_id;
    }
    // Auch Gefechte liefen, während Gemeinschaften endeten und sich unterstellten.
    assert!(battles_seen >= 1);
}

// --- 8. Determinismus -----------------------------------------------------------------------

/// Eine Partie, in der nach zwei Jahren hintereinander alle vier Vorgänge geschehen.
fn eventful(seed: u64) -> Headless {
    let mut sim = observer(seed);
    sim.run_days(2 * DAYS_PER_YEAR);
    upheaval(&mut sim);
    sim
}

/// Alle vier Vorgänge auf einmal: Gründung, Übertragung unter Zwang, friedliches Ende,
/// Unterstellung.
fn upheaval(sim: &mut Headless) {
    let tick = sim.clock().tick;
    let (a, b, c, d) = (PolityId(0), PolityId(1), PolityId(2), PolityId(3));
    let colony = colonies(sim, a)[0];
    found_free_state(sim, colony);
    let taken = colonies(sim, b)[0];
    sim.succession(|succession| succession.transfer_planet(taken, c, forced_terms(c), tick))
        .unwrap();
    sim.succession(|succession| succession.absorb(d, a, Path::Joined, tick))
        .unwrap();
    sim.succession(|succession| succession.subordinate(b, c, false, tick))
        .unwrap();
}

#[test]
fn determinism() {
    // Derselbe Seed, dieselben Eingriffe: derselbe Verlauf. Ein anderer Seed: ein anderer.
    let mut first = eventful(14);
    let mut second = eventful(14);
    assert_eq!(first.hash(), second.hash());
    assert_ne!(first.hash(), eventful(15).hash());
    assert_ne!(
        first.hash(),
        {
            let mut plain = observer(14);
            plain.run_days(2 * DAYS_PER_YEAR);
            plain.hash()
        },
        "die Eingriffe haben die Welt verändert"
    );

    // Speichern und Laden unmittelbar nach den Eingriffen stellt den Zustand genau her.
    let text = first.save().to_ron().unwrap();
    let save = SaveGame::from_ron(&text).unwrap();
    assert!(save.matches_data(data()));
    let mut loaded = Headless::from_save(data().clone(), save);
    assert_eq!(loaded.hash(), first.hash());
    assert_sound(&loaded, "nach dem Laden");

    // Mit Leertakten oder ohne, am Stück oder geladen: derselbe Zustand.
    first.run_days(400);
    second.run_days_exact(400);
    loaded.run_days(150);
    loaded.run_days(250);
    assert_eq!(first.clock(), second.clock());
    assert_eq!(first.hash(), second.hash());
    assert_eq!(loaded.hash(), first.hash());
    assert_sound(&first, "400 Tage nach den Eingriffen");
    assert_eq!(first.polities().alive().count(), 6);
}
