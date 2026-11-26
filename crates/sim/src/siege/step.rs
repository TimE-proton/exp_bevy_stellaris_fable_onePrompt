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

    /// Gelänge der Sturm, wenn jetzt alles landete? Er gilt als gelungen, wenn der Planet
    /// fällt und die Truppe dabei höchstens den Anteil `storm_loss_limit` verliert.
    pub fn storm_succeeds(&self, balance: &SiegeBalance) -> bool {
        let mut trial = self.clone();
        let force = trial.siege.army + trial.orbit.embarked.max(0.0);
        if force <= 0.0 {
            return false;
        }
        let tolerated = trial.own_losses + balance.storm_loss_limit * force;
        trial.land(trial.orbit.embarked, balance);
        trial.siege.orders.assault = true;
        for _ in self.days..balance.max_siege_days {
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
                self.siege.orders.fire = Fire::Support;
                if ready {
                    self.land(self.orbit.embarked, balance);
                }
                self.siege.orders.assault = self.siege.army > 0.0;
            }
            Course::Area => {
                self.siege.orders.fire = Fire::Area;
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
/// Bevölkerung, Zusammenhalt, Versorgung und Marktanteil der Garnison bleiben dabei, wie
/// die Sicht sie nennt; Magazine, Tanks und Rumpf der Flotte schwinden mit.
pub fn forecast(view: &SiegeView, course: Course, balance: &SiegeBalance) -> Forecast {
    trace(view, course, balance, &mut |_, _| {})
}
