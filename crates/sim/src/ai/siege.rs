//! Belagerung und Verteidigung der Welten durch die KI-Gemeinschaften.
//!
//! Angreifer, täglich ([`lead_sieges`]): Im Krieg wählt die Führung eine Welt des Gegners
//! ([`choose_target`]), sammelt ihre Hauptflotte an der eigenen Hauptwelt, schifft Truppen
//! ein ([`troops_wanted`]), marschiert und belagert. Über dem Ziel entscheidet sie jeden
//! Tag aus den drei vorausgerechneten Wegen der Belagerungskarte ([`decide`]): aushungern,
//! stürmen oder beschießen, landen, heimkehren zum Auftanken oder Truppenholen, aufgeben.
//!
//! Verteidiger: monatlich je Welt ein Verteidigungsplan nach Lage und Haushalt
//! ([`plan_defenses`], [`planned_defense`]); täglich die Befehle belagerter Welten
//! ([`garrison_orders`]) und die Übergabe, wenn Halten sinnlos ist ([`should_capitulate`]).
//! Den Entsatz führt die Grundhaltung ([`super::military`]) mit dem Hebel `relief_odds`.
//!
//! Befehle ergehen nur über [`super::levers`]. Über fremde Welten weiß die Führung, was
//! [`super::estimate::world_estimate`] hergibt; über die eigene Belagerung, was die
//! Karte des Lesemodells zeigt. Alle Schwellen stehen unter `military.ai.siege`.

use super::estimate::{
    ClassValues, WorldEstimate, add, odds, own_values, sightings, tech_edge, world_estimate,
};
use super::levers::{Hand, destination};
use super::military::{Situation, led};
use super::{AiState, Campaign, CampaignStage, MilitaryMode, MilitaryPlan};
use crate::battle::Doctrine;
use crate::clock::DAYS_PER_MONTH;
use crate::command::CommandQueue;
use crate::data::{DefenseRow, GameData};
use crate::economy::to_credits;
use crate::fleet::{Fleet, Stance, fleet_claim, supply_base};
use crate::ids::{PlanetId, PolityId, SystemId};
use crate::rng::{Domain, Rng};
use crate::siege::orbit::{berths_of, fleet_firepower};
use crate::siege::{
    Course, DefensePlan, Fire, Limit, PlanetDefense, Posture, Projection, SiegeCard, SiegeOrders,
    SiegeRefs, Works, ZoneKind, firm_value, level_caps, occupation_force, planet_upkeep,
    siege_card, siege_view,
};
use std::collections::BTreeMap;

/// Was nach dem Rückweg von einer Belagerung noch im Tank sein soll.
const FUEL_RESERVE: f64 = 0.05;
/// Stufen, in denen die Verteidigungspläne gekürzt werden, wenn der Haushalt sie nicht
/// trägt.
const PLAN_SCALES: [f64; 5] = [1.0, 0.75, 0.5, 0.25, 0.0];
/// So viele Tage wartet eine Flotte über dem Ziel darauf, dass ihre Belagerung beginnt.
const OPENING_DAYS: u64 = 30;
/// Ein neues Ziel sucht die Führung nur an jedem so vielten Tag: Die Suche wägt jede
/// Welt des Gegners und ist die teuerste Rechnung der Führung.
const TARGET_EVERY: u64 = 5;

/// Haltung einer Gemeinschaft, fest für die ganze Partie: aus dem Seed und ihrer Nummer.
/// Die verborgenen Dispositionen der Diplomatie ersetzen sie später.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Disposition {
    /// Wagemut 0..1: Wer mehr hat, dem ist ein Tag Belagerung teurer.
    pub boldness: f64,
    /// Standhaftigkeit 0..1: Wer mehr hat, übergibt später und die Hauptwelt nie.
    pub resolve: f64,
}

pub fn disposition(seed: u64, polity: PolityId) -> Disposition {
    let mut rng = Rng::stream(seed, Domain::Ai, polity.index() as u64, 0);
    Disposition {
        boldness: rng.f64(),
        resolve: rng.f64(),
    }
}

// --- Zielwahl und Truppen ---------------------------------------------------------------

/// Eine Welt des Gegners, wie die Führung sie als Ziel bewertet.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TargetFacts {
    pub planet: PlanetId,
    /// Hauptwelt ihrer Gemeinschaft.
    pub capital: bool,
    /// Wert der Beute in Credits und Bevölkerung in Millionen.
    pub value: f64,
    pub population: f64,
    /// Anteil des Tanks, den der Weg vom nächsten eigenen Hafen kostet.
    pub route_fuel: f64,
    /// Kräfteverhältnis der eigenen Flotte gegen alles, was dort eingreifen kann.
    pub odds: f64,
    pub estimate: WorldEstimate,
}

/// Was die Flotte eines Feldzugs mitbringt.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Force {
    /// Plätze für Truppen an Bord.
    pub berths: f64,
    pub firepower: f64,
}

/// Tage, die eine Einschließung ohne Feuer und Landung dauert, bis die Garnison aufgibt:
/// die Vorräte, gestreckt um den Markt vor Ort, und danach der Zerfall.
pub fn starve_days(data: &GameData, depot_days: f64) -> f64 {
    let balance = &data.balance.siege;
    depot_days.max(0.0) / (1.0 - balance.forage_share).max(0.05)
        + (1.0 / balance.capitulate_frac.max(1e-6)).ln() / balance.starve_rate.max(1e-6)
}

/// Verbände, die ein Feldzug gegen diese Welt mitnehmen soll: für einen Sturm das
/// `storm_ratio`-Fache der geschätzten Verteidiger, mindestens die Besatzungstruppe mit
/// Aufschlag, höchstens was an Bord passt.
pub fn troops_wanted(data: &GameData, facts: &TargetFacts, force: &Force) -> f64 {
    let levers = &data.balance.military.ai.siege;
    let occupation =
        levers.occupation_margin * occupation_force(facts.population, &data.balance.siege);
    let storm = levers.storm_ratio * facts.estimate.defenders();
    storm.max(occupation).min(force.berths)
}

/// Bewertet ein Ziel; `None`, wenn ein Feldzug dagegen nicht in Frage kommt.
///
/// In Frage kommt eine Welt, die in Reichweite liegt (`reach`, bei der weiteren Suche
/// `reach_wide`), an der die Flotte dem gewachsen ist, was eingreifen kann (`odds`, bei
/// der weiteren Suche `odds_wide`), deren Stationen sie in `station_days`
/// niederkämpft, für die die Besatzungstruppe an Bord passt, und die auf einem der drei
/// Wege zu nehmen ist: Die Truppen an Bord reichen für einen Sturm, oder der Tank reicht,
/// um sie auszuhungern, oder kein Schild schützt sie vor Beschuss.
///
/// Der Wert ist die Beute (eine Hauptwelt zählt `capital_bonus`-fach), geteilt durch den
/// Weg und durch die Stärke der Verteidigung, gemessen an den Plätzen an Bord.
pub fn target_score(
    data: &GameData,
    facts: &TargetFacts,
    force: &Force,
    wide: bool,
) -> Option<f64> {
    let ai = &data.balance.military.ai;
    let (levers, balance) = (&ai.siege, &data.balance.siege);
    let estimate = &facts.estimate;
    let (reach, least_odds) = if wide {
        (levers.reach_wide, levers.odds_wide)
    } else {
        (levers.reach, levers.odds)
    };
    if facts.route_fuel > reach as f64 || facts.odds < least_odds {
        return None;
    }
    let occupation = levers.occupation_margin * occupation_force(facts.population, balance);
    if force.berths < occupation {
        return None;
    }
    if estimate.stations > 0.0
        && estimate.stations * balance.station_hp > levers.station_days * force.firepower
    {
        return None;
    }
    let station_time = (levers.sail_fuel as f64 - 2.0 * facts.route_fuel - FUEL_RESERVE)
        / data.balance.military.fuel_per_day as f64;
    let can_storm = levers.storm_ratio * estimate.defenders() <= force.berths;
    let can_starve = starve_days(data, estimate.depot_days.1) <= station_time;
    let can_bombard = estimate.shield == 0 && force.firepower > 0.0;
    if !(can_storm || can_starve || can_bombard) {
        return None;
    }
    let bonus = if facts.capital {
        levers.capital_bonus
    } else {
        1.0
    };
    let way = 1.0 + facts.route_fuel / levers.reach as f64;
    let strength = 1.0 + estimate.defenders() / force.berths.max(1.0);
    Some(facts.value.max(0.0) * bonus / (way * strength))
}

/// Das beste Ziel und die Verbände, die der Feldzug mitnehmen soll. Gleichstand
/// entscheidet die kleinere Nummer.
pub fn choose_target(
    data: &GameData,
    candidates: &[TargetFacts],
    force: &Force,
    wide: bool,
) -> Option<(PlanetId, f64)> {
    candidates
        .iter()
        .filter_map(|facts| Some((facts, target_score(data, facts, force, wide)?)))
        .max_by(|a, b| a.1.total_cmp(&b.1).then(b.0.planet.cmp(&a.0.planet)))
        .map(|(facts, _)| (facts.planet, troops_wanted(data, facts, force)))
}

/// Verbände, die eine Flotte am Sammelplatz heute einschifft: was dem Feldzug noch
/// fehlt, soweit die Welt es über ihre Stammgarnison hinaus hergibt und an Bord Platz
/// ist. Bruchteile unter einem halben Verband bleiben an Land, außer sie machen die
/// Truppe voll.
pub fn embark_strength(wanted: f64, aboard: f64, berths: f64, regulars: f64, keep: f64) -> f64 {
    let missing = (wanted - aboard).max(0.0);
    let strength = missing
        .min((regulars - keep).max(0.0))
        .min((berths - aboard).max(0.0));
    if strength >= 0.5 || (strength > 0.0 && strength >= missing - 1e-9) {
        strength
    } else {
        0.0
    }
}

// --- Wahl des Wegs ----------------------------------------------------------------------

/// Was die Führung außer der Karte der Belagerung weiß.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SiegeContext {
    /// Anteil des Tanks, den der Weg zum nächsten eigenen Hafen kostet.
    pub fuel_home: f64,
    /// Kräfteverhältnis der belagernden Flotte gegen alles, was im Umkreis eingreifen
    /// kann; unendlich, wenn dort nichts steht.
    pub relief_odds: f64,
    /// Wagemut der Gemeinschaft ([`Disposition::boldness`]).
    pub boldness: f64,
    /// Sold und Wartung der belagernden Flotte je Tag, in Anteilen der Beute.
    pub upkeep_share: f64,
    /// Staatskasse in Monaten Flottenkosten.
    pub treasury_months: f64,
    /// Reguläre, die am Sammelplatz über die Stammgarnison hinaus bereitstehen, und
    /// freie Plätze für sie an Bord (die Truppe am Boden braucht ihre für den Abzug).
    pub troops_home: f64,
    pub berths_free: f64,
    /// Darf der Feldzug noch einmal heimkehren und wiederkommen?
    pub sorties_left: bool,
}

/// Warum eine Belagerung aufgehoben wird.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Withdraw {
    /// Der Rückweg ist sonst nicht mehr im Tank: auftanken und wiederkommen.
    Refuel,
    /// Die Welt ist nur mit mehr Truppen zu nehmen: holen und wiederkommen.
    FetchTroops,
    /// Kein Weg führt zum Ziel.
    Hopeless,
    /// Ein überlegener Entsatz ist im Anmarsch.
    Relief,
}

/// Entscheidung eines Tages über eine eigene Belagerung.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SiegeDecision {
    /// Der gewählte Weg; `None`, wenn die Belagerung ruht oder aufgehoben wird.
    pub course: Option<Course>,
    pub orders: SiegeOrders,
    /// Alle Truppen an Bord jetzt landen.
    pub land: bool,
    pub withdraw: Option<Withdraw>,
}

/// Der Weg, dem die stehenden Befehle folgen.
pub fn course_of(orders: &SiegeOrders) -> Course {
    match (orders.fire, orders.assault) {
        (Fire::Area, _) => Course::Area,
        (Fire::Hold, false) => Course::Starve,
        _ => Course::Storm,
    }
}

/// Tage, die die Flotte noch über dem Ziel bleiben kann, ohne dass ihr der Rückweg fehlt.
pub fn days_on_station(data: &GameData, fuel: f64, fuel_home: f64) -> f64 {
    (fuel - fuel_home - FUEL_RESERVE) / data.balance.military.fuel_per_day as f64
}

/// Entscheidet aus der Karte einer eigenen Belagerung, wie es heute weitergeht.
///
/// - Ein Entsatz, dem die Flotte nicht gewachsen ist (`defend_odds`), hebt die
///   Belagerung auf. Ruht sie, weil der Orbit umstritten ist, bleibt alles, wie es ist.
/// - Reicht der Tank nicht mehr für den Rückweg, kehrt die Flotte heim zum Auftanken,
///   wenn ein voller Tank vom nächsten Hafen aus über dem Ziel Zeit für einen der Wege
///   ließe; sonst gibt sie auf.
/// - Sonst gilt unter den drei vorausgerechneten Wegen der billigste, der die Welt
///   nimmt, bevor der Tank zur Neige geht, ohne die Flotte mehr als `hull_limit` ihres
///   Rumpfs und die Truppe mehr als `storm_loss_limit` zu kosten. Die Kosten zählen in
///   Anteilen der Beute: Tage (`day_cost` und Sold und Wartung der Flotte mal
///   `upkeep_weight`, mal `haste`, wenn Entsatz droht), verlorene Truppen, verlorene
///   Beute, verlorener Rumpf. Der laufende Weg gilt um `stick` billiger.
///   Flächenbeschuss scheidet aus, wo die Gemeinschaft selbst viel besitzt.
/// - Führt kein Weg zum Ziel, holt die Flotte Truppen, wenn es an ihnen liegt und zu
///   Hause welche stehen; sonst gibt sie auf.
pub fn decide(data: &GameData, card: &SiegeCard, context: &SiegeContext) -> SiegeDecision {
    let ai = &data.balance.military.ai;
    let (levers, balance) = (&ai.siege, &data.balance.siege);
    let orders = card.view.siege.orders;
    let rest = SiegeDecision {
        course: None,
        orders,
        land: false,
        withdraw: None,
    };
    let leave = |reason: Withdraw| SiegeDecision {
        withdraw: Some(reason),
        ..rest
    };
    if card.handover.is_some() {
        return rest;
    }
    if context.relief_odds < ai.defend_odds {
        return leave(Withdraw::Relief);
    }
    if !card.control {
        return rest;
    }
    let again = |reason: Withdraw| {
        leave(if context.sorties_left {
            reason
        } else {
            Withdraw::Hopeless
        })
    };
    let force = card.troops.army + card.troops.embarked;
    // Was ein voller Tank vom nächsten eigenen Hafen aus über dem Ziel an Zeit ließe, und
    // ob einer der Wege darin zum Ziel führte: Nur dann lohnt die Heimkehr zum Auftanken.
    let full_tank = days_on_station(
        data,
        levers.sail_fuel as f64 - context.fuel_home,
        context.fuel_home,
    );
    let in_reach = card.forecasts.iter().any(|(course, forecast)| {
        forecast.falls
            && forecast.days as f64 <= full_tank
            && forecast.hull_loss <= levers.hull_limit
            && (*course == Course::Starve
                || forecast.own_losses <= balance.storm_loss_limit * force)
    });
    let usable = days_on_station(data, card.fuel, context.fuel_home);
    if usable <= 0.0 {
        return if in_reach {
            again(Withdraw::Refuel)
        } else {
            leave(Withdraw::Hopeless)
        };
    }

    let threatened = context.relief_odds < ai.offensive_odds;
    let day_cost = (levers.day_cost + levers.upkeep_weight * context.upkeep_share.max(0.0))
        * (0.5 + context.boldness.clamp(0.0, 1.0))
        * if threatened { levers.haste } else { 1.0 };
    let running = course_of(&orders);
    let mut best: Option<(Course, f64)> = None;
    for (course, forecast) in &card.forecasts {
        if !forecast.falls
            || forecast.days as f64 > usable
            || forecast.hull_loss > levers.hull_limit
        {
            continue;
        }
        if *course == Course::Area && card.view.siege.stake() > levers.area_stake_max {
            continue;
        }
        if *course != Course::Starve && forecast.own_losses > balance.storm_loss_limit * force {
            continue;
        }
        let mut cost = forecast.days as f64 * day_cost
            + levers.loss_weight * forecast.own_losses / force.max(1.0)
            + levers.prize_weight * forecast.prize_loss
            + levers.hull_weight * forecast.hull_loss;
        if *course == running && card.days > 0 {
            cost *= 1.0 - levers.stick;
        }
        if best.is_none_or(|(_, least)| cost < least) {
            best = Some((*course, cost));
        }
    }
    let Some((course, _)) = best else {
        // Liegt es an den Truppen? Dann holen, wenn zu Hause welche stehen.
        let troops_short = card.forecasts.iter().any(|(course, forecast)| {
            matches!(
                forecast.limited_by,
                Some(Limit::NoTroops) | Some(Limit::Repelled)
            ) || (*course != Course::Starve
                && forecast.falls
                && forecast.own_losses > balance.storm_loss_limit * force)
        });
        if troops_short
            && context.troops_home >= 1.0
            && context.berths_free >= 1.0
            && context.sorties_left
        {
            return leave(Withdraw::FetchTroops);
        }
        // Reichte ein voller Tank? Dann auftanken und wiederkommen.
        if in_reach && usable < full_tank - 1.0 {
            return again(Withdraw::Refuel);
        }
        return leave(Withdraw::Hopeless);
    };

    let invested = card.control && card.defense.stations <= 0.0;
    let aboard = card.troops.embarked > 0.0;
    let ashore = card.troops.army > 0.0;
    // Schwerpunkt, sobald das Umland genommen ist: die Batterie, solange sie feuert,
    // sonst das Zentrum.
    let zone = |kind: ZoneKind| {
        card.view
            .siege
            .zones
            .iter()
            .position(|zone| zone.kind == kind && !zone.taken)
    };
    let focus = if card.defense.guns_active {
        zone(ZoneKind::Battery)
    } else {
        None
    }
    .or_else(|| zone(ZoneKind::Center))
    .unwrap_or(orders.focus as usize) as u8;
    let sack = context.treasury_months < levers.sack_below_months;
    let (fire, assault, land) = match course {
        Course::Starve => (Fire::Hold, false, false),
        Course::Storm => (Fire::Support, true, invested && aboard),
        Course::Area => {
            // Gelandet wird, sobald der Sturm unter dem Beschuss mit tragbaren Verlusten
            // gelingt: dieselbe Probe, mit der die Prognose rechnet. Erst, wenn der
            // Beschuss schon seit dem Vortag gilt: Eine Prognose, die nur einen Tag hielt,
            // trägt keine Landung.
            let land = invested && aboard && running == Course::Area && card.days > 1 && {
                let mut trial = Projection::new(&card.view, balance);
                trial.siege.orders.fire = Fire::Area;
                trial.storm_succeeds(balance)
            };
            (Fire::Area, ashore || land, land)
        }
    };
    SiegeDecision {
        course: Some(course),
        orders: SiegeOrders {
            fire,
            assault,
            focus,
            sack,
        },
        land,
        withdraw: None,
    }
}

// --- Verteidigung -----------------------------------------------------------------------

/// Lage einer eigenen Welt für den Verteidigungsplan.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Exposure {
    Capital,
    /// Ein Kriegsgegner liegt nahe, oder die Welt liegt an einem Engpass an fremdem Gebiet.
    Exposed,
    Interior,
}

/// Bestimmt die Lage einer Welt: Hauptwelt; bedroht, wenn ein System eines Kriegsgegners
/// höchstens `defense_jumps` Sprünge entfernt liegt (`enemy_hops`) oder ihr System ein
/// Engpass ist, der an fremdes Gebiet grenzt (`foreign_hops` höchstens 1); sonst Hinterland.
pub fn exposure(
    data: &GameData,
    capital: bool,
    chokepoint: bool,
    enemy_hops: u32,
    foreign_hops: u32,
) -> Exposure {
    let levers = &data.balance.military.ai.siege;
    if capital {
        Exposure::Capital
    } else if enemy_hops <= levers.defense_jumps || (chokepoint && foreign_hops <= 1) {
        Exposure::Exposed
    } else {
        Exposure::Interior
    }
}

/// Das Ausbauziel der Lage.
pub fn defense_row(data: &GameData, exposure: Exposure) -> &DefenseRow {
    let levers = &data.balance.military.ai.siege;
    match exposure {
        Exposure::Capital => &levers.defense_capital,
        Exposure::Exposed => &levers.defense_exposed,
        Exposure::Interior => &levers.defense_interior,
    }
}

/// Reguläre, die eine Welt nach ihrem Ausbauziel als Stammgarnison hält.
pub fn base_regulars(
    data: &GameData,
    row: &DefenseRow,
    militia: f64,
    war: bool,
    scale: f64,
) -> f64 {
    row.regulars as f64 * militia.max(0.0) * readiness(data, war) * scale.clamp(0.0, 1.0)
}

/// Anteil der Ausbauziele, der gilt: im Krieg das Ganze, im Frieden `defense_peace`.
fn readiness(data: &GameData, war: bool) -> f64 {
    if war {
        1.0
    } else {
        data.balance.military.ai.siege.defense_peace
    }
}

/// Der Verteidigungsplan einer Welt nach ihrem Ausbauziel `row`.
///
/// Anlagen wachsen bis zur Stufe des Ziels (im Frieden nur der Anteil `defense_peace`,
/// gekürzt um `scale`, begrenzt durch die Technik in `caps`); was schon steht, bleibt
/// stehen, außer `dismantle` erlaubt den Abbau. Reguläre: das Ziel mal der Miliz, im
/// Frieden derselbe Anteil, mindestens `hold` (die Garnison, die eine besetzte Welt ruhig
/// hält, soweit sie schon steht), dazu `extra` für einen Feldzug; nie mehr, als die Welt
/// ausheben darf oder schon hält, in ganzen Verbänden. Das Depot wird nicht gekürzt: Es
/// kostet keinen Unterhalt, und ohne Vorräte löst sich die Miliz vom ersten Tag an auf.
pub fn planned_defense(
    data: &GameData,
    row: &DefenseRow,
    caps: [u8; 4],
    defense: &PlanetDefense,
    war: bool,
    scale: f64,
    extra: f64,
    hold: f64,
    dismantle: bool,
) -> DefensePlan {
    let balance = &data.balance.siege;
    let wanted = [row.stations, row.guns, row.shield, row.bunkers];
    let level = |works: Works| {
        let index = works.index();
        let standing = if dismantle {
            0
        } else {
            works.level(defense).ceil() as u8
        };
        let target =
            (wanted[index] as f64 * readiness(data, war) * scale.clamp(0.0, 1.0)).floor() as u8;
        target.max(standing).min(caps[index])
    };
    let most = (balance.regulars_cap * defense.militia).max(defense.regulars);
    let base = base_regulars(data, row, defense.militia, war, scale);
    let regulars = (base.max(hold.min(defense.regulars)) + extra.max(0.0))
        .min(most)
        .max(0.0)
        .floor();
    DefensePlan {
        stations: level(Works::Stations),
        guns: level(Works::Guns),
        shield: level(Works::Shield),
        bunkers: level(Works::Bunkers),
        regulars: regulars as f32,
        depot_days: row.depot_days.min(balance.depot_days_cap),
    }
}

/// Sold und Unterhalt je Monat, wenn der Plan erfüllt ist, in Credits.
pub fn plan_upkeep(data: &GameData, plan: &DefensePlan) -> f64 {
    let planned = PlanetDefense {
        stations: plan.stations as f32,
        guns: plan.guns as f32,
        shield: plan.shield,
        bunkers: plan.bunkers,
        regulars: plan.regulars as f64,
        ..Default::default()
    };
    planet_upkeep(&data.balance.siege, &planned)
}

/// Die größte Stufe aus [`PLAN_SCALES`], bei der die Pläne aller Welten zusammen in den
/// Etat passen; `upkeep` nennt Sold und Unterhalt aller Pläne zu einer Stufe.
pub fn affordable_scale(budget: f64, upkeep: impl Fn(f64) -> f64) -> f64 {
    PLAN_SCALES
        .into_iter()
        .find(|scale| upkeep(*scale) <= budget)
        .unwrap_or(0.0)
}

/// Eine belagerte eigene Welt, wie ihr Besitzer sie sieht.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Plight {
    pub capital: bool,
    /// Vorräte der Garnison in Tagen.
    pub stores_days: f64,
    /// Anteil der Garnison vom Beginn der Belagerung, der noch steht (0..1).
    pub garrison_left: f64,
    /// Verteidiger an der Oberfläche (Miliz und Reserve) und im Untergrund.
    pub surface: f64,
    pub deep: f64,
    /// Gelandete Truppe des Belagerers.
    pub army: f64,
    /// Steht eine Besatzungstruppe bereit, die die Übergabe annehmen kann?
    pub occupation_ready: bool,
    /// Das beste Kräfteverhältnis einer eigenen Flotte gegen die des Belagerers.
    pub relief_odds: f64,
    pub guns: f64,
    pub bunkers: u8,
    /// Hält die Garnison das Zentrum noch?
    pub center_held: bool,
    /// Am Vortag sind Truppen gelandet.
    pub landed_yesterday: bool,
    pub posture: Posture,
}

/// Übergibt der Besitzer die Welt? Nur, wenn jemand da ist, der sie annimmt, kein
/// Entsatz möglich ist (`relief_odds`) und die Gemeinschaft dazu neigt: Ihre
/// Standhaftigkeit liegt unter `capitulate_resolve`, für die Hauptwelt unter
/// `capitulate_capital_resolve`. Dann aus Hunger (die Vorräte sind leer, und von der
/// Garnison steht weniger als `capitulate_garrison` ihres Anfangs), oder wenn die Welt
/// überrannt ist (das Zentrum ist verloren, im Untergrund sitzt niemand, und die
/// gelandete Truppe übertrifft die Verteidiger an der Oberfläche um `capitulate_ratio`).
pub fn should_capitulate(data: &GameData, plight: &Plight, disposition: &Disposition) -> bool {
    let levers = &data.balance.military.ai.siege;
    if !plight.occupation_ready || plight.relief_odds >= levers.relief_odds {
        return false;
    }
    let limit = if plight.capital {
        levers.capitulate_capital_resolve
    } else {
        levers.capitulate_resolve
    };
    if disposition.resolve >= limit {
        return false;
    }
    let starving = plight.stores_days <= 0.0 && plight.garrison_left < levers.capitulate_garrison;
    let overrun = plight.army > 0.0
        && !plight.center_held
        && plight.deep <= 0.0
        && plight.army >= levers.capitulate_ratio * plight.surface;
    starving || overrun
}

/// Haltung der Garnison und ob die Geschütze schweigen.
///
/// In die Tiefe gestaffelt wird, wenn Bunker stehen, das Zentrum gehalten wird und die
/// Verteidiger an der Oberfläche weniger als das `deep_ratio`-Fache der gelandeten
/// Truppe zählen; zurück nach vorn geht es nicht. Die Geschütze schweigen unter
/// `hold_fire_days` Tagen Vorrat, außer am Tag nach einer Landung.
pub fn garrison_orders(data: &GameData, plight: &Plight) -> (Posture, bool) {
    let levers = &data.balance.military.ai.siege;
    let deep = plight.posture == Posture::Deep
        || (plight.bunkers > 0
            && plight.center_held
            && plight.army > 0.0
            && plight.surface < levers.deep_ratio * plight.army);
    let hold_fire =
        plight.guns > 0.0 && plight.stores_days < levers.hold_fire_days && !plight.landed_yesterday;
    (
        if deep {
            Posture::Deep
        } else {
            Posture::Forward
        },
        hold_fire,
    )
}

// --- Führung in der Partie --------------------------------------------------------------

fn refs<'a>(sit: &Situation<'a>) -> SiegeRefs<'a> {
    SiegeRefs {
        data: sit.data,
        galaxy: sit.galaxy,
        polities: sit.polities,
        diplomacy: sit.diplomacy,
        economy: sit.economy,
        finance: sit.finance,
        logistics: sit.logistics,
        fleets: sit.fleets,
        research: sit.research,
        sieges: sit.sieges,
    }
}

/// Bauwert der Flotten der Gegner je System, geteilt durch den eigenen Vorsprung der
/// Technik; wer marschiert, zählt am nächsten System seines Wegs, wer sich sammelt, nicht.
fn foes_by_system(
    sit: &Situation,
    id: PolityId,
    enemies: &[PolityId],
) -> BTreeMap<SystemId, ClassValues> {
    let mut foes: BTreeMap<SystemId, ClassValues> = BTreeMap::new();
    let mut edges: BTreeMap<PolityId, f64> = BTreeMap::new();
    for sighting in sightings(sit.data, sit.fleets, id) {
        if !enemies.contains(&sighting.owner) || sighting.regrouping {
            continue;
        }
        let edge = *edges
            .entry(sighting.owner)
            .or_insert_with(|| tech_edge(sit.data, sit.research, id, sighting.owner));
        let mut values = sighting.values;
        for value in &mut values {
            *value /= edge;
        }
        add(foes.entry(sighting.position()).or_default(), &values);
    }
    foes
}

/// Was im Umkreis von `response_jumps` Sprüngen um ein System an Gegnern steht.
fn foes_around(
    sit: &Situation,
    foes: &BTreeMap<SystemId, ClassValues>,
    system: SystemId,
) -> ClassValues {
    let jumps = sit.data.balance.military.ai.response_jumps;
    let hops = sit.galaxy.hops_from(system);
    let mut around = [0.0; 6];
    for (at, values) in foes {
        if hops[at.index()] <= jumps {
            add(&mut around, values);
        }
    }
    around
}

/// Eigene Häfen: Systeme, in denen die Gemeinschaft heute Vorräte fassen kann.
fn bases(sit: &Situation, id: PolityId) -> Vec<SystemId> {
    sit.galaxy
        .systems
        .iter()
        .map(|system| system.id)
        .filter(|system| {
            supply_base(sit.galaxy, sit.diplomacy, id, *system) && !sit.fleets.is_blockaded(*system)
        })
        .collect()
}

/// Routenlänge von jedem System zum nächsten der genannten Häfen.
fn way_to_bases(sit: &Situation, bases: &[SystemId]) -> Vec<f32> {
    let mut nearest = vec![f32::INFINITY; sit.galaxy.systems.len()];
    for base in bases {
        let (lengths, _) = sit.galaxy.distances_from(*base, |_| true);
        for (slot, length) in nearest.iter_mut().zip(lengths) {
            *slot = slot.min(length);
        }
    }
    nearest
}

/// Der eigene Hafen, der einem System auf dem Routennetz am nächsten liegt.
fn last_port(sit: &Situation, bases: &[SystemId], system: SystemId) -> Option<SystemId> {
    let (lengths, _) = sit.galaxy.distances_from(system, |_| true);
    bases
        .iter()
        .copied()
        .filter(|base| lengths[base.index()].is_finite())
        .min_by(|a, b| {
            lengths[a.index()]
                .total_cmp(&lengths[b.index()])
                .then(a.cmp(b))
        })
}

/// Lage einer eigenen Welt nach den Sprüngen zu Kriegsgegnern und fremdem Gebiet.
fn exposure_of(sit: &Situation, id: PolityId, planet: PlanetId) -> Exposure {
    let world = sit.galaxy.planet(planet);
    let capital = sit.polities.get(id).capital == Some(planet);
    if capital {
        return Exposure::Capital;
    }
    let hops = sit.galaxy.hops_from(world.system);
    let (mut enemy, mut foreign) = (u32::MAX, u32::MAX);
    for system in &sit.galaxy.systems {
        let Some(owner) = sit.galaxy.system_owner(system.id) else {
            continue;
        };
        if owner == id {
            continue;
        }
        let away = hops[system.index()];
        foreign = foreign.min(away);
        if sit.diplomacy.at_war(id, owner) {
            enemy = enemy.min(away);
        }
    }
    exposure(
        sit.data,
        false,
        sit.galaxy.system(world.system).chokepoint,
        enemy,
        foreign,
    )
}

/// Stammgarnison, die eine Welt nach ihrem Ausbauziel hält (ungekürzt).
fn home_guard(sit: &Situation, id: PolityId, planet: PlanetId) -> f64 {
    let row = defense_row(sit.data, exposure_of(sit, id, planet));
    base_regulars(
        sit.data,
        row,
        sit.sieges.defense(planet).militia,
        sit.diplomacy.is_at_war(id),
        1.0,
    )
}

/// Reguläre, die am Sammelplatz über den Teil der Stammgarnison hinaus bereitstehen, der
/// beim Einschiffen bleibt.
fn troops_ready(sit: &Situation, id: PolityId, staging: PlanetId) -> f64 {
    let keep = sit.data.balance.military.ai.siege.garrison_keep * home_guard(sit, id, staging);
    (sit.sieges.defense(staging).regulars - keep).max(0.0)
}

fn aboard(sit: &Situation, fleet: &Fleet) -> f64 {
    sit.sieges
        .embarked
        .get(&fleet.id)
        .map_or(0.0, |force| force.troops)
}

/// Die Flotte, mit der die Gemeinschaft einen Feldzug führen kann: ihre wertvollste, die
/// weder belagert noch kämpft und ohne Marschbefehl in einem eigenen Hafen liegt.
fn expedition_fleet<'a>(
    sit: &'a Situation,
    ai: &AiState,
    id: PolityId,
    bases: &[SystemId],
) -> Option<&'a Fleet> {
    let main = sit
        .fleets
        .of(id)
        .filter(|fleet| !ai.is_reserved(fleet.id))
        .max_by(|a, b| {
            a.build_value(sit.data)
                .total_cmp(&b.build_value(sit.data))
                .then(b.id.cmp(&a.id))
        })?;
    (main.is_ready() && main.route.is_empty() && bases.contains(&main.system)).then_some(main)
}

/// Tagesbeginn, vor der Führung der Flotten: Jede KI-Gemeinschaft führt ihre Feldzüge
/// und die Verteidigung ihrer belagerten Welten.
pub fn lead_sieges(
    sit: &Situation,
    ai: &mut AiState,
    queue: &mut CommandQueue,
    player: Option<PolityId>,
) {
    for (id, _) in led(sit, player) {
        let mode = ai.mode(id);
        if mode == MilitaryMode::Resting {
            continue;
        }
        let character = disposition(sit.seed, id);
        let mut hand = Hand::new(queue, id);
        defend_besieged(sit, &mut hand, id, &character);
        if mode != MilitaryMode::Active {
            continue;
        }
        let mut plan = ai.plan(id);
        plan.spurned.retain(|_, until| sit.day < *until);
        let enemies: Vec<PolityId> = sit.diplomacy.enemies_of(id).collect();
        if enemies.is_empty() {
            plan.searching_since = None;
        }
        if let Some(campaign) = plan.campaign {
            let fleet = sit
                .fleets
                .get(campaign.fleet)
                .filter(|fleet| fleet.alive && fleet.owner == id);
            // Die Welt ist gefallen oder kein Ziel mehr, der Sammelplatz verloren.
            let sound = fleet.is_some()
                && sit.galaxy.planet(campaign.staging).owner == Some(id)
                && sit
                    .galaxy
                    .planet(campaign.target)
                    .owner
                    .is_some_and(|owner| enemies.contains(&owner));
            if !sound {
                match fleet {
                    Some(fleet) => release(sit, &mut plan, &mut hand, id, fleet),
                    None => {
                        plan.campaign = None;
                        plan.campaign_end = sit.day;
                    }
                }
            }
        }
        if plan.campaign.is_some() {
            run_campaign(sit, &mut plan, &mut hand, id, &enemies, &character);
        } else if !enemies.is_empty()
            && sit.day >= plan.no_offensive_before
            && (sit.day + id.index() as u64).is_multiple_of(TARGET_EVERY)
        {
            start_campaign(sit, ai, &mut plan, id, &enemies);
        }
        land_idle_troops(sit, &plan, &mut hand, id, enemies.is_empty());
        if ai.plan(id) != plan {
            *ai.plan_mut(id) = plan;
        }
    }
}

/// Eine Welt des Gegners, wie die Führung sie heute als Ziel sieht: Weg vom nächsten
/// eigenen Hafen (`ways`), Kräfteverhältnis der Flotte (`own`) gegen die Gegner im Umkreis
/// (`foes`), Schätzung der Verteidigung.
fn target_facts(
    sit: &Situation,
    id: PolityId,
    planet: PlanetId,
    ways: &[f32],
    own: &ClassValues,
    foes: &BTreeMap<SystemId, ClassValues>,
) -> TargetFacts {
    let data = sit.data;
    let world = sit.galaxy.planet(planet);
    TargetFacts {
        planet,
        capital: world
            .owner
            .is_some_and(|owner| sit.polities.get(owner).capital == Some(planet)),
        value: prize_value(sit, planet),
        population: world.population,
        route_fuel: ways[world.system.index()] as f64
            * data.balance.military.fuel_per_distance as f64,
        odds: odds(data, own, &foes_around(sit, foes, world.system), 1.0),
        estimate: world_estimate(data, sit.galaxy, sit.sieges, sit.seed, id, planet),
    }
}

/// Wert der Beute einer Welt in Credits: ihre Betriebe und ihre Bevölkerung.
fn prize_value(sit: &Situation, planet: PlanetId) -> f64 {
    firm_value(sit.data, sit.galaxy, sit.economy, sit.logistics, planet)
        + sit.data.balance.siege.prize_per_million * sit.galaxy.planet(planet).population
}

/// Verteidigung zuerst: ein System, das der Gemeinschaft gehört (nach der Mehrheit
/// seiner Bevölkerung) und in dem ein Gegner steht. Solange es eines gibt, bleibt die
/// Hauptflotte. Eine eigene Welt in fremdem Gebiet macht das System nicht zum eigenen:
/// Dass der Gegner in seinem eigenen System liegt, hält keinen Feldzug auf.
fn intruder_at_home(
    sit: &Situation,
    id: PolityId,
    foes: &BTreeMap<SystemId, ClassValues>,
) -> Option<SystemId> {
    foes.keys()
        .copied()
        .find(|system| sit.galaxy.system_owner(*system) == Some(id))
}

/// Sucht die Führung weiter? Ja, wenn sie seit `widen_days` Tagen kein Ziel findet.
pub fn wide_search(data: &GameData, searching_since: Option<u64>, day: u64) -> bool {
    let levers = &data.balance.military.ai.siege;
    searching_since.is_some_and(|since| day >= since + levers.widen_days)
}

/// So lange ruht eine Welt, an der ein Feldzug gescheitert ist: `retry_days`, nach jedem
/// früheren Scheitern am selben Ziel einmal mehr.
pub fn retry_ban(data: &GameData, failures: u32) -> u64 {
    data.balance.military.ai.siege.retry_days * (1 + failures as u64)
}

/// Etat der Verteidigung im Monat, in Credits: `defense_share` der Einnahmen (im Krieg mal
/// `war_factor`). Es zählt das lange Mittel `usual`, aber höchstens das Mittel der
/// letzten Monate `recent`: Wer Gebiet verloren hat, nimmt weniger ein, und das lange
/// Mittel folgt erst nach Jahren.
pub fn defense_budget(data: &GameData, usual: f64, recent: &[f64], war: bool) -> f64 {
    let levers = &data.balance.military.ai;
    let revenue = if recent.is_empty() {
        usual
    } else {
        usual.min(recent.iter().sum::<f64>() / recent.len() as f64)
    };
    levers.siege.defense_share * revenue * if war { levers.war_factor } else { 1.0 }
}

/// Beginnt einen Feldzug, wenn die Hauptflotte im Hafen liegt, kein Gegner im eigenen
/// Gebiet steht, eine Welt des Gegners als Ziel taugt und am Sammelplatz wenigstens die
/// Besatzungstruppe für sie bereitsteht. Findet die Führung `widen_days` lang kein Ziel,
/// sucht sie weiter ([`target_score`]).
fn start_campaign(
    sit: &Situation,
    ai: &AiState,
    plan: &mut MilitaryPlan,
    id: PolityId,
    enemies: &[PolityId],
) {
    let data = sit.data;
    let levers = &data.balance.military.ai.siege;
    let Some(staging) = sit.polities.get(id).seat(sit.galaxy) else {
        return;
    };
    let ports = bases(sit, id);
    if !ports.contains(&sit.galaxy.planet(staging).system) {
        return;
    }
    let Some(fleet) = expedition_fleet(sit, ai, id, &ports) else {
        return;
    };
    let foes = foes_by_system(sit, id, enemies);
    if intruder_at_home(sit, id, &foes).is_some() {
        return;
    }
    let force = Force {
        berths: berths_of(data, fleet),
        firepower: fleet_firepower(data, fleet),
    };
    let own = own_values(data, fleet);
    let ways = way_to_bases(sit, &ports);
    let wide = wide_search(data, plan.searching_since, sit.day);
    let candidates: Vec<TargetFacts> = sit
        .galaxy
        .planets
        .iter()
        .filter(|world| world.owner.is_some_and(|owner| enemies.contains(&owner)))
        .filter(|world| !plan.spurned.contains_key(&world.id))
        .filter(|world| {
            sit.sieges
                .siege(world.id)
                .is_none_or(|siege| siege.attacker == id)
        })
        .filter(|world| ways[world.system.index()].is_finite())
        .map(|world| target_facts(sit, id, world.id, &ways, &own, &foes))
        .map(|mut facts| {
            // Ein Krieg mit dem Ziel der Unterwerfung sucht die Hauptwelt des Gegners.
            let owner = sit.galaxy.planet(facts.planet).owner;
            let subjugating = owner.and_then(|owner| sit.diplomacy.war_between(id, owner)).is_some_and(
                |war| war.attacker == id && war.aim == crate::diplomacy::WarAim::Subjugation,
            );
            if facts.capital && subjugating {
                facts.value *= data.diplomacy.levers.aim_capital_bonus;
            }
            facts
        })
        .collect();
    let Some((target, troops)) = choose_target(data, &candidates, &force, wide) else {
        plan.searching_since.get_or_insert(sit.day);
        return;
    };
    // Ohne die Besatzungstruppe am Sammelplatz wartet der Feldzug nur: Erst wenn sie
    // ausgehoben ist, beginnt er.
    let least = levers.occupation_margin
        * occupation_force(sit.galaxy.planet(target).population, &data.balance.siege);
    if troops_ready(sit, id, staging) + aboard(sit, fleet) < least {
        return;
    }
    plan.searching_since = None;
    plan.operations.remove(&fleet.id);
    plan.campaigns += 1;
    plan.campaign = Some(Campaign {
        target,
        staging,
        fleet: fleet.id,
        stage: CampaignStage::Muster,
        started_day: sit.day,
        stage_day: sit.day,
        troops,
        sorties: 0,
        wide,
    });
}

/// Taugt das Ziel des Feldzugs noch, gemessen an den Häfen, der Schätzung und den
/// Gegnern von heute? Gezählt werden die Truppen, die die Flotte an Bord hat: Für einen
/// Sturm müssen sie reichen, sonst der Tank zum Aushungern oder der Beschuss.
fn still_worth(
    sit: &Situation,
    id: PolityId,
    enemies: &[PolityId],
    campaign: &Campaign,
    fleet: &Fleet,
    troops: f64,
) -> bool {
    let data = sit.data;
    let foes = foes_by_system(sit, id, enemies);
    let ways = way_to_bases(sit, &bases(sit, id));
    let facts = target_facts(
        sit,
        id,
        campaign.target,
        &ways,
        &own_values(data, fleet),
        &foes,
    );
    let force = Force {
        berths: troops,
        firepower: fleet_firepower(data, fleet),
    };
    target_score(data, &facts, &force, campaign.wide).is_some()
}

/// Beendet den Feldzug und gibt die Flotte der Grundhaltung zurück: Sie läuft den
/// nächsten eigenen Hafen an, statt über dem Ziel liegen zu bleiben, bis ihr Tank leer
/// ist.
fn release(sit: &Situation, plan: &mut MilitaryPlan, hand: &mut Hand, id: PolityId, fleet: &Fleet) {
    plan.campaign = None;
    plan.campaign_end = sit.day;
    hand.set_stance(fleet, Stance::Escort);
    if let Some(port) = last_port(sit, &bases(sit, id), fleet.system) {
        hand.move_fleet(fleet, port);
    }
}

/// Führt den laufenden Feldzug einen Tag weiter.
fn run_campaign(
    sit: &Situation,
    plan: &mut MilitaryPlan,
    hand: &mut Hand,
    id: PolityId,
    enemies: &[PolityId],
    character: &Disposition,
) {
    let data = sit.data;
    let ai = &data.balance.military.ai;
    let levers = &ai.siege;
    let Some(mut campaign) = plan.campaign else {
        return;
    };
    let Some(fleet) = sit.fleets.get(campaign.fleet) else {
        plan.campaign = None;
        plan.campaign_end = sit.day;
        return;
    };
    // Im Gefecht und beim Sammeln nimmt die Flotte keine Befehle an.
    if !fleet.is_ready() {
        return;
    }
    let target = sit.galaxy.planet(campaign.target);
    let staging = sit.galaxy.planet(campaign.staging);
    let foes = foes_by_system(sit, id, enemies);
    let own = own_values(data, fleet);
    let odds_at = |system: SystemId| odds(data, &own, &foes_around(sit, &foes, system), 1.0);
    let docked_in = |system: SystemId| fleet.route.is_empty() && fleet.system == system;
    let troops = aboard(sit, fleet);
    // Aufgeben: Mit `ban` liegt es am Ziel, und es ruht `retry_days`, nach jedem weiteren
    // Scheitern ein Vielfaches davon; mit `cautious` beginnt so bald kein Angriff.
    let give_up = |plan: &mut MilitaryPlan, hand: &mut Hand, cautious: bool, ban: bool| {
        release(sit, plan, hand, id, fleet);
        if ban {
            let failures = plan.failures.entry(campaign.target).or_insert(0);
            plan.spurned
                .insert(campaign.target, sit.day + retry_ban(data, *failures));
            *failures += 1;
        }
        if cautious {
            plan.no_offensive_before = sit.day + ai.caution_days;
        }
    };
    let waited = sit.day.saturating_sub(campaign.stage_day);

    match campaign.stage {
        CampaignStage::Muster => {
            hand.set_stance(fleet, Stance::Escort);
            if !docked_in(staging.system) {
                hand.move_fleet(fleet, staging.system);
                return;
            }
            let strength = embark_strength(
                campaign.troops,
                troops,
                berths_of(data, fleet),
                sit.sieges.defense(staging.id).regulars,
                levers.garrison_keep * home_guard(sit, id, staging.id),
            );
            // Eine eingeschlossene Welt gibt keine Truppen her.
            let shut = sit.sieges.siege(staging.id).is_some();
            if strength > 0.0 && !shut {
                hand.embark(fleet, staging.id, strength, levers.supply_days);
            }
            let have = troops + if shut { 0.0 } else { strength };
            let least =
                levers.occupation_margin * occupation_force(target.population, &data.balance.siege);
            let (ammo, _) = fleet.supply(data);
            let supplied = fleet.min_fuel() >= ai.ready_supply && ammo >= ai.ready_supply;
            let manned = have >= campaign.troops - 0.5
                || (waited >= levers.muster_days / 2 && have >= least);
            let safe = odds_at(target.system) >= levers.odds;
            if supplied && manned && safe && strength <= 0.0 {
                // Bevor sie ausläuft: Taugt das Ziel noch, von den Häfen von heute aus?
                if !still_worth(sit, id, enemies, &campaign, fleet, have) {
                    give_up(plan, hand, false, true);
                    return;
                }
                campaign.stage = CampaignStage::March;
                campaign.stage_day = sit.day;
                hand.set_stance(fleet, Stance::Blockade);
                hand.set_orders(fleet, fleet.formation, Doctrine::Balanced);
            } else if waited >= levers.muster_days {
                // Truppen, Treibstoff oder die Lage am Ziel kommen nicht zusammen. Am Ziel
                // liegt das nicht: Es bleibt wählbar.
                give_up(plan, hand, false, false);
                return;
            }
        }
        CampaignStage::March => {
            if docked_in(target.system) {
                campaign.stage = CampaignStage::Siege;
                campaign.stage_day = sit.day;
                hand.begin_siege(sit.sieges, fleet, target.id);
            } else if odds_at(target.system) < ai.defend_odds {
                // Am Ziel steht inzwischen, wem die Flotte nicht gewachsen ist.
                give_up(plan, hand, true, true);
                return;
            } else if destination(fleet) != target.system {
                // Der Weg führt über den eigenen Hafen, der dem Ziel am nächsten liegt:
                // Dort fasst die Flotte noch einmal Treibstoff, damit über dem Ziel
                // bleibt, was die Zielwahl versprochen hat.
                let gate = last_port(sit, &bases(sit, id), target.system);
                match gate {
                    Some(gate) if !docked_in(gate) => {
                        hand.move_fleet(fleet, gate);
                    }
                    Some(_) if fleet.min_fuel() < levers.sail_fuel => {}
                    _ => {
                        // Den Hafen verlässt sie nur, wenn das Ziel von dort aus noch in
                        // Reichweite liegt und mit den Truppen an Bord zu nehmen ist:
                        // Ging unterwegs ein näherer Hafen verloren, wäre es das nicht.
                        if !still_worth(sit, id, enemies, &campaign, fleet, troops) {
                            give_up(plan, hand, false, true);
                            return;
                        }
                        hand.move_fleet(fleet, target.system);
                    }
                }
            }
        }
        CampaignStage::Siege => {
            let home = |campaign: &mut Campaign| {
                campaign.stage = CampaignStage::Muster;
                campaign.stage_day = sit.day;
                campaign.sorties += 1;
            };
            if !docked_in(target.system) {
                // Aus dem Orbit gedrängt: sammeln und wiederkommen, solange es erlaubt ist.
                if campaign.sorties < levers.sorties {
                    home(&mut campaign);
                } else {
                    give_up(plan, hand, true, true);
                    return;
                }
            } else if sit
                .sieges
                .siege(target.id)
                .is_some_and(|siege| siege.attacker != id)
            {
                // Ein anderer belagert die Welt schon.
                give_up(plan, hand, false, true);
                return;
            } else if sit.sieges.assign.get(&fleet.id) != Some(&target.id) {
                if fleet.has_fuel() && waited < OPENING_DAYS {
                    hand.begin_siege(sit.sieges, fleet, target.id);
                } else {
                    give_up(plan, hand, false, true);
                    return;
                }
            } else if let Some(card) = siege_card(&refs(sit), target.id, sit.day) {
                let ports = bases(sit, id);
                let way = way_to_bases(sit, &ports)[target.system.index()] as f64;
                let claim = to_credits(fleet_claim(
                    data,
                    sit.galaxy,
                    sit.economy,
                    sit.fleets,
                    sit.polities.get(id),
                ));
                let military = &data.balance.military;
                let prize = prize_value(sit, target.id);
                let context = SiegeContext {
                    fuel_home: if way.is_finite() {
                        way * military.fuel_per_distance as f64
                    } else {
                        0.0
                    },
                    relief_odds: odds_at(target.system),
                    boldness: character.boldness,
                    upkeep_share: if prize > 0.0 {
                        fleet.build_value(data)
                            * (military.upkeep_per_month + military.maintenance_per_month)
                            / DAYS_PER_MONTH as f64
                            / prize
                    } else {
                        0.0
                    },
                    treasury_months: if claim > 0.0 {
                        to_credits(sit.economy.treasury(id)) / claim
                    } else {
                        f64::INFINITY
                    },
                    troops_home: troops_ready(sit, id, staging.id),
                    berths_free: berths_of(data, fleet) - troops - card.troops.army,
                    sorties_left: campaign.sorties < levers.sorties,
                };
                let decision = decide(data, &card, &context);
                match decision.withdraw {
                    Some(reason) => {
                        // Mit Truppen am Boden braucht der Abzug die Orbitalhoheit.
                        if card.troops.army > 0.0 && !card.control {
                            return;
                        }
                        hand.lift_siege(target.id);
                        match reason {
                            Withdraw::Refuel => home(&mut campaign),
                            Withdraw::FetchTroops => {
                                home(&mut campaign);
                                // Diesmal so viel, wie an Bord passt.
                                campaign.troops = berths_of(data, fleet);
                            }
                            Withdraw::Hopeless => {
                                give_up(plan, hand, false, true);
                                return;
                            }
                            Withdraw::Relief => {
                                give_up(plan, hand, true, true);
                                return;
                            }
                        }
                    }
                    None => {
                        hand.siege_orders(sit.sieges, target.id, decision.orders);
                        if decision.land && troops > 0.0 {
                            hand.land(fleet, target.id, troops);
                        }
                    }
                }
            } else if waited >= OPENING_DAYS {
                // Die Belagerung kommt nicht zustande (kein Treibstoff, Orbit umstritten).
                give_up(plan, hand, true, true);
                return;
            }
        }
    }
    plan.campaign = Some(campaign);
}

/// Truppen, die noch an Bord einer Flotte ohne Feldzug sind, gehen in einem eigenen
/// Hafen wieder an Land: im Frieden sofort, im Krieg `land_after_days` nach dem Ende des
/// letzten Feldzugs. Bis dahin warten sie auf einen neuen; danach fehlen sie der
/// Verteidigung, und ihr Sold liefe für nichts.
fn land_idle_troops(
    sit: &Situation,
    plan: &MilitaryPlan,
    hand: &mut Hand,
    id: PolityId,
    peace: bool,
) {
    let levers = &sit.data.balance.military.ai.siege;
    let idle =
        peace || (plan.campaign.is_none() && sit.day >= plan.campaign_end + levers.land_after_days);
    if !idle || sit.sieges.embarked.is_empty() {
        return;
    }
    for fleet in sit.fleets.of(id) {
        let troops = aboard(sit, fleet);
        if troops <= 0.0
            || !fleet.is_ready()
            || !fleet.route.is_empty()
            || sit.sieges.assign.contains_key(&fleet.id)
            || plan
                .campaign
                .is_some_and(|campaign| campaign.fleet == fleet.id)
        {
            continue;
        }
        // Die volkreichste eigene Welt im System nimmt sie auf.
        let harbour = sit
            .galaxy
            .planets_in(fleet.system)
            .filter(|world| world.owner == Some(id) && sit.sieges.siege(world.id).is_none())
            .max_by(|a, b| a.population.total_cmp(&b.population).then(b.id.cmp(&a.id)));
        if let Some(world) = harbour {
            hand.land(fleet, world.id, troops);
        }
    }
}

/// Befehle für die belagerten eigenen Welten: Haltung, Feuer, Übergabe.
fn defend_besieged(sit: &Situation, hand: &mut Hand, id: PolityId, character: &Disposition) {
    let data = sit.data;
    let besieged: Vec<PlanetId> = sit
        .sieges
        .active
        .iter()
        .filter(|(_, siege)| siege.defender == id)
        .map(|(planet, _)| *planet)
        .collect();
    if besieged.is_empty() {
        return;
    }
    let capital = sit.polities.get(id).capital;
    for planet in besieged {
        if sit.sieges.pending.iter().any(|due| due.planet == planet) {
            continue;
        }
        // Die Lage ohne die Prognosen der Karte: Der Verteidiger braucht sie nicht.
        let Some(view) = siege_view(&refs(sit), planet, sit.day) else {
            continue;
        };
        let (siege, defense) = (&view.siege, &view.defense);
        let system = sit.galaxy.planet(planet).system;
        // Was der Belagerer über der Welt zeigt, gegen die beste eigene Flotte.
        let mut besiegers: ClassValues = [0.0; 6];
        let edge = tech_edge(data, sit.research, id, siege.attacker);
        for sighting in sightings(data, sit.fleets, id) {
            if sighting.owner == siege.attacker
                && sighting.position() == system
                && !sighting.regrouping
            {
                let mut values = sighting.values;
                for value in &mut values {
                    *value /= edge;
                }
                add(&mut besiegers, &values);
            }
        }
        let relief_odds = sit
            .fleets
            .of(id)
            .filter(|fleet| !fleet.is_regrouping())
            .map(|fleet| odds(data, &own_values(data, fleet), &besiegers, 1.0))
            .fold(0.0, f64::max);
        let surface = siege.zone_militia() + siege.reserve(defense);
        let ready = siege.army
            + if view.orbit.control {
                view.orbit.embarked
            } else {
                0.0
            };
        let needed = occupation_force(sit.galaxy.planet(planet).population, &data.balance.siege);
        let plight = Plight {
            capital: capital == Some(planet),
            stores_days: defense.depot,
            garrison_left: if siege.garrison_start > 0.0 {
                ((surface + siege.deep) / siege.garrison_start).clamp(0.0, 1.0)
            } else {
                0.0
            },
            surface,
            deep: siege.deep,
            army: siege.army,
            occupation_ready: ready > 0.0 && ready >= needed,
            relief_odds,
            guns: defense.guns as f64,
            bunkers: defense.bunkers,
            center_held: !siege.zone_taken(ZoneKind::Center),
            landed_yesterday: siege.landed_day.is_some_and(|day| day + 1 >= sit.day),
            posture: defense.posture,
        };
        if should_capitulate(data, &plight, character) {
            hand.capitulate(planet);
            continue;
        }
        let (posture, hold_fire) = garrison_orders(data, &plight);
        hand.garrison_orders(defense, planet, posture, hold_fire);
    }
}

/// Monatsbeginn, nach der Rüstung: Jede KI-Gemeinschaft setzt die Verteidigungspläne
/// ihrer Welten.
///
/// Je Welt gilt das Ausbauziel ihrer Lage ([`exposure`]). Die Hauptwelt hält im Krieg
/// zusätzlich die Truppen für einen Feldzug bereit: so viele, wie dem laufenden fehlen,
/// sonst für `expedition_share` der Plätze der Hauptflotte. Kosten Sold und Unterhalt
/// aller Pläne mehr als `defense_share` der Einnahmen (im Krieg mal `war_factor`),
/// werden alle Pläne in Stufen gekürzt. Es zählen die Einnahmen auf lange Sicht, aber
/// höchstens die der letzten drei Monate: Wer Gebiet verloren hat, nimmt weniger ein.
/// Trägt der Etat schon nicht, was steht, plant die KI nach `dismantle_after_months`
/// Monaten in Folge auch stehende Anlagen ab. Welten unter Belagerung und solche, deren
/// Plan ein Drehbuch gesetzt hat, bleiben, wie sie sind.
pub fn plan_defenses(
    sit: &Situation,
    ai: &mut AiState,
    queue: &mut CommandQueue,
    player: Option<PolityId>,
) {
    let data = sit.data;
    let levers = &data.balance.military.ai;
    let balance = &data.balance.siege;
    for (id, _) in led(sit, player) {
        let mode = ai.mode(id);
        if mode == MilitaryMode::Resting {
            continue;
        }
        let mut plan = ai.plan(id);
        // Erst planen, wenn die Einnahmen bekannt sind.
        if plan.usual_revenue <= 0.0 {
            continue;
        }
        let war = sit.diplomacy.is_at_war(id);
        let caps = level_caps(balance, sit.research, id);
        let seat = sit.polities.get(id).seat(sit.galaxy);
        // Truppen für den Feldzug: was dem laufenden fehlt, sonst ein Anteil der Plätze
        // der wertvollsten Flotte; an Bord und am Boden zählt mit.
        let fielded: f64 = sit
            .fleets
            .of(id)
            .map(|fleet| aboard(sit, fleet))
            .sum::<f64>()
            + sit
                .sieges
                .active
                .values()
                .filter(|siege| siege.attacker == id)
                .map(|siege| siege.army)
                .sum::<f64>();
        let expedition = if war && mode == MilitaryMode::Active {
            let lift = plan.campaign.map_or_else(
                || {
                    levers.siege.expedition_share
                        * sit
                            .fleets
                            .of(id)
                            .map(|fleet| berths_of(data, fleet))
                            .fold(0.0, f64::max)
                },
                |campaign| campaign.troops,
            );
            (lift - fielded).max(0.0)
        } else {
            0.0
        };
        let worlds: Vec<(PlanetId, Exposure, f64)> = sit
            .galaxy
            .planets_of(id)
            .map(|world| {
                let defense = sit.sieges.defense(world.id);
                // Eine besetzte Welt hält die Garnison, die sie ruhig hält.
                let hold = if defense.occupied_by == Some(id) && defense.occupation > 0.0 {
                    balance.hold_k * world.population.max(0.0).sqrt()
                } else {
                    0.0
                };
                (world.id, exposure_of(sit, id, world.id), hold)
            })
            .collect();
        let dismantle = plan.defense_over_months >= levers.siege.dismantle_after_months;
        let plans_at = |scale: f64, dismantle: bool| -> Vec<(PlanetId, DefensePlan)> {
            worlds
                .iter()
                .map(|(planet, exposure, hold)| {
                    let extra = if Some(*planet) == seat {
                        expedition
                    } else {
                        0.0
                    };
                    (
                        *planet,
                        planned_defense(
                            data,
                            defense_row(data, *exposure),
                            caps,
                            sit.sieges.defense(*planet),
                            war,
                            scale,
                            extra,
                            *hold,
                            dismantle,
                        ),
                    )
                })
                .collect()
        };
        let upkeep_at = |scale: f64, dismantle: bool| -> f64 {
            plans_at(scale, dismantle)
                .iter()
                .map(|(_, plan)| plan_upkeep(data, plan))
                .sum()
        };
        let recent: Vec<f64> = sit.economy.budget(id).map_or_else(Vec::new, |budget| {
            budget
                .history
                .iter()
                .rev()
                .take(3)
                .map(|month| to_credits(month.revenue))
                .collect()
        });
        let budget = defense_budget(data, plan.usual_revenue, &recent, war);
        let scale = affordable_scale(budget, |scale| upkeep_at(scale, dismantle));
        // Monate, in denen schon das, was steht, den Etat übersteigt.
        let over = upkeep_at(0.0, false) > budget;
        let over_months = if over {
            plan.defense_over_months.saturating_add(1)
        } else {
            0
        };
        if plan.defense_over_months != over_months {
            plan.defense_over_months = over_months;
            *ai.plan_mut(id) = plan;
        }
        let mut hand = Hand::new(queue, id);
        for (planet, wanted) in plans_at(scale, dismantle) {
            if ai.defense_hold.contains(&planet) || sit.sieges.siege(planet).is_some() {
                continue;
            }
            hand.defense_plan(sit.sieges.defense(planet), planet, wanted);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::{Campaign, CampaignStage};
    use crate::ids::FleetId;
    use crate::siege::Embarked;
    use crate::siege::orbit::testing::Scene;

    fn data() -> GameData {
        GameData::load_workspace().unwrap()
    }

    fn estimate(militia: f64, regulars: f64, depot: f64) -> WorldEstimate {
        WorldEstimate {
            stations: 0.0,
            guns: 0.0,
            shield: 0,
            militia,
            regulars: (regulars, regulars),
            depot_days: (depot, depot),
        }
    }

    fn colony(planet: u32) -> TargetFacts {
        TargetFacts {
            planet: PlanetId(planet),
            capital: false,
            value: 10_000.0,
            population: 100.0,
            route_fuel: 0.1,
            odds: f64::INFINITY,
            estimate: estimate(5.0, 2.0, 30.0),
        }
    }

    const FLEET: Force = Force {
        berths: 17.5,
        firepower: 130.0,
    };

    #[test]
    fn the_target_is_the_most_valuable_world_that_can_be_reached_and_taken() {
        let data = data();
        let levers = &data.balance.military.ai.siege;
        let near = colony(1);
        assert!(target_score(&data, &near, &FLEET, false).is_some());
        // Außer Reichweite, gegen eine überlegene Flotte oder ohne Platz für die
        // Besatzungstruppe kommt eine Welt nicht in Frage.
        let far = TargetFacts {
            route_fuel: levers.reach as f64 + 0.01,
            ..near
        };
        let guarded = TargetFacts {
            odds: levers.odds - 0.01,
            ..near
        };
        let huge = TargetFacts {
            population: 40_000.0,
            ..near
        };
        for rejected in [far, guarded, huge] {
            assert_eq!(target_score(&data, &rejected, &FLEET, false), None);
        }
        // Stationen, die die Flotte nicht in der Frist niederkämpft.
        let mut stations = near;
        stations.estimate.stations = 3.0;
        assert_eq!(target_score(&data, &stations, &FLEET, false), None);
        stations.estimate.stations = 1.0;
        assert!(target_score(&data, &stations, &FLEET, false).is_some());
        // Zu stark für einen Sturm, zu gut versorgt zum Aushungern, vom Schild gedeckt:
        // Kein Weg führt hinein. Ohne Schild bleibt der Beschuss.
        let mut fortress = near;
        fortress.estimate = estimate(20.0, 30.0, 200.0);
        fortress.estimate.shield = 1;
        assert_eq!(target_score(&data, &fortress, &FLEET, false), None);
        fortress.estimate.shield = 0;
        assert!(target_score(&data, &fortress, &FLEET, false).is_some());
        // Knappe Vorräte nahe dem eigenen Hafen: Der Tank reicht, um sie auszuhungern.
        fortress.estimate = estimate(20.0, 30.0, 30.0);
        fortress.estimate.shield = 1;
        assert!(target_score(&data, &fortress, &FLEET, false).is_some());

        // Unter gleichen Welten gewinnt die nähere, dann die kleinere Nummer; mehr Beute
        // wiegt einen längeren Weg auf, eine Hauptwelt zählt mehrfach, eine starke
        // Garnison schreckt ab.
        let distant = TargetFacts {
            planet: PlanetId(2),
            route_fuel: 0.25,
            ..near
        };
        let pick = |worlds: &[TargetFacts]| choose_target(&data, worlds, &FLEET, false).unwrap().0;
        assert_eq!(pick(&[distant, near]), PlanetId(1));
        assert_eq!(
            pick(&[
                TargetFacts {
                    planet: PlanetId(3),
                    ..near
                },
                near
            ]),
            PlanetId(1)
        );
        let rich = TargetFacts {
            value: 30_000.0,
            ..distant
        };
        assert_eq!(pick(&[near, rich]), PlanetId(2));
        let seat = TargetFacts {
            capital: true,
            ..distant
        };
        assert_eq!(pick(&[near, seat]), PlanetId(2));
        let mut held = TargetFacts {
            planet: PlanetId(4),
            value: 12_000.0,
            ..near
        };
        assert_eq!(pick(&[near, held]), PlanetId(4));
        held.estimate = estimate(5.0, 10.0, 30.0);
        assert_eq!(pick(&[near, held]), PlanetId(1));
        assert_eq!(choose_target(&data, &[far, guarded], &FLEET, false), None);
    }

    #[test]
    fn a_campaign_takes_troops_for_a_storm_or_at_least_for_the_occupation() {
        let data = data();
        let levers = &data.balance.military.ai.siege;
        let balance = &data.balance.siege;
        // Sturm: das storm_ratio-Fache der geschätzten Verteidiger.
        let weak = colony(1);
        assert_eq!(
            troops_wanted(&data, &weak, &FLEET),
            levers.storm_ratio * 7.0
        );
        // Mehr, als an Bord passt, gibt es nicht.
        let mut strong = weak;
        strong.estimate = estimate(20.0, 30.0, 30.0);
        assert_eq!(troops_wanted(&data, &strong, &FLEET), FLEET.berths);
        // Ohne Garnison wenigstens die Besatzungstruppe mit Aufschlag.
        let mut empty = weak;
        empty.estimate = estimate(0.0, 0.0, 0.0);
        assert_eq!(
            troops_wanted(&data, &empty, &FLEET),
            levers.occupation_margin * occupation_force(100.0, balance)
        );

        // Einschiffen: was fehlt, soweit die Welt es über ihre Stammgarnison hinaus
        // hergibt und an Bord Platz ist.
        assert_eq!(embark_strength(10.0, 0.0, 17.5, 20.0, 5.0), 10.0);
        assert_eq!(embark_strength(10.0, 4.0, 17.5, 20.0, 5.0), 6.0);
        assert_eq!(embark_strength(10.0, 0.0, 17.5, 8.0, 5.0), 3.0);
        assert_eq!(embark_strength(30.0, 10.0, 17.5, 40.0, 5.0), 7.5);
        // Nichts, wenn die Truppe voll ist oder die Welt nur ihre Stammgarnison hält;
        // ein Bruchteil nur, wenn er die Truppe voll macht.
        assert_eq!(embark_strength(10.0, 10.0, 17.5, 20.0, 5.0), 0.0);
        assert_eq!(embark_strength(10.0, 0.0, 17.5, 5.0, 5.0), 0.0);
        assert_eq!(embark_strength(10.0, 0.0, 17.5, 5.2, 5.0), 0.0);
        assert_eq!(embark_strength(10.0, 9.8, 17.5, 20.0, 5.0), 10.0 - 9.8);
    }

    /// Die Karte einer Belagerung: eine Kolonie mit Regulären und Vorräten, die Flotte
    /// mit Truppen an Bord, nach dem ersten Tag.
    fn card_of(regulars: f64, depot: f64, troops: f64) -> (GameData, SiegeCard) {
        let mut scene = Scene::colony();
        let defense = scene.state.sieges.defense_mut(scene.planet);
        defense.regulars = regulars;
        defense.depot = depot;
        if troops > 0.0 {
            scene.state.sieges.embarked.insert(
                FleetId(0),
                Embarked {
                    troops,
                    supply: 90.0 * troops,
                },
            );
        }
        scene.day();
        let refs = SiegeRefs::of_state(&scene.state, &scene.data);
        let card = siege_card(&refs, scene.planet, scene.day).expect("Belagerung");
        (scene.data.clone(), card)
    }

    /// Nahe am eigenen Hafen, kein Gegner in Sicht, eine vorsichtige Gemeinschaft.
    const CALM: SiegeContext = SiegeContext {
        fuel_home: 0.02,
        relief_odds: f64::INFINITY,
        boldness: 0.0,
        upkeep_share: 0.0,
        treasury_months: 24.0,
        troops_home: 0.0,
        berths_free: 0.0,
        sorties_left: true,
    };

    fn forecast_of(card: &SiegeCard, course: Course) -> crate::siege::Forecast {
        card.forecasts
            .iter()
            .find(|(named, _)| *named == course)
            .map(|(_, forecast)| *forecast)
            .unwrap()
    }

    #[test]
    fn a_safe_siege_with_short_stores_starves_and_haste_or_a_short_tank_storms() {
        let (data, card) = card_of(6.0, 10.0, 18.0);
        let levers = &data.balance.military.ai;
        let (starve, storm) = (
            forecast_of(&card, Course::Starve),
            forecast_of(&card, Course::Storm),
        );
        assert!(starve.falls && storm.falls, "{starve:?} {storm:?}");
        assert!(storm.days < starve.days && storm.own_losses > 0.0);

        // Der Orbit ist sicher, der Tank reicht, die Vorräte sind knapp: aushungern.
        let calm = decide(&data, &card, &CALM);
        assert_eq!(calm.course, Some(Course::Starve), "{starve:?} {storm:?}");
        assert_eq!(calm.withdraw, None);
        assert!(!calm.land && !calm.orders.assault);
        assert_eq!(calm.orders.fire, Fire::Hold);

        // Droht Entsatz, zählt jeder Tag mehrfach: stürmen, jetzt landen.
        let threatened = SiegeContext {
            relief_odds: levers.offensive_odds - 0.1,
            boldness: 1.0,
            ..CALM
        };
        let hasty = decide(&data, &card, &threatened);
        assert_eq!(hasty.course, Some(Course::Storm));
        assert!(hasty.land && hasty.orders.assault);
        assert_eq!(hasty.orders.fire, Fire::Support);

        // Reicht der Tank nicht bis zum Ende des Aushungerns, wohl aber für den Sturm:
        // stürmen.
        let per_day = data.balance.military.fuel_per_day as f64;
        let short = SiegeContext {
            fuel_home: card.fuel - FUEL_RESERVE - (storm.days as f64 + 5.0) * per_day,
            ..CALM
        };
        assert!(days_on_station(&data, card.fuel, short.fuel_home) < starve.days as f64);
        assert_eq!(decide(&data, &card, &short).course, Some(Course::Storm));

        // Die laufende Wahl hält: Wer schon stürmt, wechselt nicht wegen eines kleinen
        // Unterschieds zurück.
        assert_eq!(course_of(&calm.orders), Course::Starve);
        assert_eq!(course_of(&hasty.orders), Course::Storm);
        assert_eq!(
            course_of(&SiegeOrders {
                fire: Fire::Area,
                assault: true,
                ..Default::default()
            }),
            Course::Area
        );
    }

    #[test]
    fn a_siege_is_lifted_for_fuel_for_troops_before_relief_or_when_hopeless() {
        let (data, card) = card_of(6.0, 10.0, 18.0);
        let levers = &data.balance.military.ai;
        // Ein überlegener Entsatz: aufheben.
        let relief = SiegeContext {
            relief_odds: levers.defend_odds - 0.05,
            ..CALM
        };
        assert_eq!(
            decide(&data, &card, &relief).withdraw,
            Some(Withdraw::Relief)
        );

        let per_day = data.balance.military.fuel_per_day as f64;
        // Der Rückweg ist nicht mehr im Tank: heim zum Auftanken, solange der Feldzug
        // noch wiederkommen darf; sonst ist er zu Ende.
        let mut drained = card.clone();
        drained.fuel = CALM.fuel_home;
        assert_eq!(
            decide(&data, &drained, &CALM).withdraw,
            Some(Withdraw::Refuel)
        );
        let spent = SiegeContext {
            sorties_left: false,
            ..CALM
        };
        assert_eq!(
            decide(&data, &drained, &spent).withdraw,
            Some(Withdraw::Hopeless)
        );
        // Kostet schon der Weg vom nächsten Hafen so viel, dass auch ein voller Tank über
        // dem Ziel keine Zeit ließe, kehrt die Flotte nicht zum Auftanken heim, um
        // wiederzukommen: Sie gibt auf.
        let stranded = SiegeContext {
            fuel_home: card.fuel,
            ..CALM
        };
        assert!(days_on_station(&data, card.fuel, stranded.fuel_home) <= 0.0);
        assert_eq!(
            decide(&data, &card, &stranded).withdraw,
            Some(Withdraw::Hopeless)
        );
        assert!((days_on_station(&data, 0.5, 0.25) - 0.2 / per_day).abs() < 1e-9);

        // Für keinen Weg reicht der Rest im Tank, ein voller Tank aber reichte: auftanken.
        let mut low = card.clone();
        low.fuel = CALM.fuel_home + FUEL_RESERVE + 2.0 * per_day;
        assert_eq!(decide(&data, &low, &CALM).withdraw, Some(Withdraw::Refuel));
        // Liegt der Hafen so fern, dass auch ein voller Tank nicht reichte: aufgeben.
        let remote = SiegeContext {
            fuel_home: card.fuel - FUEL_RESERVE - 2.0 * per_day,
            ..CALM
        };
        assert_eq!(
            decide(&data, &card, &remote).withdraw,
            Some(Withdraw::Hopeless)
        );

        // Ohne Truppen an Bord fällt die Welt auf keinem Weg: Truppen holen, wenn zu
        // Hause welche stehen und an Bord Platz ist; sonst aufgeben.
        let (data, bare) = card_of(6.0, 10.0, 0.0);
        assert!(bare.forecasts.iter().all(|(_, forecast)| !forecast.falls));
        let waiting = SiegeContext {
            troops_home: 12.0,
            berths_free: 17.5,
            ..CALM
        };
        assert_eq!(
            decide(&data, &bare, &waiting).withdraw,
            Some(Withdraw::FetchTroops)
        );
        assert_eq!(
            decide(&data, &bare, &CALM).withdraw,
            Some(Withdraw::Hopeless)
        );
        let full = SiegeContext {
            berths_free: 0.0,
            ..waiting
        };
        assert_eq!(
            decide(&data, &bare, &full).withdraw,
            Some(Withdraw::Hopeless)
        );
    }

    #[test]
    fn a_poor_treasury_sacks_and_an_own_stake_forbids_area_fire() {
        let (data, mut card) = card_of(6.0, 10.0, 18.0);
        let levers = &data.balance.military.ai.siege;
        assert!(!decide(&data, &card, &CALM).orders.sack);
        let poor = SiegeContext {
            treasury_months: levers.sack_below_months - 0.5,
            ..CALM
        };
        assert!(decide(&data, &card, &poor).orders.sack);

        // Nur der Flächenbeschuss führt zum Ziel: Er gilt, außer die Gemeinschaft hält
        // selbst einen großen Anteil an den Betrieben der Welt.
        for (course, forecast) in &mut card.forecasts {
            if *course != Course::Area {
                forecast.falls = false;
                forecast.limited_by = Some(Limit::Time);
            }
        }
        if forecast_of(&card, Course::Area).falls {
            assert_eq!(decide(&data, &card, &CALM).course, Some(Course::Area));
            card.view
                .siege
                .stakes
                .insert(card.attacker, levers.area_stake_max + 0.1);
            assert_eq!(
                decide(&data, &card, &CALM).withdraw,
                Some(Withdraw::Hopeless)
            );
        } else {
            panic!("die Lage sollte den Flächenbeschuss tragen");
        }
    }

    #[test]
    fn defence_plans_follow_exposure_war_and_what_already_stands() {
        let data = data();
        let levers = &data.balance.military.ai.siege;
        let balance = &data.balance.siege;
        // Lage: Hauptwelt; bedroht nahe einem Kriegsgegner oder am Engpass an fremdem
        // Gebiet; sonst Hinterland.
        assert_eq!(
            exposure(&data, true, false, u32::MAX, u32::MAX),
            Exposure::Capital
        );
        assert_eq!(
            exposure(&data, false, false, levers.defense_jumps, 9),
            Exposure::Exposed
        );
        assert_eq!(
            exposure(&data, false, false, levers.defense_jumps + 1, 1),
            Exposure::Interior
        );
        assert_eq!(exposure(&data, false, true, u32::MAX, 1), Exposure::Exposed);
        assert_eq!(
            exposure(&data, false, true, u32::MAX, 2),
            Exposure::Interior
        );

        let caps = [3, 5, 1, 1];
        let colony = PlanetDefense {
            militia: 8.0,
            ..Default::default()
        };
        let row = defense_row(&data, Exposure::Exposed);
        // Im Krieg das ganze Ausbauziel, begrenzt durch Technik und Aushebung.
        let war = planned_defense(&data, row, caps, &colony, true, 1.0, 0.0, 0.0, false);
        assert_eq!(
            (war.stations, war.guns, war.shield, war.bunkers),
            (
                row.stations,
                row.guns,
                row.shield.min(1),
                row.bunkers.min(1)
            )
        );
        assert_eq!(
            war.regulars as f64,
            (row.regulars as f64 * 8.0)
                .min(balance.regulars_cap * 8.0)
                .floor()
        );
        assert_eq!(war.depot_days, row.depot_days);
        // Im Frieden nur der Anteil defense_peace; das Depot bleibt.
        let peace = planned_defense(&data, row, caps, &colony, false, 1.0, 0.0, 0.0, false);
        assert!(peace.regulars < war.regulars && peace.guns <= war.guns);
        assert_eq!(
            peace.regulars as f64,
            (row.regulars as f64 * 8.0 * levers.defense_peace).floor()
        );
        assert_eq!(peace.depot_days, row.depot_days);
        assert!(peace.depot_days > 0, "keine Welt ohne Vorräte");
        assert!(defense_row(&data, Exposure::Interior).depot_days > 0);
        // Was steht, bleibt stehen, auch im Frieden und bei gekürztem Plan.
        let built = PlanetDefense {
            guns: 2.0,
            bunkers: 1,
            ..colony.clone()
        };
        let kept = planned_defense(&data, row, caps, &built, false, 0.0, 0.0, 0.0, false);
        assert_eq!((kept.guns, kept.bunkers), (2, 1));
        assert_eq!(kept.regulars, 0.0);
        // Was die Technik nicht hergibt, plant niemand.
        let low = planned_defense(
            &data,
            row,
            [0, 0, 0, 0],
            &colony,
            true,
            1.0,
            0.0,
            0.0,
            false,
        );
        assert_eq!(
            (low.stations, low.guns, low.shield, low.bunkers),
            (0, 0, 0, 0)
        );
        // Truppen für den Feldzug kommen hinzu, bis zur Grenze der Aushebung; eine
        // besetzte Welt behält von ihrer Garnison, was sie ruhig hält.
        let muster = planned_defense(&data, row, caps, &colony, true, 1.0, 5.0, 0.0, false);
        assert_eq!(muster.regulars, war.regulars + 5.0);
        let capped = planned_defense(&data, row, caps, &colony, true, 1.0, 500.0, 0.0, false);
        assert_eq!(capped.regulars as f64, balance.regulars_cap * 8.0);
        let occupied = PlanetDefense {
            militia: 0.0,
            regulars: 12.0,
            ..Default::default()
        };
        let held = planned_defense(&data, row, caps, &occupied, true, 1.0, 0.0, 7.0, false);
        assert_eq!(held.regulars, 7.0);

        // Der Etat: Passen alle Pläne nicht hinein, gilt die größte Stufe, die passt.
        let upkeep = |scale: f64| 1000.0 * scale;
        assert_eq!(affordable_scale(2000.0, upkeep), 1.0);
        assert_eq!(affordable_scale(800.0, upkeep), 0.75);
        assert_eq!(affordable_scale(300.0, upkeep), 0.25);
        assert_eq!(affordable_scale(100.0, upkeep), 0.0);
        assert_eq!(
            plan_upkeep(&data, &war),
            balance.pay * war.regulars as f64
                + balance.level_upkeep
                    * (war.stations + war.guns + war.shield + war.bunkers) as f64
        );
    }

    fn plight() -> Plight {
        Plight {
            capital: false,
            stores_days: 0.0,
            garrison_left: 0.3,
            surface: 4.0,
            deep: 0.0,
            army: 0.0,
            occupation_ready: true,
            relief_odds: 0.0,
            guns: 1.0,
            bunkers: 0,
            center_held: true,
            landed_yesterday: false,
            posture: Posture::Forward,
        }
    }

    const YIELDING: Disposition = Disposition {
        boldness: 0.5,
        resolve: 0.1,
    };

    #[test]
    fn a_world_is_handed_over_only_when_holding_is_pointless_and_the_community_yields() {
        let data = data();
        let levers = &data.balance.military.ai.siege;
        // Ausgehungert, kein Entsatz möglich, die Besatzungstruppe wartet: übergeben.
        let starving = plight();
        assert!(should_capitulate(&data, &starving, &YIELDING));
        // Nicht, solange Vorräte da sind oder die Garnison noch steht.
        assert!(!should_capitulate(
            &data,
            &Plight {
                stores_days: 3.0,
                ..starving
            },
            &YIELDING
        ));
        assert!(!should_capitulate(
            &data,
            &Plight {
                garrison_left: levers.capitulate_garrison + 0.1,
                ..starving
            },
            &YIELDING
        ));
        // Nicht, wenn niemand da ist, der sie annimmt, oder eine eigene Flotte dem
        // Belagerer gewachsen ist.
        assert!(!should_capitulate(
            &data,
            &Plight {
                occupation_ready: false,
                ..starving
            },
            &YIELDING
        ));
        assert!(!should_capitulate(
            &data,
            &Plight {
                relief_odds: levers.relief_odds,
                ..starving
            },
            &YIELDING
        ));
        // Eine standhafte Gemeinschaft hält bis zum Ende; die Hauptwelt übergibt nur,
        // wer noch nachgiebiger ist.
        let steadfast = Disposition {
            resolve: levers.capitulate_resolve,
            ..YIELDING
        };
        assert!(!should_capitulate(&data, &starving, &steadfast));
        let seat = Plight {
            capital: true,
            ..starving
        };
        assert!(should_capitulate(&data, &seat, &YIELDING));
        let middling = Disposition {
            resolve: levers.capitulate_capital_resolve,
            ..YIELDING
        };
        assert!(!should_capitulate(&data, &seat, &middling));
        assert!(should_capitulate(&data, &starving, &middling));
        // Überrannt: das Zentrum verloren, niemand im Untergrund, die gelandete Truppe
        // weit überlegen. Mit Vorräten, aber ohne Aussicht.
        let overrun = Plight {
            stores_days: 40.0,
            garrison_left: 0.8,
            army: levers.capitulate_ratio * 4.0,
            center_held: false,
            ..starving
        };
        assert!(should_capitulate(&data, &overrun, &YIELDING));
        for holding in [
            Plight {
                center_held: true,
                ..overrun
            },
            Plight {
                deep: 3.0,
                ..overrun
            },
            Plight {
                army: levers.capitulate_ratio * 4.0 - 1.0,
                ..overrun
            },
        ] {
            assert!(!should_capitulate(&data, &holding, &YIELDING));
        }
    }

    #[test]
    fn a_besieged_garrison_saves_its_stores_and_goes_underground_when_outnumbered() {
        let data = data();
        let levers = &data.balance.military.ai.siege;
        let full = Plight {
            stores_days: levers.hold_fire_days + 1.0,
            ..plight()
        };
        assert_eq!(garrison_orders(&data, &full), (Posture::Forward, false));
        // Knappe Vorräte: Die Geschütze schweigen, außer am Tag nach einer Landung.
        let short = Plight {
            stores_days: levers.hold_fire_days - 1.0,
            ..full
        };
        assert_eq!(garrison_orders(&data, &short), (Posture::Forward, true));
        assert!(
            !garrison_orders(
                &data,
                &Plight {
                    landed_yesterday: true,
                    ..short
                }
            )
            .1
        );
        assert!(!garrison_orders(&data, &Plight { guns: 0.0, ..short }).1);
        // Gelandet und überlegen: mit Bunkern in die Tiefe, solange das Zentrum hält;
        // zurück nach vorn geht es nicht.
        let landed = Plight {
            army: 10.0,
            bunkers: 1,
            ..full
        };
        assert_eq!(garrison_orders(&data, &landed).0, Posture::Deep);
        for forward in [
            Plight {
                bunkers: 0,
                ..landed
            },
            Plight {
                center_held: false,
                ..landed
            },
            Plight {
                army: 4.0 / levers.deep_ratio - 0.5,
                ..landed
            },
        ] {
            assert_eq!(garrison_orders(&data, &forward).0, Posture::Forward);
        }
        let below = Plight {
            posture: Posture::Deep,
            ..full
        };
        assert_eq!(garrison_orders(&data, &below).0, Posture::Deep);
    }

    /// Die Lage einer gestellten Partie, wie die Führung sie liest.
    fn situation(scene: &Scene) -> Situation<'_> {
        let state = &scene.state;
        Situation {
            data: &scene.data,
            galaxy: &state.galaxy,
            polities: &state.polities,
            economy: &state.economy,
            fleets: &state.fleets,
            battles: &state.battles,
            diplomacy: &state.diplomacy,
            research: &state.research,
            finance: &state.finance,
            logistics: &state.logistics,
            sieges: &state.sieges,
            seed: 9,
            day: scene.day,
        }
    }

    use crate::siege::orbit::testing::{FOE, ME};

    #[test]
    fn an_enemy_in_its_own_system_does_not_hold_back_a_campaign() {
        let mut scene = Scene::new();
        // Die Gemeinschaft 0 besitzt eine kleine Welt in einem System, das nach der
        // Mehrheit seiner Bevölkerung dem Gegner gehört; dort liegt dessen Flotte.
        let (system, planet) = scene
            .state
            .galaxy
            .systems
            .iter()
            .filter(|system| scene.state.galaxy.system_owner(system.id) == Some(FOE))
            .find_map(|system| {
                let mut worlds: Vec<_> = scene.state.galaxy.planets_in(system.id).collect();
                worlds.sort_by(|a, b| a.population.total_cmp(&b.population));
                (worlds.len() >= 2).then(|| (system.id, worlds[0].id))
            })
            .expect("ein System des Gegners mit zwei Welten");
        scene.state.galaxy.planet_mut(planet).owner = Some(ME);
        scene.state.galaxy.rebuild_index();
        assert_eq!(scene.state.galaxy.system_owner(system), Some(FOE));
        assert!(scene.state.galaxy.owns_planet_in(ME, system));
        scene.place(1, system);
        let sit = situation(&scene);
        let foes = foes_by_system(&sit, ME, &[FOE]);
        assert!(foes.contains_key(&system));
        assert_eq!(intruder_at_home(&sit, ME, &foes), None);
        // Im eigenen System hält derselbe Gegner die Hauptflotte zurück.
        let home = scene
            .state
            .galaxy
            .planet(scene.state.polities.get(ME).capital.unwrap())
            .system;
        scene.place(1, home);
        let sit = situation(&scene);
        let foes = foes_by_system(&sit, ME, &[FOE]);
        assert_eq!(intruder_at_home(&sit, ME, &foes), Some(home));
    }

    #[test]
    fn a_long_search_widens_and_repeated_failure_bans_longer() {
        let data = data();
        let levers = &data.balance.military.ai.siege;
        assert!(!wide_search(&data, None, 10_000));
        assert!(!wide_search(&data, Some(100), 100 + levers.widen_days - 1));
        assert!(wide_search(&data, Some(100), 100 + levers.widen_days));
        // Die weitere Suche nimmt Welten bis reach_wide und ab odds_wide.
        let near = colony(1);
        let far = TargetFacts {
            route_fuel: (levers.reach + levers.reach_wide) as f64 / 2.0,
            odds: (levers.odds + levers.odds_wide) / 2.0,
            ..near
        };
        assert_eq!(target_score(&data, &far, &FLEET, false), None);
        assert!(target_score(&data, &far, &FLEET, true).is_some());
        let beyond = TargetFacts {
            route_fuel: levers.reach_wide as f64 + 0.01,
            ..near
        };
        assert_eq!(target_score(&data, &beyond, &FLEET, true), None);
        // Jedes weitere Scheitern am selben Ziel lässt es länger ruhen.
        assert_eq!(retry_ban(&data, 0), levers.retry_days);
        assert_eq!(retry_ban(&data, 2), 3 * levers.retry_days);
    }

    #[test]
    fn an_expensive_fleet_storms_rather_than_waits() {
        let (data, card) = card_of(6.0, 10.0, 18.0);
        assert_eq!(decide(&data, &card, &CALM).course, Some(Course::Starve));
        // Sold und Wartung der Flotte je Tag in Anteilen der Beute: Wo sie teuer ist,
        // kostet jeder Tag Warten mehr als der Sturm.
        let costly = SiegeContext {
            upkeep_share: 0.05,
            ..CALM
        };
        let decision = decide(&data, &card, &costly);
        assert_eq!(decision.course, Some(Course::Storm));
        assert!(decision.land);
    }

    #[test]
    fn troops_land_under_area_fire_only_after_the_forecast_held_a_day() {
        let (data, mut card) = card_of(6.0, 10.0, 18.0);
        for (course, forecast) in &mut card.forecasts {
            if *course != Course::Area {
                forecast.falls = false;
                forecast.limited_by = Some(Limit::Time);
            }
        }
        let mut trial = Projection::new(&card.view, &data.balance.siege);
        trial.siege.orders.fire = Fire::Area;
        assert!(trial.storm_succeeds(&data.balance.siege), "die Probe trägt");
        // Am ersten Tag des Beschusses: feuern, nicht landen.
        let first = decide(&data, &card, &CALM);
        assert_eq!(first.course, Some(Course::Area));
        assert!(!first.land);
        // Galt der Beschuss schon gestern, und die Probe trägt noch: landen.
        card.view.siege.orders = first.orders;
        card.days = card.days.max(2);
        let second = decide(&data, &card, &CALM);
        assert_eq!(second.course, Some(Course::Area));
        assert!(second.land);
    }

    #[test]
    fn a_shrunken_budget_follows_recent_revenue_and_may_dismantle_works() {
        let data = data();
        let levers = &data.balance.military.ai;
        // Das lange Mittel, aber höchstens die letzten Monate.
        let share = levers.siege.defense_share;
        assert_eq!(defense_budget(&data, 1000.0, &[], false), share * 1000.0);
        assert_eq!(
            defense_budget(&data, 1000.0, &[400.0, 600.0], false),
            share * 500.0
        );
        assert_eq!(
            defense_budget(&data, 1000.0, &[2000.0], true),
            share * 1000.0 * levers.war_factor
        );
        // Mit Abbau gilt auch für Anlagen, die stehen, nur noch der Plan.
        let row = defense_row(&data, Exposure::Interior);
        let built = PlanetDefense {
            militia: 8.0,
            guns: 2.0,
            bunkers: 1,
            ..Default::default()
        };
        let kept = planned_defense(
            &data,
            row,
            [3, 5, 1, 1],
            &built,
            false,
            0.0,
            0.0,
            0.0,
            false,
        );
        assert_eq!((kept.guns, kept.bunkers), (2, 1));
        let torn = planned_defense(&data, row, [3, 5, 1, 1], &built, false, 0.0, 0.0, 0.0, true);
        assert_eq!((torn.guns, torn.bunkers), (0, 0));
        assert!(plan_upkeep(&data, &torn) < plan_upkeep(&data, &kept));
        assert_eq!(torn.depot_days, kept.depot_days);
    }

    /// Ein Feldzug der Gemeinschaft 0 mit ihrer Flotte, die über der Hauptwelt des
    /// Gegners liegt.
    fn campaign_against(scene: &Scene, target: PlanetId, stage: CampaignStage) -> AiState {
        let mut ai = AiState::default();
        ai.plan_mut(ME).campaign = Some(Campaign {
            target,
            staging: scene.state.polities.get(ME).capital.unwrap(),
            fleet: FleetId(0),
            stage,
            started_day: 0,
            stage_day: 0,
            troops: 4.0,
            sorties: 0,
            wide: false,
        });
        ai
    }

    #[test]
    fn a_campaign_that_ends_sends_its_fleet_to_the_nearest_port() {
        let mut scene = Scene::new();
        scene.day = 100;
        // Die Welt ist kein Ziel mehr: Sie gehört einer dritten Gemeinschaft, mit der kein
        // Krieg herrscht.
        let neutral = scene.state.polities.get(PolityId(2)).capital.unwrap();
        let mut ai = campaign_against(&scene, neutral, CampaignStage::Siege);
        let sit = situation(&scene);
        let mut queue = CommandQueue::default();
        lead_sieges(&sit, &mut ai, &mut queue, None);
        let plan = ai.plan(ME);
        assert_eq!(plan.campaign, None);
        assert_eq!(plan.campaign_end, 100);
        let fleet = sit.fleets.get(FleetId(0)).unwrap();
        let port = last_port(&sit, &bases(&sit, ME), fleet.system).expect("ein Hafen");
        assert_ne!(port, fleet.system);
        assert!(queue.pending.iter().any(|queued| queued.polity == ME
            && queued.command
                == crate::command::SimCommand::MoveFleet {
                    fleet: FleetId(0),
                    to: port
                }));
    }

    #[test]
    fn a_campaign_does_not_sail_for_a_target_it_can_no_longer_take() {
        let mut scene = Scene::new();
        scene.day = 100;
        // Die Flotte liegt im eigenen Hafen, der dem Ziel am nächsten liegt, mit vollem
        // Tank, und ist auf dem Marsch. Die Hauptwelt des Gegners ist inzwischen eine
        // Festung: Schild, reiche Vorräte, viele Reguläre.
        let target = scene.planet;
        let defense = scene.state.sieges.defense_mut(target);
        defense.shield = 1;
        defense.depot = 400.0;
        defense.regulars = 400.0;
        scene.state.sieges.assign.clear();
        let gate = {
            let sit = situation(&scene);
            last_port(&sit, &bases(&sit, ME), scene.system).expect("ein Hafen")
        };
        scene.place(0, gate);
        // Die Flotte des Gegners steht fern: Am Ziel ist die Flotte jedem gewachsen.
        let away = scene.state.polities.get(PolityId(2)).capital.unwrap();
        let away = scene.state.galaxy.planet(away).system;
        scene.place(1, away);
        let mut ai = campaign_against(&scene, target, CampaignStage::March);
        let sit = situation(&scene);
        let fleet = sit.fleets.get(FleetId(0)).unwrap();
        assert!(fleet.min_fuel() >= sit.data.balance.military.ai.siege.sail_fuel);
        let foes = foes_by_system(&sit, ME, &[FOE]);
        let own = own_values(sit.data, fleet);
        assert!(
            odds(sit.data, &own, &foes_around(&sit, &foes, scene.system), 1.0)
                >= sit.data.balance.military.ai.siege.odds
        );
        let mut queue = CommandQueue::default();
        lead_sieges(&sit, &mut ai, &mut queue, None);
        assert!(!queue.pending.iter().any(|queued| queued.polity == ME
            && queued.command
                == crate::command::SimCommand::MoveFleet {
                    fleet: FleetId(0),
                    to: scene.system
                }));
        let plan = ai.plan(ME);
        assert_eq!(plan.campaign, None);
        assert!(plan.spurned.contains_key(&target));
        assert_eq!(plan.failures.get(&target), Some(&1));
    }

    #[test]
    fn a_muster_that_fails_does_not_ban_the_target() {
        let mut scene = Scene::new();
        let target = scene.planet;
        scene.state.sieges.assign.clear();
        let capital = scene.state.polities.get(ME).capital.unwrap();
        let seat = scene.state.galaxy.planet(capital).system;
        scene.place(0, seat);
        // Keine Truppen am Sammelplatz, und die Frist ist um.
        scene.state.sieges.defense_mut(capital).regulars = 0.0;
        scene.day = 1000;
        let mut ai = campaign_against(&scene, target, CampaignStage::Muster);
        let sit = situation(&scene);
        let mut queue = CommandQueue::default();
        lead_sieges(&sit, &mut ai, &mut queue, None);
        let plan = ai.plan(ME);
        assert_eq!(plan.campaign, None);
        assert!(!plan.spurned.contains_key(&target));
    }

    #[test]
    fn dispositions_are_fixed_by_seed_and_community() {
        let first = disposition(7, PolityId(2));
        assert_eq!(first, disposition(7, PolityId(2)));
        assert_ne!(first, disposition(7, PolityId(3)));
        assert_ne!(first, disposition(8, PolityId(2)));
        for polity in 0..40 {
            let character = disposition(1, PolityId(polity));
            assert!((0.0..1.0).contains(&character.boldness));
            assert!((0.0..1.0).contains(&character.resolve));
        }
        // Über viele Gemeinschaften kommen nachgiebige und standhafte vor.
        let data = data();
        let limit = data.balance.military.ai.siege.capitulate_resolve;
        let yielding = (0..40)
            .filter(|polity| disposition(1, PolityId(*polity)).resolve < limit)
            .count();
        assert!((5..35).contains(&yielding), "{yielding} von 40");
    }
}
