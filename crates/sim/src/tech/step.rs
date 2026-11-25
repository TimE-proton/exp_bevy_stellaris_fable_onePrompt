//! Monatlicher Forschungsschritt, automatische Zielwahl und die Befehle dazu.

use super::intel::{ownership_ties, update_intel};
use super::{PolityResearch, Research, TechEffects, available_of};
use crate::chronicle::{Chronicle, ChronicleKind};
use crate::data::{GameData, ResearchBalance};
use crate::diplomacy::Diplomacy;
use crate::economy::{Economy, pay_households, state_reserve, to_credits, to_money};
use crate::finance::Finance;
use crate::galaxy::Galaxy;
use crate::ids::PolityId;
use crate::logistics::Logistics;
use crate::polity::Polities;
use std::collections::VecDeque;

/// Forschungspunkte aus den Gehältern eines Monats. Geld zählt voll, die Bevölkerung
/// (in Millionen) nur mit `population_exponent`: Ein großes Volk bringt mehr Köpfe hervor,
/// aber nicht im gleichen Maß. `bonus` ist der Faktor aus der Technik selbst.
pub fn research_points(
    balance: &ResearchBalance,
    credits: f64,
    population: f64,
    bonus: f64,
) -> f64 {
    if credits <= 0.0 || population <= 0.0 {
        return 0.0;
    }
    credits * balance.efficiency * (population / 1000.0).powf(balance.population_exponent) * bonus
}

/// Sprungdistanz jedes Systems zum Gebiet einer Gemeinschaft (allen Systemen, in denen sie
/// eine Welt besitzt). `None`, wenn sie kein Gebiet hat.
fn territory_hops(galaxy: &Galaxy, polity: PolityId) -> Option<Vec<u32>> {
    let mut hops = vec![u32::MAX; galaxy.systems.len()];
    let mut queue = VecDeque::new();
    for planet in galaxy.planets_of(polity) {
        if hops[planet.system.index()] != 0 {
            hops[planet.system.index()] = 0;
            queue.push_back(planet.system);
        }
    }
    if queue.is_empty() {
        return None;
    }
    while let Some(current) = queue.pop_front() {
        for neighbor in galaxy.neighbors(current) {
            if hops[neighbor.system.index()] == u32::MAX {
                hops[neighbor.system.index()] = hops[current.index()] + 1;
                queue.push_back(neighbor.system);
            }
        }
    }
    Some(hops)
}

/// Wie stark je zwei Gemeinschaften Wissen austauschen (0..1): 1, wenn sie ein System
/// teilen, halbiert je `spillover_half_hops` Sprüngen zwischen ihren Gebieten. Im Krieg
/// und ohne Gebiet ruht der Austausch. Wer mitten unter anderen sitzt, hat viele nahe
/// Nachbarn und lernt schneller als eine Gemeinschaft am Rand.
pub fn contact_weights(
    galaxy: &Galaxy,
    polities: &Polities,
    diplomacy: &Diplomacy,
    balance: &ResearchBalance,
) -> Vec<Vec<f64>> {
    let count = polities.list.len();
    let reach: Vec<Option<Vec<u32>>> = polities
        .list
        .iter()
        .map(|polity| {
            polity
                .alive
                .then(|| territory_hops(galaxy, polity.id))
                .flatten()
        })
        .collect();
    let mut weights = vec![vec![0.0f64; count]; count];
    for a in 0..count {
        let Some(hops) = &reach[a] else {
            continue;
        };
        for b in (a + 1)..count {
            let (first, second) = (PolityId::from_index(a), PolityId::from_index(b));
            if reach[b].is_none() || diplomacy.at_war(first, second) {
                continue;
            }
            let nearest = galaxy
                .planets_of(second)
                .map(|planet| hops[planet.system.index()])
                .min()
                .unwrap_or(u32::MAX);
            if nearest == u32::MAX {
                continue;
            }
            let weight = 0.5f64.powf(nearest as f64 / balance.spillover_half_hops);
            weights[a][b] = weight;
            weights[b][a] = weight;
        }
    }
    weights
}

/// Nachlass auf eine Technologie aus der Summe der Gewichte aller Gemeinschaften in
/// Kontakt, die sie beherrschen. Wächst mit jeder weiteren, erreicht `spillover_max` nie.
pub fn spillover_discount(balance: &ResearchBalance, weight_sum: f64) -> f64 {
    let sum = weight_sum.max(0.0);
    balance.spillover_max * sum / (1.0 + sum)
}

/// Automatische Zielwahl: die erforschbare Technologie mit dem höchsten Verhältnis von
/// Gewicht ihres Zweigs zu ihren Restkosten. Im Frieden wiegen Wirtschaft und Antrieb
/// schwerer, im Krieg Waffen und Verteidigung (`auto_peace`, `auto_war`). Die Restkosten
/// berücksichtigen angefangene Arbeit und den Nachlass aus fremdem Wissen: Begonnenes wird
/// beendet, und was die Nachbarn schon können, wird bevorzugt aufgeholt. Bei Gleichstand
/// gewinnt die Technologie, die in `techs.ron` zuerst steht.
pub fn choose_target(data: &GameData, entry: &PolityResearch, at_war: bool) -> Option<u16> {
    let balance = &data.balance.research;
    let weights = if at_war {
        &balance.auto_war
    } else {
        &balance.auto_peace
    };
    let mut best: Option<(u16, f64)> = None;
    for tech in available_of(data, &entry.known) {
        // Ein Rest unter einem Punkt gilt als ein Punkt: Fast Fertiges gewinnt, ohne dass
        // durch null geteilt wird.
        let remaining = entry.remaining(data, tech).max(1.0);
        let score = weights.of(data.techs[tech as usize].branch) / remaining;
        if best.is_none_or(|(_, best_score)| score > best_score) {
            best = Some((tech, score));
        }
    }
    best.map(|(tech, _)| tech)
}

/// Der Forschungsschritt eines Monats für alle Gemeinschaften, in ID-Reihenfolge.
///
/// 1. Einträge für alle Gemeinschaften sicherstellen, Einblick fortschreiben.
/// 2. Nachlass aus fremdem Wissen bestimmen, für alle aus dem Stand zu Monatsbeginn:
///    Die Reihenfolge der Gemeinschaften spielt keine Rolle.
/// 3. Je Gemeinschaft: Ziel prüfen oder wählen, Gehälter zahlen (aus der Staatskasse über
///    der Rücklage an die Haushalte), Punkte gutschreiben, fertige Technologien abschließen.
///
/// `player` ist die vom Menschen geführte Gemeinschaft. Alle anderen forschen immer
/// automatisch; ihr gehören außerdem die einzigen Chronikeinträge, sobald ein Mensch
/// mitspielt, denn den genauen Stand der anderen soll er nicht sehen.
pub fn research_month(
    research: &mut Research,
    economy: &mut Economy,
    galaxy: &Galaxy,
    polities: &Polities,
    finance: &Finance,
    logistics: &Logistics,
    diplomacy: &Diplomacy,
    chronicle: &mut Chronicle,
    data: &GameData,
    player: Option<PolityId>,
    tick: u64,
) {
    let balance = &data.balance.research;
    let count = polities.list.len();
    research.sync(data, count);

    let ties = ownership_ties(finance, galaxy, logistics, research.intel.len());
    update_intel(research, &ties, balance);

    let weights = contact_weights(galaxy, polities, diplomacy, balance);
    let discounts: Vec<Vec<f64>> = (0..count)
        .map(|learner| {
            (0..data.techs.len() as u16)
                .map(|tech| {
                    let sum: f64 = (0..count)
                        .filter(|other| *other != learner)
                        .filter(|other| research.knows(PolityId::from_index(*other), tech))
                        .map(|other| weights[learner][other])
                        .sum();
                    spillover_discount(balance, sum)
                })
                .collect()
        })
        .collect();
    for (entry, discount) in research.polities.iter_mut().zip(discounts) {
        entry.spillover = discount;
    }

    for polity in &polities.list {
        let id = polity.id;
        let entry = &mut research.polities[id.index()];
        entry.points_last_month = 0.0;
        entry.spent_last_month = 0;
        let population = galaxy.population_of(id);
        if !polity.alive || population <= 0.0 {
            continue;
        }
        let at_war = diplomacy.is_at_war(id);
        // KI-geführte Gemeinschaften forschen immer automatisch. Ein von außen gesetztes
        // Ziel (Skript, früherer Spieler) gilt noch bis zu seinem Abschluss.
        if player != Some(id) && entry.target.is_none() {
            entry.auto = true;
        }
        if entry
            .target
            .is_some_and(|tech| entry.known[tech as usize] || !entry.prerequisites_met(data, tech))
        {
            entry.target = None;
        }
        // Die Automatik prüft ihre Wahl jeden Monat: Fortschritt bleibt beim Wechsel
        // erhalten, und so folgt sie einem Kriegsausbruch oder neuem Wissen der Nachbarn.
        if entry.auto {
            entry.target = choose_target(data, entry, at_war);
        }
        if entry.target.is_none() {
            // Nichts zu erforschen: Das Geld bleibt in der Staatskasse.
            continue;
        }

        // Der Etat ist ein Anteil der Einnahmen. Gezahlt wird er nur aus dem, was über der
        // Rücklage des Staates liegt: Eine leere Kasse bremst die Forschung ebenso wie ein
        // Kriegsschatz, der erst gefüllt werden muss.
        let revenue = finance
            .polities
            .get(id.index())
            .map_or(0.0, |state| state.revenue);
        let free = economy.treasury(id) - state_reserve(economy, data, id, population);
        let budget = to_money(entry.budget_share * revenue.max(0.0)).min(free);
        let spent = pay_households(galaxy, economy, id, budget);
        let points = research_points(
            balance,
            to_credits(spent),
            population,
            entry.effects.research,
        );
        entry.spent_last_month = spent;
        entry.spent_total += spent;
        entry.points_last_month = points;
        entry.points_total += points;

        // Punkte gutschreiben. Was nach einem Abschluss übrig ist, fließt in das nächste
        // Ziel; gibt es keines, wartet der Rest in `carry`.
        let mut pool = points + std::mem::take(&mut entry.carry);
        while let Some(tech) = entry.target {
            let slot = tech as usize;
            entry.progress[slot] += pool;
            let cost = entry.cost(data, tech);
            if entry.progress[slot] < cost {
                pool = 0.0;
                break;
            }
            pool = entry.progress[slot] - cost;
            entry.progress[slot] = 0.0;
            entry.known[slot] = true;
            entry.completed += 1;
            entry.effects = TechEffects::of(data, &entry.known);
            if player.is_none_or(|human| human == id) {
                let def = &data.techs[slot];
                chronicle.push(
                    tick,
                    ChronicleKind::Tech,
                    Some(id),
                    polity.capital.map(|capital| galaxy.planet(capital).system),
                    format!(
                        "{} erforscht {} ({}, Stufe {}).",
                        polity.name,
                        def.name,
                        def.branch.label(),
                        def.tier
                    ),
                );
            }
            entry.target = if entry.auto {
                choose_target(data, entry, at_war)
            } else {
                None
            };
        }
        entry.carry = pool;
    }
}

fn living(polities: &Polities, polity: PolityId) -> Result<(), String> {
    if polities
        .list
        .get(polity.index())
        .is_some_and(|found| found.alive)
    {
        Ok(())
    } else {
        Err("Gemeinschaft besteht nicht mehr".into())
    }
}

/// Befehl: Forschungsziel setzen. `None` gibt die Wahl an die Automatik zurück, die
/// sofort neu wählt. Ein gesetztes Ziel muss unerforscht sein und alle Voraussetzungen
/// haben; der Fortschritt am bisherigen Ziel bleibt erhalten.
pub fn set_target(
    research: &mut Research,
    data: &GameData,
    polities: &Polities,
    diplomacy: &Diplomacy,
    polity: PolityId,
    tech: Option<u16>,
) -> Result<(), String> {
    living(polities, polity)?;
    research.sync(data, polities.list.len());
    let entry = &mut research.polities[polity.index()];
    let Some(tech) = tech else {
        entry.auto = true;
        entry.target = choose_target(data, entry, diplomacy.is_at_war(polity));
        return Ok(());
    };
    let Some(def) = data.techs.get(tech as usize) else {
        return Err("Technologie unbekannt".into());
    };
    if entry.known[tech as usize] {
        return Err(format!("{} ist bereits erforscht", def.name));
    }
    if let Some(missing) = def
        .requires_ids
        .iter()
        .find(|required| !entry.known[**required as usize])
    {
        return Err(format!(
            "Voraussetzung fehlt: {}",
            data.techs[*missing as usize].name
        ));
    }
    entry.target = Some(tech);
    entry.auto = false;
    Ok(())
}

/// Befehl: Anteil der Staatseinnahmen für die Forschung, zwischen 0 und `max_budget_share`.
pub fn set_budget(
    research: &mut Research,
    data: &GameData,
    polities: &Polities,
    polity: PolityId,
    share: f64,
) -> Result<(), String> {
    living(polities, polity)?;
    let max = data.balance.research.max_budget_share;
    if !(0.0..=max).contains(&share) {
        return Err(format!(
            "Forschungsanteil muss zwischen 0 und {:.0} % liegen",
            max * 100.0
        ));
    }
    research.sync(data, polities.list.len());
    research.polities[polity.index()].budget_share = share;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{SimConfig, SimState};

    fn data() -> GameData {
        GameData::load_workspace().unwrap()
    }

    fn state(data: &GameData, seed: u64) -> SimState {
        SimState::generate(data, SimConfig { seed, player: None })
    }

    #[test]
    fn points_grow_with_money_and_sublinearly_with_people() {
        let balance = data().balance.research;
        let base = research_points(&balance, 100.0, 4000.0, 1.0);
        assert!(base > 0.0);
        // Geld zählt voll.
        let double_money = research_points(&balance, 200.0, 4000.0, 1.0);
        assert!((double_money / base - 2.0).abs() < 1e-9);
        // Menschen zählen, aber weniger als proportional.
        let double_people = research_points(&balance, 100.0, 8000.0, 1.0);
        assert!(double_people > base * 1.05, "{base} → {double_people}");
        assert!(double_people < base * 1.9, "{base} → {double_people}");
        assert!(
            (double_people / base - 2f64.powf(balance.population_exponent)).abs() < 1e-9,
            "Exponent aus balance.ron"
        );
        // Bei einer Milliarde Einwohnern gilt der Grundwert.
        assert!(
            (research_points(&balance, 50.0, 1000.0, 1.0) - 50.0 * balance.efficiency).abs() < 1e-9
        );
        // Die Technik selbst wirkt als Faktor; ohne Geld oder Menschen entsteht nichts.
        assert!((research_points(&balance, 100.0, 4000.0, 1.05) / base - 1.05).abs() < 1e-9);
        assert_eq!(research_points(&balance, 0.0, 4000.0, 1.0), 0.0);
        assert_eq!(research_points(&balance, 100.0, 0.0, 1.0), 0.0);
    }

    #[test]
    fn discount_grows_with_every_knower_but_stays_below_the_cap() {
        let balance = data().balance.research;
        assert_eq!(spillover_discount(&balance, 0.0), 0.0);
        let mut last = 0.0;
        for knowers in 1..=8 {
            let discount = spillover_discount(&balance, knowers as f64);
            assert!(discount > last && discount < balance.spillover_max);
            last = discount;
        }
        assert!((spillover_discount(&balance, 1.0) - balance.spillover_max / 2.0).abs() < 1e-12);
    }

    #[test]
    fn contact_is_symmetric_fades_with_distance_and_rests_in_war() {
        let data = data();
        let state = state(&data, 3);
        let balance = &data.balance.research;
        let mut diplomacy = Diplomacy::default();
        let weights = contact_weights(&state.galaxy, &state.polities, &diplomacy, balance);
        let count = state.polities.list.len();
        let home = |index: usize| {
            state
                .galaxy
                .planet(state.polities.list[index].capital.unwrap())
                .system
        };
        for a in 0..count {
            assert_eq!(weights[a][a], 0.0);
            let hops = state.galaxy.hops_from(home(a));
            for b in 0..count {
                assert_eq!(weights[a][b], weights[b][a]);
                if a != b {
                    // Zu Beginn besteht jedes Gebiet aus dem Heimatsystem.
                    let expected =
                        0.5f64.powf(hops[home(b).index()] as f64 / balance.spillover_half_hops);
                    assert!((weights[a][b] - expected).abs() < 1e-12);
                    assert!(weights[a][b] > 0.0 && weights[a][b] < 1.0);
                }
            }
        }
        // Der nächste Nachbar wiegt am schwersten.
        let hops = state.galaxy.hops_from(home(0));
        let nearest = (1..count).min_by_key(|b| hops[home(*b).index()]).unwrap();
        let farthest = (1..count).max_by_key(|b| hops[home(*b).index()]).unwrap();
        assert!(weights[0][nearest] > weights[0][farthest]);
        // Krieg unterbricht den Austausch zwischen genau diesen beiden.
        diplomacy
            .wars
            .insert((PolityId(0), PolityId::from_index(nearest)));
        let at_war = contact_weights(&state.galaxy, &state.polities, &diplomacy, balance);
        assert_eq!(at_war[0][nearest], 0.0);
        assert_eq!(at_war[nearest][0], 0.0);
        assert_eq!(at_war[0][farthest], weights[0][farthest]);
    }

    #[test]
    fn the_automatic_rule_weighs_branch_against_cost_and_follows_war() {
        let data = data();
        let mut entry = PolityResearch::new(&data);
        let tech = |key: &str| data.tech_index(key).unwrap();
        // Im Frieden zuerst die Wirtschaft, im Krieg die Waffen.
        assert_eq!(
            choose_target(&data, &entry, false),
            Some(tech("wirtschaft_1"))
        );
        assert_eq!(choose_target(&data, &entry, true), Some(tech("waffen_1")));
        // Die zweite Stufe kostet das Doppelte: Erst kommt die Breite.
        entry.known[tech("wirtschaft_1") as usize] = true;
        assert_eq!(choose_target(&data, &entry, false), Some(tech("antrieb_1")));
        // Angefangenes wird beendet.
        entry.progress[tech("boden_1") as usize] = 2900.0;
        assert_eq!(choose_target(&data, &entry, false), Some(tech("boden_1")));
        entry.progress[tech("boden_1") as usize] = 0.0;
        // Bei gleichem Gewicht und gleichen Kosten entscheidet die Reihenfolge der Datei …
        entry.known[tech("antrieb_1") as usize] = true;
        entry.known[tech("wirtschaft_2") as usize] = true;
        assert_eq!(choose_target(&data, &entry, false), Some(tech("waffen_1")));
        // … und was die Nachbarn schon können, ist billiger und rückt vor.
        entry.spillover[tech("verteidigung_1") as usize] = 0.2;
        assert_eq!(
            choose_target(&data, &entry, false),
            Some(tech("verteidigung_1"))
        );
        // Ist der ganze Baum erforscht, gibt es kein Ziel mehr.
        entry.known.iter_mut().for_each(|known| *known = true);
        assert_eq!(choose_target(&data, &entry, false), None);
    }

    #[test]
    fn the_automatic_rule_never_picks_something_unresearchable() {
        let data = data();
        let mut entry = PolityResearch::new(&data);
        // Die Automatik arbeitet den ganzen Baum ab, ohne je eine Voraussetzung zu
        // überspringen, im Frieden wie im Krieg.
        for step in 0..data.techs.len() {
            let pick = choose_target(&data, &entry, step % 2 == 1).expect("noch etwas offen");
            assert!(!entry.known[pick as usize]);
            assert!(entry.prerequisites_met(&data, pick));
            entry.known[pick as usize] = true;
        }
        assert!(entry.known.iter().all(|known| *known));
    }
}
