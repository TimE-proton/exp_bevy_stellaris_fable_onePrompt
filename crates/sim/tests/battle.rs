//! Der taktische Gefechtsrechner: Fairness zwischen den Seiten, Konter der Klassen, Geleit,
//! Flankieren, Nachschub, Technik, Ausfälle, Moral, Determinismus, Ergebnisse, Aufstellung.
//!
//! Gemessen wird mit vielen Seeds; die Gefechte laufen auf mehreren Kernen. Jedes einzelne
//! ist deterministisch, die Reihenfolge der Ergebnisse folgt den Seeds.

use sim::GameData;
use sim::battle::matchup::{self, Outcome, Side};
use sim::battle::{
    Battle, CombatModifiers, CombatantSetup, Doctrine, Fate, Formation, ShipSpec, ShipStatus, V2,
};
use sim::data::{ModuleKind, ShipClass, WeaponKind};
use sim::ids::{BattleId, PolityId, ShipId, SystemId};
use std::sync::OnceLock;

fn data() -> &'static GameData {
    static DATA: OnceLock<GameData> = OnceLock::new();
    DATA.get_or_init(|| GameData::load_workspace().expect("Spieldaten"))
}

fn design(key: &str) -> u16 {
    data().design_index(key).expect("Entwurf")
}

fn fleet(text: &str) -> Side {
    Side::parse(data(), text).expect("Flotte")
}

/// Bauwert, bei dem die Klassen gegeneinander antreten: zwei Schlachtschiffe.
fn budget() -> f64 {
    data().designs[design("bastion") as usize].stats.value * 2.0
}

fn class_fleet(key: &str) -> Side {
    Side::for_budget(data(), design(key), budget())
}

/// Ein Gefecht je Seed, verteilt auf die Kerne.
fn fight_seeds(seeds: std::ops::Range<u64>, sides: &[Side]) -> Vec<Outcome> {
    let seeds: Vec<u64> = seeds.collect();
    let jobs = std::thread::available_parallelism()
        .map(|count| count.get())
        .unwrap_or(1)
        .clamp(1, 8);
    let chunk = seeds.len().div_ceil(jobs).max(1);
    std::thread::scope(|scope| {
        let handles: Vec<_> = seeds
            .chunks(chunk)
            .map(|part| {
                scope.spawn(move || {
                    part.iter()
                        .map(|seed| matchup::fight(data(), *seed, sides))
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|handle| handle.join().expect("Gefecht"))
            .collect()
    })
}

fn wins(outcomes: &[Outcome], side: usize) -> usize {
    outcomes
        .iter()
        .filter(|outcome| outcome.winner() == Some(side))
        .count()
}

fn mean_loss(outcomes: &[Outcome], side: usize) -> f32 {
    outcomes
        .iter()
        .map(|outcome| outcome.sides[side].loss_share())
        .sum::<f32>()
        / outcomes.len().max(1) as f32
}

/// Kein NaN, keine Schiffe weit außerhalb des Feldes, Bilanz stimmig.
fn assert_sane(battle: &Battle) {
    let exit = data().balance.battle.exit_radius;
    for ship in &battle.ships {
        assert!(ship.pos.x.is_finite() && ship.pos.y.is_finite());
        assert!(ship.heading.is_finite() && ship.morale.is_finite());
        assert!(ship.hull.is_finite() && ship.shield.is_finite());
        if ship.status == ShipStatus::Active {
            assert!(
                ship.pos.length() <= exit * 1.001,
                "kämpfendes Schiff jenseits des Randes"
            );
        }
    }
    for combatant in &battle.combatants {
        assert!(combatant.lost_value <= combatant.initial_value + 1.0);
    }
}

// --- 1. Fairness ------------------------------------------------------------------------

/// Spiegelgefechte einer Flotte: Siege von A und B, mittlere Verluste, und wie weit der
/// Unterschied der Verluste je Gefecht streut.
struct Mirror {
    a: usize,
    b: usize,
    runs: usize,
    stalemates: usize,
    /// Mittel und Standardfehler von (Verlust A - Verlust B).
    loss_gap: f32,
    loss_gap_error: f32,
    winner_loss: f32,
    loser_loss: f32,
}

fn mirror(side: &Side, seeds: std::ops::Range<u64>) -> Mirror {
    let outcomes = fight_seeds(seeds, &[side.clone(), side.clone()]);
    let n = outcomes.len() as f32;
    let gaps: Vec<f32> = outcomes
        .iter()
        .map(|outcome| outcome.sides[0].loss_share() - outcome.sides[1].loss_share())
        .collect();
    let mean = gaps.iter().sum::<f32>() / n;
    let variance = gaps.iter().map(|gap| (gap - mean).powi(2)).sum::<f32>() / (n - 1.0);
    let (mut winner_loss, mut loser_loss) = (0.0, 0.0);
    for outcome in &outcomes {
        if let Some(winner) = outcome.winner() {
            winner_loss += outcome.sides[winner].loss_share();
            loser_loss += outcome.sides[1 - winner].loss_share();
        }
    }
    let (a, b) = (wins(&outcomes, 0), wins(&outcomes, 1));
    let decided = (a + b).max(1) as f32;
    Mirror {
        a,
        b,
        runs: outcomes.len(),
        stalemates: outcomes.iter().filter(|outcome| outcome.stalemate).count(),
        loss_gap: mean,
        loss_gap_error: (variance / n).sqrt(),
        winner_loss: winner_loss / decided,
        loser_loss: loser_loss / decided,
    }
}

impl Mirror {
    /// Abweichung des Siegesanteils von A von der Hälfte, in Standardabweichungen einer
    /// fairen Münze bei dieser Zahl entschiedener Gefechte.
    fn z(&self) -> f32 {
        let decided = (self.a + self.b).max(1) as f32;
        (self.a as f32 / decided - 0.5) / (0.25 / decided).sqrt()
    }
}

/// Spiegelgefechte sind gerecht: Über viele Seeds gewinnt A so oft wie B, auf drei
/// Standardabweichungen genau, und verliert gleich viel. 400 Seeds je Flotte, 800 für die
/// Kreuzer: Eine Schieflage wie die frühere versetzte Zielwahl (gut 58 % für A) fiele
/// auf, 60 Seeds mit einem Band von 35 bis 65 % ließen sie durch.
#[test]
fn mirror_matches_are_fair() {
    let fleets: Vec<(&str, Side, u64)> = vec![
        ("dolch", class_fleet("dolch"), 400),
        ("waechter", class_fleet("waechter"), 400),
        ("speer", fleet("speer:5"), 800),
        ("bastion", class_fleet("bastion"), 400),
        (
            "gemischt",
            fleet("bastion:1,speer:1,waechter:1,dolch:2"),
            400,
        ),
    ];
    let (mut pooled_a, mut pooled_b) = (0, 0);
    for (label, side, seeds) in fleets {
        let result = mirror(&side, 0..seeds);
        // Unentschieden darf nur ein Patt bleiben (Fregatten kommen mit wenigen Lasern
        // manchmal nicht mehr durch die Schilde), und auch das selten.
        let decided = result.a + result.b;
        assert_eq!(
            decided + result.stalemates,
            result.runs,
            "{label}: Gefechte ohne Sieger und ohne Patt"
        );
        assert!(
            decided * 10 >= result.runs * 8,
            "{label}: nur {decided} von {} entschieden",
            result.runs
        );
        assert!(
            result.z().abs() < 3.0,
            "{label}: A gewinnt {}, B {} (z = {:.1})",
            result.a,
            result.b,
            result.z()
        );
        assert!(
            result.loss_gap.abs() < 3.0 * result.loss_gap_error,
            "{label}: Verluste A - B im Mittel {:.3} ± {:.3}",
            result.loss_gap,
            result.loss_gap_error
        );
        // Entschieden heißt: Der Verlierer hat deutlich mehr verloren als der Sieger.
        assert!(
            result.loser_loss > result.winner_loss + 0.1,
            "{label}: Sieger verliert {:.2}, Verlierer {:.2}",
            result.winner_loss,
            result.loser_loss
        );
        pooled_a += result.a;
        pooled_b += result.b;
    }
    let pooled = Mirror {
        a: pooled_a,
        b: pooled_b,
        runs: 0,
        stalemates: 0,
        loss_gap: 0.0,
        loss_gap_error: 0.0,
        winner_loss: 0.0,
        loser_loss: 0.0,
    };
    assert!(
        pooled.z().abs() < 3.0,
        "alle Flotten zusammen: A {pooled_a}, B {pooled_b} (z = {:.1})",
        pooled.z()
    );
}

/// Zwei Trägerflotten: Die Jäger reiben sich auf, danach rücken die Träger heran und
/// schießen mit ihren Geschützen, kommen aber nicht durch die Schilde des anderen. Das ist
/// ein Patt, kein Sieg dessen, der zuletzt getroffen hat.
#[test]
fn carrier_mirrors_end_in_stalemate_not_in_a_coin_toss() {
    let result = mirror(&class_fleet("horst"), 0..60);
    assert!(
        result.stalemates * 10 >= result.runs * 9,
        "nur {} von {} als Patt beendet (A {}, B {})",
        result.stalemates,
        result.runs,
        result.a,
        result.b
    );
    assert!(result.loss_gap.abs() < 3.0 * result.loss_gap_error.max(0.005));
}

// --- 2. Konter ----------------------------------------------------------------------------

/// Zwei Flotten aus je einem Entwurf zum gleichen Bauwert: die andere Seite so groß, wie
/// das Budget es hergibt, die bevorzugte so groß, dass sie höchstens 3 % mehr wert ist.
/// Billiger darf sie sein (das erschwert ihr nur den Sieg), aber nicht um mehr als 10 %.
fn equal_value_pair(favoured: &str, other: &str) -> (Side, Side) {
    let value = |key: &str| data().designs[design(key) as usize].stats.value;
    let (strong_value, weak_value) = (value(favoured), value(other));
    let weak = ((budget() / weak_value).round() as u32).max(1);
    let target = weak as f64 * weak_value;
    let strong = ((target * 1.03) / strong_value).floor().max(1.0) as u32;
    let ratio = strong as f64 * strong_value / target;
    assert!(
        (0.9..=1.03).contains(&ratio),
        "{favoured} gegen {other}: kein gleicher Bauwert ({strong} gegen {weak}, {ratio:.3})"
    );
    (
        Side::new(vec![(design(favoured), strong)]),
        Side::new(vec![(design(other), weak)]),
    )
}

/// Gewinnt `favoured` gegen `other` bei gleichem Bauwert in beiden Aufstellungen mindestens
/// 70 % der Gefechte (je 24 Seeds)?
fn assert_counter(favoured: &str, other: &str) {
    let (strong, weak) = equal_value_pair(favoured, other);
    let seeds = 0..24u64;
    let as_a = fight_seeds(seeds.clone(), &[strong.clone(), weak.clone()]);
    let as_b = fight_seeds(seeds.clone(), &[weak.clone(), strong.clone()]);
    let (won_a, won_b) = (wins(&as_a, 0), wins(&as_b, 1));
    let needed = (0.7f32 * as_a.len() as f32).ceil() as usize;
    assert!(
        won_a >= needed && won_b >= needed,
        "{favoured} ({}×) gegen {other} ({}×): {won_a}/{} als A, {won_b}/{} als B",
        strong.ship_count(),
        weak.ship_count(),
        as_a.len(),
        as_b.len()
    );
}

#[test]
fn equal_value_pairs_really_are_equal() {
    // Früher reichte ein Fenster von ±10 %: 14 Korvetten gegen 2 Träger waren 8 % mehr
    // wert, und der Konter hing an dieser Rundung.
    let (corvettes, carriers) = equal_value_pair("dolch", "horst");
    let ratio = corvettes.value(data()) / carriers.value(data());
    assert!((0.97..=1.03).contains(&ratio), "{ratio:.3}");
}

#[test]
fn frigates_beat_corvettes() {
    assert_counter("waechter", "dolch");
}

#[test]
fn cruisers_beat_frigates_and_corvettes() {
    assert_counter("speer", "waechter");
    assert_counter("speer", "dolch");
}

#[test]
fn battleships_beat_cruisers_and_frigates() {
    assert_counter("bastion", "speer");
    assert_counter("bastion", "waechter");
}

#[test]
fn corvettes_beat_battleships_and_carriers() {
    assert_counter("dolch", "bastion");
    assert_counter("dolch", "horst");
}

#[test]
fn carriers_beat_battleships_and_lose_to_frigates() {
    assert_counter("horst", "bastion");
    assert_counter("waechter", "horst");
}

// --- 3. Geleit ----------------------------------------------------------------------------

#[test]
fn escorts_protect_battleships_against_fighters_and_torpedoes() {
    // Gleicher Gesamtwert: das Geleit ist sogar etwas billiger als das zweite Schlachtschiff.
    let alone = fleet("bastion:2");
    let escorted = fleet("bastion:1,waechter:5");
    assert!(escorted.value(data()) <= alone.value(data()));
    for (enemy, label) in [
        (fleet("horst:2"), "Träger"),
        (class_fleet("dolch"), "Korvetten"),
    ] {
        let alone_runs = fight_seeds(0..12, &[alone.clone(), enemy.clone()]);
        let escorted_runs = fight_seeds(0..12, &[escorted.clone(), enemy.clone()]);
        let (alone_wins, escorted_wins) = (wins(&alone_runs, 0), wins(&escorted_runs, 0));
        let (alone_loss, escorted_loss) = (mean_loss(&alone_runs, 0), mean_loss(&escorted_runs, 0));
        let (alone_dealt, escorted_dealt) =
            (mean_loss(&alone_runs, 1), mean_loss(&escorted_runs, 1));
        assert!(
            escorted_wins >= alone_wins + 4 || escorted_loss + 0.2 < alone_loss,
            "gegen {label}: allein {alone_wins} Siege ({alone_loss:.2} Verlust), mit Geleit \
             {escorted_wins} ({escorted_loss:.2})"
        );
        assert!(
            escorted_dealt > alone_dealt,
            "gegen {label}: Verluste des Gegners allein {alone_dealt:.2}, mit Geleit {escorted_dealt:.2}"
        );
    }
    // Die Punktverteidigung des Geleits fängt tatsächlich Lenkwaffen ab.
    let mut battle = matchup::build(data(), 5, &[escorted, fleet("horst:2")]);
    let mut intercepts = 0;
    for _ in 0..1500 {
        battle.step(data());
        intercepts += battle
            .beams
            .iter()
            .filter(|beam| {
                beam.side == 0 && beam.kind == sim::battle::BeamKind::PointDefense && beam.hit
            })
            .count();
    }
    assert!(
        intercepts > 50,
        "nur {intercepts} Treffer der Punktverteidigung"
    );
}

#[test]
fn a_balanced_mixed_fleet_holds_its_own() {
    // Gemessen über 40 Seeds je Aufstellung: Die gemischte Flotte schlägt Korvetten,
    // Fregatten und Träger fast immer und hält gegen reine Kreuzer etwa die Waage. Gegen
    // zwei Schlachtschiffe verliert sie meist (gut drei von vier Gefechten) und fügt ihnen
    // nur gut 15 % Verluste zu: Deren schwere Geschütze reichen weiter als alles, was sie
    // hat, und ihr eigenes Schlachtschiff steht allein. Das ist eine bekannte Schwäche, kein
    // Ziel; gefordert wird hier, was gilt und die Rolle ausmacht.
    let mixed = fleet("bastion:1,speer:2,waechter:1,dolch:1");
    let value = mixed.value(data());
    for (key, min_share, min_dealt) in [
        ("dolch", 0.7, 0.15),
        ("waechter", 0.7, 0.3),
        ("speer", 0.3, 0.25),
        ("bastion", 0.1, 0.1),
        ("horst", 0.7, 0.4),
    ] {
        let other = Side::for_budget(data(), design(key), value);
        let as_a = fight_seeds(0..24, &[mixed.clone(), other.clone()]);
        let as_b = fight_seeds(0..24, &[other, mixed.clone()]);
        let (won_a, won_b) = (wins(&as_a, 0), wins(&as_b, 1));
        let dealt = (mean_loss(&as_a, 1) + mean_loss(&as_b, 0)) * 0.5;
        let needed = (min_share * 24.0f32).ceil() as usize;
        assert!(
            won_a >= needed && won_b >= needed && dealt >= min_dealt,
            "gegen {key}: {won_a}/24 als A, {won_b}/24 als B, Gegner verliert {dealt:.2}"
        );
    }
}

// --- 4. Flankieren ------------------------------------------------------------------------

/// Zwei gleiche Linien sind im Gefecht; dann trifft ein Verband als dritte, verbündete
/// Partei ein, gleich weit vom Gegner und zur selben Zeit, entweder vor dessen Bug (wo seine
/// Geschütze hinzeigen) oder in seinem Rücken. Gemessen wird, was der Verband in den
/// folgenden 30 Sekunden anrichtet und einsteckt (Panzerung und Rumpf).
fn reinforcement_exchange(seed: u64, behind: bool) -> (f32, f32) {
    let data = data();
    let line = fleet("speer:4");
    let mut battle = matchup::build(data, seed, &[line.clone(), line]);
    // Bis kurz nach dem ersten Kontakt.
    while battle
        .contact_time
        .is_none_or(|time| battle.time < time + 10.0)
    {
        battle.step(data);
        assert!(!battle.finished);
    }
    let setup = CombatantSetup::new(PolityId(0), data);
    let specs = fleet("speer:2").specs(data);
    let side = battle.add_combatant(data, setup, &specs);
    battle.set_hostile(0, side, false);
    battle.set_hostile(1, side, true);
    let enemy = &battle.combatants[1];
    let bow = V2::from_angle(enemy.facing);
    let center = enemy.anchor;
    let at = if behind {
        center + bow * -650.0
    } else {
        center + bow * 650.0
    };
    battle.deploy(side, at, (center - at).angle());
    for _ in 0..300 {
        battle.step(data);
    }
    let force = &battle.combatants[side];
    let dealt = force.damage_dealt.iter().sum::<f32>();
    let taken = force.damage_taken;
    matchup::finish(data, &mut battle);
    assert!(battle.finished && !battle.timed_out);
    (dealt, taken)
}

#[test]
fn arriving_in_the_enemy_rear_pays_off() {
    let seeds: Vec<u64> = (0..12).collect();
    let results: Vec<((f32, f32), (f32, f32))> = std::thread::scope(|scope| {
        let handles: Vec<_> = seeds
            .iter()
            .map(|seed| {
                scope.spawn(move || {
                    (
                        reinforcement_exchange(*seed, true),
                        reinforcement_exchange(*seed, false),
                    )
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join().expect("Gefecht"))
            .collect()
    });
    let total = |pick: fn(&((f32, f32), (f32, f32))) -> (f32, f32)| {
        results
            .iter()
            .map(pick)
            .fold((0.0, 0.0), |acc, (dealt, taken)| {
                (acc.0 + dealt, acc.1 + taken)
            })
    };
    let (rear_dealt, rear_taken) = total(|pair| pair.0);
    let (front_dealt, front_taken) = total(|pair| pair.1);
    // Im Rücken trifft der Verband dünnere Panzerung und Ziele, deren Geschütze nach vorn
    // zeigen: Er richtet mehr an und steckt weniger ein.
    assert!(
        rear_dealt > 1.25 * front_dealt,
        "angerichtet im Rücken {rear_dealt:.0}, vor dem Bug {front_dealt:.0}"
    );
    assert!(
        rear_taken < front_taken,
        "eingesteckt im Rücken {rear_taken:.0}, vor dem Bug {front_taken:.0}"
    );
}

// --- 5. Nachschub -------------------------------------------------------------------------

#[test]
fn empty_magazines_lose_and_only_beams_keep_firing() {
    let full = class_fleet("speer");
    let empty = class_fleet("speer").with_supplies(0.03, 1.0);
    let as_a = fight_seeds(0..12, &[full.clone(), empty.clone()]);
    let as_b = fight_seeds(0..12, &[empty, full]);
    assert!(wins(&as_a, 0) >= 10 && wins(&as_b, 1) >= 10);

    // Ganz ohne Munition: keine Geschosse und Lenkwaffen der Partei, aber Strahlen.
    let dry = class_fleet("speer").with_supplies(0.0, 1.0);
    let mut battle = matchup::build(data(), 2, &[dry, class_fleet("speer")]);
    let mut beams = 0;
    for _ in 0..1200 {
        battle.step(data());
        assert!(
            battle.shots.iter().all(|shot| shot.side != 0),
            "ohne Munition darf Partei A nichts verschießen"
        );
        beams += battle
            .beams
            .iter()
            .filter(|beam| beam.side == 0 && beam.kind != sim::battle::BeamKind::PointDefense)
            .count();
        if battle.finished {
            break;
        }
    }
    assert!(beams > 20, "Strahlwaffen müssen weiterfeuern ({beams})");
    // Und die Munition der vollen Seite nimmt ab.
    let used: f32 = battle
        .ships
        .iter()
        .filter(|ship| ship.side == 1)
        .map(|ship| ship.ammo_max - ship.ammo)
        .sum();
    assert!(used > 0.0);
}

// --- 6. Technik ---------------------------------------------------------------------------

fn tech_share(modifiers: CombatModifiers, fleet_text: &str, seeds: std::ops::Range<u64>) -> f32 {
    let plain = fleet(fleet_text);
    let advanced = fleet(fleet_text).with_modifiers(modifiers);
    let as_a = fight_seeds(seeds.clone(), &[advanced.clone(), plain.clone()]);
    let as_b = fight_seeds(seeds.clone(), &[plain, advanced]);
    let won = wins(&as_a, 0) + wins(&as_b, 1);
    won as f32 / (as_a.len() + as_b.len()) as f32
}

#[test]
fn one_tier_of_weapons_wins_a_clear_majority() {
    // `waffen_2` gibt +8 % Schaden.
    let tech = data().tech_index("waffen_2").expect("waffen_2");
    assert!(
        data().techs[tech as usize]
            .effects
            .contains(&sim::data::TechEffect::WeaponDamage(0.08))
    );
    let modifiers = CombatModifiers {
        damage: 1.08,
        ..Default::default()
    };
    let share = tech_share(modifiers, "speer:5", 0..20);
    assert!(share >= 0.6, "eine Waffenstufe gewinnt nur {share:.2}");
}

#[test]
fn a_full_tier_of_weapons_and_defence_wins_almost_always() {
    // Alle Technologien bis Stufe 2 in Waffen und Verteidigung: Reichweite +5 %, Schaden
    // +8 %, Rumpf +8 %, Schilde +12 %. Eine Flottenstärke von zehn Kreuzern und 40 Seeds
    // je Aufstellung, damit der Vorsprung nicht im Zufall weniger Treffer untergeht.
    let modifiers = CombatModifiers {
        damage: 1.08,
        range: 1.05,
        shields: 1.12,
        armor: 1.0,
        hull: 1.08,
        speed: 1.0,
    };
    let share = tech_share(modifiers, "speer:10", 0..40);
    assert!(share >= 0.9, "eine volle Stufe gewinnt nur {share:.2}");
}

// --- 7. Module ----------------------------------------------------------------------------

fn spec_with(design_key: &str, damaged: impl Fn(&ModuleKind) -> bool) -> ShipSpec {
    let id = design(design_key);
    let design = &data().designs[id as usize];
    let mut spec = ShipSpec::fresh(data(), id);
    spec.modules = design
        .modules
        .iter()
        .map(|slot| {
            if damaged(&data().modules[slot.module_id as usize].kind) {
                0.0
            } else {
                1.0
            }
        })
        .collect();
    spec
}

fn two_ship_battle(first: ShipSpec, second: ShipSpec) -> Battle {
    let mut battle = Battle::new(BattleId(1), SystemId(0), 4);
    let setup = CombatantSetup::new(PolityId(0), data());
    battle.add_combatant(data(), setup, &[first, second]);
    battle
}

/// Ohne Gegner endet ein Gefecht sofort, und die Flotte steht. Hier soll sie nur fahren:
/// ein Takt mit vorgegebener Fahrt des Bezugspunkts.
fn cruise(battle: &mut Battle, velocity: V2) {
    battle.finished = false;
    battle.combatants[0].anchor_vel = velocity;
    battle.step(data());
    battle.combatants[0].anchor_vel = velocity;
}

#[test]
fn lost_engines_cost_mobility_and_damaged_shields_hold_less() {
    let intact = spec_with("speer", |_| false);
    let no_engines = spec_with("speer", |kind| matches!(kind, ModuleKind::Engine { .. }));
    let mut battle = two_ship_battle(intact.clone(), no_engines);
    let (start_a, start_b) = (battle.ships[0].pos, battle.ships[1].pos);
    assert_eq!(battle.ships[1].mobility, 0.0);
    for _ in 0..300 {
        cruise(&mut battle, V2::new(30.0, 0.0));
    }
    let moved_a = battle.ships[0].pos.distance(start_a);
    let moved_b = battle.ships[1].pos.distance(start_b);
    assert!(moved_a > 500.0, "mit Antrieb nur {moved_a:.0}");
    assert!(
        moved_b < 0.4 * moved_a,
        "ohne Antrieb {moved_b:.0}, mit {moved_a:.0}"
    );

    let mut weak_shield = intact.clone();
    let design = &data().designs[design("speer") as usize];
    weak_shield.modules = design
        .modules
        .iter()
        .map(|slot| match data().modules[slot.module_id as usize].kind {
            ModuleKind::Shield { .. } => 0.2,
            _ => 1.0,
        })
        .collect();
    let battle = two_ship_battle(intact, weak_shield);
    assert!(battle.ships[1].shield_max < 0.7 * battle.ships[0].shield_max);
    assert!(battle.ships[1].shield_regen < battle.ships[0].shield_regen);
}

#[test]
fn long_battles_leave_damaged_modules_and_results_report_them() {
    let side = fleet("speer:8");
    let mut battle = matchup::build(data(), 11, &[side.clone(), side]);
    matchup::finish(data(), &mut battle);
    let results = battle.results(data());
    assert_eq!(results.len(), 16);
    let survivors: Vec<_> = results
        .iter()
        .filter(|result| result.fate != Fate::Lost)
        .collect();
    assert!(!survivors.is_empty());
    assert!(
        survivors
            .iter()
            .any(|result| result.modules.iter().any(|module| *module < 0.999)),
        "kein Überlebender mit beschädigtem Modul"
    );
    assert!(survivors.iter().any(|result| result.hull < 0.999));
}

// --- 8. Moral und Rückzug -----------------------------------------------------------------

#[test]
fn an_outnumbered_fleet_breaks_off_and_saves_ships() {
    let outcomes = fight_seeds(0..12, &[fleet("speer:3"), fleet("speer:8")]);
    let mut saved = 0;
    for outcome in &outcomes {
        assert_eq!(outcome.winner(), Some(1));
        assert!(outcome.sides[0].retreated || outcome.sides[0].escaped.iter().sum::<u32>() > 0);
        saved += outcome.sides[0].escaped.iter().sum::<u32>();
    }
    assert!(saved >= 6, "nur {saved} Schiffe entkommen");
}

#[test]
fn even_battles_leave_the_loser_some_ships() {
    let outcomes = fight_seeds(0..20, &[fleet("speer:10"), fleet("speer:10")]);
    let (mut winner_loss, mut loser_loss, mut decided) = (0.0, 0.0, 0);
    for outcome in &outcomes {
        if let Some(winner) = outcome.winner() {
            decided += 1;
            winner_loss += outcome.sides[winner].loss_share();
            loser_loss += outcome.sides[1 - winner].loss_share();
        }
    }
    let (winner_loss, loser_loss) = (winner_loss / decided as f32, loser_loss / decided as f32);
    assert!(decided >= 16);
    assert!(
        (0.3..=0.7).contains(&(1.0 - winner_loss)),
        "Sieger behält {:.2}",
        1.0 - winner_loss
    );
    assert!(loser_loss < 0.85, "Verlierer verliert {loser_loss:.2}");
}

fn retreat_time(doctrine: Doctrine, seed: u64) -> Option<f32> {
    let weak = fleet("speer:4").with_doctrine(doctrine);
    let mut battle = matchup::build(data(), seed, &[weak, fleet("speer:6")]);
    matchup::finish(data(), &mut battle);
    let contact = battle.contact_time.unwrap_or(0.0);
    battle.combatants[0].retreat_time.map(|time| time - contact)
}

#[test]
fn defensive_doctrine_retreats_earlier_than_aggressive() {
    let seeds = 0..12u64;
    let mean = |doctrine: Doctrine| {
        let times: Vec<f32> = seeds
            .clone()
            .map(|seed| retreat_time(doctrine, seed).unwrap_or(f32::INFINITY))
            .collect();
        times.iter().map(|time| time.min(400.0)).sum::<f32>() / times.len() as f32
    };
    let (defensive, aggressive) = (mean(Doctrine::Defensive), mean(Doctrine::Aggressive));
    assert!(
        defensive + 10.0 < aggressive,
        "defensiv {defensive:.0} s, offensiv {aggressive:.0} s"
    );
}

#[test]
fn routing_ships_escape_and_retreat_is_never_mutual() {
    for seed in 0..12 {
        let side = fleet("speer:6");
        let mut battle = matchup::build(data(), seed, &[side.clone(), side]);
        matchup::finish(data(), &mut battle);
        assert_sane(&battle);
        // Höchstens eine Seite zieht ab (nur ein Patt löst beide, und das gibt es zwischen
        // Kreuzern nicht).
        let retreated = battle
            .combatants
            .iter()
            .filter(|combatant| combatant.retreat_time.is_some())
            .count();
        assert!(retreated <= 1, "Seed {seed}: beide Seiten ziehen ab");
        assert!(
            battle
                .combatants
                .iter()
                .all(|combatant| !combatant.stalemate)
        );
    }
    // Wer flieht, kommt auch weg.
    let outcomes = fight_seeds(0..12, &[fleet("speer:3"), fleet("speer:8")]);
    assert!(
        outcomes
            .iter()
            .filter(|outcome| outcome.sides[0].escaped.iter().sum::<u32>() > 0)
            .count()
            >= 6
    );
}

// --- 9. Determinismus ---------------------------------------------------------------------

fn bytes(battle: &Battle) -> Vec<u8> {
    postcard::to_stdvec(battle).expect("postcard")
}

#[test]
fn same_seed_gives_identical_states() {
    let sides = [
        fleet("horst:1,speer:2,dolch:3"),
        fleet("bastion:1,waechter:3"),
    ];
    let mut first = matchup::build(data(), 21, &sides);
    let mut second = matchup::build(data(), 21, &sides);
    for checkpoint in 0..8 {
        for _ in 0..250 {
            first.step(data());
            second.step(data());
        }
        assert_eq!(bytes(&first), bytes(&second), "Prüfpunkt {checkpoint}");
    }
}

#[test]
fn a_battle_saved_midway_continues_identically() {
    let sides = [
        fleet("horst:1,speer:2,dolch:3"),
        fleet("bastion:1,waechter:3"),
    ];
    let mut straight = matchup::build(data(), 33, &sides);
    matchup::finish(data(), &mut straight);
    let mut saved = matchup::build(data(), 33, &sides);
    let (mut checkpoints, mut with_missiles, mut with_fighters) = (0, 0, 0);
    while !saved.finished {
        for _ in 0..41 {
            saved.step(data());
        }
        // Gespeichert wird nur mitten im Gefecht mit Geschossen im Flug, denn deren Zustand
        // ist das Heikle; dazu gezählt, wie oft Lenkwaffen und gestartete Jäger dabei waren.
        let missiles = saved
            .shots
            .iter()
            .any(|shot| shot.kind == WeaponKind::Missile);
        let fighters = saved
            .ships
            .iter()
            .any(|ship| ship.class == ShipClass::Fighter && ship.status.in_play());
        if saved.finished || saved.shots.is_empty() {
            continue;
        }
        checkpoints += 1;
        with_missiles += missiles as u32;
        with_fighters += fighters as u32;
        let via_postcard: Battle =
            postcard::from_bytes(&postcard::to_stdvec(&saved).expect("postcard"))
                .expect("postcard");
        let text = ron::to_string(&saved).expect("ron");
        let via_ron: Battle = ron::from_str(&text).expect("ron");
        for mut copy in [via_postcard, via_ron] {
            matchup::finish(data(), &mut copy);
            assert_eq!(
                bytes(&copy),
                bytes(&straight),
                "Prüfpunkt Takt {}",
                saved.tick
            );
        }
    }
    assert!(
        checkpoints >= 8 && with_missiles >= 2 && with_fighters >= 4,
        "{checkpoints} Prüfpunkte mit Geschossen im Flug, {with_missiles} mit Lenkwaffen, \
         {with_fighters} mit Jägern"
    );
}

// --- 10. Verstärkung, drei Parteien, Tempo --------------------------------------------------

#[test]
fn reinforcements_and_three_way_battles_end_properly() {
    // Verstärkung mitten im Gefecht.
    let mut battle = matchup::build(data(), 8, &[fleet("speer:3"), fleet("speer:4")]);
    for _ in 0..500 {
        battle.step(data());
    }
    let setup = CombatantSetup::new(PolityId(0), data());
    let side = battle.add_combatant(data(), setup, &fleet("speer:2,waechter:2").specs(data()));
    assert_eq!(battle.hostile.len(), 9);
    battle.set_hostile(0, side, false);
    battle.set_hostile(1, side, true);
    assert!(battle.is_hostile(1, 2) && battle.is_hostile(2, 1) && !battle.is_hostile(0, 2));
    assert!(battle.is_hostile(0, 1));
    matchup::finish(data(), &mut battle);
    assert!(battle.finished && !battle.timed_out);
    assert!(battle.combatants[side].damage_dealt.iter().sum::<f32>() > 0.0);
    assert_sane(&battle);

    // Drei Parteien, jede gegen jede.
    for seed in 0..6 {
        let sides = [fleet("speer:3"), fleet("waechter:5"), fleet("dolch:7")];
        let mut battle = matchup::build(data(), seed, &sides);
        matchup::finish(data(), &mut battle);
        assert!(battle.finished && !battle.timed_out, "Seed {seed}");
        assert!(battle.victors().len() <= 1);
        assert_sane(&battle);
    }
}

#[test]
fn a_battle_whose_last_ships_drift_ends() {
    let mut battle = matchup::build(data(), 3, &[fleet("speer:2"), fleet("speer:2")]);
    for _ in 0..300 {
        battle.step(data());
    }
    for ship in battle.ships.iter_mut().filter(|ship| ship.side == 0) {
        ship.status = ShipStatus::Disabled;
    }
    let ticks = battle.tick;
    while !battle.finished {
        battle.step(data());
        assert!(battle.tick < ticks + 100, "Gefecht endet nicht");
    }
    assert_eq!(battle.victors(), vec![1]);
    // Treibende Schiffe des Verlierers gehen an den Gegner verloren.
    let results = battle.results(data());
    assert!(
        results
            .iter()
            .filter(|result| result.side == 0)
            .all(|result| result.fate == Fate::Lost)
    );
}

#[test]
fn standard_battles_finish_in_time() {
    let classes = ["dolch", "waechter", "speer", "bastion", "horst"];
    for a in classes {
        for b in classes {
            let outcomes = fight_seeds(0..3, &[class_fleet(a), class_fleet(b)]);
            for outcome in outcomes {
                assert!(!outcome.timed_out, "{a} gegen {b} läuft in die Zeitgrenze");
            }
        }
    }
}

#[test]
fn pacing_stays_in_its_windows() {
    let cruisers = fight_seeds(0..12, &[fleet("speer:3"), fleet("speer:3")]);
    let mean = cruisers.iter().map(Outcome::fighting_time).sum::<f32>() / cruisers.len() as f32;
    assert!(
        (90.0..=240.0).contains(&mean),
        "3 gegen 3 Kreuzer: {mean:.0} s nach dem Kontakt"
    );
    let mixed = "horst:1,bastion:2,speer:5,waechter:6,dolch:6";
    let big = fight_seeds(0..6, &[fleet(mixed), fleet(mixed)]);
    for outcome in big {
        assert!(
            outcome.fighting_time() <= 600.0,
            "20 gegen 20: {:.0} s",
            outcome.fighting_time()
        );
        assert!(!outcome.timed_out);
    }
}

// --- 11. Ergebnisse -----------------------------------------------------------------------

#[test]
fn results_report_fates_fractions_and_returning_fighters() {
    let carrier = design("horst");
    let capacity = data().designs[carrier as usize].stats.fighters;
    let mut battle = matchup::build(data(), 6, &[fleet("horst:2,waechter:2"), fleet("speer:4")]);
    // Bis die Staffeln gestartet sind.
    for _ in 0..400 {
        battle.step(data());
    }
    let snapshot = battle.results(data());
    assert_eq!(snapshot.len(), 8);
    for result in &snapshot {
        assert!(result.fate == Fate::Holding || result.fate == Fate::Lost);
    }
    // Ein Träger fällt aus; seine Jäger fliegen weiter.
    let carriers: Vec<usize> = (0..battle.ships.len())
        .filter(|index| battle.ships[*index].class == ShipClass::Carrier)
        .collect();
    let aloft = battle
        .ships
        .iter()
        .filter(|ship| ship.class == ShipClass::Fighter && ship.side == 0 && ship.status.in_play())
        .count() as u32;
    assert!(aloft >= 3, "zu wenige Jäger in der Luft ({aloft})");
    battle.ships[carriers[0]].status = ShipStatus::Destroyed;
    // Im verbliebenen Träger ist nur noch für zwei Jäger Platz.
    battle.ships[carriers[1]].hangar = capacity - 2;
    let results = battle.results(data());
    let lost = results
        .iter()
        .find(|result| result.source == battle.ships[carriers[0]].source.unwrap())
        .unwrap();
    assert_eq!(lost.fate, Fate::Lost);
    assert_eq!(lost.fighters, 0);
    assert_eq!(lost.hull, 0.0);
    let kept = results
        .iter()
        .find(|result| result.source == battle.ships[carriers[1]].source.unwrap())
        .unwrap();
    // Mehr Jäger überleben, als in den Hangar passen: Er wird voll, mehr nicht.
    assert_eq!(kept.fighters, capacity);
    battle.ships[carriers[1]].hangar = 0;
    let kept = battle
        .results(data())
        .into_iter()
        .find(|result| result.source == battle.ships[carriers[1]].source.unwrap())
        .unwrap();
    assert_eq!(kept.fighters, aloft.min(capacity));

    matchup::finish(data(), &mut battle);
    let results = battle.results(data());
    assert_eq!(results.len(), 8);
    let in_order: Vec<ShipId> = battle.ships.iter().filter_map(|ship| ship.source).collect();
    assert_eq!(
        results
            .iter()
            .map(|result| result.source)
            .collect::<Vec<_>>(),
        in_order
    );
    for result in &results {
        for value in [result.hull, result.ammo, result.fuel]
            .iter()
            .chain(&result.modules)
        {
            assert!((0.0..=1.0).contains(value));
        }
        assert!(result.fighters <= capacity);
        if result.fate == Fate::Lost {
            assert_eq!(result.hull, 0.0);
        }
    }
}

// --- 12. Aufstellung ----------------------------------------------------------------------

#[test]
fn every_ship_gets_its_own_slot() {
    let mixes = [
        "speer:1",
        "dolch:3",
        "waechter:2,dolch:1",
        "bastion:1,speer:4,waechter:3,dolch:5,horst:1",
        "horst:3",
        "bastion:3,speer:7,waechter:9,dolch:11,horst:2",
        "speer:2,horst:1",
    ];
    for mix in mixes {
        for formation in Formation::ALL {
            let side = fleet(mix).with_formation(formation);
            let battle = matchup::build(data(), 0, &[side]);
            let slots: Vec<V2> = battle.ships.iter().map(|ship| ship.slot).collect();
            for (a, first) in slots.iter().enumerate() {
                for second in &slots[a + 1..] {
                    assert!(
                        first.distance(*second) > 20.0,
                        "{mix} in {}: Plätze {first:?} und {second:?} fallen zusammen",
                        formation.label()
                    );
                }
            }
        }
    }
}

#[test]
fn point_defence_only_engages_small_targets() {
    // Die Nahbereichsabwehr der Fregatten schießt auf Lenkwaffen und Korvetten, nie auf
    // Kreuzer, obwohl die ebenfalls in Reichweite kommen.
    let mut battle = matchup::build(data(), 1, &[fleet("waechter:4"), fleet("speer:2,dolch:4")]);
    let (mut on_corvettes, mut on_missiles) = (0, 0);
    for _ in 0..1500 {
        battle.step(data());
        // Ein Treffer auf ein Schiff endet genau in dessen Mitte, einer auf eine Lenkwaffe
        // an deren Ort.
        for beam in battle.beams.iter().filter(|beam| {
            beam.side == 0 && beam.hit && beam.kind == sim::battle::BeamKind::PointDefense
        }) {
            let on = |class: ShipClass| {
                battle
                    .ships
                    .iter()
                    .any(|ship| ship.class == class && ship.pos.distance(beam.to) < 1.0)
            };
            assert!(
                !on(ShipClass::Cruiser),
                "die Punktverteidigung zielt auf einen Kreuzer"
            );
            if on(ShipClass::Corvette) {
                on_corvettes += 1;
            } else {
                on_missiles += 1;
            }
        }
        if battle.finished {
            break;
        }
    }
    assert!(
        on_corvettes > 20,
        "nur {on_corvettes} Schüsse auf Korvetten"
    );
    assert!(on_missiles > 20, "nur {on_missiles} Schüsse auf Lenkwaffen");
}

#[test]
fn engagements_stay_in_the_middle_of_the_field() {
    // Früher liefen beide Linien parallel vom Feld und klebten am Rand. Jetzt kreisen sie
    // um die gemeinsame Mitte, und die wandert zur Feldmitte zurück.
    let balance = &data().balance.battle;
    for (text, seed) in [("speer:5", 0), ("bastion:2,speer:3", 1), ("speer:10", 2)] {
        let side = fleet(text);
        let mut battle = matchup::build(data(), seed, &[side.clone(), side]);
        let (mut farthest_anchor, mut near_edge, mut active) = (0.0f32, 0, 0);
        while !battle.finished {
            battle.step(data());
            if battle.contact_time.is_none() {
                continue;
            }
            // Solange beide kämpfen; danach setzt der Sieger den Fliehenden nach.
            if battle
                .combatants
                .iter()
                .all(|combatant| !combatant.retreating)
            {
                for combatant in &battle.combatants {
                    farthest_anchor = farthest_anchor.max(combatant.anchor.length());
                }
            }
            for ship in battle
                .ships
                .iter()
                .filter(|ship| ship.status == ShipStatus::Active)
            {
                active += 1;
                if ship.pos.length() > 0.9 * balance.exit_radius {
                    near_edge += 1;
                }
            }
        }
        assert!(
            farthest_anchor < balance.spawn_radius,
            "{text}: Bezugspunkt bis {farthest_anchor:.0} von der Mitte"
        );
        assert!(
            near_edge * 100 <= active,
            "{text}: {near_edge} von {active} Schiffstakten am Rand"
        );
    }
}

// --- 13. Träger, Abstand, Patt ------------------------------------------------------------

/// Ein Schiff ohne Antrieb: Es steht still und dient als Ziel.
fn immobile(key: &str) -> ShipSpec {
    spec_with(key, |kind| matches!(kind, ModuleKind::Engine { .. }))
}

/// Zwei feindliche Parteien aus einzelnen Schiffen.
fn duel(first: &[ShipSpec], second: &[ShipSpec], seed: u64) -> Battle {
    let mut battle = Battle::new(BattleId(2), SystemId(0), seed);
    for (index, specs) in [first, second].into_iter().enumerate() {
        let setup = CombatantSetup::new(PolityId(index as u32), data());
        battle.add_combatant(data(), setup, specs);
    }
    battle.set_hostile(0, 1, true);
    battle
}

fn class_specs(text: &str) -> Vec<ShipSpec> {
    fleet(text).specs(data())
}

#[test]
fn carriers_without_fighters_close_to_their_gun_range() {
    // Früher maß die Flotte den Abstand vom Bezugspunkt, während die Träger weit dahinter
    // standen: Sie kamen nie auf die Reichweite ihrer Geschütze.
    let gun = data().designs[design("horst") as usize].stats.gun_range;
    for seed in 0..4 {
        let side = class_fleet("horst");
        let mut battle = matchup::build(data(), seed, &[side.clone(), side]);
        let (mut closest, mut carrier_beams) = (f32::MAX, 0);
        while !battle.finished {
            battle.step(data());
            if !battle
                .combatants
                .iter()
                .all(|combatant| combatant.carriers_in_line)
            {
                continue;
            }
            let carriers: Vec<&sim::battle::BattleShip> = battle
                .ships
                .iter()
                .filter(|ship| ship.class == ShipClass::Carrier && ship.status.can_act())
                .collect();
            for a in carriers.iter().filter(|ship| ship.side == 0) {
                for b in carriers.iter().filter(|ship| ship.side == 1) {
                    closest = closest.min(a.pos.distance(b.pos));
                }
            }
            carrier_beams += battle
                .beams
                .iter()
                .filter(|beam| {
                    beam.kind != sim::battle::BeamKind::PointDefense
                        && carriers.iter().any(|ship| ship.pos == beam.from)
                })
                .count();
        }
        assert!(
            closest <= gun,
            "Seed {seed}: Träger kommen nur auf {closest:.0} heran"
        );
        assert!(
            carrier_beams > 20,
            "Seed {seed}: nur {carrier_beams} Schüsse der Träger"
        );
    }
}

/// Mittlerer Abstand der Schiffe einer Klasse von Partei 0 zum Schwerpunkt der Partei 1 im
/// Zeitfenster `window` (Sekunden).
fn held_distance(battle: &mut Battle, class: ShipClass, window: (f32, f32)) -> f32 {
    let (mut sum, mut count) = (0.0, 0);
    while !battle.finished && battle.time < window.1 {
        battle.step(data());
        if battle.time < window.0 {
            continue;
        }
        let enemy: Vec<V2> = battle
            .ships
            .iter()
            .filter(|ship| ship.side == 1 && ship.status.can_act())
            .map(|ship| ship.pos)
            .collect();
        if enemy.is_empty() {
            break;
        }
        let center =
            enemy.iter().fold(V2::ZERO, |acc, pos| acc + *pos) * (1.0 / enemy.len() as f32);
        for ship in battle.ships.iter().filter(|ship| {
            ship.side == 0 && ship.class == class && ship.status == ShipStatus::Active
        }) {
            sum += ship.pos.distance(center);
            count += 1;
        }
    }
    assert!(count > 0, "kein Schiff im Zeitfenster");
    sum / count as f32
}

#[test]
fn fleets_keep_their_range_from_their_own_ships() {
    let balance = &data().balance.battle;
    let keep = balance.range_keep.1;
    let bands = &balance.range_bands;
    // Träger mit Jägern gegen ein Ziel, das stillsteht und nicht so weit reicht.
    let mut battle = duel(&class_specs("horst:1"), &[immobile("bastion")], 3);
    let standoff = balance.carrier_standoff * keep;
    let distance = held_distance(&mut battle, ShipClass::Carrier, (70.0, 110.0));
    assert!(
        (bands.urgent * standoff..=1.1 * standoff).contains(&distance),
        "Träger halten {distance:.0}, gewünscht {standoff:.0}"
    );
    // Kreuzer gegen dasselbe Ziel: ihre eigene Reichweite. Sie umkreisen es in der
    // Breitseite, das trägt sie etwas weiter nach außen.
    let mut battle = duel(&class_specs("speer:3"), &[immobile("bastion")], 3);
    let line = data().designs[design("speer") as usize].stats.main_range * keep;
    let distance = held_distance(&mut battle, ShipClass::Cruiser, (85.0, 115.0));
    assert!(
        (bands.urgent * line..=1.15 * line).contains(&distance),
        "Kreuzer halten {distance:.0}, gewünscht {line:.0}"
    );
}

#[test]
fn a_stalemate_ends_without_a_winner() {
    // Beide Seiten mit so starken Schilden, dass keine durchkommt.
    let walls = CombatModifiers {
        shields: 30.0,
        ..Default::default()
    };
    let side = fleet("speer:3").with_modifiers(walls);
    for seed in 0..4 {
        let outcome = matchup::fight(data(), seed, &[side.clone(), side.clone()]);
        assert!(outcome.stalemate, "Seed {seed}: kein Patt");
        assert_eq!(outcome.winner(), None);
        assert!(!outcome.timed_out);
        assert!(outcome.sides.iter().all(|side| side.lost_value == 0.0));
        let limit = data().balance.battle.stalemate_time + 90.0;
        assert!(
            outcome.fighting_time() < limit,
            "Seed {seed}: Patt erst nach {:.0} s",
            outcome.fighting_time()
        );
    }
}

#[test]
fn a_side_that_cannot_hurt_its_enemy_withdraws_alone() {
    let harmless = fleet("speer:3").with_modifiers(CombatModifiers {
        damage: 0.0,
        ..Default::default()
    });
    for seed in 0..4 {
        let mut battle = matchup::build(data(), seed, &[harmless.clone(), fleet("speer:3")]);
        let outcome = matchup::finish(data(), &mut battle);
        assert!(!outcome.stalemate, "Seed {seed}: als Patt gewertet");
        assert_eq!(outcome.winner(), Some(1), "Seed {seed}");
        assert!(battle.combatants[0].retreat_time.is_some());
        assert!(battle.combatants[1].retreat_time.is_none());
    }
}

#[test]
fn fighters_return_to_their_carrier_and_rearm() {
    let balance = &data().balance.battle;
    let mut battle = duel(&class_specs("horst:1"), &[immobile("bastion")], 5);
    // Je Jäger: Zeit, zu der er verschossen war.
    let mut empty_since: Vec<Option<f32>> = Vec::new();
    let mut rearmed = Vec::new();
    while !battle.finished && battle.time < 240.0 {
        battle.step(data());
        empty_since.resize(battle.ships.len(), None);
        for (index, ship) in battle.ships.iter().enumerate() {
            if ship.class != ShipClass::Fighter || !ship.status.can_act() {
                continue;
            }
            if ship.ammo < 0.5 * ship.ammo_max {
                empty_since[index].get_or_insert(battle.time);
            } else if let Some(since) = empty_since[index].take() {
                // Wieder voll: beim Träger, und nicht schneller als rearm_time.
                let carrier = &battle.ships[ship.carrier.expect("Träger") as usize];
                assert!(ship.pos.distance(carrier.pos) < carrier.radius + 60.0);
                assert_eq!(ship.ammo, ship.ammo_max);
                rearmed.push(battle.time - since);
            }
        }
    }
    assert!(
        rearmed.len() >= 5,
        "nur {} Jäger nachgeladen",
        rearmed.len()
    );
    for duration in &rearmed {
        assert!(*duration >= balance.rearm_time - 1e-3);
    }
    // Rückflug und Nachladen dauern nicht ewig.
    let mean = rearmed.iter().sum::<f32>() / rearmed.len() as f32;
    assert!(
        mean < balance.rearm_time + 40.0,
        "Nachladen dauert {mean:.0} s"
    );
}

#[test]
fn ships_without_fuel_are_slower_and_fuel_is_used() {
    let full = ShipSpec::fresh(data(), design("speer"));
    let mut dry = full.clone();
    dry.fuel = 0.0;
    let mut battle = two_ship_battle(full, dry);
    let (start_full, start_dry) = (battle.ships[0].pos, battle.ships[1].pos);
    for _ in 0..300 {
        cruise(&mut battle, V2::new(40.0, 0.0));
    }
    let moved_full = battle.ships[0].pos.distance(start_full);
    let moved_dry = battle.ships[1].pos.distance(start_dry);
    let dry_speed = data().balance.battle.dry_speed;
    assert!(moved_full > 700.0, "mit Treibstoff nur {moved_full:.0}");
    assert!(
        moved_dry < (dry_speed + 0.15) * moved_full,
        "ohne Treibstoff {moved_dry:.0}, mit {moved_full:.0}"
    );
    assert!(battle.ships[0].fuel < battle.ships[0].fuel_max);
    assert_eq!(battle.ships[1].fuel, 0.0);
}

#[test]
fn weapons_only_fire_inside_their_arcs() {
    // Ein Schlachtschiff, dem nur die Strahllanzen geblieben sind (Feuerwinkel 200 Grad nach
    // vorn), und ein wehrloses Ziel genau achteraus.
    let lances_only = spec_with("bastion", |kind| match kind {
        ModuleKind::Engine { .. } => true,
        ModuleKind::Weapon(weapon) => weapon.kind != WeaponKind::Beam || weapon.damage < 20.0,
        _ => false,
    });
    let target = spec_with("speer", |kind| {
        matches!(kind, ModuleKind::Engine { .. } | ModuleKind::Weapon(_))
    });
    let mut battle = duel(&[lances_only], &[target], 1);
    battle.ships[0].pos = V2::ZERO;
    battle.ships[0].heading = 0.0;
    battle.ships[0].turn = 0.0;
    battle.ships[1].pos = V2::new(-400.0, 0.0);
    let fired = |battle: &Battle| battle.beams.iter().filter(|beam| beam.side == 0).count();
    let mut behind = 0;
    for _ in 0..100 {
        battle.step(data());
        behind += fired(&battle);
    }
    assert!(battle.ships[1].status.in_play());
    assert_eq!(behind, 0, "die Lanzen feuern nach achtern");
    // Zum Ziel gedreht, feuern sie.
    let bearing = (battle.ships[1].pos - battle.ships[0].pos).angle();
    battle.ships[0].heading = bearing;
    let mut ahead = 0;
    for _ in 0..100 {
        battle.step(data());
        ahead += fired(&battle);
    }
    assert!(ahead > 0, "die Lanzen feuern auch nach vorn nicht");
}

#[test]
fn a_ship_left_with_point_defence_only_breaks_off_against_large_ships() {
    let flak_only = spec_with(
        "speer",
        |kind| matches!(kind, ModuleKind::Weapon(weapon) if weapon.kind != WeaponKind::PointDefense),
    );
    // Gegen Kreuzer richtet die Nahbereichsabwehr nichts aus: Das Schiff flieht.
    let mut battle = duel(std::slice::from_ref(&flak_only), &class_specs("speer:2"), 1);
    for _ in 0..5 {
        battle.step(data());
    }
    assert_eq!(battle.ships[0].status, ShipStatus::Routing);
    assert!(!battle.ships[0].armed);
    // Gegen Korvetten mit Lenkwaffen hat sie zu tun: Es bleibt.
    let mut battle = duel(&[flak_only], &class_specs("dolch:4"), 1);
    for _ in 0..100 {
        battle.step(data());
    }
    assert_eq!(battle.ships[0].status, ShipStatus::Active);
    assert!(battle.ships[0].armed);
}

#[test]
fn carriers_with_wrecked_hangars_count_as_carriers_without_fighters() {
    let wrecked = spec_with("horst", |kind| matches!(kind, ModuleKind::Hangar { .. }));
    assert!(wrecked.fighters > 0, "der Hangar ist noch voll");
    let mut battle = duel(&[wrecked], &class_specs("speer:1"), 1);
    let combatant = &battle.combatants[0];
    let gun = data().designs[design("horst") as usize].stats.gun_range;
    assert!(combatant.carriers_in_line);
    assert_eq!(combatant.fleet_range, gun);
    assert!(!battle.has_fighters(data(), 0));
    for _ in 0..200 {
        battle.step(data());
    }
    assert!(
        battle
            .ships
            .iter()
            .all(|ship| ship.class != ShipClass::Fighter),
        "aus zerstörten Hangars starten Jäger"
    );
    // Die Jäger im Hangar zählen nicht zum Kampfwert.
    let hull_only = battle.ships[0].value
        * (data().balance.battle.value_hull_floor
            + (1.0 - data().balance.battle.value_hull_floor) * battle.ships[0].hull
                / battle.ships[0].hull_max);
    assert!((battle.fighting_value(data(), 0) - hull_only).abs() < 1e-3);
}

#[test]
fn fighters_are_accounted_for_after_every_battle() {
    // Jeder Bordjäger ist am Ende entweder als verloren gezählt oder steht wieder in einem
    // Hangar; keiner verschwindet unbemerkt.
    let pairs = [
        ("horst:1", "speer:3"),
        ("horst:2,waechter:2", "dolch:8"),
        ("horst:1,bastion:1", "speer:4"),
        ("horst:2", "horst:2"),
    ];
    for (a, b) in pairs {
        for seed in 0..6 {
            let mut battle = matchup::build(data(), seed, &[fleet(a), fleet(b)]);
            matchup::finish(data(), &mut battle);
            let results = battle.results(data());
            for (side, combatant) in battle.combatants.iter().enumerate() {
                let fighter = ShipClass::Fighter.index();
                let returned: u32 = results
                    .iter()
                    .filter(|result| result.side as usize == side)
                    .map(|result| result.fighters)
                    .sum();
                assert_eq!(
                    combatant.initial[fighter],
                    combatant.lost[fighter] + returned,
                    "{a} gegen {b}, Seed {seed}, Partei {side}"
                );
            }
        }
    }
}

#[test]
fn ships_adrift_at_the_time_limit_count_as_lost_in_the_tally_and_the_results() {
    let mut battle = matchup::build(data(), 2, &[fleet("speer:2"), fleet("speer:2")]);
    while battle.contact_time.is_none() {
        battle.step(data());
    }
    // Kurz vor der Zeitgrenze: Partei B hat ein treibendes und ein fliehendes Schiff.
    battle.time = data().balance.battle.time_limit - 0.5;
    let b: Vec<usize> = (0..battle.ships.len())
        .filter(|index| battle.ships[*index].side == 1)
        .collect();
    battle.ships[b[0]].status = ShipStatus::Disabled;
    battle.ships[b[1]].status = ShipStatus::Routing;
    matchup::finish(data(), &mut battle);
    assert!(battle.finished && battle.timed_out);
    let adrift = battle.ships[b[0]].source.expect("Kennung");
    let result = battle
        .results(data())
        .into_iter()
        .find(|result| result.source == adrift)
        .expect("Ergebnis");
    assert_eq!(result.fate, Fate::Lost);
    assert_eq!(battle.ships[b[0]].status, ShipStatus::Destroyed);
    assert_eq!(battle.combatants[1].lost[ShipClass::Cruiser.index()], 1);
}

#[test]
fn the_breakaway_of_fighters_lasts_the_same_time_at_any_tick_length() {
    let mut fine = data().clone();
    fine.balance.battle.dt = 0.05;
    let wanted = data().balance.battle.breakaway_time;
    for data in [data(), &fine] {
        let dt = data.balance.battle.dt;
        let mut battle = matchup::build(data, 1, &[fleet("horst:1"), fleet("speer:2")]);
        let mut started: Vec<Option<f32>> = Vec::new();
        let mut durations = Vec::new();
        while !battle.finished && battle.time < 200.0 {
            battle.step(data);
            started.resize(battle.ships.len(), None);
            for (index, ship) in battle.ships.iter().enumerate() {
                if ship.class != ShipClass::Fighter || ship.status != ShipStatus::Active {
                    started[index] = None;
                    continue;
                }
                if ship.breakaway > 0.0 {
                    started[index].get_or_insert(battle.time);
                } else if let Some(since) = started[index].take() {
                    durations.push(battle.time - since);
                }
            }
        }
        assert!(durations.len() > 10, "dt {dt}: kaum Anflüge");
        let longest = durations.iter().fold(0.0f32, |a, b| a.max(*b));
        assert!(
            (longest - wanted).abs() <= 2.0 * dt + 1e-3,
            "dt {dt}: Abdrehen dauert bis zu {longest:.2} s statt {wanted:.2} s"
        );
    }
}

#[test]
fn war_weariness_lowers_the_starting_morale() {
    let balance = &data().balance.battle;
    let fresh = CombatantSetup::new(PolityId(0), data());
    assert_eq!(fresh.morale, balance.morale_start);
    let weary = CombatantSetup::new(PolityId(0), data()).with_war_weariness(data(), 50.0);
    let expected = balance.morale_start - 50.0 * balance.morale_per_weariness;
    assert!((weary.morale - expected).abs() < 1e-4);
    // Mehr als 100 Punkte gibt es nicht.
    let worn = CombatantSetup::new(PolityId(0), data()).with_war_weariness(data(), 500.0);
    assert!(
        (worn.morale - (balance.morale_start - 100.0 * balance.morale_per_weariness)).abs() < 1e-4
    );
    let mut battle = Battle::new(BattleId(3), SystemId(0), 1);
    battle.add_combatant(data(), weary, &class_specs("speer:2"));
    assert!(
        battle
            .ships
            .iter()
            .all(|ship| (ship.morale_base - expected).abs() < 1e-4)
    );
}

#[test]
fn small_even_battles_usually_leave_the_loser_a_ship() {
    // In kleinen gleichen Gefechten soll der Verlierer meist etwas retten; eine Seite, die
    // jedes Mal bis zum letzten Schiff bleibt, wäre ein Rückfall.
    for text in ["speer:3", "bastion:2"] {
        let side = fleet(text);
        let outcomes = fight_seeds(0..100, &[side.clone(), side]);
        let (mut decided, mut wiped) = (0, 0);
        for outcome in &outcomes {
            let Some(winner) = outcome.winner() else {
                continue;
            };
            decided += 1;
            let loser = &outcome.sides[1 - winner];
            let ships = |counts: &[u32; 6]| -> u32 {
                counts.iter().sum::<u32>() - counts[ShipClass::Fighter.index()]
            };
            if ships(&loser.lost) == ships(&loser.initial) {
                wiped += 1;
            }
        }
        assert!(decided >= 90, "{text}: nur {decided} entschieden");
        assert!(
            wiped * 2 < decided,
            "{text}: Verlierer verliert in {wiped} von {decided} Gefechten alles"
        );
    }
}

#[test]
fn a_large_battle_stays_within_the_tick_budget() {
    // Grobe Schranke gegen Rückfälle; die genaue Messung macht `headless battle --bench`.
    let side = fleet("horst:8,bastion:17,speer:42,waechter:67,dolch:66");
    let mut battle = matchup::build(data(), 0, &[side.clone(), side]);
    let start = std::time::Instant::now();
    let ticks = 600;
    for _ in 0..ticks {
        battle.step(data());
    }
    let mean = start.elapsed().as_secs_f64() / ticks as f64;
    assert!(
        battle.contact_time.is_some(),
        "kein Kontakt in {ticks} Takten"
    );
    assert!(mean < 0.02, "{:.1} ms je Takt", mean * 1e3);
}
