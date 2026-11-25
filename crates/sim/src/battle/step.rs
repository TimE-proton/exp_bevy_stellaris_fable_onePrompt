//! Ein Takt Gefecht.
//!
//! Reihenfolge: Effekte verblassen, Gitter füllen, Flotten führen, Ziele wählen, Schiffe
//! bewegen, Jäger starten, feuern, Geschosse fliegen lassen, Treffer wirken lassen,
//! Schilde und Moral nachführen, Ende prüfen.
//!
//! Gerecht zwischen den Seiten: Jede Entscheidung eines Schiffs (Ziel, Kurs, Feuer) liest
//! den Zustand vom Anfang des Takts, und alle Treffer eines Takts wirken erst, wenn alle
//! geschossen haben. Wer im Feld vorn steht, schießt also nicht zuerst. Alle Schleifen
//! laufen in Indexreihenfolge; der Zufall kommt aus einem Strom je Gefecht und Takt.
//! Gleicher Ausgangszustand ergibt denselben Verlauf.

use super::math::{distance_to_segment_squared, turn_toward, wrap_angle};
use super::{
    Battle, BattleShip, Beam, BeamKind, Doctrine, Impact, ShipSpec, ShipStatus, Shot, V2, Wreck,
    make_ship, refresh_armed, refresh_ship,
};
use crate::data::{
    BattleBalance, DesignModule, Facing, GameData, ModuleKind, ShipClass, WeaponDef, WeaponKind,
    arc_covers,
};
use crate::rng::{Domain, Rng};
use std::f32::consts::{FRAC_PI_4, PI};

/// Ein Treffer, der am Ende des Takts wirkt.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Hit {
    target: u32,
    /// Partei des Schützen und Art der Waffe.
    side: u16,
    kind: WeaponKind,
    amount: f32,
    vs: [f32; 3],
    /// Woher der Treffer kam: bestimmt die getroffene Seite.
    origin: V2,
}

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
    /// Ab diesem Verhältnis von Seiten- zu Bugfeuer lohnt die Breitseite.
    broadside_min: f32,
    broadside_angle: f32,
}

#[derive(Clone, Copy)]
struct Steering {
    velocity: V2,
    heading: f32,
    breakaway: Option<f32>,
}

/// Fester Zufallswert in `[0, 1)` je Paar aus Schütze und Ziel. Hängt nicht von der Lage
/// der Indizes ab, die die Seiten trennt; eine Formel über die rohen Indizes bevorzugte
/// messbar die zweite Partei.
fn pair_jitter(seed: u64, shooter: usize, target: u32) -> f32 {
    let mut x = seed
        ^ (shooter as u64 + 1).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ (target as u64 + 1).wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^= x >> 31;
    (x >> 40) as f32 * (1.0 / (1u32 << 24) as f32)
}

/// Braucht ein Jäger Nachschub vom Träger?
fn needs_rearm(ship: &BattleShip) -> bool {
    ship.class == ShipClass::Fighter
        && ((ship.ammo_max > 0.0 && ship.ammo < 0.5 * ship.ammo_max)
            || (ship.fuel_max > 0.0 && ship.fuel < 0.25 * ship.fuel_max))
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
        self.missiles.rebuild(
            balance.grid_cell,
            half_extent,
            self.shots.iter().map(|shot| {
                (shot.kind == WeaponKind::Missile && shot.hp > 0.0 && shot.ttl > 0.0)
                    .then_some(shot.pos)
            }),
        );
        if self.tick % command_ticks == 1 || command_ticks == 1 {
            self.command(data);
        }
        for combatant in &mut self.combatants {
            combatant.anchor += combatant.anchor_vel * dt;
        }
        let carriers: Vec<u32> = (0..self.ships.len() as u32)
            .filter(|index| {
                let ship = &self.ships[*index as usize];
                ship.class == ShipClass::Carrier && ship.status == ShipStatus::Active
            })
            .collect();
        self.select_targets(data);
        self.move_ships(data, dt, &carriers);
        self.launch_fighters(data, dt);
        self.fire_weapons(data, dt, &mut rng);
        self.advance_shots(dt);
        self.resolve_hits(data, &mut rng);
        self.upkeep(data, dt, &carriers);
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
        for side in 0..sides {
            self.refresh_fleet_profile(data, side);
        }
        // Schwerpunkt der Schiffe je Seite: Kämpfende voll, Fliehende halb, damit der
        // Sieger ihnen nachsetzt.
        let mut weight = vec![0.0f32; sides];
        let mut center = vec![V2::ZERO; sides];
        for ship in &self.ships {
            let share = match ship.status {
                ShipStatus::Active => 1.0,
                ShipStatus::Routing => 0.5,
                _ => continue,
            };
            weight[ship.side as usize] += ship.value * share;
            center[ship.side as usize] += ship.pos * (ship.value * share);
        }
        let mut strength: Vec<f32> = (0..sides)
            .map(|side| self.fighting_value(data, side))
            .collect();
        // Verbündete (nicht feindliche Parteien) zählen zur eigenen Stärke: Eine kleine
        // Verstärkung soll nicht abziehen, weil sie allein unterlegen wäre.
        let friendly_strength = |battle: &Battle, strength: &[f32], side: usize| -> f32 {
            (0..sides)
                .filter(|other| !battle.is_hostile(side as u16, *other as u16))
                .map(|other| strength[other])
                .sum()
        };
        let enemy_strength = |battle: &Battle, strength: &[f32], side: usize| -> f32 {
            (0..sides)
                .filter(|other| battle.is_hostile(side as u16, *other as u16))
                .map(|other| strength[other])
                .sum()
        };

        // Rückzug. Die Seiten entscheiden nacheinander, die am schlechtesten stehende
        // zuerst; ein Rückzug gilt sofort, so dass die nächste ihn schon sieht. So ziehen
        // nie beide Seiten im selben Augenblick ab.
        let loss_share: Vec<f32> = self
            .combatants
            .iter()
            .map(|combatant| combatant.lost_value / combatant.initial_value.max(1.0))
            .collect();
        let mut order: Vec<(usize, f32)> = (0..sides)
            .map(|side| {
                let doctrine = self.combatants[side].doctrine;
                (
                    side,
                    loss_share[side] - doctrine.pick(&balance.retreat_loss),
                )
            })
            .collect();
        // Gleichstand (etwa im Spiegelgefecht nach gleichen Verlusten) entscheidet ein
        // Zufallswert je Takt, nicht die Nummer der Partei.
        let seed = self.seed ^ self.tick.wrapping_mul(0xD6E8_FEB8_6659_FD93);
        order.sort_by(|a, b| {
            b.1.total_cmp(&a.1)
                .then(pair_jitter(seed, a.0, 0).total_cmp(&pair_jitter(seed, b.0, 0)))
                .then(a.0.cmp(&b.0))
        });
        for (side, _) in order {
            let combatant = &self.combatants[side];
            if combatant.retreating || !combatant.contact {
                continue;
            }
            let enemy = enemy_strength(self, &strength, side);
            if enemy <= 0.0 {
                continue;
            }
            let own = friendly_strength(self, &strength, side);
            let doctrine = combatant.doctrine;
            // Wer lange nichts mehr ausrichtet, bricht ab.
            let idle = self.time - combatant.last_hit > balance.stalemate_time;
            let outmatched = own < doctrine.pick(&balance.retreat_ratio) * enemy;
            let bled = loss_share[side] > doctrine.pick(&balance.retreat_loss)
                && own < balance.retreat_unless_ahead * enemy;
            if outmatched || bled || idle {
                self.order_retreat(side);
                strength[side] = 0.0;
            }
        }

        let mut contact = false;
        for side in 0..sides {
            let (mut enemy_weight, mut enemy_center) = (0.0, V2::ZERO);
            for other in 0..sides {
                if self.is_hostile(side as u16, other as u16) {
                    enemy_weight += weight[other];
                    enemy_center += center[other];
                }
            }
            let enemy = enemy_strength(self, &strength, side);
            let own = friendly_strength(self, &strength, side);
            let combatant = &mut self.combatants[side];
            if enemy_weight <= 0.0 {
                combatant.anchor_vel = V2::ZERO;
                combatant.engaged = false;
                combatant.outlook = 0.0;
                continue;
            }
            let enemy_center = enemy_center * (1.0 / enemy_weight);
            combatant.enemy_center = enemy_center;
            // Wer unterlegen ist, verliert an Zuversicht; wer vorn liegt, nicht.
            combatant.outlook = if combatant.contact && enemy > 0.0 {
                balance.morale_outlook * (1.0 - own / enemy).clamp(0.0, 1.0)
            } else {
                0.0
            };

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
            if combatant.engaged && !combatant.contact {
                combatant.contact = true;
                combatant.last_hit = self.time;
            }
            contact |= combatant.engaged;
            let mut velocity = V2::ZERO;
            if combatant.retreating {
                let away = (combatant.anchor - enemy_center).normalized();
                let away = if away == V2::ZERO {
                    V2::from_angle(combatant.home_angle)
                } else {
                    away
                };
                velocity = away * combatant.fleet_speed;
            } else {
                // Reichweitenkontrolle: heran bis zur gewünschten Entfernung, zu nah wird
                // (außer bei offensiver Doktrin) wieder Abstand genommen.
                if distance > keep * 1.05 {
                    velocity += direction * combatant.fleet_speed;
                } else if distance < keep * 0.8 && combatant.doctrine != Doctrine::Aggressive {
                    // Je näher der Gegner, desto entschiedener wird Abstand genommen.
                    let urgency = if distance < keep * 0.6 { 1.0 } else { 0.6 };
                    velocity += direction * (-urgency * combatant.fleet_speed);
                }
                if combatant.engaged {
                    // Breitseite: Die Linie läuft quer zum Gegner. Alle Seiten haben
                    // denselben Drehsinn, die Flotten kreisen um ihre gemeinsame Mitte.
                    if combatant.broadside {
                        velocity += direction.perp()
                            * (combatant.orbit * balance.orbit_share * combatant.fleet_speed);
                    }
                    // Die Mitte des Gefechts wandert zur Feldmitte zurück.
                    let middle = (combatant.anchor + enemy_center) * 0.5;
                    velocity +=
                        (middle * -balance.center_pull).clamped(0.5 * combatant.fleet_speed);
                }
            }
            combatant.anchor_vel = velocity;
        }
        if contact && self.contact_time.is_none() {
            self.contact_time = Some(self.time);
        }
    }

    /// Jedes Schiff wählt gestaffelt sein Hauptziel: nach Klassenvorliebe, Nähe und
    /// Zustand des Gegners.
    fn select_targets(&mut self, data: &GameData) {
        let balance = &data.balance.battle;
        let every = ((balance.retarget_every / balance.dt).round() as u64).max(1);
        // Gestaffelt nach der Stelle des Schiffs in seiner eigenen Partei, nicht nach dem
        // Index im ganzen Feld: Sonst wählten die Schiffe der zweiten Partei ihre Ziele
        // stets um einige Takte versetzt, und das verschob die Ausgänge messbar.
        let mut rank = vec![0u64; self.combatants.len()];
        for index in 0..self.ships.len() {
            let ship = &self.ships[index];
            let position = rank[ship.side as usize];
            rank[ship.side as usize] += 1;
            if !(position + self.tick).is_multiple_of(every) {
                continue;
            }
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
                score *= 1.0 + 0.3 * (1.0 - other.hull / other.hull_max);
                score *= match other.status {
                    ShipStatus::Routing => 0.75,
                    ShipStatus::Disabled => 0.3,
                    _ => 1.0,
                };
                if ship.target == Some(other_index) {
                    score *= 1.25;
                }
                // Kleine feste Streuung, damit nicht alle dasselbe Ziel wählen.
                score *= 1.0 + 0.15 * pair_jitter(self.seed, index, other_index);
                if best.is_none_or(|(best_index, best_score)| {
                    score > best_score || (score == best_score && other_index < best_index)
                }) {
                    best = Some((other_index, score));
                }
            });
            self.ships[index].target = best.map(|(target, _)| target);
        }
    }

    /// Erst wählt jedes Schiff seinen Kurs nach dem Zustand vom Anfang des Takts, dann
    /// bewegen sich alle.
    fn move_ships(&mut self, data: &GameData, dt: f32, carriers: &[u32]) {
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
                broadside_min: balance.broadside_min,
                broadside_angle: balance.broadside_angle.to_radians(),
            })
            .collect();
        let steerings: Vec<Option<Steering>> = (0..self.ships.len())
            .map(|index| {
                let ship = &self.ships[index];
                ship.status
                    .can_act()
                    .then(|| self.steer(index, &leads[ship.side as usize], carriers))
            })
            .collect();
        for (index, steering) in steerings.into_iter().enumerate() {
            let ship = &mut self.ships[index];
            let Some(steering) = steering else {
                if ship.status == ShipStatus::Disabled {
                    ship.pos += ship.vel * dt;
                    ship.vel = ship.vel * 0.995;
                    ship.speed *= 0.995;
                }
                continue;
            };
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
            if ship.class == ShipClass::Fighter && !needs_rearm(ship) {
                // Jäger bleiben in Bewegung, außer längsseits ihres Trägers.
                goal_speed = goal_speed.max(0.55 * limit);
            }
            let accel = ship.accel * ship.mobility.max(0.1);
            let change = (goal_speed - ship.speed).clamp(-accel * 1.5 * dt, accel * dt);
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

    /// Träger, bei dem ein Jäger landet: sein eigener, sonst der nächste der Seite.
    fn home_carrier(&self, index: usize, carriers: &[u32]) -> Option<u32> {
        let ship = &self.ships[index];
        if let Some(mother) = ship.carrier
            && self.ships[mother as usize].status == ShipStatus::Active
        {
            return Some(mother);
        }
        let mut best: Option<(u32, f32)> = None;
        for carrier in carriers {
            let other = &self.ships[*carrier as usize];
            if other.side != ship.side {
                continue;
            }
            let distance = other.pos.distance(ship.pos);
            if best.is_none_or(|(_, nearest)| distance < nearest) {
                best = Some((*carrier, distance));
            }
        }
        best.map(|(carrier, _)| carrier)
    }

    /// Gewünschte Bewegung eines Schiffs nach seiner Rolle.
    fn steer(&self, index: usize, lead: &Lead, carriers: &[u32]) -> Steering {
        let ship = &self.ships[index];
        if ship.status == ShipStatus::Routing {
            // Weg vom Gegner und hinaus zum Rand des Feldes.
            let away = (ship.pos - lead.enemy_center).normalized();
            let outward = ship.pos.normalized();
            let mut direction = (away * 0.6 + outward * 0.4).normalized();
            if direction == V2::ZERO {
                direction = V2::from_angle(lead.home_angle);
            }
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
                // Verschossen: zurück zum Träger und dort längsseits bleiben.
                if needs_rearm(ship)
                    && let Some(carrier) = self.home_carrier(index, carriers)
                {
                    let mother = &self.ships[carrier as usize];
                    let offset = mother.pos - ship.pos;
                    let velocity = if offset.length() < mother.radius + 20.0 {
                        mother.vel
                    } else {
                        mother.vel + offset.normalized() * ship.max_speed
                    };
                    return Steering {
                        velocity,
                        heading: offset.angle(),
                        breakaway: Some(0.0),
                    };
                }
                let Some(target) = target else {
                    // Ohne Ziel: zurück zum Verband.
                    let home = self
                        .home_carrier(index, carriers)
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
                // Geleit: am Platz bleiben, aber kleinen Angreifern in der Nähe entgegengehen.
                // Größere Gegner überlässt es der Linie.
                let Some(target) = target else {
                    return hold_slot(lead.facing);
                };
                if !matches!(target.class, ShipClass::Fighter | ShipClass::Corvette) {
                    return hold_slot((target.pos - ship.pos).angle());
                }
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
                        if in_range && ship.broadside > lead.broadside_min {
                            // Breitseite: das Ziel querab nehmen, im Umlaufsinn der Flotte.
                            wrap_angle(bearing + lead.orbit * lead.broadside_angle)
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
            spec.morale = Some(self.ships[carrier].morale_base);
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

    /// Alle Schiffe feuern. Treffer auf Schiffe werden gesammelt und wirken erst nach dem
    /// Takt; nur das Abfangen von Lenkwaffen wirkt sofort, damit die Punktverteidigung
    /// einer Seite nicht zweimal auf dieselbe Lenkwaffe schießt.
    fn fire_weapons(&mut self, data: &GameData, dt: f32, rng: &mut Rng) {
        let balance = &data.balance.battle;
        // Die Reihenfolge wechselt jeden Takt, damit keine Partei die Zufallszahlen des
        // Takts (Streuung, Trefferwurf) stets zuerst zieht.
        let count = self.ships.len();
        for position in 0..count {
            let index = if self.tick.is_multiple_of(2) {
                position
            } else {
                count - 1 - position
            };
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
            for (slot_index, slot) in design.modules.iter().enumerate() {
                let module = &data.modules[slot.module_id as usize];
                let Some(weapon) = module.weapon() else {
                    continue;
                };
                let ship = &self.ships[index];
                let state = ship.modules[slot_index];
                if state.hp <= 0.0 || state.cooldown > 0.0 {
                    continue;
                }
                // Ohne Munition schweigen Geschütze und Werfer; Strahlwaffen brauchen Strom.
                if weapon.ammo > 0.0 && ship.ammo < weapon.ammo {
                    continue;
                }
                if matches!(weapon.kind, WeaponKind::Beam) && ship.power < 0.25 {
                    continue;
                }
                let range = weapon.range * modifiers.range;
                let Some(aim) = self.pick_aim(index, slot, weapon, range) else {
                    continue;
                };
                let (origin, own_velocity, side) = {
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
                    (ship.pos, ship.vel, ship.side)
                };
                let damage = weapon.damage * modifiers.damage * balance.damage_scale;
                let vs = [weapon.vs_shield, weapon.vs_armor, weapon.vs_hull];
                match (weapon.kind, aim) {
                    (WeaponKind::PointDefense, Aim::Shot(shot_index)) => {
                        let shot = &mut self.shots[shot_index as usize];
                        let hit = rng.chance(balance.point_defense_hit);
                        if hit {
                            // Gegen Lenkwaffen zählt der volle Waffenwert: Das Tempo der
                            // Gefechte soll das Abfangen nicht schwächen.
                            shot.hp -= weapon.damage * modifiers.damage;
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
                            self.hits.push(Hit {
                                target,
                                side,
                                kind: weapon.kind,
                                amount: damage,
                                vs,
                                origin,
                            });
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
                        let mut direction = (aim_point - origin).normalized();
                        if direction == V2::ZERO {
                            direction = V2::from_angle(self.ships[index].heading);
                        }
                        let direction = direction.rotated(spread * rng.normal() as f32);
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
                        let mut direction = (other.pos - origin).normalized();
                        if direction == V2::ZERO {
                            direction = V2::from_angle(self.ships[index].heading);
                        }
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
            // Lenkwaffen über ihr eigenes Gitter: Die Suche wächst nicht mit der Zahl
            // aller Geschosse.
            let mut best: Option<(u32, f32)> = None;
            self.missiles.for_each_near(ship.pos, range, |shot_index| {
                let shot = &self.shots[shot_index as usize];
                if shot.hp <= 0.0 || shot.ttl <= 0.0 || !self.is_hostile(ship.side, shot.side) {
                    return;
                }
                let distance = ship.pos.distance(shot.pos);
                if distance <= range
                    && in_arc(shot.pos)
                    && best.is_none_or(|(_, nearest)| distance < nearest)
                {
                    best = Some((shot_index, distance));
                }
            });
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
            // Gegen größere Schiffe richtet die Nahbereichsabwehr nichts aus und schweigt.
            return best.map(|(other_index, _)| Aim::Ship(other_index));
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

    /// Geschosse fliegen; wer trifft, legt einen Treffer für das Ende des Takts ab.
    fn advance_shots(&mut self, dt: f32) {
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
                        if hit.is_none_or(|(nearest_index, nearest)| {
                            along < nearest || (along == nearest && other_index < nearest_index)
                        }) {
                            hit = Some((other_index, along));
                        }
                    }
                });
            if let Some((target, _)) = hit {
                let shot = &mut self.shots[index];
                shot.ttl = -1.0;
                self.hits.push(Hit {
                    target,
                    side,
                    kind: shot.kind,
                    amount: shot.damage,
                    vs: shot.vs,
                    origin: previous,
                });
                let size = if is_missile { 16.0 } else { 5.0 };
                self.impacts.push(Impact {
                    pos: position,
                    size,
                    ttl: 0.3,
                });
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

    /// Die gesammelten Treffer des Takts wirken, in der Reihenfolge, in der sie entstanden.
    fn resolve_hits(&mut self, data: &GameData, rng: &mut Rng) {
        let mut hits = std::mem::take(&mut self.hits);
        for hit in &hits {
            let pierced = self.apply_damage(
                data,
                hit.target as usize,
                hit.amount,
                hit.vs,
                hit.origin,
                rng,
            );
            if pierced > 0.0 {
                let combatant = &mut self.combatants[hit.side as usize];
                combatant.last_hit = self.time;
                combatant.damage_dealt[hit.kind as usize] += pierced;
                let victim = self.ships[hit.target as usize].side as usize;
                self.combatants[victim].damage_taken += pierced;
            }
        }
        hits.clear();
        self.hits = hits;
    }

    /// Schaden durch die drei Schichten: Schild, Panzerung der getroffenen Seite, Rumpf.
    /// Jeder Rumpftreffer beschädigt auch ein Modul. Gibt den Schaden an Panzerung und
    /// Rumpf zurück (0, wenn die Schilde alles hielten).
    fn apply_damage(
        &mut self,
        data: &GameData,
        target: usize,
        amount: f32,
        vs: [f32; 3],
        origin: V2,
        rng: &mut Rng,
    ) -> f32 {
        let balance = &data.balance.battle;
        let ship = &mut self.ships[target];
        if !ship.status.in_play() {
            return 0.0;
        }
        let facing = facing_of(ship, origin);
        let mut remaining = amount;
        if ship.shield > 0.0 {
            let absorbed = (remaining * vs[0]).min(ship.shield);
            ship.shield -= absorbed;
            remaining -= absorbed / vs[0].max(0.05);
        }
        if remaining <= 0.01 {
            return 0.0;
        }
        let armor = &mut ship.armor[facing as usize];
        let mut dealt = 0.0;
        if *armor > 0.0 {
            let absorbed = (remaining * vs[1]).min(*armor);
            *armor -= absorbed;
            dealt += absorbed;
            remaining -= absorbed / vs[1].max(0.05);
        }
        if remaining <= 0.01 {
            return dealt;
        }
        // Treffer ins Heck gehen auf die Antriebe und wiegen schwerer.
        let rear = facing == Facing::Rear;
        let hull_damage = remaining * vs[2] * if rear { balance.rear_hull_factor } else { 1.0 };
        ship.hull -= hull_damage;
        dealt += hull_damage;
        // Von achtern getroffen zu werden, erschreckt zusätzlich (vorübergehend).
        if rear {
            ship.shock +=
                balance.morale_hull_loss * balance.morale_rear_factor * hull_damage / ship.hull_max;
        }
        if ship.hull <= 0.0 {
            self.destroy(data, target, false);
            return dealt;
        }
        self.damage_module(
            data,
            target,
            hull_damage * balance.module_damage,
            facing,
            rng,
        );
        dealt
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
        let mut weights = std::mem::take(&mut self.weights);
        weights.clear();
        weights.extend(design.modules.iter().zip(&self.ships[target].modules).map(
            |(slot, state)| {
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
            },
        ));
        let any = weights.iter().any(|weight| *weight > 0.0);
        let slot_index = if any {
            Some(rng.pick_weighted(&weights))
        } else {
            None
        };
        self.weights = weights;
        let Some(slot_index) = slot_index else {
            return;
        };
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
                    ship.hull -= 0.1 * ship.hull_max;
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
            ship.speed = 0.0;
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

        // Der Verlust erschüttert die Eigenen in der Nähe und ermutigt den Gegner, beides
        // vorübergehend.
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
                balance.morale_friend_lost * weight
            } else if self.is_hostile(side, ship.side) {
                -balance.morale_enemy_lost * weight
            } else {
                0.0
            };
            let ship = &mut self.ships[*other as usize];
            ship.shock = (ship.shock + change).clamp(-20.0, 100.0);
        }
        self.scratch = nearby;
        self.refresh_fleet_profile(data, side as usize);
    }

    /// Schilde laden nach, Schreck klingt ab, die Moral folgt Rumpf, Schreck und Lage. Wer
    /// sie verliert oder nichts mehr ausrichten kann, flieht; wer fliehend den Rand erreicht
    /// oder lange genug flieht, ist entkommen. Jäger laden beim Träger nach.
    fn upkeep(&mut self, data: &GameData, dt: f32, carriers: &[u32]) {
        let balance = &data.balance.battle;
        for index in 0..self.ships.len() {
            if !self.ships[index].status.in_play() {
                continue;
            }
            // Nachladen beim Träger.
            if needs_rearm(&self.ships[index]) {
                let docked = self.home_carrier(index, carriers).filter(|carrier| {
                    let mother = &self.ships[*carrier as usize];
                    mother.pos.distance(self.ships[index].pos) < mother.radius + 45.0
                });
                let ship = &mut self.ships[index];
                if let Some(carrier) = docked {
                    ship.carrier = Some(carrier);
                    ship.rearm += dt;
                    if ship.rearm >= balance.rearm_time {
                        ship.ammo = ship.ammo_max;
                        ship.fuel = ship.fuel_max;
                        ship.rearm = 0.0;
                    }
                }
            }
            let combatant = &self.combatants[self.ships[index].side as usize];
            let (outlook, range_factor) = (combatant.outlook, combatant.modifiers.range);
            let ship = &mut self.ships[index];
            if ship.status != ShipStatus::Disabled && ship.shield < ship.shield_max {
                ship.shield =
                    (ship.shield + ship.shield_regen * ship.power * dt).min(ship.shield_max);
            }
            let recovery = balance.morale_shock_recovery * dt;
            ship.shock -= ship.shock.clamp(-recovery, recovery);
            let damage = 1.0 - (ship.hull / ship.hull_max).clamp(0.0, 1.0);
            ship.morale =
                (ship.morale_base - balance.morale_hull_loss * damage - ship.shock - outlook)
                    .clamp(0.0, 100.0);
            match ship.status {
                ShipStatus::Active => {
                    refresh_armed(data, ship, range_factor);
                    if ship.morale < balance.morale_break || !ship.armed {
                        ship.status = ShipStatus::Routing;
                        ship.target = None;
                    }
                }
                ShipStatus::Routing => {
                    ship.rout_time += dt;
                    if ship.pos.length() >= balance.exit_radius
                        || ship.rout_time >= balance.jump_out_time
                    {
                        ship.status = ShipStatus::Escaped;
                        ship.target = None;
                        let (side, class) = (ship.side as usize, ship.class);
                        self.combatants[side].escaped[class.index()] += 1;
                    }
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
                ShipStatus::Disabled
                    if hostile_victor[side] && !fighting[side] && !self.timed_out =>
                {
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

/// Seite des Schiffs, die ein Treffer aus Richtung `origin` trifft.
pub(crate) fn facing_of(ship: &BattleShip, origin: V2) -> Facing {
    let relative = wrap_angle((origin - ship.pos).angle() - ship.heading);
    if relative.abs() <= FRAC_PI_4 {
        Facing::Front
    } else if relative.abs() >= PI - FRAC_PI_4 {
        Facing::Rear
    } else if relative > 0.0 {
        Facing::Left
    } else {
        Facing::Right
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::battle::matchup::{self, Side};

    fn data() -> GameData {
        GameData::load_workspace().expect("Spieldaten")
    }

    /// Ein Kreuzer gegen einen Kreuzer, Schiff 0 steht still mit dem Bug nach +x und ohne
    /// Schilde, damit nur Panzerung, Rumpf und Module zählen.
    fn target_practice(data: &GameData) -> Battle {
        let speer = data.design_index("speer").expect("speer");
        let mut battle = matchup::build(
            data,
            3,
            &[Side::new(vec![(speer, 1)]), Side::new(vec![(speer, 1)])],
        );
        let ship = &mut battle.ships[0];
        ship.pos = V2::ZERO;
        ship.heading = 0.0;
        ship.shield = 0.0;
        battle
    }

    /// Dieselbe Folge von Treffern aus einer Richtung: verlorener Rumpf und verlorene
    /// Modulpunkte.
    fn barrage(data: &GameData, origin: V2) -> (f32, f32) {
        let mut battle = target_practice(data);
        let modules_before: f32 = battle.ships[0].modules.iter().map(|state| state.hp).sum();
        let hull_before = battle.ships[0].hull;
        let mut rng = Rng::new(17);
        for _ in 0..8 {
            battle.ships[0].shield = 0.0;
            battle.apply_damage(data, 0, 30.0, [1.0, 1.0, 1.0], origin, &mut rng);
        }
        let ship = &battle.ships[0];
        assert!(
            ship.status.in_play(),
            "acht Treffer dürfen den Kreuzer nicht zerstören"
        );
        let modules_after: f32 = ship.modules.iter().map(|state| state.hp).sum();
        (hull_before - ship.hull, modules_before - modules_after)
    }

    #[test]
    fn hits_into_the_rear_cost_more_hull_and_modules_than_into_the_bow() {
        let data = data();
        let (bow_hull, bow_modules) = barrage(&data, V2::new(600.0, 0.0));
        let (rear_hull, rear_modules) = barrage(&data, V2::new(-600.0, 0.0));
        // Bug: dicke Panzerung fängt zuerst ab. Heck: dünne Panzerung und Aufschlag.
        assert!(
            rear_hull > 1.3 * bow_hull,
            "Heck {rear_hull:.0} gegen Bug {bow_hull:.0}"
        );
        assert!(
            rear_modules > 1.3 * bow_modules,
            "Module Heck {rear_modules:.0} gegen Bug {bow_modules:.0}"
        );
    }

    #[test]
    fn the_side_of_a_hit_follows_the_heading() {
        let data = data();
        let battle = target_practice(&data);
        let ship = &battle.ships[0];
        assert_eq!(facing_of(ship, V2::new(100.0, 5.0)), Facing::Front);
        assert_eq!(facing_of(ship, V2::new(-100.0, -5.0)), Facing::Rear);
        // Winkel gegen den Uhrzeigersinn: +y liegt links, also Backbord.
        assert_eq!(facing_of(ship, V2::new(0.0, 100.0)), Facing::Left);
        assert_eq!(facing_of(ship, V2::new(0.0, -100.0)), Facing::Right);
    }

    #[test]
    fn losing_the_last_reactor_leaves_a_ship_adrift_or_destroys_it() {
        let data = data();
        for seed in 0..12 {
            let mut battle = target_practice(&data);
            let design = &data.designs[battle.ships[0].design as usize];
            // Nur ein Reaktor bleibt als mögliches Ziel, alles andere ist schon ausgefallen.
            let mut kept = false;
            for (slot, state) in design.modules.iter().zip(&mut battle.ships[0].modules) {
                let is_reactor = matches!(
                    data.modules[slot.module_id as usize].kind,
                    ModuleKind::Reactor { .. }
                );
                if is_reactor && !kept {
                    kept = true;
                } else {
                    state.hp = 0.0;
                }
            }
            let mut rng = Rng::new(seed);
            battle.damage_module(&data, 0, 1000.0, Facing::Right, &mut rng);
            let status = battle.ships[0].status;
            assert!(
                matches!(status, ShipStatus::Disabled | ShipStatus::Destroyed),
                "Seed {seed}: {status:?}"
            );
            if status == ShipStatus::Disabled {
                // Treibend: Es steuert nicht mehr und wird nicht schneller.
                battle.ships[0].speed = 10.0;
                let heading = battle.ships[0].heading;
                for _ in 0..50 {
                    battle.step(&data);
                }
                let ship = &battle.ships[0];
                assert!(matches!(
                    ship.status,
                    ShipStatus::Disabled | ShipStatus::Destroyed
                ));
                assert!(ship.speed <= 10.0 && ship.heading == heading);
            }
        }
    }

    #[test]
    fn shocks_fade_but_hull_damage_keeps_weighing_on_morale() {
        let data = data();
        let balance = &data.balance.battle;
        let mut battle = target_practice(&data);
        let start = battle.ships[0].morale_base;
        battle.ships[0].hull = battle.ships[0].hull_max * 0.5;
        battle.ships[0].shock = 15.0;
        battle.upkeep(&data, balance.dt, &[]);
        let shaken = battle.ships[0].morale;
        let lasting = balance.morale_hull_loss * 0.5;
        assert!(shaken < start - lasting - 14.0);
        // Nach einer Minute ist der Schreck vergangen, der Rumpfschaden nicht.
        for _ in 0..600 {
            battle.upkeep(&data, balance.dt, &[]);
        }
        let ship = &battle.ships[0];
        assert!(ship.shock.abs() < 1e-3);
        assert!((ship.morale - (start - lasting)).abs() < 1e-3);
        assert!(ship.morale > shaken);
    }

    #[test]
    fn pair_jitter_is_uniform_and_independent_of_index_ranges() {
        // Die Streuung bei der Zielwahl darf nicht davon abhängen, ob die Indizes eines
        // Paars klein oder groß sind: Das unterschied früher die Parteien.
        let mean = |range: std::ops::Range<usize>| {
            let mut sum = 0.0;
            let mut count = 0.0;
            for shooter in range.clone() {
                for target in 0..64u32 {
                    let value = pair_jitter(99, shooter, target);
                    assert!((0.0..1.0).contains(&value));
                    sum += value;
                    count += 1.0;
                }
            }
            sum / count
        };
        let low = mean(0..64);
        let high = mean(64..128);
        assert!((low - 0.5).abs() < 0.03 && (high - 0.5).abs() < 0.03);
    }
}
