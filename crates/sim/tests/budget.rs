//! Der Staatshaushalt in der laufenden Partie: was Flotte und Forschung kosten, wem es
//! fehlt und wann das Geld nicht mehr reicht.
//!
//! Die Rechenregeln eines Monatsabschlusses prüfen die Tests neben dem Code in
//! `sim::economy`. Hier laufen ganze Partien über Jahre, ohne dass ein Zustand von Hand
//! hergestellt würde: Verglichen wird dieselbe Partie mit mehr, mit weniger und ohne
//! Rüstung, und ein langer Krieg nach Drehbuch.

use sim::GameData;
use sim::chronicle::Chronicle;
use sim::clock::{DAYS_PER_MONTH, DAYS_PER_YEAR};
use sim::economy::{Budget, Claim, Economy, Money};
use sim::finance::Finance;
use sim::fleet::{Battles, Fleets, crew_morale};
use sim::harness::{Headless, ScriptedAct, ScriptedEvent, WarScript, check_invariants};
use sim::ids::PolityId;
use sim::state::{SimConfig, money_drift};
use std::sync::OnceLock;

/// Die Spieldaten des Workspace, einmal geladen.
fn data() -> &'static GameData {
    static DATA: OnceLock<GameData> = OnceLock::new();
    DATA.get_or_init(|| GameData::load_workspace().unwrap())
}

/// Dieselben Daten, aber jede Gemeinschaft beginnt mit dem `factor`-fachen der Startflotte.
fn with_fleets(factor: u32) -> GameData {
    let mut data = data().clone();
    for entry in &mut data.balance.military.starting_fleet_ids {
        entry.1 *= factor;
    }
    for entry in &mut data.balance.military.starting_fleet {
        entry.1 *= factor;
    }
    data
}

/// Dieselben Daten, aber Flotte und Forschung kosten nichts: kein Sold, keine Wartung,
/// kein Verbrauch auf Station, kein Forschungsetat.
fn without_armament() -> GameData {
    let mut data = data().clone();
    let military = &mut data.balance.military;
    military.upkeep_per_month = 0.0;
    military.maintenance_per_month = 0.0;
    military.fuel_per_day = 0.0;
    data.balance.research.budget_share = 0.0;
    for template in &mut data.polities {
        template.research_share = Some(0.0);
    }
    data
}

fn budget(sim: &Headless, polity: PolityId) -> &Budget {
    sim.world()
        .resource::<Economy>()
        .budget(polity)
        .expect("jede Gemeinschaft führt einen Haushalt")
}

/// Was die Haushalte aller Gemeinschaften einer Partie über ihre Laufzeit ergeben haben.
#[derive(Clone, Copy, Debug, Default)]
struct Totals {
    revenue: Money,
    fleet: Money,
    research: Money,
    handout: Money,
    /// Deckung des Bedarfs der Haushalte, bevölkerungsgewichtet über alle Welten und
    /// gemittelt über die Jahresenden.
    needs_met: f64,
}

impl Totals {
    fn share(&self, amount: Money) -> f64 {
        amount as f64 / self.revenue.max(1) as f64
    }
}

/// Deckung des Bedarfs der Haushalte aller besiedelten Welten, bevölkerungsgewichtet.
fn needs_met(sim: &Headless) -> f64 {
    let economy = sim.world().resource::<Economy>();
    let (mut people, mut met) = (0.0, 0.0);
    for planet in &sim.galaxy().planets {
        if planet.owner.is_none() || planet.population <= 0.0 {
            continue;
        }
        let local = economy.planet(planet.id);
        people += planet.population;
        met += planet.population * local.needs_met.iter().sum::<f64>()
            / local.needs_met.len().max(1) as f64;
    }
    met / people
}

/// Spielt eine Partie ohne Eingriff und summiert jedes Jahr die abgeschlossenen
/// Haushaltsmonate aller Gemeinschaften.
fn play(data: &GameData, seed: u64, years: u64) -> (Headless, Totals) {
    let mut sim = Headless::new(data.clone(), SimConfig { seed, player: None });
    let mut totals = Totals::default();
    for _ in 0..years {
        sim.run_days(DAYS_PER_YEAR);
        for polity in &sim.polities().list {
            for month in &budget(&sim, polity.id).history {
                totals.revenue += month.revenue;
                totals.fleet += month.line(Claim::Fleet).paid;
                totals.research += month.line(Claim::Research).paid;
                totals.handout += month.handout();
            }
        }
        totals.needs_met += needs_met(&sim) / years as f64;
    }
    assert_eq!(money_drift(sim.world()), Some(0), "Seed {seed}");
    assert_eq!(check_invariants(sim.world()), Ok(()), "Seed {seed}");
    (sim, totals)
}

#[test]
fn armament_has_a_price_the_households_pay_in_every_game() {
    // Acht Partien, jede dreimal: wie sie ist, mit doppelter Startflotte, und ohne
    // Kosten für Flotte und Forschung. Fünf Jahre, alle Gemeinschaften zusammen.
    let doubled_data = with_fleets(2);
    let free_data = without_armament();
    for seed in 0..8 {
        let (_, usual) = play(data(), seed, 5);
        let (_, doubled) = play(&doubled_data, seed, 5);
        let (_, free) = play(&free_data, seed, 5);

        // Die Größenordnung, auf die `balance.ron` ausgelegt ist: Die Startflotte kostet
        // rund ein Zehntel der laufenden Staatseinnahmen, die Forschung zum üblichen Etat
        // etwas mehr.
        let fleet = usual.share(usual.fleet);
        let research = usual.share(usual.research);
        assert!(
            (0.08..=0.12).contains(&fleet),
            "Seed {seed}: Flotte {fleet:.3}"
        );
        assert!(
            (0.10..=0.15).contains(&research),
            "Seed {seed}: Forschung {research:.3}"
        );

        // Eine zweite Flotte kostet noch einmal so viel …
        let twice = doubled.fleet as f64 / usual.fleet as f64;
        assert!((1.8..=2.3).contains(&twice), "Seed {seed}: {twice:.2}-mal");
        // … und was sie kostet, fehlt der Ausschüttung und dem Verbrauch der Haushalte.
        assert!(
            (doubled.handout as f64) < usual.handout as f64 * 0.7,
            "Seed {seed}: Ausschüttung {} mit doppelter Flotte, {} sonst",
            doubled.handout,
            usual.handout
        );
        assert!(
            doubled.needs_met < usual.needs_met - 0.01,
            "Seed {seed}: Bedarf gedeckt {:.3} mit doppelter Flotte, {:.3} sonst",
            doubled.needs_met,
            usual.needs_met
        );

        // Umgekehrt: Wer für Flotte und Forschung nichts ausgibt, schüttet weit mehr aus,
        // und die Haushalte verbrauchen sichtbar mehr.
        assert_eq!((free.fleet, free.research), (0, 0));
        assert!(
            free.handout as f64 > usual.handout as f64 * 1.7,
            "Seed {seed}: Ausschüttung {} ohne Rüstung, {} sonst",
            free.handout,
            usual.handout
        );
        assert!(
            free.needs_met > usual.needs_met + 0.025,
            "Seed {seed}: Bedarf gedeckt {:.3} ohne Rüstung, {:.3} sonst",
            free.needs_met,
            usual.needs_met
        );
    }
}

/// Wie es um den Haushalt einer Gemeinschaft über eine Reihe von Monaten stand.
#[derive(Clone, Debug, Default)]
struct Strain {
    months: u32,
    /// Monate, in denen die Kasse nach dem Abschluss unter der Rücklage lag.
    below_reserve: u32,
    /// Monate, in denen die Ansprüche mehr verlangten, als der Monat einbrachte.
    deficit: u32,
    /// Einnahmen abzüglich dessen, was die Ansprüche verlangten, über alle Monate:
    /// negativ heißt, der Haushalt trägt seine Ansprüche nicht.
    balance: Money,
    handout: Money,
    /// Höchster Soldrückstand der ganzen Partie.
    max_arrears: f32,
    /// Ob die Gemeinschaft Anleihen ausgegeben hat.
    borrowed: bool,
}

#[test]
fn four_times_the_starting_fleet_runs_a_deficit_within_a_few_years() {
    // Dieselbe Partie mit der Startflotte und mit ihrem Vierfachen, im Frieden und ohne
    // dass jemand die Steuer anhebt. Gemessen wird ab dem dritten Jahr, drei Jahre lang.
    let large = with_fleets(4);
    for seed in [3, 5] {
        let measure = |data: &GameData| -> Vec<Strain> {
            let mut sim = Headless::new(data.clone(), SimConfig { seed, player: None });
            let count = sim.polities().list.len();
            let mut strain = vec![Strain::default(); count];
            for month in 1..=5 * 12 {
                sim.run_days(DAYS_PER_MONTH);
                for (index, entry) in strain.iter_mut().enumerate() {
                    let id = PolityId::from_index(index);
                    let arrears = sim.world().resource::<Fleets>().arrears_of(id);
                    entry.max_arrears = entry.max_arrears.max(arrears);
                    let finance = sim.world().resource::<Finance>();
                    entry.borrowed |= finance.bonds.iter().any(|bond| bond.issuer == id);
                    if month <= 24 {
                        continue;
                    }
                    let last = budget(&sim, id).last().expect("ein abgeschlossener Monat");
                    entry.months += 1;
                    entry.below_reserve += u32::from(last.treasury < last.reserve_target);
                    entry.deficit += u32::from(last.claimed() > last.revenue);
                    entry.balance += last.revenue - last.claimed();
                    entry.handout += last.handout();
                }
            }
            assert_eq!(
                sim.world().resource::<Economy>().tax_rates,
                vec![data.balance.economy.income_tax; count],
                "niemand hat die Steuer angehoben"
            );
            assert_eq!(check_invariants(sim.world()), Ok(()));
            strain
        };
        // Mit der Startflotte trägt sich der Haushalt: Es bleibt etwas übrig, es wird
        // ausgeschüttet, niemand bleibt Sold schuldig.
        for (index, usual) in measure(data()).iter().enumerate() {
            assert!(usual.balance > 0, "Seed {seed}, G{index}: {usual:?}");
            assert!(usual.handout > 0, "Seed {seed}, G{index}: {usual:?}");
            assert_eq!(usual.max_arrears, 0.0, "Seed {seed}, G{index}: {usual:?}");
            assert!(
                usual.below_reserve * 2 < usual.months,
                "Seed {seed}, G{index}: {usual:?}"
            );
        }
        // Mit dem Vierfachen nicht: Die Ansprüche verlangen mehr, als die Monate einbringen,
        // die Kasse liegt dauernd unter der Rücklage, die Ausschüttung ist eingestellt,
        // und wer sich das Geld nicht leiht, bleibt seiner Flotte Sold schuldig.
        for (index, strained) in measure(&large).iter().enumerate() {
            assert!(strained.balance < 0, "Seed {seed}, G{index}: {strained:?}");
            assert!(
                strained.deficit * 3 >= strained.months,
                "Seed {seed}, G{index}: {strained:?}"
            );
            assert!(
                strained.below_reserve * 10 >= strained.months * 9,
                "Seed {seed}, G{index}: {strained:?}"
            );
            assert_eq!(strained.handout, 0, "Seed {seed}, G{index}: {strained:?}");
            assert!(
                strained.max_arrears > 0.0 || strained.borrowed,
                "Seed {seed}, G{index}: {strained:?}"
            );
        }
    }
}

#[test]
fn arrears_occur_in_a_long_scripted_war() {
    use sim::battle::CombatantSetup;
    // Zwei Gemeinschaften führen ab dem fünften Jahr Krieg und halten ihre Marine dabei
    // auf dem Dreifachen der Startflotte: Was sie verlieren, bauen sie nach, was fehlt,
    // bauen sie auf. Alles geschieht über Befehle des Drehbuchs, wie ein Spieler sie gäbe.
    let (a, b) = (PolityId(0), PolityId(1));
    let start = 5 * DAYS_PER_YEAR;
    let navy = |who: PolityId| ScriptedEvent {
        day: start,
        act: ScriptedAct::Navy {
            who: Some(who),
            factor: 3.0,
        },
    };
    let script = WarScript::new(vec![ScriptedEvent::war(start, a, b), navy(a), navy(b)]);
    let notices = |sim: &Headless, polity: PolityId| {
        sim.world()
            .resource::<Chronicle>()
            .entries
            .iter()
            .filter(|entry| {
                entry.polity == Some(polity)
                    && entry.text.contains("kann seine Flotte nicht mehr bezahlen")
            })
            .count()
    };
    for seed in [1, 2] {
        let mut war = Headless::new(data().clone(), SimConfig { seed, player: None });
        let mut peace = Headless::new(data().clone(), SimConfig { seed, player: None });
        // Je Kriegspartei: der höchste Rückstand und die Moral ihrer Besatzungen in dem
        // Monat, in dem er erreicht wurde, gegen die Moral, die ihre Kriegsmüdigkeit
        // allein ergäbe.
        let mut worst = [(0.0f32, 0.0f32, 0.0f32); 2];
        let mut checked = false;
        for month in 1..=12 * 12 {
            script.run_until(&mut war, month * DAYS_PER_MONTH, false);
            peace.run_days(DAYS_PER_MONTH);
            for (index, polity) in [a, b].into_iter().enumerate() {
                let world = war.world();
                let (fleets, finance) = (world.resource::<Fleets>(), world.resource::<Finance>());
                let arrears = fleets.arrears_of(polity);
                assert_eq!(
                    peace.world().resource::<Fleets>().arrears_of(polity),
                    0.0,
                    "Seed {seed}: im Frieden bleibt niemand Sold schuldig"
                );
                if arrears > worst[index].0 {
                    let weary = finance.polities[polity.index()].war_weariness;
                    let rested = CombatantSetup::new(polity, data())
                        .with_war_weariness(data(), weary)
                        .morale;
                    worst[index] = (
                        arrears,
                        crew_morale(data(), finance, fleets, polity),
                        rested,
                    );
                }
                if arrears > 0.0 && !checked {
                    // Der Monat, in dem der Sold zum ersten Mal ausbleibt: Der Haushalt
                    // konnte die Flotte nicht voll bedienen, die Kasse liegt unter der
                    // Rücklage, ausgeschüttet wird nichts, und die Chronik meldet es.
                    checked = true;
                    let month = &budget(&war, polity).current;
                    let fleet = month.line(Claim::Fleet);
                    assert!(month.unfunded > 0, "Seed {seed}: {month:?}");
                    assert!(fleet.paid < fleet.wanted, "Seed {seed}: {fleet:?}");
                    assert!(month.treasury < month.reserve_target);
                    assert_eq!(month.handout(), 0);
                    assert_eq!(notices(&war, polity), 1);
                }
            }
        }
        let battles = war.world().resource::<Battles>();
        assert!(battles.next_id > 0, "Seed {seed}: kein Gefecht");
        assert!(
            battles.total_ships_lost() > 0,
            "Seed {seed}: keine Verluste"
        );
        // Mindestens eine der beiden Kriegsparteien blieb ihrer Flotte einen spürbaren
        // Teil dessen schuldig, was sie kostet, und ihre Besatzungen gingen deshalb mit
        // schlechterer Moral in den Kampf, als die Kriegsmüdigkeit allein erklärt.
        let (arrears, morale, rested) = worst
            .into_iter()
            .max_by(|x, y| x.0.total_cmp(&y.0))
            .unwrap();
        assert!(
            arrears > 0.1,
            "Seed {seed}: höchster Soldrückstand der Kriegsparteien {worst:?}"
        );
        let penalty = data().balance.military.morale_per_arrears * arrears;
        assert!((morale - (rested - penalty).max(0.0)).abs() < 1e-3);
        assert!(morale < rested);
        assert_eq!(notices(&peace, a) + notices(&peace, b), 0);
        assert_eq!(money_drift(war.world()), Some(0));
        assert_eq!(check_invariants(war.world()), Ok(()));
    }
}
