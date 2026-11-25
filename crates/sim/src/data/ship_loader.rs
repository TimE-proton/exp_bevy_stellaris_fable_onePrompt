//! Auflösen und Prüfen von Rümpfen, Modulen, Entwürfen und Technologien.

use super::ships::{DesignStats, ModuleKind, ShipClass, WeaponKind};
use super::{DataError, GameData};
use crate::ids::GoodId;
use std::collections::{BTreeMap, BTreeSet};

fn invalid(file: &'static str, message: String) -> DataError {
    DataError::Invalid { file, message }
}

/// Kleinster Winkelabstand zweier Richtungen in Grad.
fn angle_between(a: f32, b: f32) -> f32 {
    let mut diff = (a - b) % 360.0;
    if diff > 180.0 {
        diff -= 360.0;
    }
    if diff < -180.0 {
        diff += 360.0;
    }
    diff.abs()
}

/// Deckt ein Feuerwinkel (Mittelachse, Breite) die Richtung `bearing` ab?
pub fn arc_covers(facing: f32, arc: f32, bearing: f32) -> bool {
    arc >= 359.9 || angle_between(facing, bearing) <= arc * 0.5
}

pub(super) fn resolve_ships(data: &mut GameData) -> Result<(), DataError> {
    let good_ids: BTreeMap<String, GoodId> = data
        .goods
        .iter()
        .enumerate()
        .map(|(index, good)| (good.key.clone(), GoodId::from_index(index)))
        .collect();
    let good = |file: &'static str, owner: &str, key: &str| {
        good_ids
            .get(key)
            .copied()
            .ok_or_else(|| invalid(file, format!("`{owner}`: unbekanntes Gut `{key}`")))
    };

    // --- Technologien ---
    let tech_keys: Vec<String> = data.techs.iter().map(|tech| tech.key.clone()).collect();
    let tech = |file: &'static str, owner: &str, key: &str| {
        tech_keys
            .iter()
            .position(|candidate| candidate == key)
            .map(|index| index as u16)
            .ok_or_else(|| invalid(file, format!("`{owner}`: unbekannte Technologie `{key}`")))
    };
    let mut seen = BTreeSet::new();
    for entry in &mut data.techs {
        let file = "techs.ron";
        if !seen.insert(entry.key.clone()) {
            return Err(invalid(file, format!("Schlüssel `{}` doppelt", entry.key)));
        }
        if entry.cost <= 0.0 || entry.tier == 0 {
            return Err(invalid(
                file,
                format!("`{}`: Kosten und Stufe müssen positiv sein", entry.key),
            ));
        }
        entry.requires_ids.clear();
        for key in &entry.requires {
            if *key == entry.key {
                return Err(invalid(
                    file,
                    format!("`{}` setzt sich selbst voraus", entry.key),
                ));
            }
            entry.requires_ids.push(tech(file, &entry.key, key)?);
        }
    }

    // --- Module ---
    let mut seen = BTreeSet::new();
    for module in &mut data.modules {
        let file = "modules.ron";
        if !seen.insert(module.key.clone()) {
            return Err(invalid(file, format!("Schlüssel `{}` doppelt", module.key)));
        }
        if module.hp <= 0.0 || module.power < 0.0 {
            return Err(invalid(
                file,
                format!(
                    "`{}`: hp muss positiv, power nicht negativ sein",
                    module.key
                ),
            ));
        }
        module.cost_ids.clear();
        for (key, amount) in &module.cost {
            module
                .cost_ids
                .push((good(file, &module.key, key)?, *amount));
        }
        module.tech_id = match &module.tech {
            Some(key) => Some(tech(file, &module.key, key)?),
            None => None,
        };
        if let ModuleKind::Weapon(weapon) = &module.kind {
            let flying = matches!(weapon.kind, WeaponKind::Kinetic | WeaponKind::Missile);
            if weapon.range <= 0.0
                || weapon.cooldown <= 0.0
                || weapon.damage <= 0.0
                || (flying && weapon.speed <= 0.0)
                || (weapon.kind == WeaponKind::Missile
                    && (weapon.turn <= 0.0 || weapon.shot_hp <= 0.0))
            {
                return Err(invalid(
                    file,
                    format!("`{}`: Waffenwerte unvollständig", module.key),
                ));
            }
        }
    }
    let module_keys: Vec<String> = data
        .modules
        .iter()
        .map(|module| module.key.clone())
        .collect();

    // --- Rümpfe ---
    let mut seen = BTreeSet::new();
    for hull in &data.hulls {
        let file = "hulls.ron";
        if !seen.insert(hull.key.clone()) {
            return Err(invalid(file, format!("Schlüssel `{}` doppelt", hull.key)));
        }
        let split = hull.armor_split.0 + 2.0 * hull.armor_split.1 + hull.armor_split.2;
        if (split - 1.0).abs() > 0.01 {
            return Err(invalid(
                file,
                format!(
                    "`{}`: armor_split ergibt {split:.2} statt 1 (Bug + 2 × Flanke + Heck)",
                    hull.key
                ),
            ));
        }
        if hull.hull <= 0.0 || hull.speed <= 0.0 || hull.turn <= 0.0 || hull.radius <= 0.0 {
            return Err(invalid(
                file,
                format!("`{}`: Kennwerte müssen positiv sein", hull.key),
            ));
        }
    }
    let hull_keys: Vec<String> = data.hulls.iter().map(|hull| hull.key.clone()).collect();
    let design_keys: Vec<String> = data
        .designs
        .iter()
        .map(|design| design.key.clone())
        .collect();

    // --- Entwürfe: erster Durchgang ohne die Kosten der Bordjäger ---
    let hull_good = good(
        "balance.ron",
        "military.hull_good",
        &data.balance.military.hull_good,
    )?;
    let mut seen = BTreeSet::new();
    for index in 0..data.designs.len() {
        let file = "designs.ron";
        let key = data.designs[index].key.clone();
        if !seen.insert(key.clone()) {
            return Err(invalid(file, format!("Schlüssel `{key}` doppelt")));
        }
        let hull_id = hull_keys
            .iter()
            .position(|candidate| *candidate == data.designs[index].hull)
            .ok_or_else(|| {
                invalid(
                    file,
                    format!("`{key}`: unbekannter Rumpf `{}`", data.designs[index].hull),
                )
            })?;
        let hull = data.hulls[hull_id].clone();
        if data.designs[index].modules.len() > hull.slots as usize {
            return Err(invalid(
                file,
                format!(
                    "`{key}`: {} Module, der Rumpf fasst {}",
                    data.designs[index].modules.len(),
                    hull.slots
                ),
            ));
        }
        let mut stats = DesignStats {
            class: hull.class,
            hull: hull.hull,
            accel: hull.accel,
            turn: hull.turn,
            radius: hull.radius,
            signature: hull.signature,
            ammo: hull.ammo,
            fuel: hull.fuel,
            build_days: hull.build_days,
            ..Default::default()
        };
        let mut parts: BTreeMap<GoodId, f32> = BTreeMap::new();
        *parts.entry(hull_good).or_default() += hull.segments;
        if let Some(tech_key) = &data.designs[index].tech {
            stats.techs.push(tech(file, &key, tech_key)?);
        }
        let (mut thrust, mut engines, mut armor) = (0.0f32, 0u32, 0.0f32);
        // Feuerkraft je Waffentyp (für die Hauptreichweite) und je Richtung.
        let mut by_weapon: BTreeMap<u16, (f32, f32)> = BTreeMap::new();
        let (mut front, mut right, mut left) = (0.0f32, 0.0f32, 0.0f32);
        for slot in 0..data.designs[index].modules.len() {
            let entry = &data.designs[index].modules[slot];
            let module_id = module_keys
                .iter()
                .position(|candidate| *candidate == entry.module)
                .ok_or_else(|| {
                    invalid(
                        file,
                        format!("`{key}`: unbekanntes Modul `{}`", entry.module),
                    )
                })?;
            if !(entry.arc > 0.0 && entry.arc <= 360.0) {
                return Err(invalid(
                    file,
                    format!("`{key}`: Feuerwinkel muss zwischen 0 und 360 Grad liegen"),
                ));
            }
            let (facing, arc) = (entry.facing, entry.arc);
            let module = &data.modules[module_id];
            stats.power_draw += module.power;
            for (good, amount) in &module.cost_ids {
                *parts.entry(*good).or_default() += *amount;
            }
            if let Some(tech_id) = module.tech_id
                && !stats.techs.contains(&tech_id)
            {
                stats.techs.push(tech_id);
            }
            match &module.kind {
                ModuleKind::Engine { thrust: value } => {
                    thrust += value;
                    engines += 1;
                }
                ModuleKind::Reactor { output } => stats.power_output += output,
                ModuleKind::Tank { fuel } => stats.fuel += fuel,
                ModuleKind::Magazine { ammo } => stats.ammo += ammo,
                ModuleKind::Shield { capacity, regen } => {
                    stats.shield += capacity;
                    stats.shield_regen += regen;
                }
                ModuleKind::Armor { points } => armor += points,
                ModuleKind::Weapon(weapon) => {
                    if weapon.kind != WeaponKind::PointDefense {
                        let dps = weapon.dps();
                        let slot = by_weapon
                            .entry(module_id as u16)
                            .or_insert((0.0, weapon.range));
                        slot.0 += dps;
                        if arc_covers(facing, arc, 0.0) {
                            front += dps;
                        }
                        if arc_covers(facing, arc, 90.0) {
                            right += dps;
                        }
                        if arc_covers(facing, arc, -90.0) {
                            left += dps;
                        }
                    }
                }
                ModuleKind::Hangar { count, .. } => stats.fighters += count,
            }
            data.designs[index].modules[slot].module_id = module_id as u16;
        }
        if engines == 0 {
            return Err(invalid(file, format!("`{key}`: kein Antrieb")));
        }
        if stats.power_output + 1e-3 < stats.power_draw {
            return Err(invalid(
                file,
                format!(
                    "`{key}`: Reaktorleistung {:.0} reicht nicht für den Bedarf {:.0}",
                    stats.power_output, stats.power_draw
                ),
            ));
        }
        stats.speed = hull.speed * (thrust / engines as f32);
        stats.armor = [
            armor * hull.armor_split.0,
            armor * hull.armor_split.1,
            armor * hull.armor_split.2,
            armor * hull.armor_split.1,
        ];
        stats.main_range = by_weapon
            .values()
            .fold((0.0f32, 0.0f32), |best, (dps, range)| {
                if *dps > best.0 { (*dps, *range) } else { best }
            })
            .1;
        let side = right.max(left);
        stats.broadside = if front.max(side) > 0.0 {
            side / front.max(1e-6)
        } else {
            0.0
        };
        stats.parts = parts.into_iter().collect();
        data.designs[index].hull_id = hull_id as u16;
        data.designs[index].stats = stats;
    }

    // --- Hangars: Bordjäger auflösen und ihre Kosten aufschlagen ---
    let standoff = data.balance.battle.carrier_standoff;
    for index in 0..data.designs.len() {
        let file = "designs.ron";
        let key = data.designs[index].key.clone();
        let mut craft: Option<u16> = None;
        for slot in 0..data.designs[index].modules.len() {
            let module_id = data.designs[index].modules[slot].module_id as usize;
            let ModuleKind::Hangar {
                craft: craft_key,
                count,
                ..
            } = &data.modules[module_id].kind
            else {
                continue;
            };
            let craft_id = design_keys
                .iter()
                .position(|candidate| candidate == craft_key)
                .ok_or_else(|| {
                    invalid(
                        "modules.ron",
                        format!("Hangar: unbekannter Entwurf `{craft_key}`"),
                    )
                })?;
            let craft_stats = data.designs[craft_id].stats.clone();
            if craft_stats.class != ShipClass::Fighter || craft_stats.fighters > 0 {
                return Err(invalid(
                    "modules.ron",
                    format!("Hangar: `{craft_key}` ist kein Jäger"),
                ));
            }
            if craft.is_some_and(|existing| existing as usize != craft_id) {
                return Err(invalid(
                    file,
                    format!("`{key}`: Hangars mit verschiedenen Jägern"),
                ));
            }
            craft = Some(craft_id as u16);
            let count = *count as f32;
            let stats = &mut data.designs[index].stats;
            for (good, amount) in &craft_stats.parts {
                match stats
                    .parts
                    .iter_mut()
                    .find(|(existing, _)| existing == good)
                {
                    Some((_, total)) => *total += amount * count,
                    None => stats.parts.push((*good, amount * count)),
                }
            }
            for tech_id in &craft_stats.techs {
                if !stats.techs.contains(tech_id) {
                    stats.techs.push(*tech_id);
                }
            }
        }
        // Hangar-Module tragen den aufgelösten Entwurf; er steht im Moduldatensatz.
        let stats = &mut data.designs[index].stats;
        stats.craft = craft;
        stats.parts.sort_by_key(|(good, _)| *good);
        stats.value = stats
            .parts
            .iter()
            .map(|(good, amount)| *amount as f64 * data.goods[good.index()].base_price)
            .sum();
        if stats.class == ShipClass::Fighter && stats.main_range <= 0.0 {
            return Err(invalid(file, format!("`{key}`: Jäger ohne Waffe")));
        }
        // Träger halten Abstand: Ihre Hauptwaffe sind die Jäger.
        stats.gun_range = stats.main_range;
        if stats.fighters > 0 {
            stats.main_range = stats.main_range.max(standoff);
        }
    }
    for module in &mut data.modules {
        if let ModuleKind::Hangar {
            craft, craft_id, ..
        } = &mut module.kind
        {
            *craft_id = design_keys
                .iter()
                .position(|candidate| candidate == craft)
                .unwrap_or(0) as u16;
        }
    }

    // Jede Klasse braucht einen Entwurf, der ohne Forschung baubar ist.
    for class in ShipClass::ALL {
        if !data
            .designs
            .iter()
            .any(|design| design.stats.class == class && design.stats.techs.is_empty())
        {
            return Err(invalid(
                "designs.ron",
                format!(
                    "für die Klasse {} fehlt ein Entwurf ohne Technologie",
                    class.label()
                ),
            ));
        }
    }

    // --- Balance ---
    let military = &mut data.balance.military;
    military.hull_good_id = hull_good;
    military.ammo_good_id = good("balance.ron", "military.ammo_good", &military.ammo_good)?;
    military.starting_fleet_ids.clear();
    for (key, count) in &military.starting_fleet {
        let index = design_keys
            .iter()
            .position(|candidate| candidate == key)
            .ok_or_else(|| {
                invalid(
                    "balance.ron",
                    format!("military.starting_fleet: unbekannter Entwurf `{key}`"),
                )
            })?;
        military.starting_fleet_ids.push((index as u16, *count));
    }
    let battle = &data.balance.battle;
    if battle.dt <= 0.0 || battle.grid_cell <= 0.0 || battle.exit_radius <= battle.spawn_radius {
        return Err(invalid(
            "balance.ron",
            "battle: dt, grid_cell oder Radien unplausibel".into(),
        ));
    }
    let share = |value: f32| (0.0..=1.0).contains(&value);
    let doctrine_shares =
        |values: (f32, f32, f32)| share(values.0) && share(values.1) && share(values.2);
    if !(battle.damage_scale > 0.0
        && (0.0..=1.0).contains(&battle.point_defense_hit)
        && (0.0..=1.0).contains(&battle.reactor_breach)
        && battle.rear_hull_factor >= 1.0
        && battle.jump_out_time > 0.0
        && battle.rearm_time > 0.0
        && battle.stalemate_time > 0.0
        && battle.stalemate_pressure > 0.0
        && battle.stalemate_pressure <= battle.stalemate_time
        && battle.time_limit > 0.0
        && battle.morale_shock_recovery >= 0.0
        && battle.morale_break < battle.morale_start
        && doctrine_shares(battle.retreat_loss)
        && doctrine_shares(battle.retreat_ratio)
        && battle.retreat_unless_ahead >= 1.0
        && (0.0..=100.0).contains(&battle.morale_per_weariness))
    {
        return Err(invalid(
            "balance.ron",
            "battle: Tempo-, Moral- oder Rückzugswerte außerhalb ihres Bereichs".into(),
        ));
    }
    let hits = &battle.module_hits;
    let weights = [
        hits.engine_rear,
        hits.engine,
        hits.weapon_front,
        hits.weapon,
        hits.hangar,
        hits.reactor,
        hits.shield,
        hits.magazine,
        hits.tank,
    ];
    let loss = &battle.loss_shock;
    let targeting = &battle.targeting;
    let bands = &battle.range_bands;
    let flank = &battle.flanking;
    let non_negative = |value: f32| value.is_finite() && value >= 0.0;
    if !(weights.iter().all(|weight| non_negative(*weight))
        && weights.iter().any(|weight| *weight > 0.0)
        && share(battle.magazine_blast)
        && share(battle.tank_leak)
        && share(battle.damaged_weapon_below)
        && battle.damaged_weapon_cooldown >= 1.0
        && share(battle.dry_speed)
        && battle.dry_speed > 0.0
        && non_negative(loss.fighter)
        && non_negative(loss.escort)
        && non_negative(loss.capital)
        && non_negative(battle.point_defense_signature_weight)
        && battle.tiny_signature > 0.0
        && battle.tiny_signature <= battle.small_signature
        && non_negative(battle.slow_tracking)
        && non_negative(battle.point_defense_size_bias)
        && non_negative(targeting.damaged)
        && targeting.routing > 0.0
        && targeting.disabled > 0.0
        && targeting.keep >= 1.0
        && non_negative(targeting.jitter)
        && bands.urgent < bands.back_off
        && bands.back_off < 1.0
        && bands.approach >= 1.0
        && share(bands.back_off_speed)
        && flank.engage_distance > 0.0
        && flank.reach > 0.0
        && flank.wide > 0.0
        && non_negative(battle.frigate_leash)
        && share(battle.fighter_min_speed)
        && non_negative(battle.first_launch)
        && battle.launch_interval > 0.0
        && battle.breakaway_time > 0.0
        && share(battle.value_hull_floor))
    {
        return Err(invalid(
            "balance.ron",
            "battle: Modul-, Ziel-, Bewegungs- oder Trägerwerte außerhalb ihres Bereichs".into(),
        ));
    }
    Ok(())
}
