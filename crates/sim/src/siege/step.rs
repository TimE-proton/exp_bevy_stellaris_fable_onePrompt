//! Tagesrechner der Belagerung und die Prognose darüber.
//!
//! [`resolve`] rechnet einen Tag einer Belagerung aus einfachen Daten: Datensatz,
//! Verteidigung, der Planet und der Orbit an diesem Tag. Ohne Zufall, ohne Zugriff auf
//! Flotten, Märkte oder Forschung. Was der Tag außerhalb des Datensatzes bewirkt (Rumpf,
//! Munition, Bevölkerung, Kapital), kommt als [`DayEffects`] zurück; die Partie wendet es
//! an. [`forecast`] rechnet die drei Wege des Belagerers mit demselben Tagesrechner voraus.

use super::{
    Blocked, Course, Fire, Forecast, Gate, Limit, OrbitView, Outcome, PlanetDefense, PlanetFacts,
    Posture, Siege, SiegeOrders, SiegeReport, SiegeView, TechView, Zone, ZoneKind, guns_active,
    invested, occupation_force, will,
};
use crate::data::{ShipClass, SiegeBalance};
use crate::ids::PolityId;
use std::collections::BTreeMap;

/// Unter dieser Größe gilt eine Stärke, ein Vorrat oder eine Ladung als verbraucht. So
/// endet jedes geometrische Abklingen nach endlich vielen Tagen.
const EPSILON: f64 = 1e-6;
/// Beschuss und Bodenkrieg drücken eine Welt nie unter diese Bevölkerung (Mio.).
const POPULATION_FLOOR: f64 = 1.0;
/// Garnison in Verbänden, unter der eine ausgehungerte Welt in jedem Fall aufgibt.
const GARRISON_FLOOR: f64 = 0.1;

#[inline]
fn snap(value: f64) -> f64 {
    if value < EPSILON { 0.0 } else { value }
}

/// Was ein Tag außerhalb von Datensatz und Verteidigung bewirkt. Die Partie wendet es auf
/// Schiffe, Wirtschaft und Finanzen an; die Prognose auf ihre Kennzahlen.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DayEffects {
    /// Rumpfschaden an den zugeordneten Schiffen in Punkten; [`hull_shares`] verteilt ihn.
    pub hull: f64,
    /// Anteil der Magazine, den jedes zugeordnete Schiff verbraucht.
    pub ammo: f64,
    /// Anteil des Tanks, den jede zugeordnete Flotte verbraucht.
    pub fuel: f64,
    /// Faktor auf die Bevölkerung des Planeten (höchstens 1).
    pub population: f64,
    /// Faktor auf jeden Kapitalstock: durch Beschuss, durch Bodenkrieg.
    pub capital_fire: f64,
    pub capital_ground: f64,
    /// Faktor auf die Stimmung (`Issuer::shock`) der Betriebe der Welt.
    pub shock: f64,
    /// Zielwert für den Stillstand der Wirtschaft: Anteil umkämpfter und besetzter Zonen.
    pub disruption: f64,
    /// Verlorene Verbände der Landungstruppe (Gefecht, fehlender Nachschub, abgeschlagene
    /// Landung) und der Verteidiger (Gefecht und Beschuss).
    pub attacker_lost: f64,
    pub defender_lost: f64,
    /// Millionen Einwohner, die der Beschuss getötet hat.
    pub fire_victims: f64,
    /// Eingeschiffte Verbände, die heute als Besatzung an Land gegangen sind.
    pub landed: f64,
    /// Der Planet fällt: gestürmt oder ausgehungert.
    pub verdict: Option<Outcome>,
}

impl DayEffects {
    /// Ein Tag ohne jede Wirkung.
    pub const NONE: DayEffects = DayEffects {
        hull: 0.0,
        ammo: 0.0,
        fuel: 0.0,
        population: 1.0,
        capital_fire: 1.0,
        capital_ground: 1.0,
        shock: 1.0,
        disruption: 0.0,
        attacker_lost: 0.0,
        defender_lost: 0.0,
        fire_victims: 0.0,
        landed: 0.0,
        verdict: None,
    };

    /// Faktor auf jeden Kapitalstock insgesamt.
    pub fn capital(&self) -> f64 {
        self.capital_fire * self.capital_ground
    }
}

impl Default for DayEffects {
    fn default() -> Self {
        Self::NONE
    }
}

/// Miliz einer Welt in Verbänden, ohne Vorbesitz eines Belagerers.
pub fn militia_strength(population: f64, cohesion: f64, balance: &SiegeBalance) -> f64 {
    let (reference, low, high) = balance.militia_cohesion;
    balance.militia_k * population.max(0.0).sqrt() * (cohesion / reference).clamp(low, high)
}

/// Zonen eines Planeten: Umland, Schildwerk und Batterie, soweit vorhanden, Städte nach
/// der Bevölkerung, zuletzt das Zentrum. Die Miliz verteilt sich nach den Anteilen.
pub fn build_zones(
    population: f64,
    shield: bool,
    guns: bool,
    militia: f64,
    balance: &SiegeBalance,
) -> Vec<Zone> {
    let cities = if population > balance.city_from.1 {
        2
    } else if population >= balance.city_from.0 {
        1
    } else {
        0
    };
    let works = shield as usize + guns as usize;
    let rest = 1.0 - balance.center_share - works as f64 * balance.works_share;
    let even = rest / (1 + cities) as f64;
    let mut kinds = vec![(ZoneKind::Outskirts, even)];
    if shield {
        kinds.push((ZoneKind::ShieldWorks, balance.works_share));
    }
    if guns {
        kinds.push((ZoneKind::Battery, balance.works_share));
    }
    kinds.extend(std::iter::repeat_n((ZoneKind::City, even), cities));
    kinds.push((ZoneKind::Center, balance.center_share));
    kinds
        .into_iter()
        .map(|(kind, share)| Zone {
            kind,
            share: share as f32,
            militia: snap(militia.max(0.0) * share),
            taken: false,
        })
        .collect()
}

/// Legt den Datensatz einer Belagerung an: Miliz aus Bevölkerung, Zusammenhalt und
/// Vorbesitz des Belagerers, Zonen, Reguläre im Untergrund. `facts.day` wird der erste
/// Tag, `prize_start` ist der Wert der Beute bei Beginn. `stakes` sind die Anteile am Wert
/// der Betriebe der Welt je Gemeinschaft.
pub fn form(
    attacker: PolityId,
    defender: PolityId,
    defense: &mut PlanetDefense,
    facts: &PlanetFacts,
    stakes: BTreeMap<PolityId, f64>,
    prize_start: f64,
    balance: &SiegeBalance,
) -> Siege {
    let stake = stakes
        .get(&attacker)
        .copied()
        .unwrap_or(0.0)
        .clamp(0.0, 1.0);
    let militia = militia_strength(facts.population, facts.cohesion, balance) * (1.0 - stake);
    let zones = build_zones(
        facts.population,
        defense.shield > 0,
        defense.guns > 0.0,
        militia,
        balance,
    );
    let militia: f64 = zones.iter().map(|zone| zone.militia).sum();
    let deep = match defense.posture {
        Posture::Deep => defense
            .regulars
            .min(balance.deep_per_bunker * defense.bunkers as f64),
        Posture::Forward => 0.0,
    };
    defense.militia = militia;
    Siege {
        attacker,
        defender,
        started_day: facts.day,
        orders: SiegeOrders::default(),
        zones,
        army: 0.0,
        army_peak: 0.0,
        army_supply: 0.0,
        landed_day: None,
        deep,
        garrison_start: militia + defense.regulars,
        surface_start: militia + defense.regulars - deep,
        prize_start,
        population_start: facts.population.max(0.0),
        stakes,
        report: SiegeReport::default(),
    }
}

/// Ergebnis einer Landung.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Landing {
    /// Verbände, die den Boden erreicht haben.
    pub landed: f64,
    /// Verbände, die die Geschütze abgefangen haben.
    pub lost: f64,
}

/// Setzt eine Welle von `strength` Verbänden an Land. Aktive Geschütze fangen je Stufe
/// einen Anteil ab; der Schild hält Landungen nicht auf. `day` ist der Tag des Befehls:
/// Am Tag darauf stehen die Schiffe noch im niedrigen Orbit.
pub fn land(
    siege: &mut Siege,
    defense: &PlanetDefense,
    strength: f64,
    day: u64,
    balance: &SiegeBalance,
) -> Landing {
    let strength = strength.max(0.0);
    let share = if guns_active(siege, defense) {
        (balance.landing_loss * defense.guns as f64).min(balance.landing_loss_cap)
    } else {
        0.0
    };
    let lost = strength * share;
    let landed = strength - lost;
    if landed > 0.0 {
        siege.army += landed;
        siege.army_peak = siege.army_peak.max(siege.army);
        siege.landed_day = Some(day);
    }
    Landing { landed, lost }
}

/// Rumpfanteil (0..1), den jedes Schiff durch `hull` Punkte Schaden verliert. `ships`
/// nennt je Schiff Klasse und Standfestigkeit (Rumpf, Schild und Panzerung). Verteilt wird
/// nach dem Gewicht der Klasse: Große Schiffe verlieren den größeren Anteil.
pub fn hull_shares(hull: f64, ships: &[(ShipClass, f64)], balance: &SiegeBalance) -> Vec<f64> {
    let weight = |class: ShipClass| balance.gun_weight[class.index()];
    let total: f64 = ships
        .iter()
        .map(|(class, toughness)| weight(*class) * toughness.max(0.0))
        .sum();
    if total <= 0.0 || hull <= 0.0 {
        return vec![0.0; ships.len()];
    }
    ships
        .iter()
        .map(|(class, _)| (hull * weight(*class) / total).min(1.0))
        .collect()
}

/// Eine Zone fällt: Ihre Miliz löst sich auf, ihre Anlage schweigt. `garrison` ist die
/// Garnison am Morgen, für die die Vorräte bemessen sind.
fn take_zone(
    siege: &mut Siege,
    defense: &mut PlanetDefense,
    index: usize,
    garrison: f64,
    balance: &SiegeBalance,
    report: &mut SiegeReport,
) {
    if siege.zones[index].taken {
        return;
    }
    let kind = siege.zones[index].kind;
    siege.zones[index].taken = true;
    report.defender_dissolved += siege.zones[index].militia;
    siege.zones[index].militia = 0.0;
    report.fell = Some(index as u8);
    match kind {
        // Die Batterie bleibt heil, feuert aber nicht mehr.
        ZoneKind::Outskirts | ZoneKind::City | ZoneKind::Battery => {}
        ZoneKind::ShieldWorks => defense.charge = 0.0,
        ZoneKind::Center => {
            for zone in siege.zones.iter_mut().filter(|zone| !zone.taken) {
                let lost = zone.militia * (1.0 - balance.center_shock);
                zone.militia = snap(zone.militia - lost);
                report.defender_dissolved += lost;
            }
            // Die Reserve weicht in den Untergrund, soweit die Bunker reichen.
            let reserve = siege.reserve(defense);
            let room = (balance.deep_per_bunker * defense.bunkers as f64 - siege.deep).max(0.0);
            let moved = reserve.min(room);
            siege.deep += moved;
            report.defender_dissolved += reserve - moved;
            defense.regulars = siege.deep;
            // Vorräte über dem, was der Untergrund birgt, fallen der Truppe zu.
            if defense.depot > balance.bunker_days {
                siege.army_supply += (defense.depot - balance.bunker_days) * garrison;
                defense.depot = balance.bunker_days;
            }
        }
    }
}

/// Rechnet einen Tag einer Belagerung.
///
/// In dieser Reihenfolge: (1) Duell mit den Stationen, (2) Bodengeschütze gegen Schiffe im
/// niedrigen Orbit, (3) Schild lädt nach, (4) Beschuss, (5) Bodenkrieg um eine Zone,
/// (6) Untergrund, (7) Vorräte und Nachschub; danach die Urteile. Verändert Datensatz und
/// Verteidigung; alles Übrige steht im Rückgabewert und in `siege.report`.
pub fn resolve(
    siege: &mut Siege,
    defense: &mut PlanetDefense,
    facts: &PlanetFacts,
    orbit: &OrbitView,
    balance: &SiegeBalance,
) -> DayEffects {
    let mut effects = DayEffects::NONE;
    let mut report = SiegeReport {
        day: facts.day,
        ..Default::default()
    };
    let today = facts.day;
    let population = facts.population.max(0.0);
    let will = will(facts.cohesion, siege.stake(), balance);
    let fire = siege.orders.fire;
    let control = orbit.control;
    // Ohne Orbitalhoheit wirkt nichts aus dem Orbit.
    let firepower = if control {
        orbit.firepower.max(0.0)
    } else {
        0.0
    };
    let bombard = firepower * (1.0 + orbit.att.weapon_damage);
    let ammunition = control && orbit.ammo > 0.0;
    let defender_guns = 1.0 + orbit.def.weapon_damage;
    let lost_at_dawn = siege.surface_lost();
    let garrison_at_dawn = siege.defenders(defense);
    let depot_at_dawn = defense.depot;
    let force_needed = occupation_force(population, balance);

    // (1) Stationen stehen außerhalb des Schilds; die Flotte antwortet von selbst.
    if control && defense.stations > 0.0 {
        let stations = defense.stations as f64;
        effects.hull += stations * balance.station_damage * defender_guns;
        let left = snap(stations - firepower / balance.station_hp);
        report.stations_lost = stations - left;
        defense.stations = left as f32;
        if ammunition && firepower > 0.0 {
            effects.ammo += balance.ammo[0];
        }
    }
    let invested = invested(orbit, defense);
    // Warum Feuer aus dem Orbit heute nicht wirkt.
    let orbit_blocked = if !control {
        Some(Blocked::NoControl)
    } else if !invested {
        Some(Blocked::Stations)
    } else if !ammunition {
        Some(Blocked::NoAmmo)
    } else if bombard <= 0.0 {
        Some(Blocked::NoFirepower)
    } else {
        None
    };

    // (2) Bodengeschütze reichen nur in den niedrigen Orbit: wer feuert oder gestern
    // gelandet ist.
    let landed_yesterday = siege.landed_day.is_some_and(|day| day + 1 == today);
    let low_orbit = invested && (fire != Fire::Hold || landed_yesterday);
    if low_orbit && guns_active(siege, defense) {
        let guns = defense.guns as f64;
        effects.hull += guns * balance.gun_damage * defender_guns;
        defense.depot = snap(defense.depot - balance.gun_depot * guns);
    }

    // (3) Der Schild lädt nach, solange das Schildwerk steht und Vorräte da sind.
    let shield = defense.shield as f64;
    let shield_tech = 1.0 + orbit.def.shields;
    let cap = shield * balance.shield_cap * shield_tech;
    let full = shield * balance.shield_regen * shield_tech;
    let mut charge = (defense.charge as f64).clamp(0.0, cap);
    if siege.zone_taken(ZoneKind::ShieldWorks) {
        charge = 0.0;
    } else if defense.depot > 0.0 && full > 0.0 {
        let recharged = (cap - charge).min(full).max(0.0);
        charge += recharged;
        defense.depot = snap(defense.depot - balance.shield_depot * shield * recharged / full);
    }

    // (4) Beschuss trifft zuerst den Schild. Was durchkommt, trifft Miliz und Reserve,
    // Geschütze, Bevölkerung und Kapital; der Untergrund bleibt unberührt.
    let mut fire_damage = 0.0;
    if fire >= Fire::Precise {
        if lost_at_dawn {
            report.blocked.insert(Blocked::Underground);
        } else if let Some(reason) = orbit_blocked {
            report.blocked.insert(reason);
        } else {
            effects.ammo += balance.ammo[fire.index()];
            let absorbed = charge.min(bombard);
            charge = snap(charge - absorbed);
            report.shield_absorbed = absorbed;
            let through = bombard - absorbed;
            if through < EPSILON {
                report.blocked.insert(Blocked::ShieldHolds);
            } else {
                let rate = (through * balance.bombard_k / (population + balance.bombard_pop0))
                    .min(balance.bombard_cap);
                let (military, collateral) = if fire == Fire::Area {
                    balance.area
                } else {
                    balance.precise
                };
                let military = (military * rate).min(1.0);
                fire_damage = (collateral * rate).min(1.0);
                for zone in siege.zones.iter_mut().filter(|zone| !zone.taken) {
                    let lost = zone.militia * military;
                    zone.militia = snap(zone.militia - lost);
                    report.defender_bombed += lost;
                }
                let lost = siege.reserve(defense) * military;
                defense.regulars = (defense.regulars - lost).max(siege.deep);
                report.defender_bombed += lost;
                // Gegenbatteriefeuer; eine genommene Batterie gehört schon dem Belagerer.
                if defense.guns > 0.0 && !siege.zone_taken(ZoneKind::Battery) {
                    let guns = defense.guns as f64;
                    let aimed = if fire == Fire::Precise { 2.0 } else { 1.0 };
                    let left = snap(guns - through * balance.counter_battery * aimed);
                    report.guns_lost = guns - left;
                    defense.guns = left as f32;
                }
            }
        }
    }
    defense.charge = charge as f32;

    // (5) Bodenkrieg um eine Zone der Oberfläche.
    let mut fought = false;
    let mut ground_damage = 0.0;
    if siege.army > 0.0
        && !lost_at_dawn
        && let Some(index) = siege.front()
    {
        let kind = siege.zones[index].kind;
        let fort = balance.fort[kind.index()];
        let militia = siege.zones[index].militia;
        // Die Reserve stellt sich überall, in der Tiefe gestaffelt nur im Zentrum.
        let mobile = defense.posture == Posture::Forward || kind == ZoneKind::Center;
        let reserve = if mobile { siege.reserve(defense) } else { 0.0 };
        let defenders = militia + reserve;
        let supported = fire >= Fire::Support
            && orbit_blocked.is_none()
            && defense.charge <= 0.0
            && defenders > 0.0;
        let support = if supported {
            let most = if orbit.att.has(Gate::Support) {
                balance.support_max.1
            } else {
                balance.support_max.0
            };
            1.0 + most * (bombard / (balance.support_ref * defenders)).min(1.0)
        } else {
            1.0
        };
        let beach = if siege
            .landed_day
            .is_some_and(|day| today <= day + balance.beach_days)
        {
            balance.beach
        } else {
            1.0
        };
        let supply_attacker = if siege.army_supply > 0.0 {
            1.0
        } else {
            balance.unsupplied
        };
        let supply_defender = if defense.depot > 0.0 {
            1.0
        } else {
            balance.unsupplied
        };
        let attack =
            siege.army * (1.0 + orbit.att.ground_attack) * supply_attacker * beach * support;
        let defence = defenders * (1.0 + orbit.def.ground_defense) * fort * will * supply_defender;
        let (mut loss_defender, mut loss_attacker) = (0.0, 0.0);
        if siege.orders.assault {
            loss_defender = balance.r * attack;
            loss_attacker = balance.r * defence;
            fought = true;
        } else if defense.posture == Posture::Forward
            && defence > 0.0
            && defence / fort >= balance.sortie_ratio * attack
        {
            // Ausfall: Die Verteidiger verlassen ihre Stellungen und verlieren deren Schutz.
            loss_defender = balance.r * attack;
            loss_attacker = balance.r * defence / fort;
            fought = true;
            report.sortie = true;
        }
        if fought {
            loss_defender = loss_defender.min(defenders);
            loss_attacker = loss_attacker.min(siege.army);
            if defenders > 0.0 {
                // Miliz der Zone und Reserve tragen die Verluste anteilig.
                let from_militia = loss_defender * militia / defenders;
                siege.zones[index].militia = snap(militia - from_militia);
                defense.regulars =
                    (defense.regulars - (loss_defender - from_militia)).max(siege.deep);
            }
            siege.army = snap(siege.army - loss_attacker);
            report.zone = Some(index as u8);
            report.attacker_lost += loss_attacker;
            report.defender_lost += loss_defender;
            ground_damage = (balance.damage_per_loss
                * siege.zones[index].share as f64
                * (loss_attacker + loss_defender))
                .min(1.0);
            // Stützfeuer kostet nur an Tagen, an denen gekämpft wird; Beschuss ist schon bezahlt.
            if supported && fire == Fire::Support {
                effects.ammo += balance.ammo[1];
            }
            if fire >= Fire::Support && !supported && defenders > 0.0 {
                report
                    .blocked
                    .insert(orbit_blocked.unwrap_or(Blocked::ShieldHolds));
            }
        }
        let reserve = if mobile { siege.reserve(defense) } else { 0.0 };
        if siege.army > 0.0 && siege.zones[index].militia + reserve < balance.zone_fall * siege.army
        {
            take_zone(
                siege,
                defense,
                index,
                garrison_at_dawn,
                balance,
                &mut report,
            );
        }
    } else if siege.orders.assault && siege.army <= 0.0 {
        report.blocked.insert(Blocked::NoArmy);
    }
    // Ist die Oberfläche gebrochen, fällt ihr Rest ohne Kampf, das Zentrum zuletzt. Nur vor
    // einer Truppe, die eine Welt auch besetzen kann.
    if !lost_at_dawn
        && !siege.surface_lost()
        && siege.army > 0.0
        && siege.army >= force_needed
        && siege.zone_militia() + siege.reserve(defense) <= balance.zone_fall * siege.surface_start
    {
        let mut order: Vec<usize> = (0..siege.zones.len()).collect();
        order.sort_by_key(|index| (siege.zones[*index].kind == ZoneKind::Center, *index));
        for index in order {
            take_zone(
                siege,
                defense,
                index,
                garrison_at_dawn,
                balance,
                &mut report,
            );
        }
    }

    // (6) Untergrund: kein Stützfeuer, kein Beschuss, kein Schaden an der Welt.
    if lost_at_dawn && siege.army > 0.0 && siege.deep > 0.0 && siege.orders.assault {
        let tunnels = |tech: &TechView| {
            if tech.has(Gate::Tunnel) {
                balance.tunnel_tech
            } else {
                1.0
            }
        };
        let supply_attacker = if siege.army_supply > 0.0 {
            1.0
        } else {
            balance.unsupplied
        };
        let supply_defender = if defense.depot > 0.0 {
            1.0
        } else {
            balance.unsupplied
        };
        let fort = balance.under_base + balance.under_per_bunker * defense.bunkers as f64;
        let loss_defender = (balance.r
            * siege.army.min(balance.tunnel_front)
            * (1.0 + orbit.att.ground_attack)
            * supply_attacker
            * tunnels(&orbit.att))
        .min(siege.deep);
        let loss_attacker = (balance.r
            * siege.deep.min(balance.tunnel_front)
            * (1.0 + orbit.def.ground_defense)
            * fort
            * will
            * supply_defender
            * tunnels(&orbit.def))
        .min(siege.army);
        siege.deep = snap(siege.deep - loss_defender);
        defense.regulars = siege.deep;
        siege.army = snap(siege.army - loss_attacker);
        report.attacker_lost += loss_attacker;
        report.defender_lost += loss_defender;
        report.underground = true;
        fought = true;
        if fire >= Fire::Support {
            report.blocked.insert(Blocked::Underground);
        }
    }

    // (7) Vorräte. Abgeschnitten ist die Garnison, wenn der Planet eingeschlossen ist oder
    // der Belagerer die Oberfläche hält. Der Markt vor Ort deckt einen Teil des Bedarfs,
    // Mangel in der Bevölkerung zehrt doppelt; der Untergrund lebt nur vom Depot.
    let surface_lost = siege.surface_lost();
    let cut_off = invested || surface_lost;
    if cut_off {
        let used = if surface_lost {
            1.0
        } else {
            (1.0 - balance.forage_share * facts.forage.clamp(0.0, 1.0))
                * (2.0 - facts.essentials_met.clamp(0.0, 1.0))
        };
        defense.depot = snap(defense.depot - used);
        if defense.depot <= 0.0 {
            // Ohne Vorräte löst sich die Garnison auf, umso schneller, je schwächer der Wille.
            let keep = 1.0 - (balance.starve_rate / will).min(1.0);
            for zone in &mut siege.zones {
                report.defender_dissolved += zone.militia * (1.0 - keep);
                zone.militia = snap(zone.militia * keep);
            }
            report.defender_dissolved += defense.regulars * (1.0 - keep);
            defense.regulars *= keep;
            siege.deep *= keep;
        }
    }
    report.depot_used = depot_at_dawn - defense.depot;
    if siege.army > 0.0 {
        let need = siege.army
            * if fought {
                1.0
            } else {
                balance.army_idle_supply
            };
        if siege.army_supply >= need {
            siege.army_supply = snap(siege.army_supply - need);
        } else {
            // Ohne Nachschub schmilzt die Truppe.
            siege.army_supply = 0.0;
            let stranded = siege.army * balance.strand_rate;
            siege.army = snap(siege.army - stranded);
            report.attacker_stranded = stranded;
        }
    }
    if control {
        effects.fuel = balance.station_fuel;
    }

    // Kleinstwerte enden, die Buchführung bleibt in sich stimmig.
    defense.regulars = snap(defense.regulars);
    siege.deep = snap(siege.deep.min(defense.regulars));
    if siege.army <= 0.0 {
        siege.army_supply = 0.0;
    }

    // Urteile. Jede Übernahme braucht eine Besatzungstruppe: am Boden, oder an Bord der
    // zugeordneten Flotten; die geht dann ohne Verluste an Land.
    let aboard = if invested {
        orbit.embarked.max(0.0)
    } else {
        0.0
    };
    let occupied = |siege: &Siege| siege.army + aboard > 0.0 && siege.army + aboard >= force_needed;
    let occupy = |siege: &mut Siege, effects: &mut DayEffects| {
        let landed = (force_needed - siege.army).clamp(0.0, aboard);
        siege.army += landed;
        siege.army_peak = siege.army_peak.max(siege.army);
        effects.landed = landed;
    };
    if siege.surface_lost() && siege.deep <= 0.0 && occupied(siege) {
        occupy(siege, &mut effects);
        effects.verdict = Some(Outcome::Stormed);
    } else {
        if siege.army > 0.0 && siege.army <= balance.zone_fall * siege.army_peak {
            // Landung abgeschlagen: Der Rest der Truppe ist verloren, die Zonen sind zurück.
            report.attacker_lost += siege.army;
            report.repelled = true;
            siege.army = 0.0;
            siege.army_peak = 0.0;
            siege.army_supply = 0.0;
            siege.landed_day = None;
            for zone in &mut siege.zones {
                zone.taken = false;
            }
        }
        let starved_out = siege.defenders(defense)
            <= GARRISON_FLOOR.max(balance.capitulate_frac * siege.garrison_start);
        if cut_off && defense.depot <= 0.0 && starved_out && occupied(siege) {
            occupy(siege, &mut effects);
            effects.verdict = Some(Outcome::Starved);
        }
    }

    // Wirkung auf die Welt: Beschuss und Bodenkrieg kosten Menschen und Kapital.
    if fire_damage > 0.0 || ground_damage > 0.0 {
        let floor = POPULATION_FLOOR.max(balance.pop_floor * siege.population_start);
        if population > floor {
            let left = (population * (1.0 - fire_damage) * (1.0 - ground_damage)).max(floor);
            effects.population = left / population;
            effects.fire_victims =
                (population - left) * fire_damage / (fire_damage + ground_damage);
        }
        effects.capital_fire = 1.0 - fire_damage;
        effects.capital_ground = 1.0 - ground_damage;
        effects.shock = 1.0 - fire_damage;
        defense.scar = (defense.scar + fire_damage + ground_damage).min(1.0);
        defense.scar_by = Some(siege.attacker);
    }
    effects.disruption = siege
        .zones
        .iter()
        .enumerate()
        .map(|(index, zone)| {
            let fought_here = report.zone == Some(index as u8);
            let level = match (fought_here, zone.taken) {
                (true, _) => balance.disrupt.0.max(balance.disrupt.1),
                (false, true) => balance.disrupt.1,
                (false, false) => 0.0,
            };
            zone.share as f64 * level
        })
        .sum::<f64>()
        .min(1.0);

    defense.militia = siege.zone_militia();
    report.hull = effects.hull;
    report.ammo = effects.ammo;
    report.fire_damage = fire_damage;
    report.ground_damage = ground_damage;
    effects.attacker_lost = report.attacker_lost + report.attacker_stranded;
    effects.defender_lost = report.defender_lost + report.defender_bombed;
    siege.report = report;
    effects
}

/// Fortschreibung einer Belagerung über Tage: der Datensatz und, in Kennzahlen, was die
/// Partie außerhalb davon bewegt (Magazine, Tanks, Rumpf, Bevölkerung, Wert der Betriebe).
/// Die Grundlage von [`forecast`]; mit eigenen Befehlen auch ein Werkzeug zum Auswuchten.
#[derive(Clone, Debug)]
pub struct Projection {
    pub siege: Siege,
    pub defense: PlanetDefense,
    /// Der Planet am nächsten zu rechnenden Tag.
    pub facts: PlanetFacts,
    pub orbit: OrbitView,
    /// Standfestigkeit der Flotte (siehe [`SiegeView::toughness`]).
    pub toughness: f64,
    /// Füllung der Tanks, 0..1.
    pub fuel: f64,
    /// Nachschub an Bord in Verbandstagen.
    pub supply_aboard: f64,
    /// Buchwert der Betriebe der Welt.
    pub firm_value: f64,
    /// Gerechnete Tage.
    pub days: u32,
    /// Rumpfschaden an der Flotte seit Beginn, in Punkten.
    pub hull_damage: f64,
    /// Verluste der Landungstruppe, auch beim Landen, und der Verteidiger durch Gefecht
    /// und Beschuss.
    pub own_losses: f64,
    pub enemy_losses: f64,
    /// Der Planet ist gefallen.
    pub verdict: Option<Outcome>,
    /// Eine Landung wurde abgeschlagen.
    pub repelled: bool,
    firepower_intact: f64,
    prize_start: f64,
}

impl Projection {
    pub fn new(view: &SiegeView, balance: &SiegeBalance) -> Self {
        Self {
            siege: view.siege.clone(),
            defense: view.defense.clone(),
            facts: view.facts,
            orbit: view.orbit,
            toughness: view.toughness.max(0.0),
            fuel: view.fuel.clamp(0.0, 1.0),
            supply_aboard: view.supply_aboard.max(0.0),
            firm_value: view.firm_value.max(0.0),
            days: 0,
            hull_damage: 0.0,
            own_losses: 0.0,
            enemy_losses: 0.0,
            verdict: None,
            repelled: false,
            firepower_intact: view.orbit.firepower.max(0.0),
            prize_start: view.prize(balance),
        }
    }

    /// Wert der Beute: Betriebe und Bevölkerung.
    pub fn prize(&self, balance: &SiegeBalance) -> f64 {
        self.firm_value + balance.prize_per_million * self.facts.population
    }

    /// Anteil der Beute vom Beginn der Fortschreibung, der verloren ist (0..1).
    pub fn prize_loss(&self, balance: &SiegeBalance) -> f64 {
        if self.prize_start > 0.0 {
            (1.0 - self.prize(balance) / self.prize_start).clamp(0.0, 1.0)
        } else {
            0.0
        }
    }

    /// Rumpfanteil (0..1), den die schweren Schiffe der Flotte verloren haben.
    pub fn hull_loss(&self) -> f64 {
        if self.toughness > 0.0 {
            (self.hull_damage / self.toughness).min(1.0)
        } else {
            0.0
        }
    }

    pub fn invested(&self) -> bool {
        invested(&self.orbit, &self.defense)
    }

    /// Landet höchstens `strength` der eingeschifften Verbände. Die Landung zählt zum
    /// Vortag: Befehle wirken zwischen den Tagesgrenzen, der nächste Tag sieht die Schiffe
    /// im niedrigen Orbit und die Truppe im Landekopf.
    pub fn land(&mut self, strength: f64, balance: &SiegeBalance) -> Landing {
        let strength = strength.clamp(0.0, self.orbit.embarked.max(0.0));
        let day = self.facts.day.saturating_sub(1);
        let landing = land(&mut self.siege, &self.defense, strength, day, balance);
        self.orbit.embarked -= strength;
        self.own_losses += landing.lost;
        landing
    }

    /// Rechnet den nächsten Tag mit den stehenden Befehlen in `siege.orders`.
    pub fn step(&mut self, balance: &SiegeBalance) -> DayEffects {
        // Mit Orbitalhoheit geht der Nachschub an Bord zur Truppe am Boden.
        if self.orbit.control && self.siege.army > 0.0 && self.supply_aboard > 0.0 {
            self.siege.army_supply += self.supply_aboard;
            self.supply_aboard = 0.0;
        }
        let effects = resolve(
            &mut self.siege,
            &mut self.defense,
            &self.facts,
            &self.orbit,
            balance,
        );
        self.facts.population *= effects.population;
        self.facts.day += 1;
        self.firm_value *= effects.capital();
        self.orbit.ammo = (self.orbit.ammo - effects.ammo).max(0.0);
        self.orbit.embarked = (self.orbit.embarked - effects.landed).max(0.0);
        self.fuel = (self.fuel - effects.fuel).max(0.0);
        self.hull_damage += effects.hull;
        if self.toughness > 0.0 {
            // Die schweren Schiffe tragen die Feuerkraft und die meisten Treffer.
            let intact = (1.0 - self.hull_damage / self.toughness).max(0.0);
            self.orbit.firepower = self.firepower_intact * intact;
            if intact <= 0.0 {
                self.orbit.control = false;
            }
        }
        if self.fuel <= 0.0 {
            self.orbit.control = false;
        }
        self.own_losses += effects.attacker_lost;
        self.enemy_losses += effects.defender_lost;
        self.repelled |= self.siege.report.repelled;
        self.verdict = effects.verdict;
        self.days += 1;
        effects
    }

    /// Feuerstufe für den nächsten Tag: Mit leeren Magazinen bleibt die Flotte im hohen
    /// Orbit, statt sich nutzlos den Geschützen auszusetzen.
    fn fire_while_armed(&self, fire: Fire) -> Fire {
        if self.orbit.ammo > 0.0 {
            fire
        } else {
            Fire::Hold
        }
    }

    /// Gelänge der Sturm, wenn jetzt alles landete und die Feuerstufe bliebe? Er gilt als
    /// gelungen, wenn der Planet fällt und die Truppe dabei höchstens den Anteil
    /// `storm_loss_limit` verliert.
    pub fn storm_succeeds(&self, balance: &SiegeBalance) -> bool {
        let mut trial = self.clone();
        let force = trial.siege.army + trial.orbit.embarked.max(0.0);
        if force <= 0.0 {
            return false;
        }
        let tolerated = trial.own_losses + balance.storm_loss_limit * force;
        let fire = trial.siege.orders.fire;
        trial.land(trial.orbit.embarked, balance);
        trial.siege.orders.assault = true;
        for _ in self.days..balance.max_siege_days {
            trial.siege.orders.fire = trial.fire_while_armed(fire);
            trial.step(balance);
            if trial.repelled || trial.own_losses > tolerated {
                return false;
            }
            if trial.verdict.is_some() {
                return true;
            }
            if trial.siege.army <= 0.0 {
                return false;
            }
        }
        false
    }

    /// Setzt die Befehle eines Wegs für den nächsten Tag und landet, wenn der Weg es
    /// vorsieht.
    fn follow(&mut self, course: Course, balance: &SiegeBalance) {
        let ready = self.invested() && self.orbit.embarked > 0.0;
        match course {
            Course::Starve => {
                self.siege.orders.fire = Fire::Hold;
                self.siege.orders.assault = false;
            }
            Course::Storm => {
                self.siege.orders.fire = self.fire_while_armed(Fire::Support);
                if ready {
                    self.land(self.orbit.embarked, balance);
                }
                self.siege.orders.assault = self.siege.army > 0.0;
            }
            Course::Area => {
                self.siege.orders.fire = self.fire_while_armed(Fire::Area);
                if ready && self.storm_succeeds(balance) {
                    self.land(self.orbit.embarked, balance);
                }
                self.siege.orders.assault = self.siege.army > 0.0;
            }
        }
    }

    /// Kann auf diesem Weg nichts mehr geschehen, das den Planeten fallen ließe?
    fn dead_end(&self, course: Course, balance: &SiegeBalance) -> Option<Limit> {
        let ashore = self.siege.army > 0.0;
        let aboard = self.orbit.embarked > 0.0;
        if self.toughness > 0.0 && self.hull_damage >= self.toughness {
            return Some(Limit::Fleet);
        }
        if self.fuel <= 0.0 {
            return Some(Limit::Fuel);
        }
        if !self.orbit.control && !ashore {
            return Some(Limit::NoControl);
        }
        if self.orbit.control && self.defense.stations > 0.0 && self.orbit.firepower <= 0.0 {
            return Some(Limit::Stations);
        }
        if course != Course::Starve && self.repelled {
            return Some(Limit::Repelled);
        }
        if course != Course::Starve && !ashore && !aboard {
            return Some(Limit::NoTroops);
        }
        // Reif zur Übernahme, aber niemand da, der sie annimmt.
        let surface_lost = self.siege.surface_lost();
        let starved_out = (self.invested() || surface_lost)
            && self.defense.depot <= 0.0
            && self.siege.defenders(&self.defense)
                <= GARRISON_FLOOR.max(balance.capitulate_frac * self.siege.garrison_start);
        if (surface_lost && self.siege.deep <= 0.0) || starved_out {
            return Some(Limit::NoTroops);
        }
        // Flächenbeschuss ohne Munition ist kein Beschuss mehr.
        if course == Course::Area && !ashore && self.orbit.ammo <= 0.0 {
            return Some(
                if self.siege.report.blocked.contains(Blocked::ShieldHolds) {
                    Limit::Shield
                } else {
                    Limit::Ammunition
                },
            );
        }
        None
    }
}

/// Wie [`forecast`], ruft aber nach jedem gerechneten Tag `observer` mit dem Stand und der
/// Wirkung des Tages.
pub fn trace(
    view: &SiegeView,
    course: Course,
    balance: &SiegeBalance,
    observer: &mut dyn FnMut(&Projection, &DayEffects),
) -> Forecast {
    let mut projection = Projection::new(view, balance);
    let mut limit = None;
    while projection.days < balance.max_siege_days {
        projection.follow(course, balance);
        let effects = projection.step(balance);
        observer(&projection, &effects);
        if projection.verdict.is_some() {
            break;
        }
        limit = projection.dead_end(course, balance);
        if limit.is_some() {
            break;
        }
    }
    let falls = projection.verdict.is_some();
    Forecast {
        falls,
        outcome: projection.verdict,
        days: projection.days,
        own_losses: projection.own_losses,
        enemy_losses: projection.enemy_losses,
        hull_loss: projection.hull_loss(),
        prize_loss: projection.prize_loss(balance),
        limited_by: if falls {
            None
        } else {
            Some(limit.unwrap_or(Limit::Time))
        },
    }
}

/// Rechnet einen Weg des Belagerers voraus, Tag für Tag mit [`resolve`]: Aushungern (kein
/// Feuer, keine Landung), Sturm (jetzt alles landen, stürmen, Stützfeuer) oder
/// Flächenbeschuss (gelandet wird, sobald der Sturm mit tragbaren Verlusten gelingt).
/// Zusammenhalt, Versorgung und Marktanteil der Garnison bleiben dabei, wie die Sicht sie
/// nennt; Bevölkerung und Wert der Betriebe folgen dem Schaden, Magazine, Tanks und Rumpf
/// der Flotte schwinden mit. Sind die Magazine leer, stellt die Flotte das Feuer ein.
pub fn forecast(view: &SiegeView, course: Course, balance: &SiegeBalance) -> Forecast {
    trace(view, course, balance, &mut |_, _| {})
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::{GameData, siege_day_bound};
    use crate::ids::PlanetId;
    use crate::rng::{Domain, Rng};
    use crate::siege::Sieges;

    const ATTACKER: PolityId = PolityId(0);
    const DEFENDER: PolityId = PolityId(1);

    fn balance() -> SiegeBalance {
        GameData::load_workspace().unwrap().balance.siege
    }

    /// Ein Planet unter Belagerung, von Hand gebaut, samt dem, was die Partie nachführen
    /// würde: Bevölkerung, Kapital, Magazine.
    #[derive(Clone)]
    struct Case {
        siege: Siege,
        defense: PlanetDefense,
        facts: PlanetFacts,
        orbit: OrbitView,
        /// Kapitalstock im Verhältnis zum Beginn.
        capital: f64,
        ammo_spent: f64,
        hull: f64,
        attacker_lost: f64,
        defender_lost: f64,
    }

    impl Case {
        /// Zusammenhalt 70, volle Versorgung, eine Startflotte mit Orbitalhoheit, der
        /// Schild geladen. Der erste gerechnete Tag ist Tag 100.
        fn new(population: f64, mut defense: PlanetDefense, balance: &SiegeBalance) -> Self {
            defense.charge = (defense.shield as f64 * balance.shield_cap) as f32;
            let facts = PlanetFacts {
                day: 100,
                population,
                cohesion: 70.0,
                essentials_met: 1.0,
                forage: 1.0,
            };
            let stakes = BTreeMap::from([(DEFENDER, 1.0)]);
            let siege = form(
                ATTACKER,
                DEFENDER,
                &mut defense,
                &facts,
                stakes,
                0.0,
                balance,
            );
            Self {
                siege,
                defense,
                facts,
                orbit: OrbitView {
                    control: true,
                    firepower: 150.0,
                    ammo: 1.0,
                    ..Default::default()
                },
                capital: 1.0,
                ammo_spent: 0.0,
                hull: 0.0,
                attacker_lost: 0.0,
                defender_lost: 0.0,
            }
        }

        /// Rechnet einen Tag mit dem Tagesrechner und führt nach, was er zurückgibt.
        fn day(&mut self, balance: &SiegeBalance) -> DayEffects {
            let effects = resolve(
                &mut self.siege,
                &mut self.defense,
                &self.facts,
                &self.orbit,
                balance,
            );
            self.facts.population *= effects.population;
            self.facts.day += 1;
            self.capital *= effects.capital();
            self.orbit.ammo = (self.orbit.ammo - effects.ammo).max(0.0);
            self.orbit.embarked = (self.orbit.embarked - effects.landed).max(0.0);
            self.ammo_spent += effects.ammo;
            self.hull += effects.hull;
            self.attacker_lost += effects.attacker_lost;
            self.defender_lost += effects.defender_lost;
            effects
        }

        /// Landet eine Welle, wie es der Befehl am Vortag getan hätte, mit reichlich Nachschub.
        fn land(&mut self, strength: f64, balance: &SiegeBalance) -> Landing {
            let landing = land(
                &mut self.siege,
                &self.defense,
                strength,
                self.facts.day - 1,
                balance,
            );
            self.siege.army_supply += 1000.0 * strength;
            self.attacker_lost += landing.lost;
            landing
        }

        fn zone(&self, kind: ZoneKind) -> usize {
            self.siege
                .zones
                .iter()
                .position(|zone| zone.kind == kind)
                .expect("Zone vorhanden")
        }

        /// Übergibt eine Zone ohne Kampf an den Belagerer.
        fn cede(&mut self, kind: ZoneKind) {
            let index = self.zone(kind);
            self.siege.zones[index].taken = true;
            self.siege.zones[index].militia = 0.0;
            self.defense.militia = self.siege.zone_militia();
        }

        fn defenders(&self) -> f64 {
            self.siege.defenders(&self.defense)
        }

        /// Erste Auffälligkeit im Zustand, geprüft wie in der Partie.
        fn anomaly(&self) -> Option<String> {
            let mut sieges = Sieges::new(1);
            sieges.defenses[0] = self.defense.clone();
            sieges.active.insert(PlanetId(0), self.siege.clone());
            sieges.find_anomaly(1)
        }
    }

    #[test]
    fn zones_follow_population_and_installations() {
        let balance = balance();
        let kinds = |zones: &[Zone]| zones.iter().map(|zone| zone.kind).collect::<Vec<_>>();
        let colony = build_zones(50.0, false, false, 6.0, &balance);
        assert_eq!(kinds(&colony), vec![ZoneKind::Outskirts, ZoneKind::Center]);
        let world = build_zones(500.0, false, false, 18.0, &balance);
        assert_eq!(
            kinds(&world),
            vec![ZoneKind::Outskirts, ZoneKind::City, ZoneKind::Center]
        );
        let home = build_zones(4000.0, true, true, 50.0, &balance);
        assert_eq!(
            kinds(&home),
            vec![
                ZoneKind::Outskirts,
                ZoneKind::ShieldWorks,
                ZoneKind::Battery,
                ZoneKind::City,
                ZoneKind::City,
                ZoneKind::Center
            ]
        );
        for (zones, militia) in [(&colony, 6.0), (&world, 18.0), (&home, 50.0)] {
            let shares: f64 = zones.iter().map(|zone| zone.share as f64).sum();
            assert!((shares - 1.0).abs() < 1e-6, "Anteile {shares}");
            let total: f64 = zones.iter().map(|zone| zone.militia).sum();
            assert!((total - militia).abs() < 1e-9, "Miliz {total}");
            let center = zones.last().unwrap();
            assert!((center.share as f64 - balance.center_share).abs() < 1e-6);
            // Die Miliz steht dort, wo die Menschen wohnen.
            for zone in zones.iter() {
                assert!((zone.militia - militia * zone.share as f64).abs() < 1e-5);
            }
        }
        assert!((home[1].share as f64 - balance.works_share).abs() < 1e-6);
        // Umland und Städte teilen sich den Rest zu gleichen Teilen.
        assert_eq!(home[0].share, home[3].share);
        assert_eq!(home[3].share, home[4].share);
    }

    #[test]
    fn a_record_forms_from_population_cohesion_and_prior_stake() {
        let balance = balance();
        let facts = PlanetFacts {
            day: 7,
            population: 900.0,
            cohesion: 60.0,
            essentials_met: 1.0,
            forage: 1.0,
        };
        let build = |stake: f64, posture: Posture| {
            let mut defense = PlanetDefense {
                bunkers: 1,
                regulars: 20.0,
                posture,
                ..Default::default()
            };
            let stakes = BTreeMap::from([(ATTACKER, stake), (DEFENDER, 1.0 - stake)]);
            let siege = form(
                ATTACKER,
                DEFENDER,
                &mut defense,
                &facts,
                stakes,
                5000.0,
                &balance,
            );
            (siege, defense)
        };
        let (siege, defense) = build(0.0, Posture::Forward);
        let militia = balance.militia_k * 30.0;
        assert!((defense.militia - militia).abs() < 1e-9);
        assert_eq!(siege.started_day, 7);
        assert_eq!(siege.deep, 0.0);
        assert!((siege.garrison_start - (militia + 20.0)).abs() < 1e-9);
        assert_eq!(siege.surface_start, siege.garrison_start);
        assert_eq!(siege.prize_start, 5000.0);
        assert_eq!(siege.population_start, 900.0);
        assert_eq!(siege.phase(&defense), crate::siege::Phase::Investment);
        // In der Tiefe gestaffelt sitzen die Regulären im Untergrund, soweit die Bunker reichen.
        let (deep, _) = build(0.0, Posture::Deep);
        assert_eq!(deep.deep, balance.deep_per_bunker.min(20.0));
        assert!((deep.surface_start - (deep.garrison_start - deep.deep)).abs() < 1e-9);
        // Wem 60 % der Betriebe gehören, dem stellt sich nur 40 % der Miliz.
        let (owned, owned_defense) = build(0.6, Posture::Forward);
        assert!((owned_defense.militia - 0.4 * militia).abs() < 1e-9);
        assert!((owned.stake() - 0.6).abs() < 1e-12);
        // Die Miliz folgt dem Zusammenhalt, in Schranken.
        let (_, low, high) = balance.militia_cohesion;
        let base = balance.militia_k * 30.0;
        assert!((militia_strength(900.0, 0.0, &balance) - base * low).abs() < 1e-9);
        assert!((militia_strength(900.0, 1000.0, &balance) - base * high).abs() < 1e-9);
    }

    /// Test 1 der Vorgabe.
    #[test]
    fn shield_blocks_bombardment() {
        let balance = balance();
        let regen = 2.0 * balance.shield_regen;
        let build = |firepower: f64, shield: u8, fire: Fire| {
            let defense = PlanetDefense {
                shield,
                guns: 2.0,
                regulars: 8.0,
                depot: 240.0,
                ..Default::default()
            };
            let mut case = Case::new(500.0, defense, &balance);
            case.orbit.firepower = firepower;
            case.siege.orders.fire = fire;
            case
        };
        // Dreißig Tage Feuer brauchen mehr als eine Magazinfüllung: Die Flotte wird versorgt.
        let supplied_day = |case: &mut Case| {
            case.orbit.ammo = 1.0;
            case.day(&balance)
        };
        // Schwächer als die Nachladung von Schildstufe 2: Der Schild hält.
        let weak = 0.75 * regen;
        let mut shielded = build(weak, 2, Fire::Area);
        let mut twin = build(weak, 2, Fire::Hold);
        for _ in 0..30 {
            supplied_day(&mut shielded);
            supplied_day(&mut twin);
            assert!(shielded.siege.report.blocked.contains(Blocked::ShieldHolds));
        }
        assert_eq!(shielded.facts.population, twin.facts.population);
        assert_eq!(shielded.facts.population, 500.0);
        assert_eq!(shielded.capital, twin.capital);
        assert_eq!(shielded.defense.guns, twin.defense.guns);
        assert_eq!(shielded.siege.zones, twin.siege.zones);
        assert_eq!(shielded.defense.regulars, twin.defense.regulars);
        assert_eq!(shielded.defense.scar, twin.defense.scar);
        assert_eq!(shielded.defender_lost, 0.0);
        assert!(
            shielded.ammo_spent > 0.0,
            "der Beschuss kostet trotzdem Munition"
        );
        assert_eq!(twin.ammo_spent, 0.0);
        // Wer feuert, steht im niedrigen Orbit vor den Geschützen.
        assert!(shielded.hull > 0.0 && twin.hull == 0.0);

        // Ohne Schild trifft dasselbe Feuer die Welt.
        let mut open = build(weak, 0, Fire::Area);
        for _ in 0..30 {
            supplied_day(&mut open);
        }
        assert!(open.facts.population < twin.facts.population);
        assert!(open.capital < twin.capital);
        assert!(open.defense.scar > 0.0);
        assert_eq!(open.defense.scar_by, Some(ATTACKER));
        assert!(open.defenders() < twin.defenders());
        assert!(open.defense.guns < twin.defense.guns, "Gegenbatteriefeuer");

        // Starkes Feuer zehrt die Ladung auf; dann beginnt der Schaden.
        let mut broken = build(2.5 * regen, 2, Fire::Area);
        let mut first_damage = None;
        for day in 1..=30 {
            let effects = supplied_day(&mut broken);
            if effects.capital() < 1.0 {
                assert_eq!(broken.defense.charge, 0.0);
                first_damage.get_or_insert(day);
            } else {
                assert!(
                    first_damage.is_none(),
                    "einmal durch, hält der Schild nicht wieder"
                );
                assert_eq!(effects.population, 1.0);
            }
        }
        let first_damage = first_damage.expect("der Schild muss fallen");
        assert!(
            first_damage > 10,
            "erst nach Tagen, nicht sofort: Tag {first_damage}"
        );
        assert!(broken.facts.population < 500.0 && broken.defense.scar > 0.0);
    }

    /// Test 2 der Vorgabe.
    #[test]
    fn underground_gets_no_orbital_support() {
        let balance = balance();
        let underground = |firepower: f64, garrison: f64, army: f64| {
            let defense = PlanetDefense {
                bunkers: 2,
                regulars: garrison,
                depot: 400.0,
                posture: Posture::Deep,
                ..Default::default()
            };
            let mut case = Case::new(500.0, defense, &balance);
            for kind in [ZoneKind::Outskirts, ZoneKind::City, ZoneKind::Center] {
                case.cede(kind);
            }
            assert!(case.siege.surface_lost() && case.siege.deep == garrison);
            case.siege.army = army;
            case.siege.army_peak = army;
            case.siege.army_supply = 100_000.0;
            case.siege.landed_day = Some(10);
            case.siege.orders = SiegeOrders {
                fire: Fire::Support,
                assault: true,
                ..Default::default()
            };
            case.orbit.firepower = firepower;
            case
        };
        let (mut silent, mut loud) = (
            underground(0.0, 20.0, 60.0),
            underground(1000.0, 20.0, 60.0),
        );
        for _ in 0..30 {
            let quiet_day = silent.day(&balance);
            let loud_day = loud.day(&balance);
            assert_eq!(quiet_day, loud_day);
            assert_eq!(loud_day.ammo, 0.0);
            assert_eq!(silent.siege, loud.siege);
            assert_eq!(silent.defense, loud.defense);
            assert!(loud.siege.report.underground);
            assert!(loud.siege.report.blocked.contains(Blocked::Underground));
            // Unter der Erde nimmt die Welt keinen Schaden.
            assert_eq!(loud_day.capital(), 1.0);
            assert_eq!(loud_day.population, 1.0);
        }
        assert!(loud.attacker_lost > 0.0 && loud.defender_lost > 0.0);

        // Dasselbe Paar in einer Stadt: Mit Stützfeuer fallen mehr Verteidiger.
        let city = |firepower: f64| {
            let defense = PlanetDefense {
                regulars: 5.0,
                depot: 400.0,
                ..Default::default()
            };
            let mut case = Case::new(500.0, defense, &balance);
            case.cede(ZoneKind::Outskirts);
            case.siege.army = 20.0;
            case.siege.army_peak = 20.0;
            case.siege.army_supply = 100_000.0;
            case.siege.landed_day = Some(10);
            case.siege.orders = SiegeOrders {
                fire: Fire::Support,
                assault: true,
                focus: case.zone(ZoneKind::City) as u8,
                sack: false,
            };
            case.orbit.firepower = firepower;
            case
        };
        let (mut silent, mut loud) = (city(0.0), city(1000.0));
        let quiet_day = silent.day(&balance);
        let loud_day = loud.day(&balance);
        let zone = Some(loud.zone(ZoneKind::City) as u8);
        assert_eq!(loud.siege.report.zone, zone);
        assert_eq!(silent.siege.report.zone, zone);
        assert!(loud.siege.report.defender_lost > silent.siege.report.defender_lost);
        assert_eq!(
            loud.siege.report.attacker_lost,
            silent.siege.report.attacker_lost
        );
        assert!(loud_day.ammo > 0.0 && quiet_day.ammo == 0.0);

        // Gleiche Kräfte: Im Untergrund zahlt der Angreifer mindestens das Zweieinhalbfache.
        let mut tunnels = underground(0.0, 6.0, 6.0);
        tunnels.day(&balance);
        assert!(tunnels.defender_lost > 0.0);
        assert!(
            tunnels.attacker_lost >= 2.5 * tunnels.defender_lost,
            "Untergrund {:.2} : {:.2}",
            tunnels.attacker_lost,
            tunnels.defender_lost
        );
        // Im Umland höchstens das 1,3-Fache.
        let defense = PlanetDefense {
            regulars: 10.0,
            depot: 400.0,
            ..Default::default()
        };
        let mut field = Case::new(500.0, defense, &balance);
        let outskirts = field.zone(ZoneKind::Outskirts);
        let force = field.siege.zones[outskirts].militia + 10.0;
        field.siege.army = force;
        field.siege.army_peak = force;
        field.siege.army_supply = 100_000.0;
        field.siege.landed_day = Some(10);
        field.siege.orders.assault = true;
        field.day(&balance);
        assert_eq!(field.siege.report.zone, Some(outskirts as u8));
        assert!(field.attacker_lost > 0.0);
        assert!(
            field.attacker_lost <= 1.3 * field.defender_lost,
            "Umland {:.2} : {:.2}",
            field.attacker_lost,
            field.defender_lost
        );
    }

    /// Test 3 der Vorgabe.
    #[test]
    fn starving_out_ends() {
        let balance = balance();
        let population = 800.0;
        let need = occupation_force(population, &balance);
        let build = |depot: f64, forage: f64, essentials: f64| {
            let defense = PlanetDefense {
                guns: 1.0,
                regulars: 12.0,
                depot,
                ..Default::default()
            };
            let mut case = Case::new(population, defense, &balance);
            case.facts.forage = forage;
            case.facts.essentials_met = essentials;
            case.orbit.embarked = 2.0 * need;
            case
        };
        let mut days = BTreeMap::new();
        for depot in [0u32, 60, 240] {
            for forage in [0u32, 1] {
                for essentials in [0u32, 1] {
                    let mut case = build(depot as f64, forage as f64, essentials as f64);
                    let mut fell = None;
                    for day in 1..=balance.max_siege_days {
                        let effects = case.day(&balance);
                        assert_eq!(effects.attacker_lost, 0.0);
                        assert_eq!(effects.defender_lost, 0.0);
                        assert_eq!(effects.population, 1.0);
                        assert_eq!(effects.capital(), 1.0);
                        assert_eq!(effects.hull, 0.0, "wer wartet, bleibt im hohen Orbit");
                        assert_eq!(effects.ammo, 0.0);
                        if let Some(verdict) = effects.verdict {
                            assert_eq!(verdict, Outcome::Starved);
                            // Die Besatzungstruppe geht ohne Verluste an Land.
                            assert!((effects.landed - need).abs() < 1e-9);
                            assert!((case.siege.army - need).abs() < 1e-9);
                            fell = Some(day);
                            break;
                        }
                    }
                    let fell = fell.unwrap_or_else(|| {
                        panic!("Depot {depot}, Markt {forage}, Versorgung {essentials}: hält")
                    });
                    assert_eq!(case.facts.population, population);
                    assert_eq!(case.capital, 1.0);
                    assert_eq!(case.defense.scar, 0.0);
                    assert_eq!(case.defense.depot, 0.0);
                    days.insert((depot, forage, essentials), fell);
                }
            }
        }
        // Vorräte, Markt vor Ort und gute Versorgung verlängern das Aushalten; nichts
        // davon verhindert das Ende.
        for forage in [0, 1] {
            for essentials in [0, 1] {
                assert!(days[&(0, forage, essentials)] < days[&(60, forage, essentials)]);
                assert!(days[&(60, forage, essentials)] < days[&(240, forage, essentials)]);
            }
        }
        assert!(days[&(60, 0, 1)] < days[&(60, 1, 1)]);
        assert!(days[&(60, 1, 0)] < days[&(60, 1, 1)]);
        let longest = *days.values().max().unwrap();
        assert_eq!(longest, days[&(240, 1, 1)]);
        assert!(longest as f64 <= siege_day_bound(&balance));

        // Ohne Besatzungstruppe fällt nichts, auch wenn die Garnison sich auflöst.
        let mut unattended = build(60.0, 1.0, 1.0);
        unattended.orbit.embarked = 0.5 * need;
        for _ in 0..balance.max_siege_days {
            assert_eq!(unattended.day(&balance).verdict, None);
        }
        assert!(unattended.defenders() < 0.2);
        assert_eq!(unattended.siege.army, 0.0);

        // Ohne Orbitalhoheit fallen die Vorräte nicht.
        let mut open = build(60.0, 1.0, 1.0);
        open.orbit.control = false;
        let garrison = open.defenders();
        for _ in 0..100 {
            let effects = open.day(&balance);
            assert_eq!(effects.verdict, None);
            assert_eq!(effects.fuel, 0.0);
        }
        assert_eq!(open.defense.depot, 60.0);
        assert_eq!(open.defenders(), garrison);

        // Ebenso, solange eine Station steht und die Flotte ihr nichts anhaben kann.
        let mut guarded = build(60.0, 1.0, 1.0);
        guarded.defense.stations = 1.0;
        guarded.orbit.firepower = 0.0;
        for _ in 0..100 {
            assert_eq!(guarded.day(&balance).verdict, None);
        }
        assert_eq!(guarded.defense.depot, 60.0);
        assert!(guarded.hull > 0.0, "die Station schießt auf die Flotte");
    }

    /// Test 4 der Vorgabe.
    #[test]
    fn bombardment_lowers_the_prize() {
        let balance = balance();
        let population = 300.0;
        // Beute: Betriebe (am Kapital hängend) und Bevölkerung.
        let firms = 30.0 * population;
        let run = |opening: Fire| {
            let defense = PlanetDefense {
                regulars: 6.0,
                depot: 200.0,
                ..Default::default()
            };
            let mut case = Case::new(population, defense, &balance);
            case.orbit.firepower = 300.0;
            case.siege.orders.fire = opening;
            for _ in 0..20 {
                assert_eq!(case.day(&balance).verdict, None);
            }
            // Derselbe Sturm in beiden Läufen.
            case.land(30.0, &balance);
            case.siege.orders = SiegeOrders {
                fire: Fire::Support,
                assault: true,
                ..Default::default()
            };
            for _ in 0..balance.max_siege_days {
                let effects = case.day(&balance);
                if let Some(verdict) = effects.verdict {
                    assert_eq!(verdict, Outcome::Stormed);
                    let prize =
                        firms * case.capital + balance.prize_per_million * case.facts.population;
                    return (case.facts.day, prize, case.attacker_lost);
                }
            }
            panic!("der Sturm hätte gelingen müssen");
        };
        let (fire_day, fire_prize, fire_losses) = run(Fire::Area);
        let (wait_day, wait_prize, wait_losses) = run(Fire::Hold);
        assert!(
            fire_prize < wait_prize,
            "Beute {fire_prize:.0} gegen {wait_prize:.0}"
        );
        assert!(
            fire_day < wait_day,
            "Fall an Tag {fire_day} gegen {wait_day}"
        );
        assert!(
            fire_losses < wait_losses,
            "Verluste {fire_losses:.2} gegen {wait_losses:.2}"
        );
        // Auch der Sturm ohne Beschuss kostet etwas Beute, aber wenig.
        let untouched = firms + balance.prize_per_million * population;
        assert!(wait_prize < untouched && wait_prize > 0.9 * untouched);
    }

    /// Test 5 der Vorgabe.
    #[test]
    fn every_investment_with_an_occupation_force_ends_within_the_bound() {
        let balance = balance();
        let bound = siege_day_bound(&balance).ceil() as u64;
        assert!(bound <= balance.max_siege_days as u64);
        let mut outcomes = BTreeMap::new();
        for case_number in 0..200u64 {
            let mut rng = Rng::stream(11, Domain::Siege, case_number, 0);
            let population = rng.range_f64(20.0, 5000.0);
            let defense = PlanetDefense {
                guns: rng.range_u32(0, 4) as f32,
                shield: rng.range_u32(0, 4) as u8,
                bunkers: rng.range_u32(0, 4) as u8,
                regulars: rng.range_f64(0.0, 60.0),
                depot: rng.range_f64(0.0, balance.depot_days_cap as f64),
                posture: if rng.chance(0.5) {
                    Posture::Deep
                } else {
                    Posture::Forward
                },
                hold_fire: rng.chance(0.3),
                ..Default::default()
            };
            let mut case = Case::new(population, defense, &balance);
            case.facts.cohesion = rng.range_f64(0.0, 100.0);
            case.facts.essentials_met = rng.f64();
            case.facts.forage = rng.f64();
            case.orbit.firepower = rng.range_f64(0.0, 900.0);
            case.orbit.att.gates = [rng.chance(0.5); 4];
            case.orbit.def.gates = [rng.chance(0.5); 4];
            let stake = rng.f64() * rng.f64();
            case.siege.stakes = BTreeMap::from([(ATTACKER, stake), (DEFENDER, 1.0 - stake)]);
            let mut fell = None;
            for day in 1..=bound {
                // Jeden Tag andere Befehle, gelegentlich eine Landung.
                case.siege.orders = SiegeOrders {
                    fire: Fire::ALL[rng.range_usize(0, 4)],
                    assault: rng.chance(0.5),
                    focus: rng.range_u32(0, 9) as u8,
                    sack: rng.chance(0.5),
                };
                if rng.chance(0.04) {
                    let wave = rng.range_f64(0.1, 60.0);
                    let landing = land(
                        &mut case.siege,
                        &case.defense,
                        wave,
                        case.facts.day - 1,
                        &balance,
                    );
                    case.siege.army_supply += landing.landed * rng.range_f64(0.0, 60.0);
                }
                // Die Besatzungstruppe bleibt an Bord, die Magazine werden nachgefüllt.
                case.orbit.embarked = 1.5 * occupation_force(case.facts.population, &balance);
                case.orbit.ammo = 1.0;
                let effects = case.day(&balance);
                assert_eq!(case.anomaly(), None, "Fall {case_number}, Tag {day}");
                assert!(effects.population > 0.0 && effects.population <= 1.0);
                assert!(effects.capital() > 0.0 && effects.capital() <= 1.0);
                assert!(effects.hull >= 0.0 && effects.ammo >= 0.0);
                assert!(case.facts.population >= 1.0f64.min(population));
                if let Some(verdict) = effects.verdict {
                    fell = Some(verdict);
                    break;
                }
            }
            let verdict =
                fell.unwrap_or_else(|| panic!("Fall {case_number} endet nicht in {bound} Tagen"));
            *outcomes.entry(verdict).or_insert(0u32) += 1;
        }
        // Beide Enden kommen vor: gestürmt und ausgehungert.
        assert!(
            outcomes.get(&Outcome::Stormed).copied().unwrap_or(0) > 0,
            "{outcomes:?}"
        );
        assert!(
            outcomes.get(&Outcome::Starved).copied().unwrap_or(0) > 0,
            "{outcomes:?}"
        );
    }

    /// Test 6 der Vorgabe.
    #[test]
    fn guns_reach_low_orbit_only_and_hit_big_ships_hardest() {
        let balance = balance();
        // Verteilung auf die Schiffe.
        let ships = [
            (ShipClass::Battleship, 2000.0),
            (ShipClass::Corvette, 240.0),
            (ShipClass::Corvette, 240.0),
            (ShipClass::Frigate, 300.0),
            (ShipClass::Cruiser, 700.0),
        ];
        let shares = hull_shares(60.0, &ships, &balance);
        assert!(shares[0] >= 3.0 * shares[1], "{shares:?}");
        assert_eq!(shares[1], shares[2]);
        assert!(shares[3] > shares[1] && shares[3] < shares[0]);
        assert_eq!(
            shares[0], shares[4],
            "Großkampfschiffe verlieren denselben Anteil"
        );
        // In Punkten kommt genau der Schaden des Tages an.
        let dealt: f64 = shares
            .iter()
            .zip(&ships)
            .map(|(share, (_, toughness))| share * toughness)
            .sum();
        let weighted: f64 = ships
            .iter()
            .map(|(class, toughness)| balance.gun_weight[class.index()] * toughness)
            .sum();
        let expected: f64 = ships
            .iter()
            .map(|(class, toughness)| {
                60.0 * balance.gun_weight[class.index()] / weighted * toughness
            })
            .sum();
        assert!((dealt - expected).abs() < 1e-9);
        assert!(
            hull_shares(0.0, &ships, &balance)
                .iter()
                .all(|share| *share == 0.0)
        );
        assert!(hull_shares(60.0, &[], &balance).is_empty());

        // Wann die Geschütze feuern.
        let build = || {
            let defense = PlanetDefense {
                guns: 2.0,
                regulars: 5.0,
                depot: 100.0,
                ..Default::default()
            };
            Case::new(1500.0, defense, &balance)
        };
        let volley = 2.0 * balance.gun_damage;
        // Hoher Orbit: kein Feuer, kein Verbrauch über den Tagesbedarf hinaus.
        let mut high = build();
        let effects = high.day(&balance);
        assert_eq!(effects.hull, 0.0);
        let idle_use = 100.0 - high.defense.depot;
        // Niedriger Orbit durch Feuer: Die Geschütze antworten und zehren am Depot.
        let mut low = build();
        low.siege.orders.fire = Fire::Support;
        let effects = low.day(&balance);
        assert_eq!(effects.hull, volley);
        let fired_use = 100.0 - low.defense.depot;
        assert!((fired_use - idle_use - 2.0 * balance.gun_depot).abs() < 1e-9);
        // Niedriger Orbit durch eine Landung: genau am Tag danach.
        let mut landing = build();
        let wave = landing.land(10.0, &balance);
        let caught = (2.0 * balance.landing_loss).min(balance.landing_loss_cap);
        assert!((wave.lost - 10.0 * caught).abs() < 1e-9);
        assert!((wave.landed - 10.0 * (1.0 - caught)).abs() < 1e-9);
        assert_eq!(landing.day(&balance).hull, volley);
        assert_eq!(landing.day(&balance).hull, 0.0);
        // Feuer halten, leere Vorräte, genommene Batterie, keine Einschließung: Stille.
        let mut held = build();
        held.defense.hold_fire = true;
        held.siege.orders.fire = Fire::Area;
        assert_eq!(held.day(&balance).hull, 0.0);
        assert_eq!(held.land(10.0, &balance).lost, 0.0);
        let mut empty = build();
        empty.defense.depot = 0.0;
        empty.siege.orders.fire = Fire::Area;
        assert_eq!(empty.day(&balance).hull, 0.0);
        let mut taken = build();
        taken.cede(ZoneKind::Battery);
        taken.siege.orders.fire = Fire::Area;
        assert_eq!(taken.day(&balance).hull, 0.0);
        assert_eq!(
            taken.defense.guns, 2.0,
            "die Batterie fällt heil in die Hand des Belagerers"
        );
        let mut distant = build();
        distant.orbit.control = false;
        distant.siege.orders.fire = Fire::Area;
        assert_eq!(distant.day(&balance).hull, 0.0);
        // Solange eine Station steht, schießt nur sie.
        let mut guarded = build();
        guarded.defense.stations = 1.0;
        guarded.siege.orders.fire = Fire::Area;
        let effects = guarded.day(&balance);
        assert_eq!(effects.hull, balance.station_damage);
        assert!(guarded.siege.report.blocked.contains(Blocked::Stations));
        assert!((guarded.siege.report.stations_lost - 150.0 / balance.station_hp).abs() < 1e-6);
        assert_eq!(
            guarded.siege.phase(&guarded.defense),
            crate::siege::Phase::Approach
        );
    }

    /// Test 7 der Vorgabe.
    #[test]
    fn a_cut_off_landing_is_lost_and_the_zones_revert() {
        let balance = balance();
        let defense = PlanetDefense {
            regulars: 4.0,
            depot: 240.0,
            posture: Posture::Deep,
            ..Default::default()
        };
        let mut case = Case::new(500.0, defense, &balance);
        // Die Landung nimmt das Umland, dann reißt die Verbindung ab.
        case.land(30.0, &balance);
        case.siege.orders.assault = true;
        for _ in 0..40 {
            case.day(&balance);
            if case.siege.zone_taken(ZoneKind::Outskirts) {
                break;
            }
        }
        assert!(case.siege.zone_taken(ZoneKind::Outskirts));
        assert!(case.siege.army > 20.0);
        case.orbit.control = false;
        case.siege.army_supply = 0.0;
        case.siege.orders.assault = false;
        let mut gone = None;
        for day in 1..=100 {
            let effects = case.day(&balance);
            assert_eq!(effects.verdict, None);
            if case.siege.army <= 0.0 {
                assert!(case.siege.report.repelled);
                gone = Some(day);
                break;
            }
            assert!(case.siege.report.attacker_stranded > 0.0);
        }
        let gone = gone.expect("die abgeschnittene Truppe hätte vergehen müssen");
        assert!(gone > 30, "nicht an einem Tag: {gone}");
        assert!(case.siege.zones.iter().all(|zone| !zone.taken));
        assert_eq!(case.siege.landed_day, None);
        assert_eq!(case.siege.army_peak, 0.0);
        assert!(
            (case.attacker_lost - 30.0).abs() < 1e-6,
            "die ganze Truppe ist verloren"
        );
        // Der Datensatz besteht weiter, die Verteidiger halten wieder die ganze Oberfläche.
        assert!(!case.siege.surface_lost());
        assert_eq!(case.anomaly(), None);
    }

    /// Test 8 der Vorgabe.
    #[test]
    fn the_forecast_equals_the_resolver_day_for_day() {
        let balance = balance();
        let build = || {
            let defense = PlanetDefense {
                guns: 1.0,
                regulars: 6.0,
                depot: 45.0,
                ..Default::default()
            };
            let case = Case::new(400.0, defense, &balance);
            let mut orbit = case.orbit;
            orbit.firepower = 300.0;
            orbit.embarked = 40.0;
            SiegeView {
                planet: PlanetId(3),
                siege: case.siege,
                defense: case.defense,
                facts: case.facts,
                orbit,
                toughness: 2168.0,
                fuel: 1.0,
                supply_aboard: 3600.0,
                firm_value: 12_000.0,
            }
        };
        for course in Course::ALL {
            let view = build();
            let mut days = Vec::new();
            let forecast = trace(&view, course, &balance, &mut |projection, effects| {
                days.push((
                    projection.siege.clone(),
                    projection.defense.clone(),
                    *effects,
                ))
            });
            assert!(forecast.falls, "{}", course.label());
            assert_eq!(forecast.days as usize, days.len());

            // Derselbe Weg von Hand: Befehle setzen, landen, Tag für Tag rechnen. Die
            // Truppe ist stark genug, dass auch unter Flächenbeschuss sofort gelandet wird.
            let (mut siege, mut defense) = (view.siege.clone(), view.defense.clone());
            let (mut facts, mut orbit) = (view.facts, view.orbit);
            let (mut own_losses, mut enemy_losses, mut hull) = (0.0, 0.0, 0.0);
            let mut firm_value = view.firm_value;
            let mut supply = view.supply_aboard;
            for (index, (want_siege, want_defense, want_effects)) in days.iter().enumerate() {
                match course {
                    Course::Starve => {
                        siege.orders.fire = Fire::Hold;
                        siege.orders.assault = false;
                    }
                    _ => {
                        // Mit leeren Magazinen bliebe die Flotte im hohen Orbit.
                        siege.orders.fire = match course {
                            _ if orbit.ammo <= 0.0 => Fire::Hold,
                            Course::Area => Fire::Area,
                            _ => Fire::Support,
                        };
                        if orbit.embarked > 0.0 {
                            let landing = land(
                                &mut siege,
                                &defense,
                                orbit.embarked,
                                facts.day - 1,
                                &balance,
                            );
                            own_losses += landing.lost;
                            orbit.embarked = 0.0;
                        }
                        siege.orders.assault = true;
                        siege.army_supply += std::mem::take(&mut supply);
                    }
                }
                let effects = resolve(&mut siege, &mut defense, &facts, &orbit, &balance);
                assert_eq!(effects, *want_effects, "{} Tag {index}", course.label());
                assert_eq!(siege, *want_siege, "{} Tag {index}", course.label());
                assert_eq!(defense, *want_defense, "{} Tag {index}", course.label());
                facts.population *= effects.population;
                facts.day += 1;
                firm_value *= effects.capital();
                orbit.ammo = (orbit.ammo - effects.ammo).max(0.0);
                orbit.embarked = (orbit.embarked - effects.landed).max(0.0);
                hull += effects.hull;
                orbit.firepower = view.orbit.firepower * (1.0 - hull / view.toughness).max(0.0);
                own_losses += effects.attacker_lost;
                enemy_losses += effects.defender_lost;
            }
            let last = days.last().unwrap().2;
            assert_eq!(forecast.outcome, last.verdict);
            assert_eq!(forecast.own_losses, own_losses);
            assert_eq!(forecast.enemy_losses, enemy_losses);
            assert_eq!(forecast.hull_loss, (hull / view.toughness).min(1.0));
            let prize = firm_value + balance.prize_per_million * facts.population;
            assert!((forecast.prize_loss - (1.0 - prize / view.prize(&balance))).abs() < 1e-12);
            assert_eq!(forecast.limited_by, None);
            match course {
                Course::Starve => {
                    assert_eq!(forecast.outcome, Some(Outcome::Starved));
                    assert_eq!(forecast.own_losses, 0.0);
                    assert_eq!(forecast.prize_loss, 0.0);
                    assert_eq!(forecast.hull_loss, 0.0);
                }
                _ => {
                    assert_eq!(forecast.outcome, Some(Outcome::Stormed));
                    assert!(forecast.own_losses > 0.0 && forecast.hull_loss > 0.0);
                    assert_eq!(forecast.prize_loss > 0.02, course == Course::Area);
                }
            }
        }
        // Dieselbe Sicht, dieselbe Prognose.
        let view = build();
        assert_eq!(
            forecast(&view, Course::Area, &balance),
            forecast(&view, Course::Area, &balance)
        );
    }

    #[test]
    fn the_three_courses_trade_time_troops_and_prize() {
        let balance = balance();
        let defense = PlanetDefense {
            regulars: 6.0,
            depot: 60.0,
            ..Default::default()
        };
        let case = Case::new(400.0, defense, &balance);
        let mut orbit = case.orbit;
        orbit.firepower = 300.0;
        orbit.embarked = 40.0;
        let view = SiegeView {
            planet: PlanetId(0),
            siege: case.siege,
            defense: case.defense,
            facts: case.facts,
            orbit,
            toughness: 2168.0,
            fuel: 1.0,
            supply_aboard: 3600.0,
            firm_value: 12_000.0,
        };
        let starve = forecast(&view, Course::Starve, &balance);
        let storm = forecast(&view, Course::Storm, &balance);
        let area = forecast(&view, Course::Area, &balance);
        assert!(starve.falls && storm.falls && area.falls);
        // Warten: langsam, verlustfrei, die Beute bleibt heil.
        assert!(starve.days > storm.days);
        assert_eq!(starve.own_losses, 0.0);
        assert_eq!(starve.prize_loss, 0.0);
        // Sturm: schneller, kostet Truppen, kaum Beute.
        assert!(storm.own_losses > 0.0);
        assert!(storm.prize_loss < 0.05);
        // Feuer: am schnellsten, spart Truppen, kostet Beute.
        assert!(area.days <= storm.days);
        assert!(area.own_losses < storm.own_losses);
        assert!(area.prize_loss > storm.prize_loss);
        assert!(area.enemy_losses > 0.0);
    }

    #[test]
    fn forecasts_name_what_a_course_fails_on() {
        let balance = balance();
        let build = |defense: PlanetDefense| {
            let case = Case::new(600.0, defense, &balance);
            let mut orbit = case.orbit;
            orbit.embarked = 30.0;
            SiegeView {
                planet: PlanetId(0),
                siege: case.siege,
                defense: case.defense,
                facts: case.facts,
                orbit,
                toughness: 1084.0,
                fuel: 1.0,
                supply_aboard: 2700.0,
                firm_value: 18_000.0,
            }
        };
        let plain = PlanetDefense {
            depot: 60.0,
            ..Default::default()
        };
        // Ohne Truppen nimmt niemand die Übergabe an; Feuer allein nimmt keinen Planeten.
        let mut view = build(plain.clone());
        view.orbit.embarked = 0.0;
        for course in Course::ALL {
            let forecast = forecast(&view, course, &balance);
            assert!(!forecast.falls, "{}", course.label());
            assert_eq!(
                forecast.limited_by,
                Some(Limit::NoTroops),
                "{}",
                course.label()
            );
        }
        // Ohne Orbitalhoheit geschieht nichts.
        let mut view = build(plain.clone());
        view.orbit.control = false;
        let blind = forecast(&view, Course::Starve, &balance);
        assert_eq!(blind.limited_by, Some(Limit::NoControl));
        assert_eq!(blind.days, 1);
        // Leere Tanks: Die Flotte kann nicht bleiben.
        let mut view = build(plain.clone());
        view.fuel = 9.5 * balance.station_fuel;
        let dry = forecast(&view, Course::Starve, &balance);
        assert_eq!(dry.limited_by, Some(Limit::Fuel));
        assert_eq!(dry.days, 10);
        // Eine Station, der die Flotte nichts anhaben kann.
        let mut view = build(PlanetDefense {
            stations: 2.0,
            ..plain.clone()
        });
        view.orbit.firepower = 0.0;
        assert_eq!(
            forecast(&view, Course::Storm, &balance).limited_by,
            Some(Limit::Stations)
        );
        // Eine Festung, an der der Sturm zerbricht.
        let fortress = PlanetDefense {
            regulars: 80.0,
            depot: 240.0,
            ..Default::default()
        };
        let stormed = forecast(&build(fortress.clone()), Course::Storm, &balance);
        assert!(!stormed.falls);
        assert_eq!(stormed.limited_by, Some(Limit::Repelled));
        assert!(stormed.own_losses >= 0.8 * 30.0);
        // Unter Flächenbeschuss landet dieselbe Truppe gar nicht erst: Die Munition geht
        // vorher aus.
        let burned = forecast(&build(fortress.clone()), Course::Area, &balance);
        assert!(!burned.falls);
        assert_eq!(burned.limited_by, Some(Limit::Ammunition));
        assert_eq!(burned.own_losses, 0.0);
        assert!(burned.prize_loss > 0.0 && burned.enemy_losses > 0.0);
        // Mit starken Geschützen hält die Flotte den niedrigen Orbit nicht so lange durch.
        let gunned = forecast(
            &build(PlanetDefense {
                guns: 4.0,
                ..fortress.clone()
            }),
            Course::Area,
            &balance,
        );
        assert_eq!(gunned.limited_by, Some(Limit::Fleet));
        assert_eq!(gunned.hull_loss, 1.0);
        // Hinter einem Schild, den die Flotte nicht bricht, bleibt auch die Beute heil.
        let shielded = forecast(
            &build(PlanetDefense {
                shield: 3,
                ..fortress
            }),
            Course::Area,
            &balance,
        );
        assert_eq!(shielded.limited_by, Some(Limit::Shield));
        assert_eq!(shielded.prize_loss, 0.0);
    }

    #[test]
    fn the_exchange_rates_of_fire_are_the_published_ones() {
        let balance = balance();
        let (precise, area) = (balance.precise, balance.area);
        assert!(
            (precise.0 / precise.1 - 15.0).abs() < 1e-9,
            "gezielt 15 : 1"
        );
        assert!((area.0 / area.1 - 3.0).abs() < 1e-9, "flächig 3 : 1");
        // Im Rechner: Verluste der Verteidiger je vernichtetem Anteil der Welt.
        let exchange = |fire: Fire| {
            let defense = PlanetDefense {
                regulars: 10.0,
                depot: 100.0,
                ..Default::default()
            };
            let mut case = Case::new(700.0, defense, &balance);
            case.siege.orders.fire = fire;
            let garrison = case.defenders();
            let effects = case.day(&balance);
            let military = case.siege.report.defender_bombed / garrison;
            let collateral = 1.0 - effects.capital();
            assert!(collateral > 0.0);
            assert!((1.0 - effects.population - collateral).abs() < 1e-12);
            assert!((1.0 - effects.shock - collateral).abs() < 1e-12);
            assert!((effects.fire_victims - 700.0 * collateral).abs() < 1e-9);
            assert_eq!(effects.capital_ground, 1.0);
            (military, collateral)
        };
        let (military, collateral) = exchange(Fire::Precise);
        assert!((military / collateral - 15.0).abs() < 1e-6);
        let (area_military, area_collateral) = exchange(Fire::Area);
        assert!((area_military / area_collateral - 3.0).abs() < 1e-6);
        assert!(area_military > military && area_collateral > collateral);
        // Stützfeuer allein trifft die Welt nicht.
        let mut case = Case::new(700.0, PlanetDefense::default(), &balance);
        case.siege.orders.fire = Fire::Support;
        let effects = case.day(&balance);
        assert_eq!(effects.capital(), 1.0);
        assert_eq!(effects.ammo, 0.0, "ohne Gefecht kein Stützfeuer");
    }

    #[test]
    fn bombardment_spares_a_floor_of_the_population_and_big_worlds_spread_the_fire() {
        let balance = balance();
        let mut colony = Case::new(40.0, PlanetDefense::default(), &balance);
        colony.orbit.firepower = 5000.0;
        colony.siege.orders.fire = Fire::Area;
        for _ in 0..400 {
            // Magazine nachgefüllt: Hier geht es um die Untergrenze, nicht um die Munition.
            colony.orbit.ammo = 1.0;
            colony.day(&balance);
        }
        let floor = (balance.pop_floor * 40.0).max(1.0);
        assert!((colony.facts.population - floor).abs() < 1e-9);
        assert!(colony.capital < 0.01, "das Kapital hat keine Untergrenze");
        assert_eq!(colony.defense.scar, 1.0);
        // Dieselbe Flotte richtet auf einer großen Welt anteilig weniger an: Der Schaden
        // fällt mit der Bevölkerung. Die Flotte ist so klein gewählt, dass die Obergrenze
        // der Rate auch auf der kleinsten Welt nicht greift.
        let firepower =
            0.5 * balance.bombard_cap * (50.0 + balance.bombard_pop0) / balance.bombard_k;
        let damage = |population: f64, firepower: f64| {
            let mut case = Case::new(population, PlanetDefense::default(), &balance);
            case.orbit.firepower = firepower;
            case.siege.orders.fire = Fire::Area;
            1.0 - case.day(&balance).capital()
        };
        let spread = |small: f64, big: f64| {
            let expected = (big + balance.bombard_pop0) / (small + balance.bombard_pop0);
            let measured = damage(small, firepower) / damage(big, firepower);
            assert!(
                (measured / expected - 1.0).abs() < 1e-9,
                "{small} gegen {big}"
            );
            expected
        };
        assert!(spread(50.0, 500.0) > 3.0);
        assert!(spread(500.0, 4000.0) > 3.0);
        // Eine erdrückende Flotte zerstört je Tag höchstens den Anteil der Obergrenze.
        let most = balance.area.1 * balance.bombard_cap;
        assert!((damage(50.0, 1e6) - most).abs() < 1e-12);
        assert!(damage(50.0, firepower) < most);
    }

    #[test]
    fn the_center_decides_and_bunkers_take_the_reserve() {
        let balance = balance();
        let build = |population: f64, regulars: f64| {
            let defense = PlanetDefense {
                bunkers: 1,
                regulars,
                depot: 200.0,
                ..Default::default()
            };
            let mut case = Case::new(population, defense, &balance);
            case.cede(ZoneKind::Outskirts);
            // Das Zentrum ist so gut wie gefallen: kaum noch Miliz, und die Truppe ist der
            // Reserve erdrückend überlegen.
            let center = case.zone(ZoneKind::Center);
            case.siege.zones[center].militia = 0.5;
            let army = 2.0 * (0.5 + regulars) / balance.zone_fall;
            case.siege.army = army;
            case.siege.army_peak = army;
            case.siege.army_supply = 100.0 * army;
            case.siege.landed_day = Some(10);
            case.siege.orders = SiegeOrders {
                assault: true,
                focus: center as u8,
                ..Default::default()
            };
            case
        };

        // Die Wirkung für sich: Das Zentrum fällt.
        let mut case = build(600.0, 20.0);
        let (city, center) = (case.zone(ZoneKind::City), case.zone(ZoneKind::Center));
        let city_militia = case.siege.zones[city].militia;
        let garrison = case.defenders();
        let supply = case.siege.army_supply;
        let mut report = SiegeReport::default();
        take_zone(
            &mut case.siege,
            &mut case.defense,
            center,
            garrison,
            &balance,
            &mut report,
        );
        assert_eq!(report.fell, Some(center as u8));
        assert!(case.siege.zones[center].taken);
        // Die Reserve weicht in den Untergrund, soweit der Bunker reicht; der Rest löst sich auf.
        assert_eq!(case.siege.deep, balance.deep_per_bunker);
        assert_eq!(case.defense.regulars, balance.deep_per_bunker);
        assert_eq!(case.siege.reserve(&case.defense), 0.0);
        // Die Miliz der Stadt verliert den Mut.
        let shaken = city_militia * balance.center_shock;
        assert!((case.siege.zones[city].militia - shaken).abs() < 1e-9);
        let dissolved = 0.5 + (20.0 - balance.deep_per_bunker) + (city_militia - shaken);
        assert!((report.defender_dissolved - dissolved).abs() < 1e-9);
        // Vorräte über dem, was der Untergrund birgt, fallen der Truppe zu.
        assert_eq!(case.defense.depot, balance.bunker_days);
        let captured = (200.0 - balance.bunker_days) * garrison;
        assert!((case.siege.army_supply - supply - captured).abs() < 1e-6);

        // Im Tagesrechner: Nach dem Zentrum hält die erschütterte Stadt weniger als den
        // Anteil zone_fall der Oberfläche vom Beginn. Damit bricht die ganze Oberfläche;
        // die Regulären im Bunker kämpfen im Untergrund weiter.
        let mut case = build(600.0, 20.0);
        assert!(shaken <= balance.zone_fall * case.siege.surface_start);
        let effects = case.day(&balance);
        assert_eq!(effects.verdict, None);
        assert!(case.siege.surface_lost());
        // Was vom Tag im Zentrum übrig ist, sitzt jetzt im Bunker.
        let deep = case.siege.deep;
        assert!(deep > 0.5 * balance.deep_per_bunker && deep <= balance.deep_per_bunker);
        assert_eq!(case.defense.regulars, deep);
        assert_eq!(
            case.siege.phase(&case.defense),
            crate::siege::Phase::Underground
        );
        let fought_and_taken = balance.disrupt.0 * balance.center_share
            + balance.disrupt.1 * (1.0 - balance.center_share);
        assert!((effects.disruption - fought_and_taken).abs() < 1e-6);
        let before = (case.attacker_lost, case.defender_lost);
        let mut verdict = None;
        for _ in 0..balance.max_siege_days {
            let effects = case.day(&balance);
            if effects.verdict.is_some() {
                verdict = effects.verdict;
                break;
            }
            assert!(case.siege.report.underground);
            assert!((effects.disruption - balance.disrupt.1).abs() < 1e-6);
        }
        assert_eq!(verdict, Some(Outcome::Stormed));
        assert_eq!(case.siege.deep, 0.0);
        // Den Bunker auszuräumen kostet mehr, als darin sitzt.
        let killed = case.defender_lost - before.1;
        assert!((killed - deep).abs() < 1e-6);
        assert!(case.attacker_lost - before.0 > killed);

        // Auf einer großen Welt halten die Städte auch ohne Zentrum: Die Oberfläche ist
        // nicht verloren, der Bodenkrieg geht weiter.
        let mut metropolis = build(3000.0, 0.0);
        let effects = metropolis.day(&balance);
        assert_eq!(effects.verdict, None);
        assert!(metropolis.siege.zone_taken(ZoneKind::Center));
        assert!(!metropolis.siege.surface_lost());
        assert_eq!(
            metropolis.siege.phase(&metropolis.defense),
            crate::siege::Phase::Ground
        );
        metropolis.day(&balance);
        assert_eq!(
            metropolis.siege.report.zone,
            Some(metropolis.zone(ZoneKind::City) as u8)
        );
    }

    #[test]
    fn a_strong_garrison_sallies_against_a_holding_force() {
        let balance = balance();
        let build = |posture: Posture| {
            let defense = PlanetDefense {
                bunkers: 3,
                regulars: 40.0,
                depot: 200.0,
                posture,
                ..Default::default()
            };
            let mut case = Case::new(600.0, defense, &balance);
            case.land(6.0, &balance);
            case
        };
        // Vorwärts aufgestellt greift die Übermacht den Landekopf an, ohne Schutz der Stellungen.
        let mut forward = build(Posture::Forward);
        let effects = forward.day(&balance);
        assert!(forward.siege.report.sortie);
        assert!(effects.attacker_lost > 0.0 && effects.defender_lost > 0.0);
        // In der Tiefe gestaffelt bleibt die Garnison, wo sie ist.
        let mut deep = build(Posture::Deep);
        let effects = deep.day(&balance);
        assert!(!deep.siege.report.sortie);
        assert_eq!(effects.attacker_lost, 0.0);
        assert_eq!(deep.siege.report.zone, None);
        // Eine haltende Truppe verbraucht weniger als eine kämpfende.
        let used = |case: &Case| 6000.0 - case.siege.army_supply;
        assert!(used(&deep) < used(&forward));
        assert!((used(&deep) - 6.0 * balance.army_idle_supply).abs() < 1e-9);
    }

    #[test]
    fn no_planet_falls_without_an_occupation_force() {
        let balance = balance();
        // Eine Welt ohne jede Garnison: Der Belagerer besitzt sie schon ganz.
        let mut case = Case::new(900.0, PlanetDefense::default(), &balance);
        case.siege.stakes = BTreeMap::from([(ATTACKER, 1.0)]);
        for zone in &mut case.siege.zones {
            zone.militia = 0.0;
        }
        case.siege.garrison_start = 0.0;
        case.siege.surface_start = 0.0;
        let need = occupation_force(900.0, &balance);
        // Ein Trupp, der zum Besetzen nicht reicht, nimmt Zone um Zone, aber nicht den Planeten.
        case.land(0.2 * need, &balance);
        case.siege.orders.assault = true;
        for _ in 0..30 {
            assert_eq!(case.day(&balance).verdict, None);
        }
        assert!(case.siege.surface_lost());
        // Mit Verstärkung an Bord fällt er am selben Tag.
        case.orbit.embarked = need;
        let effects = case.day(&balance);
        assert_eq!(effects.verdict, Some(Outcome::Stormed));
        assert!((effects.landed - 0.8 * need).abs() < 1e-6);
        assert!((case.siege.army - need).abs() < 1e-6);
    }

    #[test]
    fn technology_gates_change_support_and_tunnel_fighting() {
        let balance = balance();
        let city = |gate: bool| {
            let defense = PlanetDefense {
                regulars: 5.0,
                depot: 400.0,
                ..Default::default()
            };
            let mut case = Case::new(500.0, defense, &balance);
            case.land(20.0, &balance);
            case.siege.landed_day = Some(10);
            case.siege.orders = SiegeOrders {
                fire: Fire::Support,
                assault: true,
                ..Default::default()
            };
            case.orbit.firepower = 10_000.0;
            case.orbit.att.gates[Gate::Support as usize] = gate;
            case.day(&balance);
            case.siege.report.defender_lost
        };
        let (plain, guided) = (city(false), city(true));
        let expected = (1.0 + balance.support_max.1) / (1.0 + balance.support_max.0);
        assert!((guided / plain - expected).abs() < 1e-9);

        let tunnel = |attacker: bool, defender: bool| {
            let defense = PlanetDefense {
                bunkers: 1,
                regulars: 6.0,
                depot: 400.0,
                posture: Posture::Deep,
                ..Default::default()
            };
            let mut case = Case::new(500.0, defense, &balance);
            for kind in [ZoneKind::Outskirts, ZoneKind::City, ZoneKind::Center] {
                case.cede(kind);
            }
            case.siege.army = 6.0;
            case.siege.army_peak = 6.0;
            case.siege.army_supply = 1000.0;
            case.siege.orders.assault = true;
            case.orbit.att.gates[Gate::Tunnel as usize] = attacker;
            case.orbit.def.gates[Gate::Tunnel as usize] = defender;
            case.day(&balance);
            (
                case.siege.report.attacker_lost,
                case.siege.report.defender_lost,
            )
        };
        let (base_attacker, base_defender) = tunnel(false, false);
        let (attacker_loss, defender_loss) = tunnel(true, false);
        assert_eq!(attacker_loss, base_attacker);
        assert!((defender_loss / base_defender - balance.tunnel_tech).abs() < 1e-9);
        let (attacker_loss, defender_loss) = tunnel(false, true);
        assert_eq!(defender_loss, base_defender);
        assert!((attacker_loss / base_attacker - balance.tunnel_tech).abs() < 1e-9);
    }

    #[test]
    fn a_prior_stake_weakens_resistance() {
        let balance = balance();
        let days_to_starve = |stake: f64| {
            let defense = PlanetDefense {
                regulars: 12.0,
                depot: 30.0,
                ..Default::default()
            };
            let mut case = Case::new(800.0, defense, &balance);
            case.siege.stakes = BTreeMap::from([(ATTACKER, stake), (DEFENDER, 1.0 - stake)]);
            case.orbit.embarked = 30.0;
            (1..=balance.max_siege_days)
                .find(|_| case.day(&balance).verdict.is_some())
                .expect("fällt")
        };
        assert!(days_to_starve(0.6) < days_to_starve(0.0));
    }
}
