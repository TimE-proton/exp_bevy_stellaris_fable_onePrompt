//! Die militärische Grundhaltung der KI-Gemeinschaften: Rüstung, Forschungspolitik und
//! die Führung der Flotten im Frieden und im Krieg.
//!
//! Ob Krieg oder Frieden herrscht, entscheidet diese Schicht nicht. Sie sorgt dafür, dass
//! eine Gemeinschaft in beidem vernünftig dasteht:
//!
//! - Monatlich ([`plan_month`]): Forschungsetat und -ziel, der Warteplatz der Hauptflotte,
//!   Bestellungen im Rahmen des Staatshaushalts, und wenn der Sold dauerhaft ausbleibt,
//!   das Stilllegen von Schiffen.
//! - Täglich ([`lead_fleets`]): Flotten versorgen, sammeln und vereinigen; im Krieg
//!   Eindringlingen begegnen, wo das Kräfteverhältnis es trägt, sonst ausweichen; bei
//!   Überlegenheit angreifen und rechtzeitig abbrechen.
//!
//! Befehle erteilt sie nur über [`super::levers`]. Über fremde Flotten weiß sie, was
//! [`super::estimate`] hergibt.

use super::estimate::{
    ClassValues, Sighting, add, battle_values, class_score, odds, own_values, seen_values,
    sightings, tech_edge, total,
};
use super::levers::{Hand, free_yards, has_yard};
use super::{AiState, Meeting, MilitaryMode, MilitaryPlan, Mission, Operation};
use crate::battle::Doctrine;
use crate::clock::DAYS_PER_MONTH;
use crate::command::CommandQueue;
use crate::data::{GameData, ShipClass};
use crate::diplomacy::Diplomacy;
use crate::economy::{BudgetMonth, Claim, Economy, to_credits};
use crate::fleet::{Battles, BuildStatus, Fleet, Fleets, Stance, fleet_claim, supply_base};
use crate::galaxy::Galaxy;
use crate::ids::{FleetId, PolityId, ShipId, SystemId};
use crate::polity::Polities;
use crate::tech::{PolityResearch, Research, ai_budget_share, available_of};
use std::collections::{BTreeMap, BTreeSet};

/// Der Stand der Welt, wie ihn die Führung liest.
pub struct Situation<'a> {
    pub data: &'a GameData,
    pub galaxy: &'a Galaxy,
    pub polities: &'a Polities,
    pub economy: &'a Economy,
    pub fleets: &'a Fleets,
    pub battles: &'a Battles,
    pub diplomacy: &'a Diplomacy,
    pub research: &'a Research,
    /// Der heutige Tag.
    pub day: u64,
}

/// So viele abgeschlossene Haushaltsmonate braucht die Rüstung im Krieg, bevor sie
/// bestellt; über so viele urteilt sie höchstens, und so viele braucht sie im Frieden.
const BUDGET_MONTHS_MIN: usize = 3;
const BUDGET_MONTHS_MAX: usize = 12;
/// Unter diesem Tankinhalt wartet eine Flotte im eigenen Hafen, statt loszufahren: Ein
/// Schiff aus der Werft kommt mit leeren Tanks.
const FUEL_TO_SAIL: f32 = 0.5;
/// Was nach dem Rückweg von einer Unternehmung noch im Tank sein soll.
const FUEL_RESERVE: f32 = 0.05;
/// Ausgewichen wird erst, wenn das Kräfteverhältnis um diesen Faktor unter dem liegt, ab
/// dem sich die Flotte stellen würde: So kippt die Entscheidung an der Schwelle nicht hin
/// und her.
const AVOID_MARGIN: f64 = 0.8;
/// Ab diesem ausgebliebenen Anteil zählt ein Monat als Monat ohne vollen Sold.
const MISSED_PAY: f32 = 0.005;

/// Gemeinschaften, die die KI führt: lebend, mit Sitz, nicht die des Spielers.
fn led<'a>(
    sit: &'a Situation<'a>,
    player: Option<PolityId>,
) -> impl Iterator<Item = (PolityId, SystemId)> + 'a {
    sit.polities.alive().filter_map(move |polity| {
        if player == Some(polity.id) {
            return None;
        }
        let seat = polity.seat(sit.galaxy)?;
        Some((polity.id, sit.galaxy.planet(seat).system))
    })
}

/// Kleinste Sprungzahl von irgendeinem System, in dem die Gemeinschaft eine Welt besitzt.
fn hops_from_own(galaxy: &Galaxy, polity: PolityId) -> Vec<u32> {
    let mut nearest = vec![u32::MAX; galaxy.systems.len()];
    for system in &galaxy.systems {
        if !galaxy.owns_planet_in(polity, system.id) {
            continue;
        }
        for (slot, hops) in nearest.iter_mut().zip(galaxy.hops_from(system.id)) {
            *slot = (*slot).min(hops);
        }
    }
    nearest
}

/// Die bedrohlichste fremde Gemeinschaft und das Maß ihrer Bedrohung: der Bauwert ihrer
/// Flotten nahe den eigenen Systemen, im Krieg schwerer gewichtet.
fn greatest_threat(
    sit: &Situation,
    polity: PolityId,
    seen: &[Sighting],
) -> Option<(PolityId, f64)> {
    let ai = &sit.data.balance.military.ai;
    let near = hops_from_own(sit.galaxy, polity);
    let mut threat: BTreeMap<PolityId, f64> = BTreeMap::new();
    for sighting in seen {
        if near[sighting.system.index()] > ai.threat_jumps {
            continue;
        }
        let weight = if sit.diplomacy.at_war(polity, sighting.owner) {
            ai.war_weight
        } else {
            1.0
        };
        *threat.entry(sighting.owner).or_default() += total(&sighting.values) * weight;
    }
    // Der höchste Wert; bei Gleichstand die kleinere Nummer.
    threat
        .into_iter()
        .filter(|(_, value)| *value > 0.0)
        .max_by(|a, b| a.1.total_cmp(&b.1).then(b.0.cmp(&a.0)))
}

/// Wo die Hauptflotte im Frieden wartet: am eigenen Engpasssystem, das dem Sitz der
/// bedrohlichsten Gemeinschaft am nächsten liegt, wenn es näher liegt als der eigene
/// Sitz; sonst am Sitz.
fn choose_station(
    sit: &Situation,
    polity: PolityId,
    home: SystemId,
    foe: Option<PolityId>,
) -> SystemId {
    let Some(foe_home) = foe
        .and_then(|foe| sit.polities.get(foe).seat(sit.galaxy))
        .map(|seat| sit.galaxy.planet(seat).system)
    else {
        return home;
    };
    let hops = sit.galaxy.hops_from(foe_home);
    sit.galaxy
        .systems
        .iter()
        .filter(|system| {
            system.chokepoint
                && sit.galaxy.system_owner(system.id) == Some(polity)
                && hops[system.index()] < hops[home.index()]
        })
        .min_by_key(|system| (hops[system.index()], system.id))
        .map_or(home, |system| system.id)
}

/// Forschungsziel im Krieg: wie die Automatik, aber eine Technologie, die einen neuen
/// Schiffsentwurf freischaltet, zählt um `research_design_bonus` mehr.
fn war_research_target(data: &GameData, entry: &PolityResearch) -> Option<u16> {
    let balance = &data.balance.research;
    let mut best: Option<(u16, f64)> = None;
    for tech in available_of(data, &entry.known) {
        let unlocks = data.designs.iter().any(|design| {
            design.stats.techs.contains(&tech)
                && design
                    .stats
                    .techs
                    .iter()
                    .all(|needed| *needed == tech || entry.known[*needed as usize])
        });
        let bonus = if unlocks {
            data.balance.military.ai.research_design_bonus
        } else {
            1.0
        };
        let branch = data.techs[tech as usize].branch;
        let score = balance.auto_war.of(branch) * entry.focus.of(branch) * bonus
            / entry.remaining(data, tech).max(1.0);
        if best.is_none_or(|(_, best_score)| score > best_score) {
            best = Some((tech, score));
        }
    }
    best.map(|(tech, _)| tech)
}

/// Monatliche Kosten eines Schiffs dieses Bauwerts für den Haushalt: während des Baus die
/// Monatsrate der Werft, danach Sold und Wartung; es zählt die höhere der beiden.
fn burden(data: &GameData, design: u16, claim_rate: f64) -> f64 {
    let stats = &data.designs[design as usize].stats;
    let months = (stats.build_days as f64 / DAYS_PER_MONTH as f64).max(1.0);
    let building = stats.value * (1.0 + data.balance.military.yard_fee) / months;
    building.max(stats.value * claim_rate)
}

/// Monatsbeginn: Forschungspolitik, Warteplatz, Rüstung und Stilllegung jeder
/// KI-Gemeinschaft.
pub fn plan_month(
    sit: &Situation,
    ai: &mut AiState,
    queue: &mut CommandQueue,
    player: Option<PolityId>,
) {
    let data = sit.data;
    let levers = &data.balance.military.ai;
    for (id, home) in led(sit, player) {
        let mode = ai.mode(id);
        let at_war = sit.diplomacy.is_at_war(id);
        let mut hand = Hand::new(queue, id);
        let plan = ai.plan_mut(id);

        // Ist wieder Sold ausgeblieben? Der Rückstand folgt dem ausgebliebenen Anteil
        // geglättet; aus seiner Änderung lässt sich der Anteil des letzten Monats lesen.
        let arrears = sit.fleets.arrears_of(id);
        let smoothing = data.balance.military.arrears_smoothing;
        let missed =
            ((arrears - (1.0 - smoothing) * plan.last_arrears) / smoothing).clamp(0.0, 1.0);
        plan.last_arrears = arrears;
        let short = mode != MilitaryMode::Resting && missed > MISSED_PAY;
        // Gezählt wird über die letzten zwölf Monate: Die Einnahmen schwanken, und wer
        // seine Flotte nicht trägt, bleibt den Sold eher jeden zweiten Monat schuldig als
        // viele in Folge.
        plan.short_mask = ((plan.short_mask << 1) | short as u16) & 0x0fff;
        plan.short_months = plan.short_mask.count_ones();
        plan.calm_months = if short {
            0
        } else {
            plan.calm_months.saturating_add(1)
        };
        plan.months_since_layup = plan.months_since_layup.saturating_add(1);
        let last_month = sit.economy.budget(id).and_then(|budget| budget.last());
        // Was die Gemeinschaft auf lange Sicht einnimmt: ein träges Mittel über Jahre.
        if let Some(month) = last_month {
            let revenue = to_credits(month.revenue);
            plan.usual_revenue = if plan.usual_revenue > 0.0 {
                plan.usual_revenue
                    + (revenue - plan.usual_revenue) / levers.revenue_memory_months as f64
            } else {
                revenue
            };
        }

        // --- Forschung: Etat der Vorlage, im Krieg höher, gekürzt, solange der Haushalt
        // ihn nicht trägt; im Krieg ein Ziel, das der Flotte hilft. ---
        if let Some(entry) = sit.research.get(id) {
            let base = ai_budget_share(&data.balance.research, entry.usual_share, at_war);
            let mut share = base;
            // Das Ziel wählt die Führung nur, solange es die Automatik oder ihre eigene
            // frühere Wahl ersetzt; was von außen gesetzt wurde, gilt bis zum Abschluss.
            let own_choice = entry.auto || entry.target == plan.research_target;
            let mut target = None;
            if mode != MilitaryMode::Resting {
                // Ein einzelner knapper Monat ist kein Grund: Gekürzt wird, wenn die
                // Forschung zwei Monate in Folge nicht bekam, was sie verlangte.
                let history = sit
                    .economy
                    .budget(id)
                    .map_or(&[][..], |budget| budget.history.as_slice());
                let starved = history.len() >= 2
                    && history[history.len() - 2..].iter().all(|month| {
                        let line = month.line(Claim::Research);
                        line.wanted > 0 && (line.paid as f64) < 0.9 * line.wanted as f64
                    });
                if short || starved {
                    plan.research_factor *= levers.research_cut;
                    if plan.research_factor < 0.05 {
                        plan.research_factor = 0.0;
                    }
                } else if last_month.is_some_and(|month| month.remainder() > 0) {
                    plan.research_factor =
                        (plan.research_factor.max(0.05) / levers.research_cut).min(1.0);
                }
                share = base * plan.research_factor;
                if at_war {
                    target = war_research_target(data, entry);
                }
            }
            hand.research_share(entry, share, data.balance.research.max_budget_share);
            if own_choice {
                hand.research_target(entry, target);
                plan.research_target = target;
            }
        }
        if mode == MilitaryMode::Resting {
            continue;
        }

        // --- Lage: wer bedroht, und womit. ---
        let seen = sightings(data, sit.fleets, id);
        let threat = greatest_threat(sit, id, &seen);
        let foe = threat.map(|(foe, _)| foe);
        let mut foe_mix: ClassValues = [0.0; 6];
        for sighting in seen
            .iter()
            .filter(|sighting| foe.is_none_or(|foe| sighting.owner == foe))
        {
            add(&mut foe_mix, &sighting.values);
        }
        plan.station = Some(choose_station(sit, id, home, foe));

        // --- Rüstung ---
        let mut own_mix: ClassValues = [0.0; 6];
        for fleet in sit.fleets.of(id) {
            add(&mut own_mix, &seen_values(data, fleet));
        }
        let afloat = total(&own_mix);
        for order in sit.fleets.orders_of(id) {
            let stats = &data.designs[order.design as usize].stats;
            own_mix[stats.class.index()] += stats.value;
        }
        let military = &data.balance.military;
        let claim_rate = if afloat > 0.0 {
            to_credits(fleet_claim(
                data,
                sit.galaxy,
                sit.economy,
                sit.fleets,
                sit.polities.get(id),
            )) / afloat
        } else {
            military.upkeep_per_month + military.maintenance_per_month
        };
        let history: &[BudgetMonth] = sit
            .economy
            .budget(id)
            .map_or(&[], |budget| budget.history.as_slice());
        let recent = &history[history.len().saturating_sub(BUDGET_MONTHS_MAX)..];
        // Bestellt wird nur aus gefüllter Rücklage: Lebt der Haushalt schon von ihr, kommt
        // nichts dazu, was er tragen müsste.
        let funded = last_month
            .is_some_and(|month| month.treasury as f64 >= 0.9 * month.reserve_target as f64);
        let mean = |f: fn(&BudgetMonth) -> i64| {
            recent.iter().map(|month| to_credits(f(month))).sum::<f64>()
                / recent.len().max(1) as f64
        };
        // Zielanteil: im Frieden der Grundwert, gehoben um die Bedrohung, im Krieg
        // vervielfacht, nie über der Obergrenze. Daraus der Bauwert, den die Einnahmen
        // der letzten Monate tragen sollen.
        let excess = threat.map_or(0.0, |(_, value)| {
            (value / afloat.max(military.starting_fleet_value) - 1.0).clamp(0.0, 2.0)
        });
        let war = if at_war { levers.war_factor } else { 1.0 };
        let share =
            (levers.peace_share * (1.0 + levers.threat_gain * excess) * war).min(levers.max_share);
        // Im Frieden urteilt die Rüstung erst über ein ganzes Jahr: Die ersten Monate
        // einer jungen Gemeinschaft sagen wenig darüber, was sie auf Dauer trägt.
        let months_needed = if at_war {
            BUDGET_MONTHS_MIN
        } else {
            BUDGET_MONTHS_MAX
        };
        let judged = recent.len() >= months_needed && claim_rate > 0.0;
        // Die Einnahmen kommen in Schüben, und ein Haushalt, dem das Geld ausgeht, nimmt
        // auch weniger ein. Tragen muss die Flotte sich in den schwachen Monaten.
        let lean_revenue = {
            let mut revenues: Vec<f64> = recent
                .iter()
                .map(|month| to_credits(month.revenue))
                .collect();
            revenues.sort_by(f64::total_cmp);
            let count = ((revenues.len() as f64 * levers.lean_share).ceil() as usize)
                .clamp(1, revenues.len().max(1));
            revenues.iter().take(count).sum::<f64>() / count as f64
        };
        // Bestellt wird bis zu dem, was die schwachen Monate und die lange Sicht beide
        // tragen. Stillgelegt wird erst, was über dem liegt, was eines von beiden trägt:
        // Einen Einbruch von einigen Monaten sitzt die Flotte aus.
        let (target_value, keep_value) = if judged {
            (
                share * lean_revenue.min(plan.usual_revenue) / claim_rate,
                share * lean_revenue.max(plan.usual_revenue) / claim_rate,
            )
        } else {
            (f64::INFINITY, f64::INFINITY)
        };
        // Im Frieden baut die KI nicht gleich wieder, was sie eben nicht tragen konnte.
        let calm = at_war
            || (plan.calm_months >= levers.quiet_months
                && plan.months_since_layup >= levers.layup_quiet_months);
        if !short && funded && judged && calm {
            let room = levers.room_use * mean(|month| month.remainder());
            let known = sit.research.known(id);
            let mut committed: f64 = sit
                .fleets
                .orders_of(id)
                .map(|order| burden(data, order.design, claim_rate))
                .sum();
            let mut yards = free_yards(
                data,
                sit.galaxy,
                sit.economy,
                sit.polities,
                sit.fleets,
                sit.diplomacy,
                id,
            );
            while let Some(yard) = yards.iter_mut().find(|(_, free)| *free > 0) {
                // Die Klasse mit der besten Summe gegen die Mischung des Gegners; was die
                // eigene Flotte schon reichlich hat, zählt weniger. Zu teuer für den
                // Haushalt oder über dem Ziel scheidet aus.
                let own_total = total(&own_mix);
                let mut best: Option<(ShipClass, f64, f64)> = None;
                for class in ShipClass::ALL {
                    if class == ShipClass::Fighter {
                        continue;
                    }
                    let Some(design) = data.best_design(class, known) else {
                        continue;
                    };
                    let value = data.designs[design as usize].stats.value;
                    let cost = burden(data, design, claim_rate);
                    // Kein einzelnes Schiff darf mehr wert sein als das ganze Ziel: Eine
                    // kleine Gemeinschaft baut kleine Schiffe.
                    if committed + cost > room
                        || value > target_value
                        || own_total + 0.5 * value > target_value
                    {
                        continue;
                    }
                    let held = if own_total > 0.0 {
                        own_mix[class.index()] / own_total
                    } else {
                        0.0
                    };
                    let score =
                        class_score(data, class, &foe_mix) * (1.0 - levers.mix_weight * held);
                    if best.is_none_or(|(_, _, best_score)| score > best_score) {
                        best = Some((class, value, score));
                    }
                }
                let Some((class, value, _)) = best else {
                    break;
                };
                hand.build_class(yard.0, class);
                yard.1 -= 1;
                own_mix[class.index()] += value;
                committed += burden(
                    data,
                    data.best_design(class, known).expect("eben gewählt"),
                    claim_rate,
                );
            }
        }

        // --- Wenn der Sold ausbleibt (nur im Frieden; im Krieg trägt die Anleihe) ---
        if short && !at_war {
            // Zuerst das, was noch nicht gebaut ist: der jüngste Auftrag, der wartet oder
            // am Geld stockt.
            if let Some(order) = sit
                .fleets
                .orders_of(id)
                .filter(|order| {
                    matches!(
                        order.status,
                        BuildStatus::Queued | BuildStatus::MissingMoney
                    )
                })
                .last()
            {
                hand.cancel_build(order);
            }
            // Dann, nach der Frist, Schiffe: so viel Bauwert, wie zuletzt Sold ausblieb,
            // die entbehrlichsten zuerst, aber nie unter das, was die Einnahmen auf lange
            // Sicht tragen. Liegt die Flotte schon darunter, ist sie nicht der Grund.
            let surplus = (missed as f64 * afloat).min(afloat - keep_value);
            if plan.short_months >= levers.layup_after_months
                && surplus > 0.0
                && lay_up_surplus(sit, &mut hand, id, &foe_mix, surplus)
            {
                // Was die Stilllegung bringt, zeigt sich erst in den nächsten Monaten.
                plan.short_mask = 0;
                plan.short_months = 0;
                plan.calm_months = 0;
                plan.months_since_layup = 0;
            }
        }
    }
}

/// Legt Schiffe im Bauwert `surplus` still (mindestens eines): die Klassen zuerst, die
/// gegen die Mischung des Gegners am wenigsten taugen, darin die jüngsten Schiffe. Nur aus
/// Flotten, die ohne Marschbefehl bei einer eigenen Werft liegen. Gibt zurück, ob ein
/// Befehl erging.
fn lay_up_surplus(
    sit: &Situation,
    hand: &mut Hand,
    polity: PolityId,
    foe_mix: &ClassValues,
    surplus: f64,
) -> bool {
    let data = sit.data;
    let docked: Vec<&Fleet> = sit
        .fleets
        .of(polity)
        .filter(|fleet| {
            !fleet.in_battle
                && fleet.route.is_empty()
                && has_yard(
                    data,
                    sit.galaxy,
                    sit.economy,
                    sit.polities,
                    sit.fleets,
                    polity,
                    fleet.system,
                )
        })
        .collect();
    let mut ships: Vec<(f64, ShipId, FleetId, f64)> = docked
        .iter()
        .flat_map(|fleet| {
            fleet.ships.iter().map(|ship| {
                let stats = &data.designs[ship.design as usize].stats;
                (
                    class_score(data, stats.class, foe_mix),
                    ship.id,
                    fleet.id,
                    stats.value,
                )
            })
        })
        .collect();
    ships.sort_by(|a, b| a.0.total_cmp(&b.0).then(b.1.cmp(&a.1)));
    let mut chosen: BTreeMap<FleetId, Vec<ShipId>> = BTreeMap::new();
    let mut shed = 0.0;
    for (_, ship, fleet, value) in ships {
        if shed >= surplus && shed > 0.0 {
            break;
        }
        chosen.entry(fleet).or_default().push(ship);
        shed += value;
    }
    let mut issued = false;
    for fleet in docked {
        if let Some(ships) = chosen.remove(&fleet.id) {
            issued |= hand.lay_up(fleet, ships, true);
        }
    }
    issued
}

/// Was die Führung einer Gemeinschaft an einem Tag über ihren Raum weiß.
struct Theatre {
    home: SystemId,
    station: SystemId,
    /// Je System: Dort kann die Gemeinschaft heute Vorräte fassen (eigenes Gebiet, nicht
    /// blockiert).
    bases: Vec<bool>,
    /// Je System: Dort hat sie eine Werft.
    yards: Vec<bool>,
}

impl Theatre {
    fn new(sit: &Situation, polity: PolityId, home: SystemId, plan: &MilitaryPlan) -> Self {
        let bases: Vec<bool> = sit
            .galaxy
            .systems
            .iter()
            .map(|system| {
                supply_base(sit.galaxy, sit.diplomacy, polity, system.id)
                    && !sit.fleets.is_blockaded(system.id)
            })
            .collect();
        let yards = sit
            .galaxy
            .systems
            .iter()
            .map(|system| {
                bases[system.index()]
                    && has_yard(
                        sit.data,
                        sit.galaxy,
                        sit.economy,
                        sit.polities,
                        sit.fleets,
                        polity,
                        system.id,
                    )
            })
            .collect();
        let station = plan
            .station
            .filter(|station| bases[station.index()])
            .unwrap_or(home);
        Self {
            home,
            station,
            bases,
            yards,
        }
    }
}

/// Das nächste System (nach Routenlänge, dann Nummer), das die Bedingung erfüllt.
fn nearest(distance: &[f32], accept: impl Fn(usize) -> bool) -> Option<SystemId> {
    (0..distance.len())
        .filter(|index| distance[*index].is_finite() && accept(*index))
        .min_by(|a, b| distance[*a].total_cmp(&distance[*b]).then(a.cmp(b)))
        .map(SystemId::from_index)
}

/// Vereinigt eigene Flotten, die einsatzbereit und ohne Marschbefehl im selben System
/// liegen: jede kleinere mit der größten dort. Gibt zurück, welche Flotten dadurch heute
/// gebunden sind.
fn merge_idle(
    sit: &Situation,
    hand: &mut Hand,
    plan: &mut MilitaryPlan,
    own: &[&Fleet],
    blocked: impl Fn(SystemId) -> bool,
) -> BTreeSet<FleetId> {
    let mut busy = BTreeSet::new();
    let mut by_system: BTreeMap<SystemId, Vec<&Fleet>> = BTreeMap::new();
    for fleet in own {
        if fleet.is_ready() && fleet.route.is_empty() && !blocked(fleet.system) {
            by_system.entry(fleet.system).or_default().push(fleet);
        }
    }
    for group in by_system.values().filter(|group| group.len() > 1) {
        let Some(largest) = group.iter().max_by(|a, b| {
            a.build_value(sit.data)
                .total_cmp(&b.build_value(sit.data))
                .then(b.id.cmp(&a.id))
        }) else {
            continue;
        };
        for fleet in group.iter().filter(|fleet| fleet.id != largest.id) {
            if hand.merge(fleet, largest) {
                plan.operations.remove(&fleet.id);
                busy.insert(fleet.id);
                busy.insert(largest.id);
            }
        }
    }
    busy
}

/// Tagesbeginn: Jede KI-Gemeinschaft führt ihre Flotten.
pub fn lead_fleets(
    sit: &Situation,
    ai: &mut AiState,
    queue: &mut CommandQueue,
    player: Option<PolityId>,
) {
    for (id, home) in led(sit, player) {
        if ai.mode(id) != MilitaryMode::Active {
            continue;
        }
        let own: Vec<&Fleet> = sit
            .fleets
            .of(id)
            .filter(|fleet| !ai.is_reserved(fleet.id))
            .collect();
        let mut plan = ai.plan(id);
        plan.operations
            .retain(|fleet, _| own.iter().any(|own| own.id == *fleet));
        if !own.is_empty() {
            let theatre = Theatre::new(sit, id, home, &plan);
            let mut hand = Hand::new(queue, id);
            let enemies: Vec<PolityId> = sit.diplomacy.enemies_of(id).collect();
            if enemies.is_empty() {
                lead_in_peace(sit, &theatre, &mut plan, &mut hand, &own);
            } else {
                lead_at_war(sit, &theatre, &mut plan, &mut hand, id, &own, &enemies);
            }
        }
        if ai.plan(id) != plan {
            *ai.plan_mut(id) = plan;
        }
    }
}

/// Frieden: Flotten sind versorgt und instand gesetzt, vereinigt, und warten am
/// Warteplatz im Geleitschutz.
fn lead_in_peace(
    sit: &Situation,
    theatre: &Theatre,
    plan: &mut MilitaryPlan,
    hand: &mut Hand,
    own: &[&Fleet],
) {
    plan.operations.clear();
    plan.met.clear();
    plan.post = None;
    plan.no_offensive_before = 0;
    let busy = merge_idle(sit, hand, plan, own, |_| false);
    // Bleibt Sold aus, sammelt sich die Flotte bei einer Werft: Nur dort lässt sich
    // stilllegen.
    let muster = plan.short_months >= 2;
    for fleet in own {
        if !fleet.is_ready() || busy.contains(&fleet.id) {
            continue;
        }
        hand.set_stance(fleet, Stance::Escort);
        hand.set_orders(fleet, fleet.formation, Doctrine::Balanced);
        let at = fleet.system.index();
        // Ein neues Schiff fasst erst Treibstoff, bevor es zur Flotte stößt.
        if fleet.route.is_empty() && theatre.bases[at] && fleet.min_fuel() < FUEL_TO_SAIL {
            continue;
        }
        let needs_yard = muster || fleet.ships.iter().any(|ship| ship.is_damaged(sit.data));
        let to = if needs_yard {
            if fleet.route.is_empty() && theatre.yards[at] {
                fleet.system
            } else {
                let (distance, _) = sit.galaxy.distances_from(fleet.system, |_| true);
                nearest(&distance, |index| theatre.yards[index]).unwrap_or(theatre.home)
            }
        } else {
            theatre.station
        };
        hand.move_fleet(fleet, to);
    }
}

/// Zustand einer Flotte 0..1: Kampfwert ihrer Schiffe gegen ihren Bauwert.
fn condition(data: &GameData, fleet: &Fleet) -> f64 {
    let value = fleet.build_value(data) * data.balance.military.strength_per_value;
    if value > 0.0 {
        (fleet.strength as f64 / value).clamp(0.0, 1.0)
    } else {
        1.0
    }
}

/// Krieg: erst die Verteidigung, dann, bei Überlegenheit, der Angriff.
fn lead_at_war(
    sit: &Situation,
    theatre: &Theatre,
    plan: &mut MilitaryPlan,
    hand: &mut Hand,
    id: PolityId,
    own: &[&Fleet],
    enemies: &[PolityId],
) {
    let data = sit.data;
    let levers = &data.balance.military.ai;
    let fuel_per_distance = data.balance.military.fuel_per_distance;

    // Gegner je System. Wer marschiert, zählt am nächsten System seines Wegs; wer sich
    // sammelt, kämpft nicht und zählt nicht. Die Technik steckt im Wert: Der Bauwert des
    // Gegners wird durch den eigenen Vorsprung geteilt.
    let seen: Vec<Sighting> = sightings(data, sit.fleets, id)
        .into_iter()
        .filter(|sighting| enemies.contains(&sighting.owner) && !sighting.regrouping)
        .collect();
    let mut foes: BTreeMap<SystemId, ClassValues> = BTreeMap::new();
    // Gegner, die ohne Marschbefehl in einem System liegen: Wer dort mit ihnen steht,
    // kämpft noch heute.
    let mut standing: BTreeSet<SystemId> = BTreeSet::new();
    let mut edges: BTreeMap<PolityId, f64> = BTreeMap::new();
    for sighting in &seen {
        let edge = *edges
            .entry(sighting.owner)
            .or_insert_with(|| tech_edge(data, sit.research, id, sighting.owner));
        let mut values = sighting.values;
        for value in &mut values {
            *value /= edge;
        }
        add(foes.entry(sighting.position()).or_default(), &values);
        if sighting.heading.is_none() {
            standing.insert(sighting.system);
        }
    }
    // Systeme, in denen eine fremde Flotte steht, mit der die eigene eben im Gefecht
    // stand, auch wenn sie sich dort sammelt. Dorthin führt kein neuer Angriff: Wer
    // ausgewichen ist, weicht wieder aus, und entschieden wird nichts.
    let known: BTreeSet<SystemId> = sightings(data, sit.fleets, id)
        .iter()
        .filter(|sighting| plan.met.contains_key(&sighting.fleet))
        .map(Sighting::position)
        .collect();
    let nobody: ClassValues = [0.0; 6];
    let foes_at = |system: SystemId| foes.get(&system).unwrap_or(&nobody);
    let clear = |index: usize| !foes.contains_key(&SystemId::from_index(index));

    // Was im Umkreis eines Systems an Gegnern steht und dort eingreifen kann: Daran misst
    // sich ein Angriff, nicht nur an dem, was im Ziel selbst liegt.
    let mut around: Vec<ClassValues> = vec![[0.0; 6]; sit.galaxy.systems.len()];
    for (system, values) in &foes {
        for (slot, hops) in around.iter_mut().zip(sit.galaxy.hops_from(*system)) {
            if hops <= levers.response_jumps {
                add(slot, values);
            }
        }
    }
    // Wer sich gestellt hat, wird nach rematch_days wieder aufgesucht; wer nur
    // ausgewichen ist, erst nach elusive_days.
    plan.met.retain(|_, meeting| {
        let wait = if meeting.fought {
            levers.rematch_days
        } else {
            levers.elusive_days
        };
        sit.day < meeting.day + wait
    });

    // --- Im Gefecht: Rückzug, bevor die Flotte verloren ist. ---
    for fleet in own.iter().filter(|fleet| fleet.in_battle) {
        let Some((battle, side)) = sit.battles.of_fleet(fleet.id) else {
            continue;
        };
        // Wer hier gegenübersteht, wird so bald nicht wieder aufgesucht: Wich er aus,
        // lässt er sich nicht stellen.
        let fought = battle
            .combatants
            .iter()
            .any(|combatant| combatant.lost_value > 0.0);
        for (other, combatant) in battle.combatants.iter().enumerate() {
            if battle.is_hostile(side as u16, other as u16)
                && let Some(foe) = combatant.fleet
            {
                plan.met.insert(
                    foe,
                    Meeting {
                        day: sit.day,
                        fought,
                    },
                );
            }
        }
        // Dieselbe Schätzung wie vor dem Treffen, auf dem Stand des Gefechts.
        let (mine, theirs, hostile) = battle_values(sit.battles, battle, side);
        // Mit demselben Vorsprung der Technik wie vor dem Treffen (gegen mehrere Gegner
        // dem kleinsten): Sonst träte die Flotte an und bräche am ersten Tag wieder ab.
        let edge = hostile
            .iter()
            .map(|foe| tech_edge(data, sit.research, id, *foe))
            .min_by(f64::total_cmp)
            .unwrap_or(1.0);
        let outmatched =
            total(&theirs) > 0.0 && odds(data, &mine, &theirs, edge) < levers.retreat_odds;
        let worn = plan.operations.get(&fleet.id).is_some_and(|operation| {
            fleet.build_value(data) < (1.0 - levers.break_off_loss) * operation.start_value
        });
        if (outmatched || worn) && hand.retreat(fleet, sit.battles) {
            // Wer so weichen musste, greift nicht gleich wieder an.
            plan.no_offensive_before = sit.day + levers.caution_days;
        }
    }

    // Je System der nächste eigene Hafen, an dem kein Gegner steht, und der Weg von dort:
    // von ihm aus beginnt ein Angriff, und zu ihm führt der Rückweg.
    let mut staging: Vec<Option<(SystemId, f32)>> = vec![None; sit.galaxy.systems.len()];
    for base in (0..theatre.bases.len()).filter(|index| theatre.bases[*index] && clear(*index)) {
        let (from_base, _) = sit
            .galaxy
            .distances_from(SystemId::from_index(base), |_| true);
        for (slot, length) in staging.iter_mut().zip(from_base) {
            if length.is_finite() && slot.is_none_or(|(_, best)| length < best) {
                *slot = Some((SystemId::from_index(base), length));
            }
        }
    }

    let busy = merge_idle(sit, hand, plan, own, |system| standing.contains(&system));
    let navy: f64 = own.iter().map(|fleet| fleet.build_value(data)).sum();
    let main = own
        .iter()
        .max_by(|a, b| {
            a.build_value(data)
                .total_cmp(&b.build_value(data))
                .then(b.id.cmp(&a.id))
        })
        .map(|fleet| fleet.id);
    // Eigene Systeme, in denen ein Gegner steht oder gleich stehen wird. Eine Flotte, die
    // im letzten Gefecht nur ausgewichen ist, zählt nicht: Sie wiche wieder aus.
    let intruded: BTreeSet<SystemId> = seen
        .iter()
        .filter(|sighting| {
            plan.met
                .get(&sighting.fleet)
                .is_none_or(|meeting| meeting.fought)
        })
        .map(Sighting::position)
        .filter(|system| sit.galaxy.owns_planet_in(id, *system))
        .collect();

    for fleet in own {
        if !fleet.is_ready() || busy.contains(&fleet.id) {
            continue;
        }
        // Wer marschiert, entscheidet vom nächsten System seines Wegs aus: Dort gilt ein
        // neuer Befehl ohnehin erst.
        let at = fleet.route.first().copied().unwrap_or(fleet.system);
        // Jede Flotte rechnet für sich: Was neben ihr liegt, ist ab morgen mit ihr vereinigt.
        let force = own_values(data, fleet);
        let chances = |system: SystemId| odds(data, &force, foes_at(system), 1.0);
        // Dasselbe gegen alles, was im Umkreis des Systems eingreifen kann.
        let expect = |system: SystemId| odds(data, &force, &around[system.index()], 1.0);
        let doctrine_for = |chance: f64| {
            if chance >= levers.offensive_odds {
                Doctrine::Aggressive
            } else if chance < levers.defend_odds {
                Doctrine::Defensive
            } else {
                Doctrine::Balanced
            }
        };

        // Der Gegner liegt schon im selben System: Das Gefecht beginnt heute. Zu befehlen
        // bleibt nur, wie die Flotte kämpft.
        if fleet.route.is_empty() && standing.contains(&at) {
            hand.set_orders(fleet, fleet.formation, doctrine_for(chances(at)));
            continue;
        }

        let (ammo, fuel) = fleet.supply(data);
        let supplies = ammo.min(fuel);
        let value = fleet.build_value(data);
        let state = condition(data, fleet);
        let (distance, _) = sit.galaxy.distances_from(at, |_| true);
        // Führt der Weg dorthin an keinem Gegner vorbei? Marschiert wird immer auf dem
        // kürzesten Weg.
        let way_clear = |index: usize| {
            index == at.index()
                || sit
                    .galaxy
                    .shortest_path(at, SystemId::from_index(index), |_| true)
                    .is_some_and(|path| path.iter().skip(1).all(|step| clear(step.index())))
        };
        let safe_base = |prefer_yard: bool| {
            nearest(&distance, |index| {
                theatre.bases[index]
                    && clear(index)
                    && (!prefer_yard || theatre.yards[index])
                    && way_clear(index)
            })
        };
        // Wohin eine Flotte geht, die Vorräte fassen oder instand gesetzt werden muss: zu
        // einer Werft, wenn der Tank bis dorthin reicht, sonst erst in den nächsten Hafen.
        // Ist kein Hafen ohne Gegner erreichbar, wartet sie, wo sie ist, statt ihm in die
        // Arme zu laufen.
        let refuge = || {
            let in_tank =
                |base: &SystemId| distance[base.index()] * fuel_per_distance <= fleet.min_fuel();
            safe_base(true)
                .filter(in_tank)
                .or_else(|| safe_base(false))
                .unwrap_or(at)
        };
        let mut operation = plan.operations.get(&fleet.id).copied();

        // --- Ausweichen: Ein überlegener Gegner ist im Anmarsch auf das eigene System
        // oder steht schon nebenan. Wer erst reagiert, wenn er da ist, kämpft. Das gilt
        // auch für eine Flotte, die gerade Vorräte fasst. ---
        let mut closing = *foes_at(at);
        for neighbor in sit.galaxy.neighbors(at) {
            add(&mut closing, foes_at(neighbor.system));
        }
        if total(&closing) > 0.0
            && odds(data, &force, &closing, 1.0) < AVOID_MARGIN * levers.defend_odds
        {
            // Ein Hafen, an dem kein Gegner steht und neben dem keiner steht.
            let quiet = |index: usize| {
                let system = SystemId::from_index(index);
                clear(index)
                    && sit
                        .galaxy
                        .neighbors(system)
                        .iter()
                        .all(|neighbor| clear(neighbor.system.index()))
            };
            // Am liebsten auf einem Weg, der auch an keinem Gegner vorbeiführt: Wer neben
            // ihm vorbeizieht, dem tritt er in den Weg.
            let way_quiet = |index: usize| {
                sit.galaxy
                    .shortest_path(at, SystemId::from_index(index), |_| true)
                    .is_some_and(|path| path.iter().skip(1).all(|step| quiet(step.index())))
            };
            // ... und den der Tank noch erreicht.
            let haven = |index: usize| {
                index != at.index()
                    && theatre.bases[index]
                    && quiet(index)
                    && distance[index] * fuel_per_distance <= fleet.min_fuel()
            };
            let away = nearest(&distance, |index| haven(index) && way_quiet(index))
                .or_else(|| nearest(&distance, |index| haven(index) && way_clear(index)));
            if let Some(away) = away {
                // Wer so weichen muss, kommt nicht gleich wieder: weder mit einem Angriff
                // noch an den Platz, den er eben räumt.
                plan.operations.remove(&fleet.id);
                plan.no_offensive_before = sit.day + levers.caution_days;
                hand.set_stance(fleet, Stance::Escort);
                hand.set_orders(fleet, fleet.formation, Doctrine::Defensive);
                hand.move_fleet(fleet, away);
                continue;
            }
        }

        // --- Abbruch: zu wenig Vorrat oder zu hohe Verluste. Dann nach Hause. ---
        // Der Rückweg muss im Tank bleiben: der Weg zum nächsten Hafen und eine Reserve.
        let way_back = staging[at.index()].map_or(0.0, |(_, length)| length * fuel_per_distance);
        let spent = match operation {
            Some(running) if running.mission != Mission::Refit => {
                supplies < levers.break_off_supply
                    || fleet.min_fuel() < way_back + FUEL_RESERVE
                    || value < (1.0 - levers.break_off_loss) * running.start_value
            }
            Some(_) => false,
            // Ohne Unternehmung fern jeder Versorgung, oder schwer beschädigt fern einer
            // Werft (etwa nach einem Rückzug).
            None => {
                (supplies < levers.break_off_supply && !theatre.bases[at.index()])
                    || (state < 0.6 && !theatre.yards[at.index()])
            }
        };
        if spent {
            let target = refuge();
            let refit = Operation {
                mission: Mission::Refit,
                target,
                start_value: value,
            };
            plan.operations.insert(fleet.id, refit);
            operation = Some(refit);
        }
        if let Some(mut refit) = operation.filter(|running| running.mission == Mission::Refit) {
            // Steht inzwischen ein Gegner im Ziel oder auf dem Weg, wählt sie neu.
            if !(clear(refit.target.index()) && way_clear(refit.target.index())) {
                refit.target = refuge();
                plan.operations.insert(fleet.id, refit);
            }
            let arrived = fleet.route.is_empty() && at == refit.target;
            let restored = supplies >= levers.ready_supply
                && (state >= levers.ready_supply as f64 || !theatre.yards[at.index()]);
            if arrived && (restored || !theatre.bases[at.index()]) {
                // Wiederhergestellt, oder der Stützpunkt taugt nicht mehr: neu entscheiden.
                plan.operations.remove(&fleet.id);
            } else {
                hand.set_stance(fleet, Stance::Escort);
                // Mit leerem Tank schleppt sich die Flotte nur: Der Weg, der heute frei
                // ist, ist es nicht mehr, wenn sie ankommt. Wer eben erst weichen musste,
                // bleibt deshalb liegen, statt denselben Weg gleich wieder zu versuchen.
                let dry = fleet.min_fuel() < distance[refit.target.index()] * fuel_per_distance;
                let stranded = dry && fleet.route.is_empty() && sit.day < plan.no_offensive_before;
                if !stranded {
                    hand.move_fleet(fleet, refit.target);
                }
                continue;
            }
        }

        // --- Verteidigung zuerst: einem Eindringling begegnen, wo die Kräfte reichen. ---
        let is_main = main == Some(fleet.id) || total(&force) >= 0.5 * navy;
        if !intruded.is_empty() {
            let meet = intruded
                .iter()
                .copied()
                .filter(|system| chances(*system) >= levers.defend_odds)
                .max_by(|a, b| {
                    let people = |system: SystemId| -> f64 {
                        sit.galaxy
                            .planets_in(system)
                            .filter(|planet| planet.owner == Some(id))
                            .map(|planet| planet.population)
                            .sum()
                    };
                    people(*a).total_cmp(&people(*b)).then(b.cmp(a))
                });
            plan.operations.remove(&fleet.id);
            hand.set_stance(fleet, Stance::Escort);
            match meet {
                Some(system) => {
                    hand.set_orders(fleet, fleet.formation, doctrine_for(chances(system)));
                    // Wo die Hauptflotte einen Eindringling stellt, bleibt sie danach
                    // stehen: Zieht sie ab, sobald er weicht, kommt er wieder.
                    if is_main {
                        plan.post = Some(system);
                    }
                    // Aus dem Hafen läuft sie erst aus, wenn der Tank für den Weg reicht:
                    // Mit leerem Tank schleppt sie sich nur und käme später an.
                    let need = (distance[system.index()] * fuel_per_distance + FUEL_RESERVE)
                        .min(levers.ready_supply);
                    let in_port = fleet.route.is_empty() && theatre.bases[at.index()];
                    if !(in_port && at != system && fleet.min_fuel() < need) {
                        hand.move_fleet(fleet, system);
                    }
                }
                None => {
                    // Kein Treffen, das sich lohnt: unter Versorgung bleiben.
                    let hold = if theatre.bases[at.index()] && clear(at.index()) {
                        at
                    } else {
                        safe_base(false).unwrap_or(at)
                    };
                    hand.set_orders(fleet, fleet.formation, Doctrine::Defensive);
                    if fleet.route.is_empty() && hold == at {
                        continue;
                    }
                    hand.move_fleet(fleet, hold);
                }
            }
            continue;
        }

        // Ein neues Schiff fasst erst Treibstoff.
        let fuelling =
            fleet.route.is_empty() && theatre.bases[at.index()] && fleet.min_fuel() < FUEL_TO_SAIL;

        // --- Kleine Verbände stoßen zur Hauptflotte, wenn die in einem Hafen liegt. ---
        if !is_main {
            plan.operations.remove(&fleet.id);
            hand.set_stance(fleet, Stance::Escort);
            let rally = own
                .iter()
                .find(|other| Some(other.id) == main)
                .filter(|main| {
                    main.is_ready() && main.route.is_empty() && theatre.bases[main.system.index()]
                })
                .map(|main| main.system);
            // Nur auf einem Weg, der an keinem Gegner vorbeiführt.
            let to = match rally {
                Some(system) if way_clear(system.index()) => system,
                _ if theatre.bases[at.index()] => at,
                _ => safe_base(false).unwrap_or(at),
            };
            if !fuelling {
                hand.move_fleet(fleet, to);
            }
            continue;
        }

        // --- Angriff: nur ausgeruht beginnen; eine laufende Unternehmung hält durch,
        // solange die Kräfte wenigstens gleich sind. ---
        let running = operation.filter(|running| running.mission != Mission::Refit);
        let fresh = supplies >= levers.ready_supply && state >= levers.ready_supply as f64;
        let mut attack: Option<(Mission, SystemId)> = None;
        // Ein neuer Angriff beginnt im Hafen, nicht mitten im Marsch: Wer unterwegs
        // umkehrt, fährt den Tank leer, bevor er ankommt.
        let rested = sit.day >= plan.no_offensive_before;
        let in_port = fleet.route.is_empty() && theatre.bases[at.index()];
        if running.is_some() || (fresh && in_port && !fuelling && rested) {
            let needed = |mission: Mission, system: SystemId| {
                if running.is_some_and(|op| op.mission == mission && op.target == system) {
                    levers.defend_odds
                } else {
                    levers.offensive_odds
                }
            };
            // Erreichbar ist, was vom nächsten eigenen Hafen aus in Reichweite liegt; eine
            // laufende Unternehmung bleibt bei ihrem Ziel.
            let in_reach = |system: SystemId| {
                running.is_some_and(|op| op.target == system)
                    || staging[system.index()].is_some_and(|(_, length)| {
                        length * fuel_per_distance <= levers.offensive_reach
                    })
            };
            // Die Hauptflotte des Gegners suchen, wenn die eigene stärker ist. Eine Flotte,
            // mit der sie eben im Gefecht stand, sucht sie nicht wieder auf.
            let quarry = seen
                .iter()
                .filter(|sighting| !known.contains(&sighting.position()))
                .max_by(|a, b| {
                    total(&a.values)
                        .total_cmp(&total(&b.values))
                        .then(b.fleet.cmp(&a.fleet))
                })
                .map(|sighting| sighting.position());
            if let Some(system) = quarry
                && in_reach(system)
                && expect(system) >= needed(Mission::Hunt, system)
            {
                attack = Some((Mission::Hunt, system));
            }
            // Sonst das volkreichste System des Gegners sperren, das erreichbar ist und
            // an dem die Kräfte reichen. Eine laufende Blockade bleibt bei ihrem Ziel.
            if attack.is_none() {
                let hostile = |system: SystemId| {
                    sit.galaxy
                        .system_owner(system)
                        .is_some_and(|owner| enemies.contains(&owner))
                };
                let fits = |system: SystemId| {
                    hostile(system)
                        && (!known.contains(&system)
                            || running.is_some_and(|op| op.target == system))
                        && in_reach(system)
                        && expect(system) >= needed(Mission::Blockade, system)
                };
                let kept = running
                    .filter(|op| op.mission == Mission::Blockade && fits(op.target))
                    .map(|op| op.target);
                let people = |system: SystemId| -> f64 {
                    sit.galaxy
                        .planets_in(system)
                        .filter(|planet| planet.owner.is_some_and(|owner| enemies.contains(&owner)))
                        .map(|planet| planet.population)
                        .sum()
                };
                let target = kept.or_else(|| {
                    sit.galaxy
                        .systems
                        .iter()
                        .map(|system| system.id)
                        .filter(|system| fits(*system))
                        .max_by(|a, b| people(*a).total_cmp(&people(*b)).then(b.cmp(a)))
                });
                attack = target.map(|system| (Mission::Blockade, system));
            }
        }
        if let Some((mission, target)) = attack {
            if running.is_none() {
                plan.offensives += 1;
            }
            plan.operations.insert(
                fleet.id,
                Operation {
                    mission,
                    target,
                    start_value: running.map_or(value, |op| op.start_value),
                },
            );
            let in_enemy_space = sit
                .galaxy
                .system_owner(target)
                .is_some_and(|owner| enemies.contains(&owner));
            hand.set_stance(
                fleet,
                if in_enemy_space {
                    Stance::Blockade
                } else {
                    Stance::Guard
                },
            );
            hand.set_orders(fleet, fleet.formation, doctrine_for(chances(target)));
            // Liegt das Ziel von hier aus zu weit, geht es erst in den Hafen davor; und
            // aus einem Hafen läuft die Flotte erst mit vollen Tanks aus.
            let direct = distance[target.index()] * fuel_per_distance;
            let gate = staging[target.index()].map(|(base, _)| base);
            if at != target
                && direct > levers.offensive_reach
                && gate.is_some_and(|gate| gate != at)
            {
                hand.move_fleet(fleet, gate.unwrap_or(at));
            } else if at == target || !in_port || fleet.min_fuel() >= levers.ready_supply {
                hand.move_fleet(fleet, target);
            }
            continue;
        }

        // --- Sonst: am Warteplatz im Geleitschutz, solange er sicher ist; im Krieg dort,
        // wo zuletzt ein Eindringling gestellt wurde. Ein Angriff, der hier endet, wurde
        // vor einem stärkeren Gegner aufgegeben und wird nicht gleich wiederholt. ---
        if running.is_some() {
            plan.no_offensive_before = sit.day + levers.caution_days;
        }
        plan.operations.remove(&fleet.id);
        hand.set_stance(fleet, Stance::Escort);
        hand.set_orders(fleet, fleet.formation, Doctrine::Balanced);
        // Dorthin geht sie nur, wenn sie dem gewachsen ist, was dort und nebenan steht (das
        // Maß, nach dem sie auch ausweicht): Sonst zöge sie hin, wiche aus, sobald sie da
        // ist, und käme wieder. Einen Posten, den sie nicht mehr halten kann, gibt sie auf.
        let tenable = |system: SystemId| {
            let mut near = *foes_at(system);
            for neighbor in sit.galaxy.neighbors(system) {
                add(&mut near, foes_at(neighbor.system));
            }
            odds(data, &force, &near, 1.0) >= AVOID_MARGIN * levers.defend_odds
        };
        if is_main
            && plan
                .post
                .is_some_and(|post| !theatre.bases[post.index()] || !tenable(post))
        {
            plan.post = None;
        }
        let station = plan.post.unwrap_or(theatre.station);
        let wary = sit.day < plan.no_offensive_before && theatre.bases[at.index()];
        let to = if tenable(station) && !wary && way_clear(station.index()) {
            station
        } else if theatre.bases[at.index()] {
            at
        } else {
            safe_base(false).unwrap_or(at)
        };
        if !fuelling {
            hand.move_fleet(fleet, to);
        }
    }
}
