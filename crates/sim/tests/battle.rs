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

#[test]
fn mirror_matches_are_fair() {
    let mut fleets: Vec<(&str, Side)> = ["dolch", "waechter", "speer", "bastion", "horst"]
        .iter()
        .map(|key| (*key, class_fleet(key)))
        .collect();
    fleets.push(("gemischt", fleet("bastion:1,speer:1,waechter:1,dolch:2")));
    for (label, side) in fleets {
        let outcomes = fight_seeds(0..60, &[side.clone(), side.clone()]);
        let (a, b) = (wins(&outcomes, 0), wins(&outcomes, 1));
        let decided = (a + b).max(1) as f32;
        let share = a as f32 / decided;
        assert!(
            (0.35..=0.65).contains(&share),
            "{label}: A gewinnt {a}, B {b}"
        );
        assert!(a + b >= 45, "{label}: nur {} entschieden", a + b);
        // Die mittleren Verluste schwanken über 60 Gefechte um etwa 0,06; mehr als das
        // Zweieinhalbfache wäre eine echte Schieflage.
        let (loss_a, loss_b) = (mean_loss(&outcomes, 0), mean_loss(&outcomes, 1));
        assert!(
            (loss_a - loss_b).abs() < 0.15,
            "{label}: Verluste A {loss_a:.2}, B {loss_b:.2}"
        );
    }
}

// --- 2. Konter ----------------------------------------------------------------------------

/// Gewinnt `favoured` gegen `other` bei gleichem Bauwert in beiden Aufstellungen mindestens
/// 70 % der Gefechte?
fn assert_counter(favoured: &str, other: &str) {
    let (strong, weak) = (class_fleet(favoured), class_fleet(other));
    let value = |side: &Side| side.value(data());
    let ratio = value(&strong) / value(&weak);
    assert!(
        (0.9..=1.1).contains(&ratio),
        "{favoured} gegen {other}: Bauwerte {:.0} und {:.0} weichen zu weit ab",
        value(&strong),
        value(&weak)
    );
    let seeds = 0..12u64;
    let as_a = fight_seeds(seeds.clone(), &[strong.clone(), weak.clone()]);
    let as_b = fight_seeds(seeds.clone(), &[weak, strong]);
    let (won_a, won_b) = (wins(&as_a, 0), wins(&as_b, 1));
    let needed = (0.7f32 * as_a.len() as f32).ceil() as usize;
    assert!(
        won_a >= needed && won_b >= needed,
        "{favoured} gegen {other}: {won_a}/12 als A, {won_b}/12 als B"
    );
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
    // Gemessen, nicht gewünscht: Die gemischte Flotte schlägt Fregatten, Träger und
    // Schlachtschiffe, hält gegen Korvetten mit, und verliert gegen reine Kreuzer, deren
    // Rolle das Brechen von Geleit ist - aber nicht ohne dem Gegner Verluste zuzufügen.
    let mixed = fleet("bastion:1,speer:2,waechter:1,dolch:1");
    let value = mixed.value(data());
    for (key, min_wins, min_dealt) in [
        ("dolch", 0, 0.25),
        ("waechter", 9, 0.4),
        ("speer", 0, 0.3),
        ("bastion", 9, 0.3),
        ("horst", 9, 0.5),
    ] {
        let other = Side::for_budget(data(), design(key), value);
        let outcomes = fight_seeds(0..12, &[mixed.clone(), other]);
        let won = wins(&outcomes, 0);
        let dealt = mean_loss(&outcomes, 1);
        assert!(
            won >= min_wins && dealt >= min_dealt,
            "gegen {key}: {won}/12 Siege, Gegner verliert {dealt:.2}"
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
    // +8 %, Rumpf +8 %, Schilde +12 %. Eine Flottenstärke von zehn Kreuzern, damit der
    // Vorsprung nicht im Zufall weniger Treffer untergeht.
    let modifiers = CombatModifiers {
        damage: 1.08,
        range: 1.05,
        shields: 1.12,
        armor: 1.0,
        hull: 1.08,
        speed: 1.0,
    };
    let share = tech_share(modifiers, "speer:10", 0..12);
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

#[test]
fn lost_engines_cost_mobility_and_damaged_shields_hold_less() {
    let intact = spec_with("speer", |_| false);
    let no_engines = spec_with("speer", |kind| matches!(kind, ModuleKind::Engine { .. }));
    let mut battle = two_ship_battle(intact.clone(), no_engines);
    let (start_a, start_b) = (battle.ships[0].pos, battle.ships[1].pos);
    assert_eq!(battle.ships[1].mobility, 0.0);
    // Ohne Gegner hält die Flotte still; wir geben ihr Fahrt nach vorn.
    battle.combatants[0].anchor_vel = V2::new(30.0, 0.0);
    for _ in 0..300 {
        battle.step(data());
        battle.combatants[0].anchor_vel = V2::new(30.0, 0.0);
    }
    let moved_a = battle.ships[0].pos.distance(start_a);
    let moved_b = battle.ships[1].pos.distance(start_b);
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
        // Nie ziehen beide Seiten im selben Augenblick ab.
        let times: Vec<Option<f32>> = battle
            .combatants
            .iter()
            .map(|combatant| combatant.retreat_time)
            .collect();
        if let (Some(a), Some(b)) = (times[0], times[1]) {
            assert!(a != b, "Seed {seed}: beide Seiten ziehen bei {a} s ab");
        }
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
    let mut saved = matchup::build(data(), 33, &sides);
    for _ in 0..700 {
        straight.step(data());
        saved.step(data());
    }
    assert!(!saved.finished && !saved.shots.is_empty() || !saved.finished);
    let via_postcard: Battle =
        postcard::from_bytes(&postcard::to_stdvec(&saved).expect("postcard")).expect("postcard");
    let text = ron::to_string(&saved).expect("ron");
    let via_ron: Battle = ron::from_str(&text).expect("ron");
    let mut copies = [via_postcard, via_ron];
    matchup::finish(data(), &mut straight);
    for copy in &mut copies {
        matchup::finish(data(), copy);
        assert_eq!(bytes(copy), bytes(&straight));
    }
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
    // Die Nahbereichsabwehr schießt auf Lenkwaffen, Jäger und Korvetten, nicht auf Kreuzer.
    let mut battle = matchup::build(data(), 1, &[fleet("waechter:4"), fleet("speer:2")]);
    let flak = data()
        .modules
        .iter()
        .position(|module| {
            module
                .weapon()
                .is_some_and(|weapon| weapon.kind == WeaponKind::PointDefense)
        })
        .expect("Punktverteidigung");
    assert!(data().modules[flak].weapon().is_some());
    for _ in 0..1500 {
        battle.step(data());
        assert!(
            battle
                .beams
                .iter()
                .filter(|beam| beam.side == 0 && beam.kind == sim::battle::BeamKind::PointDefense)
                .all(|beam| !beam.hit
                    || battle
                        .ships
                        .iter()
                        .filter(|ship| ship.class == ShipClass::Cruiser)
                        .all(|ship| ship.pos.distance(beam.to) > 1.0)),
            "die Punktverteidigung trifft einen Kreuzer"
        );
        if battle.finished {
            break;
        }
    }
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
