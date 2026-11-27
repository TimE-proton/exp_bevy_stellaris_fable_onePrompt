//! Der Staatshaushalt in der laufenden Partie: was Flotte und Forschung kosten, wem es
//! fehlt und wann das Geld nicht mehr reicht.
//!
//! Die Rechenregeln eines Monatsabschlusses prüfen die Tests neben dem Code in
//! `sim::economy`. Hier laufen ganze Partien über Jahre, ohne dass ein Zustand von Hand
//! hergestellt würde: Verglichen wird dieselbe Partie mit mehr, mit weniger und ohne
//! Rüstung, und ein langer Krieg nach Drehbuch.

use sim::GameData;
use sim::ai::MilitaryMode;
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

/// Eine Partie, in der die militärische Führung der KI ruht: Diese Prüfungen messen den
/// Haushalt an Flotten fester Größe, die das Drehbuch vorgibt. Eine KI, die selbst rüstet,
/// stilllegt und ihren Forschungsetat kürzt, würde genau das verändern, was verglichen
/// wird. Wie sie mit dem Haushalt umgeht, prüft `tests/ai.rs`.
fn staged(data: GameData, config: SimConfig) -> Headless {
    let mut sim = Headless::new(data, config);
    sim.set_military_mode(None, MilitaryMode::Resting);
    sim
}

/// Spielt eine Partie ohne Eingriff und summiert jedes Jahr die abgeschlossenen
/// Haushaltsmonate aller Gemeinschaften.
fn play(data: &GameData, seed: u64, years: u64) -> (Headless, Totals) {
    let mut sim = staged(data.clone(), SimConfig { seed, player: None });
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
    // dass jemand die Steuer anhebt. Gemessen wird ab dem dritten Jahr, fünf Jahre lang.
    let large = with_fleets(4);
    for seed in [3, 4] {
        let measure = |data: &GameData| -> Vec<Strain> {
            let mut sim = staged(data.clone(), SimConfig { seed, player: None });
            let count = sim.polities().list.len();
            let mut strain = vec![Strain::default(); count];
            for month in 1..=7 * 12 {
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
        let usual_games = measure(data());
        for (index, usual) in usual_games.iter().enumerate() {
            assert!(usual.balance > 0, "Seed {seed}, G{index}: {usual:?}");
            assert!(usual.handout > 0, "Seed {seed}, G{index}: {usual:?}");
            assert_eq!(usual.max_arrears, 0.0, "Seed {seed}, G{index}: {usual:?}");
            assert!(
                usual.below_reserve * 2 < usual.months,
                "Seed {seed}, G{index}: {usual:?}"
            );
        }
        // Mit dem Vierfachen nicht: Die Ansprüche verlangen mehr, als die Monate einbringen,
        // die Kasse liegt fast dauernd unter der Rücklage, die Ausschüttung ist eingestellt
        // (in einem einzelnen Monat über der Rücklage fällt höchstens ein Rest an, unter
        // einem Hundertstel des Üblichen), und wer sich das Geld nicht leiht, bleibt
        // seiner Flotte Sold schuldig.
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
            assert!(
                strained.handout * 100 < usual_games[index].handout,
                "Seed {seed}, G{index}: {strained:?} gegen {:?}",
                usual_games[index]
            );
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
    // Zwei Gemeinschaften führen vom fünften Jahr an sechzehn Jahre Krieg: Jedes Jahr greift
    // jede einmal das Heimatsystem der anderen an, und beide rüsten dabei auf das
    // Vierfache der Startflotte, bauen also nach, was sie verlieren, und auf, was fehlt.
    // Alles geschieht über Befehle des Drehbuchs, wie ein Spieler sie gäbe; ihre Finanzen
    // führt die KI (Kriegskasse, Anleihen). Anleihen tragen einen solchen Krieg über Jahre;
    // der Sold bleibt erst aus, wenn der Schuldendienst die Einnahmen aufzehrt.
    let (a, b) = (PolityId(0), PolityId(1));
    let start = 5 * DAYS_PER_YEAR;
    let navy = |who: PolityId| ScriptedEvent {
        day: start,
        act: ScriptedAct::Navy {
            who: Some(who),
            factor: 4.0,
        },
    };
    let mut events = vec![navy(a), navy(b)];
    for year in 5..21 {
        events.push(ScriptedEvent::war(year * DAYS_PER_YEAR, a, b));
        events.push(ScriptedEvent::war(year * DAYS_PER_YEAR + 180, b, a));
    }
    let script = WarScript::new(events);
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
    // Wie ein Krieg verläuft, hängt an der Partie. Geprüft werden drei; in mindestens
    // zweien muss eine Kriegspartei ihrer Flotte spürbar Sold schuldig bleiben.
    let mut struck = 0;
    for seed in [0, 5, 6] {
        let mut war = staged(data().clone(), SimConfig { seed, player: None });
        let mut peace = staged(data().clone(), SimConfig { seed, player: None });
        // Je Kriegspartei: der höchste Rückstand und die Moral ihrer Besatzungen in dem
        // Monat, in dem er erreicht wurde, gegen die Moral, die ihre Kriegsmüdigkeit
        // allein ergäbe.
        let mut worst = [(0.0f32, 0.0f32, 0.0f32); 2];
        let mut checked = [false; 2];
        for month in 1..=21 * 12 {
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
                if arrears > 0.0 && !checked[index] {
                    // Der Monat, in dem der Sold zum ersten Mal ausbleibt: Der Haushalt
                    // konnte die Flotte nicht voll bedienen, die Kasse liegt unter der
                    // Rücklage, ausgeschüttet wird nichts, und die Chronik meldet es.
                    checked[index] = true;
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
        // Ein Krieg mit Gefechten und Verlusten, nicht nur ein Wettrüsten.
        let battles = war.world().resource::<Battles>();
        assert!(
            battles.next_id >= 5,
            "Seed {seed}: {} Gefechte",
            battles.next_id
        );
        assert!(battles.total_ships_lost() >= 5, "Seed {seed}");
        assert_eq!(notices(&peace, a) + notices(&peace, b), 0);
        assert_eq!(money_drift(war.world()), Some(0));
        assert_eq!(check_invariants(war.world()), Ok(()));
        // Wem Sold ausblieb, dessen Besatzungen gehen mit schlechterer Moral in den Kampf,
        // als die Kriegsmüdigkeit allein erklärt.
        let (arrears, morale, rested) = worst
            .into_iter()
            .max_by(|x, y| x.0.total_cmp(&y.0))
            .unwrap();
        if arrears > 0.1 {
            struck += 1;
            let penalty = data().balance.military.morale_per_arrears * arrears;
            assert!((morale - (rested - penalty).max(0.0)).abs() < 1e-3);
            assert!(morale < rested);
        }
    }
    assert!(
        struck >= 2,
        "nur in {struck} von drei Kriegen blieb eine Kriegspartei Sold schuldig"
    );
}

#[test]
fn cutting_the_tax_does_not_cut_the_civil_base() {
    use sim::command::SimCommand;
    // Dieselbe Partie zweimal; in der einen erhebt die vom Menschen geführte Gemeinschaft
    // vom ersten Tag an keine Lohnsteuer mehr. Die Grundleistungen bemessen sich am
    // üblichen Satz: Sie bleiben, und wer die Steuer streicht, muss anderswo kürzen.
    let me = PolityId(0);
    let play = |rate: Option<f64>| {
        let mut sim = staged(
            data().clone(),
            SimConfig {
                seed: 3,
                player: Some(me),
            },
        );
        if let Some(rate) = rate {
            sim.issue(me, SimCommand::SetTaxRate { rate });
        }
        sim.run_days(2 * DAYS_PER_YEAR);
        let year = budget(&sim, me).year();
        assert_eq!(check_invariants(sim.world()), Ok(()));
        (year.revenue, year.line(Claim::Civil).wanted, year.handout())
    };
    let (revenue, civil, handout) = play(None);
    let (less, same, nothing) = play(Some(0.0));
    assert!(
        (less as f64) < revenue as f64 * 0.9,
        "ohne Lohnsteuer nimmt der Staat spürbar weniger ein: {less} gegen {revenue}"
    );
    assert!(
        (same as f64) > civil as f64 * 0.9,
        "die Grundleistungen bleiben: {same} gegen {civil}"
    );
    assert!(
        nothing < handout,
        "was an Steuer fehlt, fehlt der Ausschüttung: {nothing} gegen {handout}"
    );
}

/// Läuft bis zu einem bestimmten Takt: ganze Tage im Schnellgang, den Rest Takt für Takt.
fn advance_to(sim: &mut Headless, tick: u64) {
    use sim::clock::TICKS_PER_DAY;
    while sim.clock().tick < tick {
        let now = sim.clock().tick;
        if now.is_multiple_of(TICKS_PER_DAY) && tick - now >= TICKS_PER_DAY {
            sim.run_days((tick - now) / TICKS_PER_DAY);
        } else {
            sim.tick();
        }
    }
}

#[test]
fn a_tax_rate_raised_only_around_the_close_does_not_cut_the_civil_base() {
    use sim::clock::TICKS_PER_DAY;
    use sim::command::SimCommand;
    // Dieselbe Partie zweimal. In der einen hebt die vom Menschen geführte Gemeinschaft
    // die Lohnsteuer jeweils nur für die wenigen Takte um den Monatsabschluss auf den
    // Höchstsatz und stellt sie gleich wieder zurück. Eingenommen wird dabei so gut wie
    // dasselbe; also müssen auch die Grundleistungen dieselben bleiben.
    let me = PolityId(0);
    let usual = data().balance.economy.income_tax;
    let play = |trick: bool| {
        let mut sim = staged(
            data().clone(),
            SimConfig {
                seed: 3,
                player: Some(me),
            },
        );
        for month in 1..=24u64 {
            let close = month * DAYS_PER_MONTH * TICKS_PER_DAY;
            advance_to(&mut sim, close - 2);
            if trick {
                sim.issue(me, SimCommand::SetTaxRate { rate: 0.6 });
            }
            advance_to(&mut sim, close + 2);
            if trick {
                sim.issue(me, SimCommand::SetTaxRate { rate: usual });
            }
        }
        advance_to(
            &mut sim,
            24 * DAYS_PER_MONTH * TICKS_PER_DAY + TICKS_PER_DAY,
        );
        assert_eq!(check_invariants(sim.world()), Ok(()));
        let year = budget(&sim, me).year();
        (
            year.revenue,
            year.standard_revenue,
            year.line(Claim::Civil).paid,
        )
    };
    let (revenue, standard, civil) = play(false);
    let (tricked_revenue, tricked_standard, tricked_civil) = play(true);
    // Bis auf die Rundung je Lohnzahlung.
    assert!(
        (revenue - standard).abs() < revenue / 10_000,
        "zum üblichen Satz ist beides dasselbe: {revenue} gegen {standard}"
    );
    let near = |a: Money, b: Money| (a as f64 - b as f64).abs() < 0.1 * b as f64;
    assert!(
        near(tricked_revenue, revenue),
        "eingenommen wird fast dasselbe: {tricked_revenue} gegen {revenue}"
    );
    assert!(
        near(tricked_standard, standard),
        "die Einnahmen zum üblichen Satz bleiben: {tricked_standard} gegen {standard}"
    );
    assert!(
        near(tricked_civil, civil),
        "die Grundleistungen bleiben: {tricked_civil} gegen {civil}"
    );
}

#[test]
fn money_borrowed_after_the_close_can_be_spent_on_procurement_at_once() {
    use sim::command::SimCommand;
    use sim::economy::to_money;
    // Wer mitten im Monat Anleihen begibt, weil der Werft das Geld fehlt, muss es gleich
    // ausgeben können und nicht erst nach dem nächsten Abschluss.
    let me = PolityId(0);
    let mut sim = staged(
        data().clone(),
        SimConfig {
            seed: 5,
            player: Some(me),
        },
    );
    sim.run_days(DAYS_PER_YEAR + 1);
    let allowed = |sim: &Headless| {
        sim.world()
            .resource::<Economy>()
            .allowance(me, Claim::Procurement)
    };
    let treasury = |sim: &Headless| sim.world().resource::<Economy>().treasury(me);
    let (before, cash) = (allowed(&sim), treasury(&sim));
    sim.issue(me, SimCommand::IssueBonds { amount: 20_000.0 });
    sim.tick();
    sim.tick();
    let raised = treasury(&sim) - cash;
    assert!(
        raised > to_money(5_000.0),
        "die Anleihe wurde gezeichnet: {raised}"
    );
    assert!(
        allowed(&sim) - before >= raised * 9 / 10,
        "die Beschaffung darf das geliehene Geld ausgeben: {} nach {before}, Kasse +{raised}",
        allowed(&sim)
    );
    assert_eq!(check_invariants(sim.world()), Ok(()));
}

#[test]
fn the_fleet_gets_its_spare_parts_while_the_treasury_can_pay() {
    // Eine Partie ohne Eingriff, drei Jahre nach dem ersten (in dem die Märkte die
    // Ersatzteile noch nicht führen): In Monaten, in denen dem Haushalt kein Geld fehlte,
    // bekommt die Flotte, was sie verlangt, auch wenn die Ersatzteile im Lauf des Monats
    // teurer werden als am Monatsersten veranschlagt.
    let mut sim = staged(
        data().clone(),
        SimConfig {
            seed: 2,
            player: None,
        },
    );
    sim.run_days(DAYS_PER_YEAR);
    let (mut wanted, mut paid, mut months, mut short) = (0, 0, 0, 0);
    for _ in 0..3 {
        sim.run_days(DAYS_PER_YEAR);
        for polity in &sim.polities().list {
            for month in &budget(&sim, polity.id).history {
                let line = month.line(Claim::Fleet);
                if month.unfunded > 0 || line.wanted == 0 {
                    continue;
                }
                months += 1;
                wanted += line.wanted;
                paid += line.paid;
                if (line.paid as f64) < 0.97 * line.wanted as f64 {
                    short += 1;
                }
            }
        }
    }
    assert!(months > 150, "Monate ohne Geldmangel: {months}");
    assert!(
        paid as f64 >= 0.99 * wanted as f64,
        "die Flotte bekam {paid} von {wanted}"
    );
    assert!(
        short * 20 <= months,
        "in {short} von {months} Monaten blieb die Flotte ohne Geldmangel unter 97 %"
    );
}
