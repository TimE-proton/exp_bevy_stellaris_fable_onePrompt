//! Ein Takt Gefecht.
//!
//! Reihenfolge: Effekte verblassen, Gitter füllen, Flotten führen, Ziele wählen, Schiffe
//! bewegen, Jäger starten, feuern, Geschosse fliegen lassen, Schilde und Moral nachführen,
//! Ende prüfen. Alle Schleifen laufen in Indexreihenfolge; der Zufall kommt aus einem
//! Strom je Gefecht und Takt. Gleicher Ausgangszustand ergibt denselben Verlauf.

use super::math::{distance_to_segment_squared, turn_toward, wrap_angle};
use super::{
    Battle, BattleShip, Beam, BeamKind, CombatModifiers, Doctrine, Impact, ShipSpec, ShipStatus,
    Shot, V2, Wreck, make_ship, refresh_ship,
};
use crate::data::{
    BattleBalance, DesignModule, Facing, GameData, ModuleKind, ShipClass, WeaponDef, WeaponKind,
    arc_covers,
};
use crate::rng::{Domain, Rng};
use std::f32::consts::{FRAC_PI_4, PI};

/// Worauf eine Waffe zielt.
#[derive(Clone, Copy)]
enum Aim {
    Ship(u32),
    Shot(u32),
}

/// Was die Führung einer Partei den Schiffen vorgibt (Kopie für den Bewegungsschritt).
#[derive(Clone, Copy)]
struct Lead {
    anchor: V2,
    anchor_vel: V2,
    facing: f32,
    home_angle: f32,
    orbit: f32,
    engaged: bool,
    enemy_center: V2,
}

struct Steering {
    velocity: V2,
    heading: f32,
    breakaway: Option<f32>,
}

impl Battle {
    /// Rechnet einen Takt (0,1 s Gefechtszeit).
    pub fn step(&mut self, data: &GameData) {
        if self.finished {
            return;
        }
        let balance = &data.balance.battle;
        let dt = balance.dt;
        self.tick += 1;
        self.time += dt;
        let mut rng = Rng::stream(self.seed, Domain::Battle, self.id.0 as u64, self.tick);
        let command_ticks = ((balance.command_every / dt).round() as u64).max(1);

        self.fade_effects(dt);
        let half_extent = balance.exit_radius * 1.25;
        self.grid.rebuild(
            balance.grid_cell,
            half_extent,
            self.ships
                .iter()
                .map(|ship| ship.status.in_play().then_some(ship.pos)),
        );
        if self.tick % command_ticks == 1 || command_ticks == 1 {
            self.command(data);
        }
        for combatant in &mut self.combatants {
            combatant.anchor += combatant.anchor_vel * dt;
        }
        self.select_targets(data);
        self.move_ships(data, dt);
        self.launch_fighters(data, dt);
        self.fire_weapons(data, dt, &mut rng);
        self.advance_shots(data, dt, &mut rng);
        self.upkeep(data, dt);
        if self.tick.is_multiple_of(command_ticks) {
            self.check_end(data);
        }
    }

    fn fade_effects(&mut self, dt: f32) {
        self.beams.retain_mut(|beam| {
            beam.ttl -= dt;
            beam.ttl > 0.0
        });
        self.impacts.retain_mut(|impact| {
            impact.ttl -= dt;
            impact.ttl > 0.0
        });
        for wreck in &mut self.wrecks {
            wreck.age += dt;
            wreck.pos += wreck.vel * dt;
            wreck.vel = wreck.vel * 0.998;
        }
    }

    /// Führung der Parteien: Rückzug entscheiden, Formation auf den Gegner ausrichten,
    /// Abstand halten, bei Breitseitenschiffen quer zum Gegner laufen.
    fn command(&mut self, data: &GameData) {
        let balance = &data.balance.battle;
        let sides = self.combatants.len();
        let mut weight = vec![0.0f32; sides];
        let mut center = vec![V2::ZERO; sides];
        for ship in &self.ships {
            if ship.status == ShipStatus::Active {
                weight[ship.side as usize] += ship.value;
                center[ship.side as usize] += ship.pos * ship.value;
            }
        }
        let strength: Vec<f32> = (0..sides)
            .map(|side| self.fighting_value(data, side))
            .collect();
        let mut routed: Vec<usize> = Vec::new();
        for side in 0..sides {
            let (mut enemy_weight, mut enemy_center, mut enemy_strength) = (0.0, V2::ZERO, 0.0);
            for other in 0..sides {
                if self.is_hostile(side as u16, other as u16) {
                    enemy_weight += weight[other];
                    enemy_center += center[other];
                    enemy_strength += strength[other];
                }
            }
            let combatant = &mut self.combatants[side];
            if enemy_weight <= 0.0 {
                combatant.anchor_vel = V2::ZERO;
                combatant.engaged = false;
                continue;
            }
            let enemy_center = enemy_center * (1.0 / enemy_weight);
            combatant.enemy_center = enemy_center;

            // Rückzug, wenn der verbliebene Kampfwert zu weit hinter dem des Gegners liegt.
            // Die ersten Sekunden zählen nicht: Erst muss man einander gesehen haben.
            let threshold = combatant.doctrine.pick(&balance.retreat_ratio);
            if !combatant.retreating
                && self.time > 25.0
                && strength[side] < threshold * enemy_strength
            {
                combatant.retreating = true;
                routed.push(side);
            }

            let to_enemy = enemy_center - combatant.anchor;
            let distance = to_enemy.length();
            let direction = to_enemy.normalized();
            combatant.facing = turn_toward(
                combatant.facing,
                to_enemy.angle(),
                0.3 * balance.command_every,
            );
            let keep = combatant.fleet_range * combatant.doctrine.pick(&balance.range_keep);
            combatant.engaged = distance < keep * 1.3;
            let mut velocity = V2::ZERO;
            if combatant.retreating {
                velocity = V2::from_angle(combatant.home_angle) * combatant.fleet_speed;
            } else {
                // Reichweitenkontrolle: heran bis zur gewünschten Entfernung, zu nah wird
                // (außer bei offensiver Doktrin) wieder Abstand genommen.
                if distance > keep * 1.05 {
                    velocity += direction * combatant.fleet_speed;
                } else if distance < keep * 0.8 && combatant.doctrine != Doctrine::Aggressive {
                    velocity += direction * (-0.6 * combatant.fleet_speed);
                }
                // Breitseite: Die Linie läuft quer zum Gegner und umkreist ihn dabei.
                if combatant.engaged && combatant.broadside {
                    velocity += direction.perp() * (combatant.orbit * 0.45 * combatant.fleet_speed);
                }
            }
            combatant.anchor_vel = velocity;
        }
        for side in routed {
            self.order_retreat(side);
        }
    }

    /// Jedes Schiff wählt gestaffelt sein Hauptziel: nach Klassenvorliebe, Nähe und
    /// Zustand des Gegners.
    fn select_targets(&mut self, data: &GameData) {
        let balance = &data.balance.battle;
        let every = ((balance.retarget_every / balance.dt).round() as u64).max(1);
        for index in 0..self.ships.len() {
            if !(index as u64 + self.tick).is_multiple_of(every) {
                continue;
            }
            let ship = &self.ships[index];
            if !ship.status.can_act() {
                continue;
            }
            let weights = data.hulls[data.designs[ship.design as usize].hull_id as usize].targets;
            let reach = balance.sensor_range;
            let scale = ship.main_range.max(120.0);
            let mut best: Option<(u32, f32)> = None;
            self.grid.for_each_near(ship.pos, reach, |other_index| {
                let other = &self.ships[other_index as usize];
                if !other.status.in_play() || !self.is_hostile(ship.side, other.side) {
                    return;
                }
                let distance = ship.pos.distance(other.pos);
                if distance > reach {
                    return;
                }
                let mut score = weights.of(other.class) / (1.0 + distance / scale);
                // Angeschlagene Gegner zuerst, Fliehende und Treibende zuletzt.
                score *= 1.0 + 0.3 * (1.0 - other.hull / other.hull_max.max(1e-6));
                score *= match other.status {
                    ShipStatus::Routing => 0.6,
                    ShipStatus::Disabled => 0.3,
                    _ => 1.0,
                };
                if ship.target == Some(other_index) {
                    score *= 1.25;
                }
                // Kleine feste Streuung, damit nicht alle dasselbe Ziel wählen.
                score *= 1.0 + 0.03 * ((index * 31 + other_index as usize * 17) % 7) as f32;
                if best.is_none_or(|(_, best_score)| score > best_score) {
                    best = Some((other_index, score));
                }
            });
            self.ships[index].target = best.map(|(target, _)| target);
        }
    }

    fn move_ships(&mut self, data: &GameData, dt: f32) {
        let balance = &data.balance.battle;
        let leads: Vec<Lead> = self
            .combatants
            .iter()
            .map(|combatant| Lead {
                anchor: combatant.anchor,
                anchor_vel: combatant.anchor_vel,
                facing: combatant.facing,
                home_angle: combatant.home_angle,
                orbit: combatant.orbit,
                engaged: combatant.engaged,
                enemy_center: combatant.enemy_center,
            })
            .collect();
        for index in 0..self.ships.len() {
            match self.ships[index].status {
                ShipStatus::Destroyed | ShipStatus::Escaped => continue,
                ShipStatus::Disabled => {
                    let ship = &mut self.ships[index];
                    ship.pos += ship.vel * dt;
                    ship.vel = ship.vel * 0.995;
                    ship.speed *= 0.995;
                    continue;
                }
                ShipStatus::Active | ShipStatus::Routing => {}
            }
            let lead = leads[self.ships[index].side as usize];
            let steering = self.steer(index, &lead);
            let ship = &mut self.ships[index];
            if let Some(breakaway) = steering.breakaway {
                ship.breakaway = breakaway;
            }
            let dry = if ship.fuel <= 0.0 { 0.5 } else { 1.0 };
            let limit = ship.max_speed * ship.mobility.max(0.1) * dry;
            let wanted_speed = steering.velocity.length().min(limit);
            // Mit Fahrt folgt der Bug der Bewegung, im Stand der gewünschten Ausrichtung.
            let heading_goal = if wanted_speed > 0.12 * ship.max_speed {
                steering.velocity.angle()
            } else {
                steering.heading
            };
            ship.heading = turn_toward(
                ship.heading,
                heading_goal,
                ship.turn * ship.mobility.max(0.2) * dt,
            );
            let align = V2::from_angle(ship.heading)
                .dot(steering.velocity.normalized())
                .max(0.0);
            let mut goal_speed = wanted_speed * align;
            if ship.class == ShipClass::Fighter {
                // Jäger bleiben in Bewegung.
                goal_speed = goal_speed.max(0.55 * limit);
            }
            let change = (goal_speed - ship.speed).clamp(-ship.accel * 1.5 * dt, ship.accel * dt);
            ship.speed = (ship.speed + change).max(0.0);
            ship.vel = V2::from_angle(ship.heading) * ship.speed;
            ship.pos += ship.vel * dt;
            ship.fuel = (ship.fuel
                - balance.fuel_per_second * dt * ship.speed / ship.max_speed.max(1.0))
            .max(0.0);
            // Wer kämpft, bleibt auf dem Feld; nur Fliehende überschreiten den Rand.
            if ship.status == ShipStatus::Active {
                let distance = ship.pos.length();
                if distance > balance.exit_radius {
                    ship.pos = ship.pos * (balance.exit_radius / distance);
                }
            }
        }
    }

    /// Gewünschte Bewegung eines Schiffs nach seiner Rolle.
    fn steer(&self, index: usize, lead: &Lead) -> Steering {
        let ship = &self.ships[index];
        if ship.status == ShipStatus::Routing {
            // Weg vom Gegner, zurück in die Richtung, aus der man kam.
            let away = (ship.pos - lead.enemy_center).normalized();
            let direction = (V2::from_angle(lead.home_angle) * 0.75 + away * 0.25).normalized();
            return Steering {
                velocity: direction * ship.max_speed,
                heading: direction.angle(),
                breakaway: None,
            };
        }
        let target = ship
            .target
            .map(|target| &self.ships[target as usize])
            .filter(|target| target.status.in_play());
        let slot_position = lead.anchor + ship.slot.rotated(lead.facing);
        let hold_slot = |heading: f32| Steering {
            velocity: (lead.anchor_vel + (slot_position - ship.pos) * 0.5).clamped(ship.max_speed),
            heading,
            breakaway: None,
        };
        match ship.class {
            ShipClass::Fighter => {
                let Some(target) = target else {
                    // Ohne Ziel: zurück zum Verband.
                    let home = ship
                        .carrier
                        .map(|carrier| self.ships[carrier as usize].pos)
                        .unwrap_or(lead.anchor);
                    return Steering {
                        velocity: (home - ship.pos).clamped(ship.max_speed * 0.7),
                        heading: ship.heading,
                        breakaway: Some(0.0),
                    };
                };
                if ship.breakaway > 0.0 {
                    // Nach dem Anflug geradeaus weiter, dann neu ansetzen.
                    return Steering {
                        velocity: V2::from_angle(ship.heading) * ship.max_speed,
                        heading: ship.heading,
                        breakaway: Some(ship.breakaway - 0.1),
                    };
                }
                let offset = target.pos - ship.pos;
                let distance = offset.length();
                let lead_time = (distance / ship.max_speed.max(1.0)).min(2.0) * 0.5;
                let aim = target.pos + target.vel * lead_time;
                let breakaway = (distance < target.radius + 45.0).then_some(2.2);
                Steering {
                    velocity: (aim - ship.pos).normalized() * ship.max_speed,
                    heading: offset.angle(),
                    breakaway,
                }
            }
            ShipClass::Corvette => {
                let Some(target) = target else {
                    return hold_slot(lead.facing);
                };
                let offset = ship.pos - target.pos;
                let distance = offset.length();
                if !lead.engaged && distance > 1500.0 {
                    return hold_slot(lead.facing);
                }
                // Flankieren: weit um das Ziel herum und von achtern angreifen.
                let bow = V2::from_angle(target.heading);
                let reach = ship.main_range * 0.55;
                let behind = offset.dot(bow) < -0.25 * distance;
                let waypoint = if behind || distance < reach * 0.9 {
                    target.pos - bow * reach
                } else {
                    let side = if bow.cross(offset) >= 0.0 { 1.0 } else { -1.0 };
                    target.pos + bow.perp() * (side * ship.main_range * 0.9) - bow * (reach * 0.4)
                };
                let to_waypoint = waypoint - ship.pos;
                let arrive = (to_waypoint.length() / 90.0).min(1.0);
                Steering {
                    velocity: to_waypoint.normalized() * (ship.max_speed * arrive),
                    heading: (target.pos - ship.pos).angle(),
                    breakaway: None,
                }
            }
            ShipClass::Frigate => {
                // Geleit: am Platz bleiben, aber Bedrohungen in der Nähe entgegengehen.
                let Some(target) = target else {
                    return hold_slot(lead.facing);
                };
                let leash = (target.pos - slot_position).clamped(320.0);
                let goal = slot_position + leash * 0.8;
                Steering {
                    velocity: (lead.anchor_vel + (goal - ship.pos) * 0.5).clamped(ship.max_speed),
                    heading: (target.pos - ship.pos).angle(),
                    breakaway: None,
                }
            }
            ShipClass::Cruiser | ShipClass::Battleship | ShipClass::Carrier => {
                let heading = match target {
                    Some(target) => {
                        let bearing = (target.pos - ship.pos).angle();
                        let in_range = ship.pos.distance(target.pos) < ship.main_range * 1.2;
                        if in_range && ship.broadside > 1.05 {
                            // Breitseite: das Ziel querab nehmen, im Umlaufsinn der Flotte.
                            wrap_angle(bearing + lead.orbit * 80f32.to_radians())
                        } else {
                            bearing
                        }
                    }
                    None => lead.facing,
                };
                hold_slot(heading)
            }
        }
    }

    /// Träger starten ihre Staffeln, einen Jäger nach dem anderen.
    fn launch_fighters(&mut self, data: &GameData, dt: f32) {
        let mut launches: Vec<(usize, u16)> = Vec::new();
        for (index, ship) in self.ships.iter_mut().enumerate() {
            if ship.hangar == 0 || ship.status != ShipStatus::Active {
                continue;
            }
            let Some(craft) = data.designs[ship.design as usize].stats.craft else {
                continue;
            };
            let design = &data.designs[ship.design as usize];
            let bays = design
                .modules
                .iter()
                .zip(&ship.modules)
                .filter(|(slot, state)| {
                    state.hp > 0.0
                        && matches!(
                            data.modules[slot.module_id as usize].kind,
                            ModuleKind::Hangar { .. }
                        )
                })
                .count();
            if bays == 0 {
                continue;
            }
            ship.launch_timer -= dt * bays as f32;
            if ship.launch_timer <= 0.0 {
                ship.launch_timer = 1.6;
                ship.hangar -= 1;
                launches.push((index, craft));
            }
        }
        for (carrier, craft) in launches {
            let side = self.ships[carrier].side;
            let combatant = &self.combatants[side as usize];
            let mut spec = ShipSpec::fresh(data, craft);
            spec.morale = Some(self.ships[carrier].morale);
            let mut fighter = make_ship(data, side, &spec, combatant);
            let mother = &self.ships[carrier];
            fighter.pos =
                mother.pos + V2::from_angle(mother.heading).perp() * (mother.radius * 0.6);
            fighter.heading = mother.heading;
            fighter.speed = fighter.max_speed * 0.6;
            fighter.carrier = Some(carrier as u32);
            self.ships.push(fighter);
        }
    }

    fn fire_weapons(&mut self, data: &GameData, dt: f32, rng: &mut Rng) {
        let balance = &data.balance.battle;
        let mut intents: Vec<(usize, Aim)> = Vec::new();
        for index in 0..self.ships.len() {
            for state in &mut self.ships[index].modules {
                if state.cooldown > 0.0 {
                    state.cooldown -= dt;
                }
            }
            if !self.ships[index].status.can_act() {
                continue;
            }
            let design = &data.designs[self.ships[index].design as usize];
            let modifiers = self.combatants[self.ships[index].side as usize].modifiers;

            // Erst entscheiden, wer worauf feuert …
            intents.clear();
            let mut ammo = self.ships[index].ammo;
            for (slot_index, slot) in design.modules.iter().enumerate() {
                let Some(weapon) = data.modules[slot.module_id as usize].weapon() else {
                    continue;
                };
                let ship = &self.ships[index];
                let state = ship.modules[slot_index];
                if state.hp <= 0.0 || state.cooldown > 0.0 {
                    continue;
                }
                // Ohne Munition schweigen Geschütze und Werfer; Strahlwaffen brauchen Strom.
                if weapon.ammo > 0.0 && ammo < weapon.ammo {
                    continue;
                }
                if matches!(weapon.kind, WeaponKind::Beam) && ship.power < 0.25 {
                    continue;
                }
                let range = weapon.range * modifiers.range;
                if let Some(aim) = self.pick_aim(index, slot, weapon, range) {
                    intents.push((slot_index, aim));
                    ammo -= weapon.ammo;
                }
            }

            // … dann feuern.
            for (slot_index, aim) in intents.drain(..) {
                let slot = &design.modules[slot_index];
                let module = &data.modules[slot.module_id as usize];
                let Some(weapon) = module.weapon() else {
                    continue;
                };
                let (origin, own_velocity, side, power) = {
                    let ship = &mut self.ships[index];
                    let health = ship.modules[slot_index].hp / module.hp;
                    let mut cooldown = weapon.cooldown * rng.range_f32(0.94, 1.06);
                    if health < 0.5 {
                        cooldown *= 1.5;
                    }
                    if weapon.kind == WeaponKind::Beam {
                        cooldown /= ship.power.max(0.3);
                    }
                    ship.modules[slot_index].cooldown = cooldown;
                    ship.ammo = (ship.ammo - weapon.ammo).max(0.0);
                    (ship.pos, ship.vel, ship.side, ship.power)
                };
                let _ = power;
                let damage = weapon.damage * modifiers.damage;
                let vs = [weapon.vs_shield, weapon.vs_armor, weapon.vs_hull];
                match (weapon.kind, aim) {
                    (WeaponKind::PointDefense, Aim::Shot(shot_index)) => {
                        let shot = &mut self.shots[shot_index as usize];
                        let hit = rng.chance(0.65);
                        if hit {
                            shot.hp -= damage;
                        }
                        let at = shot.pos;
                        let destroyed = shot.hp <= 0.0;
                        self.beams.push(Beam {
                            from: origin,
                            to: at,
                            side,
                            kind: BeamKind::PointDefense,
                            hit,
                            ttl: 0.1,
                        });
                        if destroyed {
                            self.impacts.push(Impact {
                                pos: at,
                                size: 6.0,
                                ttl: 0.25,
                            });
                        }
                    }
                    (WeaponKind::Beam | WeaponKind::PointDefense, Aim::Ship(target)) => {
                        let other = &self.ships[target as usize];
                        let chance = hit_chance(weapon, balance, origin, own_velocity, other);
                        let hit = rng.chance(chance as f64);
                        let kind = match weapon.kind {
                            WeaponKind::PointDefense => BeamKind::PointDefense,
                            _ if weapon.damage >= 20.0 => BeamKind::Heavy,
                            _ => BeamKind::Light,
                        };
                        // Ein Fehlschuss geht knapp am Ziel vorbei.
                        let to = if hit {
                            other.pos
                        } else {
                            let miss = (other.pos - origin).perp().normalized()
                                * (other.radius * rng.range_f32(1.5, 3.0));
                            other.pos + if rng.chance(0.5) { miss } else { miss * -1.0 }
                        };
                        let ttl = if kind == BeamKind::Heavy { 0.3 } else { 0.12 };
                        self.beams.push(Beam {
                            from: origin,
                            to,
                            side,
                            kind,
                            hit,
                            ttl,
                        });
                        if hit {
                            self.apply_damage(data, target as usize, damage, vs, origin, rng);
                        }
                    }
                    (WeaponKind::Kinetic, Aim::Ship(target)) => {
                        let other = &self.ships[target as usize];
                        // Vorhalt: wohin das Ziel fliegt, bis das Geschoss ankommt.
                        let mut aim_point = other.pos;
                        for _ in 0..2 {
                            let time = aim_point.distance(origin) / weapon.speed;
                            aim_point = other.pos + other.vel * time;
                        }
                        // Kann der Turm dem Ziel nicht folgen, streut der Schuss stärker.
                        let rate = angular_rate(origin, own_velocity, other);
                        let strain = (rate / weapon.tracking.max(0.01)).min(4.0);
                        let spread = weapon.spread * (1.0 + 3.0 * strain * strain);
                        let direction = (aim_point - origin)
                            .normalized()
                            .rotated(spread * rng.normal() as f32);
                        self.shots.push(Shot {
                            pos: origin,
                            vel: direction * weapon.speed,
                            side,
                            kind: WeaponKind::Kinetic,
                            target: None,
                            damage,
                            vs,
                            ttl: weapon.range * modifiers.range / weapon.speed * 1.05,
                            hp: 1.0,
                            speed: weapon.speed,
                            turn: 0.0,
                        });
                    }
                    (WeaponKind::Missile, Aim::Ship(target)) => {
                        let other = &self.ships[target as usize];
                        let direction = (other.pos - origin).normalized();
                        self.shots.push(Shot {
                            pos: origin,
                            vel: direction * weapon.speed,
                            side,
                            kind: WeaponKind::Missile,
                            target: Some(target),
                            damage,
                            vs,
                            ttl: weapon.range * modifiers.range / weapon.speed * 1.6,
                            hp: weapon.shot_hp,
                            speed: weapon.speed,
                            turn: weapon.turn,
                        });
                    }
                    _ => {}
                }
            }
        }
    }

    /// Sucht das Ziel einer einzelnen Waffe: das Hauptziel des Schiffs, wenn es im
    /// Feuerwinkel und in Reichweite liegt, sonst das nächste erreichbare. Die
    /// Punktverteidigung nimmt zuerst Lenkwaffen, dann Jäger und Korvetten.
    fn pick_aim(
        &self,
        index: usize,
        slot: &DesignModule,
        weapon: &WeaponDef,
        range: f32,
    ) -> Option<Aim> {
        let ship = &self.ships[index];
        let in_arc = |point: V2| {
            let relative = wrap_angle((point - ship.pos).angle() - ship.heading);
            // Entwürfe zählen Winkel im Uhrzeigersinn (Steuerbord positiv).
            arc_covers(slot.facing, slot.arc, -relative.to_degrees())
        };
        let eligible = |other: &BattleShip| {
            if !other.status.in_play() || !self.is_hostile(ship.side, other.side) {
                return false;
            }
            // Lenkwaffen und träge Türme werden nicht an Jäger verschwendet.
            let too_small = other.class == ShipClass::Fighter
                && (weapon.kind == WeaponKind::Missile || weapon.tracking < 0.6);
            !too_small
        };
        let reachable = |other: &BattleShip| {
            let reach = range + other.radius;
            ship.pos.distance(other.pos) <= reach && in_arc(other.pos)
        };

        if weapon.kind == WeaponKind::PointDefense {
            let mut best: Option<(u32, f32)> = None;
            for (shot_index, shot) in self.shots.iter().enumerate() {
                if shot.kind != WeaponKind::Missile
                    || shot.hp <= 0.0
                    || shot.ttl <= 0.0
                    || !self.is_hostile(ship.side, shot.side)
                {
                    continue;
                }
                let distance = ship.pos.distance(shot.pos);
                if distance <= range
                    && in_arc(shot.pos)
                    && best.is_none_or(|(_, nearest)| distance < nearest)
                {
                    best = Some((shot_index as u32, distance));
                }
            }
            if let Some((shot_index, _)) = best {
                return Some(Aim::Shot(shot_index));
            }
            let mut best: Option<(u32, f32)> = None;
            self.grid
                .for_each_near(ship.pos, range + 40.0, |other_index| {
                    let other = &self.ships[other_index as usize];
                    if !matches!(other.class, ShipClass::Fighter | ShipClass::Corvette)
                        || !eligible(other)
                        || !reachable(other)
                    {
                        return;
                    }
                    let mut distance = ship.pos.distance(other.pos);
                    if other.class == ShipClass::Corvette {
                        distance *= 3.0;
                    }
                    if best.is_none_or(|(_, nearest)| distance < nearest) {
                        best = Some((other_index, distance));
                    }
                });
            if let Some((other_index, _)) = best {
                return Some(Aim::Ship(other_index));
            }
        }

        if let Some(target) = ship.target {
            let other = &self.ships[target as usize];
            if eligible(other) && reachable(other) {
                return Some(Aim::Ship(target));
            }
        }
        let mut best: Option<(u32, f32)> = None;
        self.grid
            .for_each_near(ship.pos, range + 40.0, |other_index| {
                let other = &self.ships[other_index as usize];
                if !eligible(other) || !reachable(other) {
                    return;
                }
                let distance = ship.pos.distance(other.pos);
                if best.is_none_or(|(_, nearest)| distance < nearest) {
                    best = Some((other_index, distance));
                }
            });
        best.map(|(other_index, _)| Aim::Ship(other_index))
    }

    fn advance_shots(&mut self, data: &GameData, dt: f32, rng: &mut Rng) {
        for index in 0..self.shots.len() {
            if self.shots[index].ttl <= 0.0 || self.shots[index].hp <= 0.0 {
                continue;
            }
            if self.shots[index].kind == WeaponKind::Missile {
                self.guide_missile(index, dt);
            }
            let (previous, position, side, is_missile) = {
                let shot = &mut self.shots[index];
                let previous = shot.pos;
                shot.pos += shot.vel * dt;
                shot.ttl -= dt;
                (
                    previous,
                    shot.pos,
                    shot.side,
                    shot.kind == WeaponKind::Missile,
                )
            };
            // Treffer: das erste feindliche Schiff, dessen Trefferfläche die Flugstrecke
            // dieses Takts schneidet. So schlüpft auch ein schnelles Geschoss nicht durch.
            let direction = (position - previous).normalized();
            let padding = if is_missile { 2.5 } else { 0.8 };
            let mut hit: Option<(u32, f32)> = None;
            self.grid
                .for_each_along(previous, position, 52.0, |other_index| {
                    let other = &self.ships[other_index as usize];
                    if !other.status.in_play() || !self.is_hostile(side, other.side) {
                        return;
                    }
                    let reach = other.radius + padding;
                    if distance_to_segment_squared(other.pos, previous, position) <= reach * reach {
                        let along = (other.pos - previous).dot(direction);
                        if hit.is_none_or(|(_, nearest)| along < nearest) {
                            hit = Some((other_index, along));
                        }
                    }
                });
            if let Some((target, _)) = hit {
                let (damage, vs) = (self.shots[index].damage, self.shots[index].vs);
                self.shots[index].ttl = -1.0;
                let size = if is_missile { 16.0 } else { 5.0 };
                self.impacts.push(Impact {
                    pos: position,
                    size,
                    ttl: 0.3,
                });
                self.apply_damage(data, target as usize, damage, vs, previous, rng);
            }
        }
        self.shots.retain(|shot| shot.ttl > 0.0 && shot.hp > 0.0);
    }

    /// Lenkwaffe nachführen; ist das Ziel verloren, sucht sie das nächste in ihrer Nähe.
    fn guide_missile(&mut self, index: usize, dt: f32) {
        let shot = &self.shots[index];
        let mut target = shot
            .target
            .filter(|target| self.ships[*target as usize].status.in_play());
        if target.is_none() {
            let mut best: Option<(u32, f32)> = None;
            self.grid.for_each_near(shot.pos, 420.0, |other_index| {
                let other = &self.ships[other_index as usize];
                if !other.status.in_play()
                    || other.class == ShipClass::Fighter
                    || !self.is_hostile(shot.side, other.side)
                {
                    return;
                }
                let distance = shot.pos.distance(other.pos);
                if best.is_none_or(|(_, nearest)| distance < nearest) {
                    best = Some((other_index, distance));
                }
            });
            target = best.map(|(other_index, _)| other_index);
        }
        let velocity = match target {
            Some(target) => {
                let other = &self.ships[target as usize];
                let time = (shot.pos.distance(other.pos) / shot.speed.max(1.0)).min(3.0);
                let aim = other.pos + other.vel * time;
                let heading =
                    turn_toward(shot.vel.angle(), (aim - shot.pos).angle(), shot.turn * dt);
                V2::from_angle(heading) * shot.speed
            }
            None => shot.vel,
        };
        let shot = &mut self.shots[index];
        shot.target = target;
        shot.vel = velocity;
    }

    /// Schaden durch die drei Schichten: Schild, Panzerung der getroffenen Seite, Rumpf.
    /// Rumpftreffer können ein Modul beschädigen.
    fn apply_damage(
        &mut self,
        data: &GameData,
        target: usize,
        amount: f32,
        vs: [f32; 3],
        origin: V2,
        rng: &mut Rng,
    ) {
        let balance = &data.balance.battle;
        let ship = &mut self.ships[target];
        if !ship.status.in_play() {
            return;
        }
        let relative = wrap_angle((origin - ship.pos).angle() - ship.heading);
        let facing = if relative.abs() <= FRAC_PI_4 {
            Facing::Front
        } else if relative.abs() >= PI - FRAC_PI_4 {
            Facing::Rear
        } else if relative > 0.0 {
            Facing::Left
        } else {
            Facing::Right
        };
        let mut remaining = amount;
        if ship.shield > 0.0 {
            let absorbed = (remaining * vs[0]).min(ship.shield);
            ship.shield -= absorbed;
            remaining -= absorbed / vs[0].max(0.05);
        }
        if remaining <= 0.01 {
            return;
        }
        let armor = &mut ship.armor[facing as usize];
        if *armor > 0.0 {
            let absorbed = (remaining * vs[1]).min(*armor);
            *armor -= absorbed;
            remaining -= absorbed / vs[1].max(0.05);
        }
        if remaining <= 0.01 {
            return;
        }
        // Treffer ins Heck gehen auf die Antriebe und wiegen schwerer.
        let rear = facing == Facing::Rear;
        let hull_damage = remaining * vs[2] * if rear { 1.2 } else { 1.0 };
        ship.hull -= hull_damage;
        let shaken = balance.morale_hull_loss * hull_damage / ship.hull_max.max(1.0);
        ship.morale -= shaken
            * if rear {
                balance.morale_rear_factor
            } else {
                1.0
            };
        if ship.hull <= 0.0 {
            self.destroy(data, target, false);
            return;
        }
        let chance = (hull_damage / ship.hull_max * 5.0).clamp(0.05, 0.7);
        if rng.chance(chance as f64) {
            self.damage_module(
                data,
                target,
                hull_damage * balance.module_damage,
                facing,
                rng,
            );
        }
    }

    /// Ein Rumpftreffer beschädigt ein Modul; welches, hängt von der getroffenen Seite ab.
    fn damage_module(
        &mut self,
        data: &GameData,
        target: usize,
        amount: f32,
        facing: Facing,
        rng: &mut Rng,
    ) {
        let balance = &data.balance.battle;
        let design = &data.designs[self.ships[target].design as usize];
        let weights: Vec<f32> = design
            .modules
            .iter()
            .zip(&self.ships[target].modules)
            .map(|(slot, state)| {
                if state.hp <= 0.0 {
                    return 0.0;
                }
                match &data.modules[slot.module_id as usize].kind {
                    // Panzerplatten zählen über ihre Punkte, nicht als Modul.
                    ModuleKind::Armor { .. } => 0.0,
                    ModuleKind::Engine { .. } => {
                        if facing == Facing::Rear {
                            3.0
                        } else {
                            0.6
                        }
                    }
                    ModuleKind::Weapon(_) => {
                        if facing == Facing::Front {
                            1.6
                        } else {
                            1.0
                        }
                    }
                    ModuleKind::Hangar { .. } => 1.0,
                    ModuleKind::Reactor { .. } | ModuleKind::Shield { .. } => 0.8,
                    ModuleKind::Magazine { .. } | ModuleKind::Tank { .. } => 0.6,
                }
            })
            .collect();
        if weights.iter().all(|weight| *weight <= 0.0) {
            return;
        }
        let slot_index = rng.pick_weighted(&weights);
        let module = &data.modules[design.modules[slot_index].module_id as usize];
        let ship = &mut self.ships[target];
        let state = &mut ship.modules[slot_index];
        state.hp = (state.hp - amount).max(0.0);
        let destroyed = state.hp <= 0.0;
        let modifiers = self.combatants[ship.side as usize].modifiers;
        let mut lost = false;
        if destroyed {
            match &module.kind {
                ModuleKind::Reactor { .. } => {
                    // Ein zerstörter Reaktor zerreißt das Schiff oder legt es lahm.
                    let reactors_left =
                        design
                            .modules
                            .iter()
                            .zip(&ship.modules)
                            .any(|(slot, state)| {
                                state.hp > 0.0
                                    && matches!(
                                        data.modules[slot.module_id as usize].kind,
                                        ModuleKind::Reactor { .. }
                                    )
                            });
                    if rng.chance(balance.reactor_breach) {
                        lost = true;
                    } else if !reactors_left {
                        ship.status = ShipStatus::Disabled;
                        ship.target = None;
                    }
                }
                ModuleKind::Magazine { .. } => {
                    // Das Magazin geht hoch: Munition weg, Schaden im Inneren.
                    ship.ammo = 0.0;
                    ship.hull -= 40.0;
                    lost = ship.hull <= 0.0;
                }
                ModuleKind::Tank { .. } => ship.fuel *= 0.5,
                _ => {}
            }
        }
        if lost {
            self.destroy(data, target, true);
        } else {
            refresh_ship(data, &mut self.ships[target], &modifiers);
        }
    }

    /// Verlust eines Schiffs: Bilanz, Wrack, Wirkung auf die Moral in der Umgebung.
    fn destroy(&mut self, data: &GameData, target: usize, violent: bool) {
        let balance = &data.balance.battle;
        let (position, side, class, value, radius, velocity, heading, hangar) = {
            let ship = &mut self.ships[target];
            if ship.status == ShipStatus::Destroyed {
                return;
            }
            ship.status = ShipStatus::Destroyed;
            ship.hull = 0.0;
            ship.target = None;
            (
                ship.pos,
                ship.side,
                ship.class,
                ship.value,
                ship.radius,
                ship.vel,
                ship.heading,
                ship.hangar,
            )
        };
        let combatant = &mut self.combatants[side as usize];
        combatant.lost[class.index()] += 1;
        combatant.lost_value += value;
        // Jäger, die noch im Hangar standen, gehen mit unter.
        if hangar > 0
            && let Some(craft) = data.designs[self.ships[target].design as usize].stats.craft
        {
            combatant.lost[ShipClass::Fighter.index()] += hangar;
            combatant.lost_value += hangar as f32 * data.designs[craft as usize].stats.value as f32;
            self.ships[target].hangar = 0;
        }
        self.wrecks.push(Wreck {
            pos: position,
            vel: velocity * 0.6,
            heading,
            class,
            side,
            radius,
            age: 0.0,
        });
        let size = radius * if violent { 3.5 } else { 2.2 };
        self.impacts.push(Impact {
            pos: position,
            size,
            ttl: if class.is_capital() { 1.2 } else { 0.5 },
        });

        // Der Verlust erschüttert die Eigenen in der Nähe und ermutigt den Gegner.
        let weight = match class {
            ShipClass::Fighter => 0.12,
            _ if class.is_capital() => 2.0,
            _ => 1.0,
        };
        let mut nearby = std::mem::take(&mut self.scratch);
        nearby.clear();
        self.grid
            .for_each_near(position, balance.morale_radius, |other| nearby.push(other));
        for other in &nearby {
            let ship = &self.ships[*other as usize];
            if !ship.status.can_act() || ship.pos.distance(position) > balance.morale_radius {
                continue;
            }
            let change = if ship.side == side {
                -balance.morale_friend_lost * weight
            } else if self.is_hostile(side, ship.side) {
                balance.morale_enemy_lost * weight
            } else {
                0.0
            };
            let ship = &mut self.ships[*other as usize];
            ship.morale = (ship.morale + change).clamp(0.0, 100.0);
        }
        self.scratch = nearby;
        self.refresh_fleet_profile(side as usize);
    }

    /// Schilde laden nach, die Moral erholt sich langsam; wer sie verliert, flieht, und wer
    /// fliehend den Rand erreicht, ist entkommen.
    fn upkeep(&mut self, data: &GameData, dt: f32) {
        let balance = &data.balance.battle;
        for index in 0..self.ships.len() {
            let ship = &mut self.ships[index];
            if !ship.status.in_play() {
                continue;
            }
            if ship.status != ShipStatus::Disabled && ship.shield < ship.shield_max {
                ship.shield =
                    (ship.shield + ship.shield_regen * ship.power * dt).min(ship.shield_max);
            }
            let base = self.combatants[ship.side as usize].morale;
            if ship.morale < base {
                ship.morale = (ship.morale + 0.25 * dt).min(base);
            }
            match ship.status {
                ShipStatus::Active if ship.morale < balance.morale_break => {
                    ship.status = ShipStatus::Routing;
                    ship.target = None;
                }
                ShipStatus::Routing if ship.pos.length() >= balance.exit_radius => {
                    ship.status = ShipStatus::Escaped;
                    let (side, class) = (ship.side as usize, ship.class);
                    self.combatants[side].escaped[class.index()] += 1;
                }
                _ => {}
            }
        }
    }

    /// Das Gefecht endet, wenn keine Partei mehr einen Gegner hat, der kämpft oder noch
    /// auf dem Feld flieht, oder wenn die Zeit abläuft.
    fn check_end(&mut self, data: &GameData) {
        let balance = &data.balance.battle;
        let sides = self.combatants.len();
        let fighting: Vec<bool> = (0..sides).map(|side| self.is_fighting(side)).collect();
        let present: Vec<bool> = (0..sides)
            .map(|side| {
                self.ships
                    .iter()
                    .any(|ship| ship.side as usize == side && ship.status.can_act())
            })
            .collect();
        let mut conflict = false;
        for a in 0..sides {
            for b in 0..sides {
                if self.is_hostile(a as u16, b as u16) && fighting[a] && present[b] {
                    conflict = true;
                }
            }
        }
        let timed_out = self.time >= balance.time_limit;
        if conflict && !timed_out {
            return;
        }
        self.finished = true;
        self.timed_out = conflict && timed_out;
        // Wer noch flieht, ist entkommen. Treibende Schiffe bleiben beim Sieger erhalten;
        // die eines Verlierers gehen verloren.
        let hostile_victor: Vec<bool> = (0..sides)
            .map(|side| {
                (0..sides)
                    .any(|other| self.is_hostile(side as u16, other as u16) && fighting[other])
            })
            .collect();
        let mut abandoned: Vec<usize> = Vec::new();
        for (index, ship) in self.ships.iter_mut().enumerate() {
            let side = ship.side as usize;
            match ship.status {
                ShipStatus::Routing => {
                    ship.status = ShipStatus::Escaped;
                    self.combatants[side].escaped[ship.class.index()] += 1;
                }
                ShipStatus::Disabled if hostile_victor[side] && !self.timed_out => {
                    abandoned.push(index)
                }
                _ => {}
            }
        }
        for index in abandoned {
            self.destroy(data, index, false);
        }
        self.shots.clear();
    }
}

/// Winkelgeschwindigkeit eines Ziels aus Sicht des Schützen (rad/s).
fn angular_rate(origin: V2, own_velocity: V2, target: &BattleShip) -> f32 {
    let offset = target.pos - origin;
    let distance_squared = offset.length_squared().max(1.0);
    offset.cross(target.vel - own_velocity).abs() / distance_squared
}

/// Trefferwahrscheinlichkeit einer Strahlwaffe: Nachführung gegen die Winkelgeschwindigkeit
/// des Ziels, mal einem Faktor für seine Größe. Kleine, schnelle Ziele auf kurze Distanz
/// sind für träge Türme kaum zu treffen; die Punktverteidigung ist dafür gebaut.
fn hit_chance(
    weapon: &WeaponDef,
    balance: &BattleBalance,
    origin: V2,
    own_velocity: V2,
    target: &BattleShip,
) -> f32 {
    let rate = angular_rate(origin, own_velocity, target);
    let follow = weapon.tracking / (rate + 0.03);
    let tracking = follow / (1.0 + follow);
    let exponent = if weapon.kind == WeaponKind::PointDefense {
        0.2
    } else {
        balance.beam_signature_weight
    };
    let size = target.signature.min(1.0).powf(exponent);
    (tracking * size).clamp(0.02, 0.97)
}

#[allow(dead_code)]
fn _modifiers_are_plain(_: CombatModifiers) {}
