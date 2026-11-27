//! Läufe ohne Grafik: Grundlage für Tests und das `headless`-Programm.

use crate::ai::{AiState, MilitaryMode};
use crate::clock::{DAYS_PER_MONTH, SimClock, TICKS_PER_DAY};
use crate::command::{CommandLog, CommandQueue, CommandSource, ReplayScript, SimCommand};
use crate::data::GameData;
use crate::economy::Economy;
use crate::endgame::{Endgame, Outcome};
use crate::fleet::{Fleets, Stance, yard_slots};
use crate::galaxy::Galaxy;
use crate::ids::{FleetId, PlanetId, PolityId};
use crate::polity::{Polities, Succession, SuccessionParams};
use crate::state::{SaveGame, SimConfig, SimState, hash_world, new_game};
use crate::tech::Research;
use crate::{SimPlugins, needs_fine_ticks, sim_schedule};
use bevy_app::App;
use bevy_ecs::prelude::*;
use bevy_ecs::system::SystemState;
use std::collections::{BTreeMap, BTreeSet};

/// Eine Partie ohne Fenster. Ruft das Simulations-Schedule direkt auf, ohne Wanduhr.
pub struct Headless {
    app: App,
}

impl Headless {
    pub fn new(data: GameData, config: SimConfig) -> Self {
        let mut sim = Self::empty(data);
        new_game(sim.app.world_mut(), config);
        sim
    }

    pub fn from_save(data: GameData, save: SaveGame) -> Self {
        let mut sim = Self::empty(data);
        save.state.install(sim.app.world_mut());
        sim
    }

    fn empty(data: GameData) -> Self {
        let mut app = App::new();
        app.insert_resource(data).add_plugins(SimPlugins);
        app.finish();
        app.cleanup();
        Self { app }
    }

    pub fn world(&self) -> &World {
        self.app.world()
    }

    pub fn world_mut(&mut self) -> &mut World {
        self.app.world_mut()
    }

    pub fn clock(&self) -> SimClock {
        *self.world().resource::<SimClock>()
    }

    pub fn galaxy(&self) -> &Galaxy {
        self.world().resource::<Galaxy>()
    }

    pub fn polities(&self) -> &Polities {
        self.world().resource::<Polities>()
    }

    pub fn research(&self) -> &Research {
        self.world().resource::<Research>()
    }

    pub fn hash(&self) -> u64 {
        hash_world(self.world()).expect("Partie läuft")
    }

    pub fn state(&self) -> SimState {
        SimState::capture(self.world()).expect("Partie läuft")
    }

    pub fn save(&self) -> SaveGame {
        SaveGame::capture(self.world()).expect("Partie läuft")
    }

    pub fn command_log(&self) -> &CommandLog {
        self.world().resource::<CommandLog>()
    }

    /// Stellt einen Befehl ein, als käme er vom Spieler. Nach dem Ende der Partie nimmt
    /// sie keine Befehle mehr an: Der Zustand steht still.
    pub fn issue(&mut self, polity: PolityId, command: SimCommand) {
        if self.outcome().is_some() {
            return;
        }
        self.world_mut().resource_mut::<CommandQueue>().push(
            polity,
            CommandSource::Player,
            command,
        );
    }

    /// Legt fest, wie weit die KI eine Gemeinschaft militärisch führt; ohne `who` gilt es
    /// für alle, auch für solche, die erst entstehen. Eine Einstellung der Partie wie
    /// `SimConfig::player`, kein Befehl: für Drehbücher und Prüfungen, die Flotten selbst
    /// führen, und für den Vergleich mit einer Flotte, die untätig bleibt.
    pub fn set_military_mode(&mut self, who: Option<PolityId>, mode: MilitaryMode) {
        let mut ai = self.world_mut().resource_mut::<AiState>();
        match who {
            Some(polity) => {
                ai.modes.insert(polity, mode);
            }
            None => {
                ai.modes.clear();
                ai.default_mode = mode;
            }
        }
    }

    /// Spielt ein Befehlsprotokoll ab (Wiedergabe einer früheren Partie).
    pub fn set_replay(&mut self, log: &CommandLog) {
        self.world_mut()
            .insert_resource(ReplayScript::new(log.entries.clone()));
    }

    /// Genau ein Takt.
    pub fn tick(&mut self) {
        self.app.world_mut().run_schedule(sim_schedule());
    }

    /// Läuft `days` Tage weiter und überspringt Takte, in denen nichts geschieht. Ist die
    /// Partie zu Ende, kehrt der Lauf sofort zurück: Uhr und Zustand stehen still.
    pub fn run_days(&mut self, days: u64) {
        let target = self.clock().tick + days * TICKS_PER_DAY;
        while self.clock().tick < target {
            if self.outcome().is_some() {
                return;
            }
            self.tick();
            let tick = self.clock().tick;
            // Nach dem Ende wird auch nicht mehr zum nächsten Tag gesprungen.
            if tick.is_multiple_of(TICKS_PER_DAY)
                || needs_fine_ticks(self.world())
                || self.outcome().is_some()
            {
                continue;
            }
            let mut jump = (tick / TICKS_PER_DAY + 1) * TICKS_PER_DAY;
            if let Some(next) = self
                .world()
                .get_resource::<ReplayScript>()
                .and_then(|s| s.next_tick())
            {
                jump = jump.min(next.max(tick));
            }
            self.world_mut().resource_mut::<SimClock>().tick = jump.min(target);
        }
    }

    /// Läuft `days` Tage weiter und rechnet jeden Takt, wie es das Spiel tut. Ist die
    /// Partie zu Ende, kehrt der Lauf sofort zurück.
    pub fn run_days_exact(&mut self, days: u64) {
        for _ in 0..days * TICKS_PER_DAY {
            if self.outcome().is_some() {
                return;
            }
            self.tick();
        }
    }
}

/// Volkswirtschaftliche Kennzahlen der ganzen Galaxie.
#[derive(Clone, Debug, Default)]
pub struct EconomyStats {
    /// Bevölkerung in Millionen.
    pub population: f64,
    pub capital: f64,
    /// Anteil der Arbeitskräfte in Arbeit.
    pub employment: f64,
    /// Kapitalgewichtete Auslastung der Werke.
    pub utilization: f64,
    /// Bevölkerungsgewichtete Deckung des Lebensnotwendigen.
    pub essentials_met: f64,
    /// Bevölkerungsgewichtete Deckung aller Bedarfe der Haushalte (Mittel über die
    /// Bedarfe): das Maß dafür, was bei den Haushalten ankommt.
    pub needs_met: f64,
    /// Mittleres Verhältnis von Preis zu Richtpreis auf belebten Märkten.
    pub price_level: f64,
    /// Mittlere Preisspanne zwischen teuerstem und billigstem belebten Markt je Gut.
    pub price_spread: f64,
    /// Anteil der Preise, die an Boden oder Decke kleben.
    pub pinned_prices: f64,
    pub mean_wage: f64,
    pub freighters: usize,
    pub laden_freighters: usize,
    /// Abweichung der Geldmenge vom Soll in Tausendstel Credits; muss null sein.
    pub money_drift: i64,
    /// Summe der Staatskassen in Credits.
    pub treasuries: f64,
    /// Geld des Staatsdienstes (Besatzungen, Forschende) in Credits.
    pub staff_money: f64,
    /// Geldverteilung in Credits: Haushalte, Firmen, Kontore, Handelshäuser.
    pub household_money: f64,
    pub firm_money: f64,
    pub market_money: f64,
    pub house_money: f64,
    pub elite_money: f64,
    /// Bevölkerungsgewichteter Zusammenhalt der besiedelten Welten.
    pub cohesion: f64,
    pub restless_regions: usize,
    pub autonomous_regions: usize,
    pub secessions: u32,
    /// Lebende Gemeinschaften.
    pub polities_alive: usize,
    /// Mittlerer Anteil der Einheimischen bzw. des Zentrums an den Betrieben der Kolonien.
    pub colony_local_share: f64,
    pub colony_center_share: f64,
    /// Summe aller Staatsschulden in Credits.
    pub debt: f64,
    pub anomaly: Option<String>,
}

pub fn economy_stats(world: &World) -> EconomyStats {
    let data = world.resource::<GameData>();
    let galaxy = world.resource::<Galaxy>();
    let economy = world.resource::<crate::economy::Economy>();
    let logistics = world.resource::<crate::logistics::Logistics>();
    let balance = &data.balance.economy;
    let mut stats = EconomyStats::default();
    let (mut labor, mut employed, mut used_capital, mut fed, mut wages) = (0.0, 0.0, 0.0, 0.0, 0.0);
    let mut served = 0.0;
    for planet in &galaxy.planets {
        if planet.owner.is_none() || planet.population <= 0.0 {
            continue;
        }
        let local = economy.planet(planet.id);
        let supply = planet.population * balance.workforce_share;
        stats.population += planet.population;
        labor += supply;
        employed += supply.min(local.labor_demand);
        fed += planet.population * local.essentials_met(balance);
        served += planet.population * local.needs_met.iter().sum::<f64>()
            / local.needs_met.len().max(1) as f64;
        wages += planet.population * local.wage;
        for (capital, utilization) in local.capital.iter().zip(&local.utilization) {
            stats.capital += capital;
            used_capital += capital * utilization;
        }
    }
    stats.employment = employed / labor.max(1e-9);
    stats.utilization = used_capital / stats.capital.max(1e-9);
    stats.essentials_met = fed / stats.population.max(1e-9);
    stats.needs_met = served / stats.population.max(1e-9);
    stats.mean_wage = wages / stats.population.max(1e-9);

    let live: Vec<usize> = galaxy
        .systems
        .iter()
        .filter(|system| galaxy.system_owner(system.id).is_some())
        .map(|system| system.index())
        .collect();
    let (mut level, mut level_count, mut spread, mut spread_count, mut pinned) =
        (0.0, 0, 0.0, 0, 0);
    for (good, def) in data.goods.iter().enumerate() {
        let (mut low, mut high) = (f64::MAX, 0.0f64);
        for index in &live {
            let ratio = economy.markets[*index].price[good] / def.base_price;
            level += ratio;
            level_count += 1;
            low = low.min(ratio);
            high = high.max(ratio);
            if ratio <= balance.price_floor * 1.02 || ratio >= balance.price_ceiling * 0.98 {
                pinned += 1;
            }
        }
        if !live.is_empty() {
            spread += high / low;
            spread_count += 1;
        }
    }
    stats.price_level = level / (level_count.max(1) as f64);
    stats.price_spread = spread / (spread_count.max(1) as f64);
    stats.pinned_prices = pinned as f64 / (level_count.max(1) as f64);
    stats.freighters = logistics.active_freighters();
    stats.laden_freighters = logistics
        .freighters
        .iter()
        .filter(|f| f.alive && f.cargo.is_some())
        .count();
    stats.money_drift = crate::state::money_drift(world).unwrap_or(0);
    stats.treasuries = economy
        .treasuries
        .iter()
        .map(|cash| crate::economy::to_credits(*cash))
        .sum();
    let credits = crate::economy::to_credits;
    stats.staff_money = economy.staff.iter().map(|cash| credits(*cash)).sum();
    stats.household_money = economy.planets.iter().map(|p| credits(p.households)).sum();
    stats.firm_money = economy
        .planets
        .iter()
        .flat_map(|p| p.firms.iter())
        .map(|firm| credits(firm.cash))
        .sum();
    stats.market_money = economy
        .markets
        .iter()
        .map(|market| credits(market.cash))
        .sum();
    stats.house_money = credits(logistics.money());
    let finance = world.resource::<crate::finance::Finance>();
    let polities = world.resource::<Polities>();
    stats.elite_money = credits(finance.money());
    let (mut weight, mut weighted) = (0.0, 0.0);
    let (mut colonies, mut local, mut center) = (0.0, 0.0, 0.0);
    for planet in &galaxy.planets {
        let Some(owner) = planet.owner else {
            continue;
        };
        let region = finance.region(planet.id);
        weight += planet.population;
        weighted += planet.population * region.cohesion;
        match region.stage {
            crate::finance::Stage::Loyal => {}
            crate::finance::Stage::Restless => stats.restless_regions += 1,
            crate::finance::Stage::Autonomous => stats.autonomous_regions += 1,
        }
        if polities.get(owner).capital != Some(planet.id) {
            colonies += 1.0;
            local += region.factors.local_share;
            center += region.factors.center_share;
        }
    }
    stats.cohesion = weighted / f64::max(weight, 1e-9);
    stats.colony_local_share = local / f64::max(colonies, 1.0);
    stats.colony_center_share = center / f64::max(colonies, 1.0);
    stats.secessions = finance.secessions;
    stats.polities_alive = polities.alive().count();
    stats.debt = finance
        .bonds
        .iter()
        .map(|bond| credits(bond.principal))
        .sum();
    stats.anomaly = economy.find_anomaly();
    stats
}

/// Harte Invarianten. Ein Verstoß ist ein Programmfehler, kein Balanceproblem.
pub fn check_invariants(world: &World) -> Result<(), String> {
    crate::polity::succession::check(world)?;
    let stats = economy_stats(world);
    if stats.money_drift != 0 {
        return Err(format!(
            "Geldmenge weicht um {} Tausendstel Credits ab",
            stats.money_drift
        ));
    }
    if let Some(anomaly) = stats.anomaly {
        return Err(anomaly);
    }
    // Flotten: Schiffe in ihren Grenzen, keine Flotte ohne Schiffe, Nummern eindeutig,
    // die Stärke passend zu den Schiffen.
    let fleets = world.resource::<crate::fleet::Fleets>();
    if let Some(anomaly) = fleets.find_anomaly(world.resource::<GameData>()) {
        return Err(anomaly);
    }
    // Gefechte: Jede Flotte im Gefecht gehört zu genau einem, das läuft, und umgekehrt.
    let battles = world.resource::<crate::fleet::Battles>();
    if let Some(anomaly) = battles.find_anomaly(fleets) {
        return Err(anomaly);
    }
    let galaxy = world.resource::<Galaxy>();
    for planet in &galaxy.planets {
        if !planet.population.is_finite() || planet.population < 0.0 {
            return Err(format!(
                "{}: Bevölkerung {}",
                planet.name, planet.population
            ));
        }
        if planet.population > planet.capacity * 1.001 {
            return Err(format!(
                "{}: Bevölkerung über der Tragfähigkeit",
                planet.name
            ));
        }
    }
    let polities = world.resource::<Polities>().list.len();
    if let Some(anomaly) = world
        .resource::<Research>()
        .find_anomaly(world.resource::<GameData>(), polities)
    {
        return Err(anomaly);
    }
    crate::siege::check(world)?;
    Ok(())
}

/// Kennzahlen einer Partie für Berichte und Sweeps.
#[derive(Clone, Debug)]
pub struct RunSummary {
    pub seed: u64,
    pub days: u64,
    pub hash: u64,
    pub systems: usize,
    pub planets: usize,
    pub free_planets: usize,
    pub polities: Vec<PolitySummary>,
    pub economy: EconomyStats,
    /// Gefechte, die in der Partie begonnen haben, und Schiffe, die darin verloren gingen
    /// (ohne Bordjäger).
    pub battles: u64,
    pub ships_lost: u64,
    /// Erster Invariantenverstoß, falls es einen gab.
    pub violation: Option<String>,
    /// Beendete Gefechte je Ausgang, in der Reihenfolge von `BattleOutcome::ALL`:
    /// entschieden, Patt, Zeitgrenze, eingestellt, niemand hält das Feld.
    pub battle_outcomes: [u32; 5],
    /// Befehle der KI, die angenommen und die abgelehnt wurden.
    pub ai_applied: u64,
    pub ai_rejected: u64,
    /// Ausgang der Partie, wenn sie in der Laufzeit geendet hat: Ende, Überlebende, Tag.
    pub outcome: Option<Outcome>,
    /// Belagerungen der Partie: begonnen, je Ausgang, gefallene Welten
    /// (`SiegeStats::fallen`) und Gemeinschaften, die mit ihrer letzten Welt endeten
    /// (`SiegeStats::ended`).
    pub sieges: crate::siege::SiegeStats,
    /// Besiedelte Welten am Ende der Partie, die nicht Hauptwelt sind, und wie viele
    /// davon ohne Vorräte dastehen (Depot unter einem Tag): Ihre Miliz löste sich vom
    /// ersten Tag einer Einschließung an auf.
    pub colonies: u32,
    pub colonies_without_stores: u32,
    /// Flotten, die am Ende mit fast leerem Tank (unter 5 %) außerhalb jedes Systems
    /// liegen, in dem ihre Gemeinschaft eine Welt hält.
    pub fleets_dry_abroad: u32,
}

impl RunSummary {
    /// Anteil der KI-Befehle, die abgelehnt wurden (0..1).
    pub fn ai_rejected_share(&self) -> f64 {
        self.ai_rejected as f64 / (self.ai_applied + self.ai_rejected).max(1) as f64
    }
}

#[derive(Clone, Debug)]
pub struct PolitySummary {
    pub name: String,
    pub alive: bool,
    pub planets: usize,
    pub population: f64,
    /// Zahl der erforschten Technologien.
    pub techs: usize,
    /// Höchste erforschte Stufe je Zweig, in der Reihenfolge von `TechBranch::ALL`.
    pub tiers: [u8; 5],
    /// Bestehende Flotten und die Schiffe darin.
    pub fleets: usize,
    pub ships: usize,
    /// Summe der Flottenstärken.
    pub strength: f64,
    /// Offene Bauaufträge.
    pub orders: usize,
    /// Soldrückstand 0..1.
    pub arrears: f64,
    /// Schiffe, die in Gefechten verloren gingen (ohne Bordjäger).
    pub ships_lost: usize,
    /// Staatskasse in Credits.
    pub treasury: f64,
    /// Kriegsmüdigkeit 0..100.
    pub war_weariness: f64,
    /// Bevölkerungsgewichteter Zusammenhalt der eigenen Welten, 0..100.
    pub cohesion: f64,
    /// Der Staatshaushalt der letzten zwölf abgeschlossenen Monate, summiert: Einnahmen,
    /// je Anspruch verlangt und bezahlt, Ausschüttung.
    pub budget: crate::economy::BudgetMonth,
    /// Abgeschlossene Monate, die in `budget` stecken (höchstens zwölf).
    pub budget_months: usize,
    /// Über die ganze Partie: Schiffe, die die Werften ausgeliefert haben, und Schiffe,
    /// die stillgelegt wurden.
    pub ships_built: u32,
    pub ships_laid_up: u32,
    /// Flotten, die in einem Gefecht mit allen Schiffen verloren gingen.
    pub fleets_lost: u32,
    /// Monatserste im Krieg und Monatserste mit Soldrückstand.
    pub months_at_war: u32,
    pub months_in_arrears: u32,
    /// Bauwert aller Schiffe in Credits.
    pub fleet_value: f64,
    /// Tage, an denen eine eigene Flotte ein fremdes System gesperrt hielt, und begonnene
    /// Angriffe der KI-Führung (Jagd auf die gegnerische Hauptflotte oder Blockade).
    pub blockade_days: u32,
    pub offensives: u32,
    /// Feldzüge gegen Welten, die die KI der Gemeinschaft begonnen hat.
    pub campaigns: u32,
    /// Entschiedene Gefechte, in denen die Gemeinschaft am Ende das Feld hielt.
    pub battles_won: u32,
}

pub fn summarize(sim: &Headless) -> RunSummary {
    let galaxy = sim.galaxy();
    let config = sim.world().resource::<SimConfig>();
    let data = sim.world().resource::<GameData>();
    let research = sim.research();
    let fleets = sim.world().resource::<crate::fleet::Fleets>();
    let battles = sim.world().resource::<crate::fleet::Battles>();
    let economy = sim.world().resource::<crate::economy::Economy>();
    let finance = sim.world().resource::<crate::finance::Finance>();
    let ai = sim.world().resource::<AiState>();
    let cohesion = |polity: PolityId| {
        let (mut weight, mut weighted) = (0.0, 0.0);
        for planet in galaxy.planets_of(polity) {
            weight += planet.population;
            weighted += planet.population * finance.region(planet.id).cohesion;
        }
        weighted / f64::max(weight, 1e-9)
    };
    let sieges = sim.world().resource::<crate::siege::Sieges>();
    let capitals: BTreeSet<PlanetId> = sim
        .polities()
        .alive()
        .filter_map(|polity| polity.capital)
        .collect();
    let colonies: Vec<PlanetId> = galaxy
        .planets
        .iter()
        .filter(|planet| planet.owner.is_some() && !capitals.contains(&planet.id))
        .map(|planet| planet.id)
        .collect();
    RunSummary {
        colonies: colonies.len() as u32,
        colonies_without_stores: colonies
            .iter()
            .filter(|planet| sieges.defense(**planet).depot < 1.0)
            .count() as u32,
        fleets_dry_abroad: fleets
            .list
            .iter()
            .filter(|fleet| {
                fleet.alive
                    && fleet.min_fuel() < 0.05
                    && !galaxy.owns_planet_in(fleet.owner, fleet.system)
            })
            .count() as u32,
        sieges: sieges.stats.clone(),
        seed: config.seed,
        days: sim.clock().day(),
        hash: sim.hash(),
        systems: galaxy.systems.len(),
        planets: galaxy.planets.len(),
        free_planets: galaxy
            .planets
            .iter()
            .filter(|planet| planet.owner.is_none())
            .count(),
        polities: sim
            .polities()
            .list
            .iter()
            .map(|polity| PolitySummary {
                name: polity.name.clone(),
                alive: polity.alive,
                planets: galaxy.planets_of(polity.id).count(),
                population: galaxy.population_of(polity.id),
                techs: research.known_count(polity.id),
                tiers: crate::data::TechBranch::ALL
                    .map(|branch| research.tier(data, polity.id, branch)),
                fleets: fleets.of(polity.id).count(),
                ships: fleets.ship_count(polity.id),
                // Von null an gezählt: Die Summe über nichts wäre sonst minus null.
                strength: fleets
                    .of(polity.id)
                    .fold(0.0, |sum, fleet| sum + fleet.strength as f64),
                orders: fleets.orders_of(polity.id).count(),
                arrears: fleets.arrears_of(polity.id) as f64,
                ships_lost: battles.ships_lost_by(polity.id) as usize,
                treasury: crate::economy::to_credits(economy.treasury(polity.id)),
                war_weariness: finance.polity(polity.id).war_weariness,
                cohesion: cohesion(polity.id),
                budget: economy
                    .budget(polity.id)
                    .map(|budget| budget.year())
                    .unwrap_or_default(),
                budget_months: economy
                    .budget(polity.id)
                    .map_or(0, |budget| budget.history.len()),
                ships_built: fleets.tally_of(polity.id).built,
                ships_laid_up: fleets.tally_of(polity.id).laid_up,
                fleets_lost: battles.fleets_lost_by(polity.id),
                months_at_war: fleets.tally_of(polity.id).months_at_war,
                months_in_arrears: fleets.tally_of(polity.id).months_in_arrears,
                fleet_value: fleets
                    .of(polity.id)
                    .fold(0.0, |sum, fleet| sum + fleet.build_value(data)),
                blockade_days: fleets.tally_of(polity.id).blockade_days,
                offensives: ai.plan(polity.id).offensives,
                campaigns: ai.plan(polity.id).campaigns,
                battles_won: battles.won_by(polity.id),
            })
            .collect(),
        economy: economy_stats(sim.world()),
        battles: battles.next_id as u64,
        ships_lost: battles.total_ships_lost() as u64,
        violation: check_invariants(sim.world()).err(),
        battle_outcomes: battles.outcomes,
        ai_applied: sim.command_log().ai_applied,
        ai_rejected: sim.command_log().ai_rejected,
        outcome: sim.outcome(),
    }
}

/// Ein kompletter KI-gegen-KI-Lauf.
pub fn run_ai_game(data: &GameData, seed: u64, days: u64) -> Headless {
    let mut sim = Headless::new(data.clone(), SimConfig { seed, player: None });
    sim.run_days(days);
    sim
}

/// An welcher Werft ein Drehbuch bestellt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum YardChoice {
    /// Die Werft der Hauptwelt.
    Capital,
    /// Die Kolonie mit den meisten Bauplätzen. Hat keine Kolonie eine Werft, geht die
    /// Bestellung an die größte Kolonie und wird dort abgelehnt: So steht es im Protokoll.
    Colony,
}

/// Was ein Ereignis eines Drehbuchs auslöst.
#[derive(Clone, Debug, PartialEq)]
pub enum ScriptedAct {
    /// A erklärt B den Krieg. Ruht die militärische Führung von A
    /// ([`MilitaryMode::Resting`]), schickt das Drehbuch außerdem alle Flotten von A im
    /// Blockadeauftrag in das Heimatsystem von B; sonst führt die KI sie selbst.
    War { a: PolityId, b: PolityId },
    /// A schließt mit B Frieden.
    Peace { a: PolityId, b: PolityId },
    /// Eine Bestellung: Entwürfe mit Stückzahl. Ohne `who` bestellt jede lebende
    /// Gemeinschaft dasselbe.
    Order {
        who: Option<PolityId>,
        ships: Vec<(u16, u32)>,
        yard: YardChoice,
    },
    /// Grenzkrieg: A erklärt seinem nächsten Nachbarn den Krieg ([`nearest_neighbour`]).
    /// Wer das ist, entscheidet sich an dem Tag in der Partie.
    BorderWar { a: PolityId },
    /// A schließt Frieden mit allen, mit denen es im Krieg liegt. Belagert ein Gegner
    /// eine Welt von A, kann nur er den Frieden anbieten: Dann schließt er ihn.
    PeaceWithAll { a: PolityId },
    /// Legt fest, wie weit die KI eine Gemeinschaft militärisch führt (ohne `who` alle):
    /// siehe [`Headless::set_military_mode`].
    Mode {
        who: Option<PolityId>,
        mode: MilitaryMode,
    },
    /// Wie `Mode`, aber für den nächsten Nachbarn von `of` ([`nearest_neighbour`]).
    NeighbourMode { of: PolityId, mode: MilitaryMode },
    /// Ein Flottenprogramm: Von diesem Tag an hält die Gemeinschaft ihre Marine auf dem
    /// `factor`-fachen der Startflotte und bestellt zu jedem Monatsbeginn nach, was fehlt.
    /// Faktor 0 beendet das Programm. Ohne `who` gilt es für jede lebende Gemeinschaft.
    Navy { who: Option<PolityId>, factor: f32 },
    /// Verteidigungsplan: Die Hauptwelt von `who` soll so viele reguläre Verbände halten
    /// (der Rest ihres Plans bleibt); ausgehoben wird aus dem Anspruch „Verteidigung".
    Garrison { who: PolityId, regulars: f32 },
    /// Ein Eroberungszug: A erklärt seinem nächsten Nachbarn den Krieg, und von diesem
    /// Tag an führt das Drehbuch die Flotten von A (die KI rüstet weiter, gibt ihnen aber
    /// keine Befehle): Sie schiffen zu Hause Truppen ein, belagern zuerst die nächste
    /// Welt des Gegners, die nicht sein Sitz ist, und dann seinen Sitz, bis er endet oder
    /// Frieden herrscht. Siehe [`WarScript::conduct`].
    Conquest { a: PolityId },
}

impl ScriptedAct {
    /// Reihenfolge am selben Tag: erst die Führung, dann Frieden, Krieg, Bestellungen.
    fn rank(&self) -> u8 {
        match self {
            ScriptedAct::Mode { .. } | ScriptedAct::NeighbourMode { .. } => 0,
            ScriptedAct::Peace { .. } | ScriptedAct::PeaceWithAll { .. } => 1,
            ScriptedAct::War { .. }
            | ScriptedAct::BorderWar { .. }
            | ScriptedAct::Conquest { .. } => 2,
            ScriptedAct::Order { .. } => 3,
            ScriptedAct::Navy { .. } | ScriptedAct::Garrison { .. } => 4,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ScriptedEvent {
    /// Tag, an dessen Beginn die Befehle eingestellt werden.
    pub day: u64,
    pub act: ScriptedAct,
}

impl ScriptedEvent {
    pub fn war(day: u64, a: PolityId, b: PolityId) -> Self {
        Self {
            day,
            act: ScriptedAct::War { a, b },
        }
    }

    pub fn peace(day: u64, a: PolityId, b: PolityId) -> Self {
        Self {
            day,
            act: ScriptedAct::Peace { a, b },
        }
    }

    /// Vom ersten Tag an ruht die militärische Führung aller Gemeinschaften durch die KI:
    /// für Drehbücher, die Flotten und Rüstung selbst führen.
    pub fn rest_military() -> Self {
        Self {
            day: 0,
            act: ScriptedAct::Mode {
                who: None,
                mode: MilitaryMode::Resting,
            },
        }
    }
}

/// Kriege, Bestellungen und Flottenprogramme nach Drehbuch. Ob Krieg herrscht, entscheidet
/// noch keine KI: Gefechte, Blockaden, Verluste, Nachbau und ihre Folgen für Märkte und
/// Staatskassen lassen sich nur so in ganzen Partien ansehen und prüfen. Das Drehbuch
/// stellt gewöhnliche Befehle ein, wie sie auch ein Spieler gäbe; abgelehnte stehen im
/// Befehlsprotokoll. Rüstung und Flotten führt dabei die KI, es sei denn, das Drehbuch
/// lässt ihre militärische Führung ruhen ([`ScriptedAct::Mode`]) und führt selbst.
#[derive(Clone, Debug, Default)]
pub struct WarScript {
    events: Vec<ScriptedEvent>,
}

impl WarScript {
    pub fn new(mut events: Vec<ScriptedEvent>) -> Self {
        // Nach Tag; am selben Tag in fester Reihenfolge der Arten, sonst wie angegeben.
        events.sort_by_key(|event| (event.day, event.act.rank()));
        Self { events }
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// Lebende Gemeinschaften, für die eine Angabe gilt.
    fn scope(sim: &Headless, who: Option<PolityId>) -> Vec<PolityId> {
        sim.polities()
            .alive()
            .map(|polity| polity.id)
            .filter(|id| who.is_none_or(|only| only == *id))
            .collect()
    }

    /// Die Gemeinschaft, die eine Angabe nennt, wenn es sie an diesem Tag nicht gibt.
    fn absent(sim: &Headless, who: Option<PolityId>) -> Option<PolityId> {
        who.filter(|id| {
            !sim.polities()
                .list
                .get(id.index())
                .is_some_and(|polity| polity.alive)
        })
    }

    fn issue(sim: &mut Headless, event: &ScriptedEvent) {
        match &event.act {
            ScriptedAct::Peace { a, b } => sim.issue(*a, SimCommand::MakePeace { target: *b }),
            ScriptedAct::War { a, b } => Self::open_war(sim, *a, *b),
            ScriptedAct::BorderWar { a } => {
                if let Some(b) = nearest_neighbour(sim.galaxy(), sim.polities(), *a) {
                    Self::open_war(sim, *a, b);
                }
            }
            ScriptedAct::Garrison { who, regulars } => {
                let home = sim
                    .polities()
                    .list
                    .get(who.index())
                    .and_then(|polity| polity.capital);
                if let Some(planet) = home {
                    let mut plan = sim
                        .world()
                        .resource::<crate::siege::Sieges>()
                        .defense(planet)
                        .plan
                        .clone();
                    plan.regulars = *regulars;
                    // Diesen Plan lässt die KI der Gemeinschaft fortan stehen.
                    sim.world_mut()
                        .resource_mut::<AiState>()
                        .defense_hold
                        .insert(planet);
                    sim.issue(*who, SimCommand::SetDefensePlan { planet, plan });
                }
            }
            ScriptedAct::Conquest { a } => {
                if let Some(b) = nearest_neighbour(sim.galaxy(), sim.polities(), *a) {
                    // Die Flotten führt von nun an das Drehbuch; gerüstet wird weiter.
                    sim.set_military_mode(Some(*a), MilitaryMode::PassiveFleet);
                    sim.issue(*a, SimCommand::DeclareWar { target: b });
                }
            }
            ScriptedAct::PeaceWithAll { a } => {
                let enemies: Vec<PolityId> = sim
                    .world()
                    .resource::<crate::diplomacy::Diplomacy>()
                    .enemies_of(*a)
                    .collect();
                for target in enemies {
                    // Frieden kann nur der Belagerer anbieten: Belagert der Gegner eine
                    // Welt von A, schließt er ihn.
                    let besieged = crate::siege::peace_veto(
                        sim.galaxy(),
                        sim.polities(),
                        sim.sieges(),
                        *a,
                        target,
                    )
                    .is_err();
                    if besieged {
                        sim.issue(target, SimCommand::MakePeace { target: *a });
                    } else {
                        sim.issue(*a, SimCommand::MakePeace { target });
                    }
                }
            }
            ScriptedAct::Mode { who, mode } => sim.set_military_mode(*who, *mode),
            ScriptedAct::NeighbourMode { of, mode } => {
                if let Some(neighbour) = nearest_neighbour(sim.galaxy(), sim.polities(), *of) {
                    sim.set_military_mode(Some(neighbour), *mode);
                }
            }
            ScriptedAct::Order { who, ships, yard } => {
                if let Some(absent) = Self::absent(sim, *who) {
                    // Die Bestellung ergeht trotzdem: So steht ihre Ablehnung im Protokoll.
                    for (design, _) in ships {
                        sim.issue(
                            absent,
                            SimCommand::BuildShip {
                                planet: PlanetId(0),
                                design: *design,
                            },
                        );
                    }
                }
                for polity in Self::scope(sim, *who) {
                    let Some(planet) = Self::yard(sim, polity, *yard) else {
                        continue;
                    };
                    for (design, count) in ships {
                        for _ in 0..*count {
                            sim.issue(
                                polity,
                                SimCommand::BuildShip {
                                    planet,
                                    design: *design,
                                },
                            );
                        }
                    }
                }
            }
            // Das Programm bestellt zum Monatsbeginn, siehe `top_up`. Gibt es die
            // Gemeinschaft nicht, steht eine abgelehnte Bestellung im Protokoll.
            ScriptedAct::Navy { who, .. } => {
                let design = sim
                    .world()
                    .resource::<GameData>()
                    .balance
                    .military
                    .starting_fleet_ids
                    .first()
                    .map(|(design, _)| *design);
                if let (Some(absent), Some(design)) = (Self::absent(sim, *who), design) {
                    sim.issue(
                        absent,
                        SimCommand::BuildShip {
                            planet: PlanetId(0),
                            design,
                        },
                    );
                }
            }
        }
    }

    /// Die Welt, an deren Werft eine Bestellung geht.
    fn yard(sim: &Headless, polity: PolityId, choice: YardChoice) -> Option<PlanetId> {
        let capital = sim.polities().get(polity).capital;
        if choice == YardChoice::Capital {
            return capital;
        }
        let world = sim.world();
        let (data, galaxy) = (world.resource::<GameData>(), sim.galaxy());
        let (economy, fleets) = (world.resource::<Economy>(), world.resource::<Fleets>());
        galaxy
            .planets_of(polity)
            .filter(|planet| Some(planet.id) != capital)
            .map(|planet| {
                let slots = yard_slots(data, galaxy, economy, sim.polities(), fleets, planet.id);
                (slots, planet.population, planet.id)
            })
            .max_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)).then(b.2.cmp(&a.2)))
            .map(|(_, _, planet)| planet)
    }

    fn open_war(sim: &mut Headless, a: PolityId, b: PolityId) {
        sim.issue(a, SimCommand::DeclareWar { target: b });
        // Nennt das Drehbuch eine Gemeinschaft, die es nicht gibt, wird die Erklärung
        // abgelehnt und steht so im Protokoll; Flottenbefehle folgen ihr dann nicht.
        let exists = |polity: PolityId| {
            sim.polities()
                .list
                .get(polity.index())
                .is_some_and(|found| found.alive)
        };
        if !exists(a) || !exists(b) {
            return;
        }
        // Flotten führt das Drehbuch nur, wo es die KI nicht tut.
        if sim.world().resource::<AiState>().mode(a) != MilitaryMode::Resting {
            return;
        }
        let target = sim
            .polities()
            .list
            .get(b.index())
            .and_then(|polity| polity.capital)
            .map(|capital| sim.galaxy().planet(capital).system);
        let own: Vec<FleetId> = sim
            .world()
            .resource::<Fleets>()
            .of(a)
            .map(|fleet| fleet.id)
            .collect();
        for fleet in own {
            sim.issue(
                a,
                SimCommand::SetStance {
                    fleet,
                    stance: Stance::Blockade,
                },
            );
            if let Some(to) = target {
                sim.issue(a, SimCommand::MoveFleet { fleet, to });
            }
        }
    }

    /// Führt den Eroberungszug von `a` einen Tag weiter, mit gewöhnlichen Befehlen:
    ///
    /// - Ziel ist die Welt des Gegners, die dem eigenen Sitz am nächsten liegt und nicht
    ///   sein Sitz ist, solange `a` ihm noch keine Welt genommen hat; danach sein Sitz.
    /// - Zu Hause vereinigen sich die Flotten mit der größten dort; kein Neubau läuft
    ///   einzeln aus. Jede Flotte schifft zu Hause ein, was an Regulären bereitsteht und
    ///   an Bord passt, fährt mit vollen Tanks im Blockadeauftrag zum Ziel und belagert es. Mit
    ///   fast leeren Tanks kehrt sie heim und kommt wieder; ebenso, wenn die Welt reif
    ///   zur Übernahme ist, aber weder am Boden noch an Bord eine Truppe steht.
    /// - Eine Belagerung, die älter ist als `siege.max_siege_days`, hebt das Drehbuch auf
    ///   und setzt neu an.
    /// - In den ersten zehn Tagen einer Belagerung wird gezielt gefeuert, danach
    ///   geschwiegen. Gelandet und gestürmt wird, sobald die Truppen an Bord der
    ///   Garnison an der Oberfläche anderthalbfach überlegen sind; sonst wartet die
    ///   Besatzungstruppe an Bord, bis die Welt ausgehungert ist.
    pub fn conduct(sim: &mut Headless, a: PolityId) {
        use crate::siege::{Fire, SiegeOrders, Sieges};
        let mut commands: Vec<SimCommand> = Vec::new();
        {
            let world = sim.world();
            let (data, galaxy, polities) =
                (world.resource::<GameData>(), sim.galaxy(), sim.polities());
            let diplomacy = world.resource::<crate::diplomacy::Diplomacy>();
            let (fleets, sieges) = (world.resource::<Fleets>(), world.resource::<Sieges>());
            let balance = &data.balance.siege;
            let Some(own) = polities.list.get(a.index()).filter(|polity| polity.alive) else {
                return;
            };
            let Some(home) = own.capital else { return };
            let home_system = galaxy.planet(home).system;
            let Some(enemy) = diplomacy.enemies_of(a).min() else {
                return;
            };
            let seat = polities.get(enemy).capital;
            let hops = galaxy.hops_from(home_system);
            let taken = galaxy
                .planets_of(a)
                .any(|planet| sieges.defense(planet.id).occupied_from == Some(enemy));
            let outlying = galaxy
                .planets_of(enemy)
                .filter(|planet| Some(planet.id) != seat)
                .min_by_key(|planet| (hops[planet.system.index()], planet.id))
                .map(|planet| planet.id);
            let Some(target) = outlying.filter(|_| !taken).or(seat) else {
                return;
            };
            let world_of = galaxy.planet(target);
            let mut home_left = sieges.defense(home).regulars;
            // Reif zur Übernahme, aber niemand da, der sie annimmt: Die Garnison ist
            // zerfallen, am Boden und an Bord steht keine Truppe, und zu Hause warten
            // Verbände. Dann holt die Flotte sie, statt weiter im Orbit zu liegen.
            let berths = |fleet: &crate::fleet::Fleet| {
                fleet
                    .ships
                    .iter()
                    .map(|ship| {
                        data.hulls[data.designs[ship.design as usize].hull_id as usize].segments
                            as f64
                    })
                    .sum::<f64>()
                    * balance.berth_per_segment
            };
            let troops_aboard: f64 = fleets
                .of(a)
                .filter_map(|fleet| sieges.embarked.get(&fleet.id))
                .map(|force| force.troops)
                .sum();
            let ripe = sieges
                .siege(target)
                .filter(|siege| siege.attacker == a)
                .is_some_and(|siege| {
                    let surface = siege.zone_militia() + siege.reserve(sieges.defense(target));
                    siege.army <= 0.0 && surface < 1.0
                })
                && troops_aboard <= 0.0
                && home_left >= 1.0;
            // Die größte Flotte, die einsatzbereit und ohne Marschbefehl zu Hause liegt.
            let flagship = fleets
                .of(a)
                .filter(|fleet| {
                    fleet.is_ready() && fleet.route.is_empty() && fleet.system == home_system
                })
                .max_by(|x, y| {
                    x.build_value(data)
                        .total_cmp(&y.build_value(data))
                        .then(y.id.cmp(&x.id))
                })
                .map(|fleet| fleet.id);
            // Zum Ziel fährt nur, wer wenigstens den halben Bauwert einer Startflotte
            // hat, oder die größte Flotte der Gemeinschaft.
            let largest = fleets
                .of(a)
                .max_by(|x, y| {
                    x.build_value(data)
                        .total_cmp(&y.build_value(data))
                        .then(y.id.cmp(&x.id))
                })
                .map(|fleet| fleet.id);
            let strong = |fleet: &crate::fleet::Fleet| {
                Some(fleet.id) == largest
                    || fleet.build_value(data) >= 0.5 * data.balance.military.starting_fleet_value
            };
            for fleet in fleets.of(a) {
                if !fleet.is_docked() || fleet.in_battle || fleet.is_regrouping() {
                    continue;
                }
                let aboard = sieges
                    .embarked
                    .get(&fleet.id)
                    .map_or(0.0, |force| force.troops);
                let assigned = sieges.assign.get(&fleet.id) == Some(&target);
                // Mit fast leeren Tanks geht es heim, von wo auch immer: Ohne Treibstoff
                // hält niemand einen Orbit.
                let dry = fleet.min_fuel() < 0.1;
                let at_home = fleet.system == home_system;
                if fleet.system == world_of.system && !dry && !at_home {
                    if (ripe && berths(fleet) >= 1.0) || (!assigned && !strong(fleet)) {
                        commands.push(SimCommand::MoveFleet {
                            fleet: fleet.id,
                            to: home_system,
                        });
                    } else if !assigned {
                        commands.push(SimCommand::BeginSiege {
                            fleet: fleet.id,
                            planet: target,
                        });
                    }
                    continue;
                }
                if at_home {
                    // Einzeln läuft kein neues Schiff aus: Was zu Hause ohne Marschbefehl
                    // liegt, stößt erst zur größten Flotte dort. Sonst gingen Neubauten
                    // Stück für Stück an der Verteidigung des Ziels verloren.
                    if let Some(flagship) = flagship.filter(|flagship| *flagship != fleet.id)
                        && fleet.route.is_empty()
                    {
                        commands.push(SimCommand::MergeFleets {
                            from: fleet.id,
                            into: flagship,
                        });
                        continue;
                    }
                    // Einschiffen, was da ist und an Bord passt; mit vollen Tanks los.
                    let strength = (berths(fleet) - aboard).min(home_left).floor();
                    if strength >= 1.0 {
                        home_left -= strength;
                        commands.push(SimCommand::EmbarkTroops {
                            fleet: fleet.id,
                            planet: home,
                            strength,
                            supply_days: 60,
                        });
                    }
                    if world_of.system == home_system {
                        // Das Ziel liegt im eigenen Heimatsystem: belagern, wo sie liegt.
                        if !assigned && !dry {
                            commands.push(SimCommand::BeginSiege {
                                fleet: fleet.id,
                                planet: target,
                            });
                        }
                    } else if fleet.min_fuel() >= 0.9 && strong(fleet) {
                        commands.push(SimCommand::SetStance {
                            fleet: fleet.id,
                            stance: Stance::Blockade,
                        });
                        commands.push(SimCommand::MoveFleet {
                            fleet: fleet.id,
                            to: world_of.system,
                        });
                    }
                    continue;
                }
                // Unterwegs: Wer leer fährt, während zu Hause Truppen warten, holt sie erst.
                let fetch = aboard <= 0.0 && home_left >= 1.0 && berths(fleet) >= 1.0;
                let to = if dry || fetch || !strong(fleet) {
                    home_system
                } else {
                    world_of.system
                };
                commands.push(SimCommand::MoveFleet {
                    fleet: fleet.id,
                    to,
                });
            }
            // Die laufende Belagerung des Ziels.
            if let Some(siege) = sieges.siege(target).filter(|siege| siege.attacker == a) {
                let defense = sieges.defense(target);
                let age = sim.clock().day().saturating_sub(siege.started_day);
                // Nach der Frist, in der jede Einschließung endet, ist diese gescheitert:
                // aufheben und neu ansetzen, statt eine Truppe über Jahre am Boden zu lassen.
                if age > balance.max_siege_days as u64 {
                    commands.push(SimCommand::LiftSiege { planet: target });
                }
                let fire = if age < 10 { Fire::Precise } else { Fire::Hold };
                let orders = SiegeOrders {
                    fire,
                    assault: true,
                    focus: 0,
                    sack: false,
                };
                if siege.orders != orders {
                    commands.push(SimCommand::SetSiegeOrders {
                        planet: target,
                        orders,
                    });
                }
                let surface = siege.zone_militia() + siege.reserve(defense);
                let aboard: Vec<(FleetId, f64)> = sieges
                    .assign
                    .iter()
                    .filter(|(_, planet)| **planet == target)
                    .filter_map(|(fleet, _)| Some((*fleet, sieges.embarked.get(fleet)?.troops)))
                    .filter(|(_, troops)| *troops > 0.0)
                    .collect();
                let total: f64 = aboard.iter().map(|(_, troops)| troops).sum();
                let pending = sieges.pending.iter().any(|due| due.planet == target);
                if defense.stations <= 0.0 && !pending && total >= 1.5 * surface && total > 0.0 {
                    for (fleet, strength) in aboard {
                        commands.push(SimCommand::LandTroops {
                            fleet,
                            planet: target,
                            strength,
                        });
                    }
                }
            }
        }
        for command in commands {
            sim.issue(a, command);
        }
    }

    /// Das Flottenprogramm jeder Gemeinschaft am Tag `day`: der Faktor der letzten
    /// Angabe, die bis dahin für sie galt.
    fn programmes(&self, sim: &Headless, day: u64) -> Vec<(PolityId, f32)> {
        let mut factors: BTreeMap<PolityId, f32> = BTreeMap::new();
        for event in self.events.iter().filter(|event| event.day <= day) {
            if let ScriptedAct::Navy { who, factor } = &event.act {
                for polity in Self::scope(sim, *who) {
                    factors.insert(polity, *factor);
                }
            }
        }
        factors
            .into_iter()
            .filter(|(_, factor)| *factor > 0.0)
            .collect()
    }

    /// Bestellt nach, was der Marine einer Gemeinschaft zum `factor`-fachen der Startflotte
    /// fehlt; Schiffe in Flotten und offene Aufträge zählen als vorhanden. Die Aufträge
    /// gehen an die eigenen Werften mit der kürzesten Auftragsliste, bei Gleichstand an die
    /// Hauptwelt. Nimmt keine Werft mehr etwas an, wartet der Rest bis zum nächsten Monat.
    fn top_up(sim: &mut Headless, polity: PolityId, factor: f32) {
        let orders: Vec<(PlanetId, u16)> = {
            let world = sim.world();
            let (data, galaxy) = (world.resource::<GameData>(), sim.galaxy());
            let (economy, fleets) = (world.resource::<Economy>(), world.resource::<Fleets>());
            let capital = sim.polities().get(polity).capital;
            let limit = data.balance.military.yard_queue as usize;
            let mut yards: Vec<(PlanetId, usize)> = galaxy
                .planets_of(polity)
                .filter(|planet| {
                    yard_slots(data, galaxy, economy, sim.polities(), fleets, planet.id) > 0
                })
                .map(|planet| (planet.id, fleets.orders_at(planet.id).count()))
                .collect();
            let mut orders = Vec::new();
            'designs: for (design, count) in &data.balance.military.starting_fleet_ids {
                let target = (*count as f32 * factor).round() as usize;
                let afloat = fleets
                    .of(polity)
                    .flat_map(|fleet| fleet.ships.iter())
                    .filter(|ship| ship.design == *design)
                    .count();
                let ordered = fleets
                    .orders_of(polity)
                    .filter(|order| order.design == *design)
                    .count();
                for _ in afloat + ordered..target {
                    let Some(yard) = yards
                        .iter_mut()
                        .filter(|(_, queued)| *queued < limit)
                        .min_by_key(|(planet, queued)| {
                            (*queued, Some(*planet) != capital, *planet)
                        })
                    else {
                        break 'designs;
                    };
                    yard.1 += 1;
                    orders.push((yard.0, *design));
                }
            }
            orders
        };
        for (planet, design) in orders {
            sim.issue(polity, SimCommand::BuildShip { planet, design });
        }
    }

    /// Lässt die Partie bis zum Beginn des Tages `day` laufen und stellt unterwegs die
    /// fälligen Befehle ein. `exact` rechnet jeden Takt, statt Leertakte zu überspringen.
    pub fn run_until(&self, sim: &mut Headless, day: u64, exact: bool) {
        let advance = |sim: &mut Headless, to: u64| {
            let days = to.saturating_sub(sim.clock().day());
            if exact {
                sim.run_days_exact(days);
            } else {
                sim.run_days(days);
            }
        };
        let now = sim.clock().day();
        // Halt an jedem Ereignistag und, sobald ein Flottenprogramm läuft, an jedem
        // Monatsbeginn.
        let mut stops: BTreeSet<u64> = self
            .events
            .iter()
            .map(|event| event.day)
            .filter(|at| (now..day).contains(at))
            .collect();
        let first_programme = self
            .events
            .iter()
            .filter(|event| matches!(event.act, ScriptedAct::Navy { .. }))
            .map(|event| event.day)
            .min();
        if let Some(first) = first_programme {
            let mut at = first.max(now).next_multiple_of(DAYS_PER_MONTH);
            while at < day {
                stops.insert(at);
                at += DAYS_PER_MONTH;
            }
        }
        // Ein Eroberungszug wird vom Tag seiner Kriegserklärung an täglich geführt.
        let conquests: Vec<(u64, PolityId)> = self
            .events
            .iter()
            .filter_map(|event| match event.act {
                ScriptedAct::Conquest { a } => Some((event.day, a)),
                _ => None,
            })
            .collect();
        if let Some(first) = conquests.iter().map(|(from, _)| *from).min() {
            stops.extend(first.max(now)..day);
        }
        for stop in stops {
            advance(sim, stop);
            for event in self.events.iter().filter(|event| event.day == stop) {
                Self::issue(sim, event);
            }
            for (from, a) in &conquests {
                if stop > *from {
                    Self::conduct(sim, *a);
                }
            }
            if stop.is_multiple_of(DAYS_PER_MONTH) {
                for (polity, factor) in self.programmes(sim, stop) {
                    Self::top_up(sim, polity, factor);
                }
            }
        }
        advance(sim, day);
    }
}

/// Der nächste Nachbar einer Gemeinschaft: die lebende andere, deren Sitz dem eigenen im
/// Routennetz am nächsten liegt (nach Sprüngen, bei Gleichstand die kleinere Nummer).
pub fn nearest_neighbour(galaxy: &Galaxy, polities: &Polities, of: PolityId) -> Option<PolityId> {
    let seat = |polity: &crate::polity::Polity| {
        polity
            .seat(galaxy)
            .map(|planet| galaxy.planet(planet).system)
    };
    let own = polities
        .list
        .get(of.index())
        .filter(|polity| polity.alive)?;
    let hops = galaxy.hops_from(seat(own)?);
    polities
        .alive()
        .filter(|polity| polity.id != of)
        .filter_map(|polity| Some((hops[seat(polity)?.index()], polity.id)))
        .min()
        .map(|(_, id)| id)
}

/// Das Drehbuch „Grenzkrieg“ für Sweeps: Die erste Gemeinschaft erklärt zu Beginn des
/// Jahres 25 ihrem nächsten Nachbarn den Krieg und schließt zu Beginn des Jahres 45
/// Frieden.
pub fn border_war() -> Vec<ScriptedEvent> {
    let year = crate::clock::DAYS_PER_YEAR;
    vec![
        ScriptedEvent {
            day: 25 * year,
            act: ScriptedAct::BorderWar { a: PolityId(0) },
        },
        ScriptedEvent {
            day: 45 * year,
            act: ScriptedAct::PeaceWithAll { a: PolityId(0) },
        },
    ]
}

/// Das Drehbuch „Eroberung“ für Sweeps: Die erste Gemeinschaft rüstet ihre Heimatwelt
/// vom Jahr 20 an mit Truppen auf, erklärt zu Beginn des Jahres 25 ihrem nächsten
/// Nachbarn den Krieg, belagert zuerst dessen nächste Welt und dann seinen Sitz
/// ([`WarScript::conduct`]) und schließt zu Beginn des Jahres 45 Frieden, wenn der Gegner
/// dann noch besteht. Verlorene Schiffe werden nachgebaut.
pub fn conquest_war() -> Vec<ScriptedEvent> {
    let year = crate::clock::DAYS_PER_YEAR;
    let a = PolityId(0);
    vec![
        ScriptedEvent {
            day: 20 * year,
            act: ScriptedAct::Garrison {
                who: a,
                regulars: 25.0,
            },
        },
        ScriptedEvent {
            day: 25 * year,
            act: ScriptedAct::Navy {
                who: Some(a),
                factor: 1.0,
            },
        },
        ScriptedEvent {
            day: 25 * year,
            act: ScriptedAct::Conquest { a },
        },
        ScriptedEvent {
            day: 45 * year,
            act: ScriptedAct::PeaceWithAll { a },
        },
    ]
}

/// Das Drehbuch „Feldzug“ für Sweeps: Das Drehbuch erklärt nur die Kriege und schließt
/// die Frieden; Flotten, Belagerungen und Verteidigung führt auf allen Seiten die KI. Die
/// ersten vier Gemeinschaften erklären nacheinander (zu Beginn der Jahre 20, 25, 30 und
/// 35) ihrem dann nächsten Nachbarn den Krieg und schließen je 15 Jahre später Frieden
/// mit allen, mit denen sie dann im Krieg liegen.
pub fn campaign_wars() -> Vec<ScriptedEvent> {
    let year = crate::clock::DAYS_PER_YEAR;
    (0..4u64)
        .flat_map(|index| {
            let a = PolityId(index as u32);
            let start = (20 + 5 * index) * year;
            [
                ScriptedEvent {
                    day: start,
                    act: ScriptedAct::BorderWar { a },
                },
                ScriptedEvent {
                    day: start + 15 * year,
                    act: ScriptedAct::PeaceWithAll { a },
                },
            ]
        })
        .collect()
}

/// Wie [`run_ai_game`], prüft aber jährlich die Invarianten und meldet den ersten Verstoß.
pub fn run_checked(data: &GameData, seed: u64, days: u64) -> (Headless, Option<String>) {
    run_scripted(data, seed, days, &WarScript::default())
}

/// Wie [`run_checked`], mit Kriegen nach Drehbuch.
pub fn run_scripted(
    data: &GameData,
    seed: u64,
    days: u64,
    script: &WarScript,
) -> (Headless, Option<String>) {
    let mut sim = Headless::new(data.clone(), SimConfig { seed, player: None });
    let mut violation = None;
    let mut done = 0;
    while done < days {
        let step = crate::clock::DAYS_PER_YEAR.min(days - done);
        script.run_until(&mut sim, done + step, false);
        done += step;
        if violation.is_none()
            && let Err(message) = check_invariants(sim.world())
        {
            violation = Some(format!("Tag {done}: {message}"));
        }
    }
    (sim, violation)
}

/// Viele Seeds parallel. Jeder Lauf ist für sich einfädig und deterministisch; die
/// Ergebnisse kommen in Seed-Reihenfolge zurück, unabhängig von der Thread-Zahl.
pub fn sweep(data: &GameData, seeds: &[u64], days: u64, jobs: usize) -> Vec<RunSummary> {
    sweep_scripted(data, seeds, days, jobs, &WarScript::default())
}

/// Wie [`sweep`], jeder Lauf mit denselben Kriegen nach Drehbuch.
pub fn sweep_scripted(
    data: &GameData,
    seeds: &[u64],
    days: u64,
    jobs: usize,
    script: &WarScript,
) -> Vec<RunSummary> {
    let jobs = jobs.clamp(1, seeds.len().max(1));
    let next = std::sync::atomic::AtomicUsize::new(0);
    let results = std::sync::Mutex::new(Vec::with_capacity(seeds.len()));
    std::thread::scope(|scope| {
        for _ in 0..jobs {
            scope.spawn(|| {
                loop {
                    let index = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let Some(seed) = seeds.get(index) else {
                        break;
                    };
                    let (sim, violation) = run_scripted(data, *seed, days, script);
                    let mut summary = summarize(&sim);
                    summary.violation = violation.or(summary.violation);
                    results
                        .lock()
                        .expect("Sweep-Thread abgestürzt")
                        .push((index, summary));
                }
            });
        }
    });
    let mut results = results.into_inner().expect("Sweep-Thread abgestürzt");
    results.sort_by_key(|(index, _)| *index);
    results.into_iter().map(|(_, summary)| summary).collect()
}

impl Headless {
    /// Ausgang der Partie; `None`, solange sie läuft.
    pub fn outcome(&self) -> Option<Outcome> {
        self.world()
            .get_resource::<Endgame>()
            .and_then(|endgame| endgame.outcome)
    }

    /// Zustand der Belagerungen und der stehenden Verteidigung.
    pub fn sieges(&self) -> &crate::siege::Sieges {
        self.world().resource::<crate::siege::Sieges>()
    }

    /// Alle laufenden Belagerungen als Lesemodell, wie eine Oberfläche sie zeigt.
    pub fn siege_cards(&self) -> Vec<crate::siege::SiegeCard> {
        let refs = crate::siege::SiegeRefs::of_world(self.world());
        crate::siege::siege_cards(&refs, self.clock().day())
    }

    /// Der Grund, aus dem der Kern einen Befehl der Belagerung jetzt ablehnen würde.
    pub fn command_block(&self, polity: PolityId, command: &SimCommand) -> Option<String> {
        let refs = crate::siege::SiegeRefs::of_world(self.world());
        crate::siege::command_block(&refs, polity, command, self.clock().tick)
    }

    /// Besitzwechsel, Gründung und Ende einer Gemeinschaft direkt auf der Welt, für Tests
    /// und Werkzeuge: `sim.succession(|s| s.absorb(from, into, Path::Joined, tick))`.
    pub fn succession<R>(&mut self, run: impl FnOnce(&mut Succession) -> R) -> R {
        let world = self.world_mut();
        let mut state = SystemState::<SuccessionParams>::new(world);
        let mut params = state.get_mut(world).expect("Partie läuft");
        run(&mut params.borrow())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::best_colony_target;
    use crate::clock::DAYS_PER_YEAR;
    use crate::command::CommandOutcome;
    use crate::state::{SAVE_FORMAT, SaveError};

    fn data() -> GameData {
        GameData::load_workspace().unwrap()
    }

    #[test]
    fn same_seed_gives_identical_run() {
        let data = data();
        let a = run_ai_game(&data, 7, 3 * DAYS_PER_YEAR);
        let b = run_ai_game(&data, 7, 3 * DAYS_PER_YEAR);
        let c = run_ai_game(&data, 8, 3 * DAYS_PER_YEAR);
        assert_eq!(a.hash(), b.hash());
        assert_ne!(a.hash(), c.hash());
        // Die Welt hat sich wirklich bewegt.
        assert!(
            a.hash()
                != Headless::new(
                    data,
                    SimConfig {
                        seed: 7,
                        player: None
                    }
                )
                .hash()
        );
    }

    #[test]
    fn skipping_idle_ticks_changes_nothing() {
        let data = data();
        let config = SimConfig {
            seed: 21,
            player: None,
        };
        let mut fast = Headless::new(data.clone(), config.clone());
        let mut exact = Headless::new(data, config);
        fast.run_days(400);
        exact.run_days_exact(400);
        assert_eq!(fast.clock(), exact.clock());
        assert_eq!(fast.hash(), exact.hash());
    }

    /// Spielt als Gemeinschaft 0 und gründet zu festen Zeiten Kolonien.
    fn play_scripted(sim: &mut Headless) {
        let me = PolityId(0);
        for _ in 0..4 {
            sim.run_days(250);
            let day = sim.clock().day();
            let target = {
                let world = sim.world();
                best_colony_target(
                    world.resource::<GameData>(),
                    world.resource::<Galaxy>(),
                    world.resource::<Polities>(),
                    world.resource::<crate::economy::Economy>(),
                    me,
                    day,
                )
            };
            if let Some(planet) = target {
                sim.issue(me, SimCommand::ClaimPlanet { planet });
            }
            // Ein abgelehnter Befehl gehört ebenfalls ins Protokoll.
            sim.issue(
                me,
                SimCommand::ClaimPlanet {
                    planet: crate::ids::PlanetId(9_999),
                },
            );
        }
        sim.run_days(100);
    }

    #[test]
    fn command_log_replays_to_the_same_state() {
        let data = data();
        let config = SimConfig {
            seed: 5,
            player: Some(PolityId(0)),
        };

        let mut original = Headless::new(data.clone(), config.clone());
        play_scripted(&mut original);
        let log = original.command_log().clone();
        assert!(
            log.entries
                .iter()
                .any(|entry| entry.outcome == CommandOutcome::Applied)
        );
        assert!(
            log.entries
                .iter()
                .any(|entry| matches!(entry.outcome, CommandOutcome::Rejected(_)))
        );

        let mut replay = Headless::new(data.clone(), config.clone());
        replay.set_replay(&log);
        replay.run_days(original.clock().day());
        assert_eq!(replay.hash(), original.hash());

        // Ohne die Befehle verläuft die Partie anders.
        let mut passive = Headless::new(data, config);
        passive.run_days(original.clock().day());
        assert_ne!(passive.hash(), original.hash());
    }

    #[test]
    fn load_and_continue_matches_uninterrupted_run() {
        let data = data();
        let config = SimConfig {
            seed: 13,
            player: None,
        };
        let mut straight = Headless::new(data.clone(), config.clone());
        straight.run_days(900);

        let mut first_half = Headless::new(data.clone(), config);
        first_half.run_days(450);
        let text = first_half.save().to_ron().unwrap();
        let save = SaveGame::from_ron(&text).unwrap();
        assert!(save.matches_data(&data));
        let mut resumed = Headless::from_save(data, save);
        assert_eq!(
            resumed.hash(),
            first_half.hash(),
            "Laden muss den Zustand exakt herstellen"
        );
        resumed.run_days(450);
        assert_eq!(resumed.hash(), straight.hash());
    }

    #[test]
    fn space_runs_out() {
        let data = data();
        let sim = run_ai_game(&data, 2, 40 * DAYS_PER_YEAR);
        let summary = summarize(&sim);
        // Nach 40 Jahren ist der Raum verteilt; frei sind höchstens Welten, die gerade
        // aufgegeben wurden.
        assert!(
            summary.free_planets * 10 <= summary.planets,
            "{} von {} Planeten noch frei",
            summary.free_planets,
            summary.planets
        );
        // Niemand geht bei der Ausdehnung leer aus.
        for polity in &summary.polities {
            assert!(polity.planets >= 2, "{} blieb ohne Kolonien", polity.name);
        }
    }

    #[test]
    fn sweep_is_independent_of_thread_count() {
        let data = data();
        let seeds = [1, 2, 3, 4, 5];
        let serial = sweep(&data, &seeds, 200, 1);
        let parallel = sweep(&data, &seeds, 200, 4);
        let hashes =
            |runs: &[RunSummary]| runs.iter().map(|r| (r.seed, r.hash)).collect::<Vec<_>>();
        assert_eq!(hashes(&serial), hashes(&parallel));
    }

    #[test]
    fn a_scripted_war_leads_to_battles_and_is_the_same_with_every_tick() {
        let data = data();
        // Angegeben in beliebiger Reihenfolge; gespielt nach Tagen.
        // Das Drehbuch führt die Flotten selbst; die militärische Führung der KI ruht.
        let script = WarScript::new(vec![
            ScriptedEvent::rest_military(),
            ScriptedEvent::peace(700, PolityId(0), PolityId(1)),
            ScriptedEvent::war(40, PolityId(0), PolityId(1)),
            ScriptedEvent::war(40, PolityId(2), PolityId(1)),
        ]);
        assert!(!script.is_empty() && WarScript::default().is_empty());
        let (sim, violation) = run_scripted(&data, 4, 2 * DAYS_PER_YEAR, &script);
        assert_eq!(violation, None);
        let summary = summarize(&sim);
        assert!(
            summary.battles >= 1,
            "die Flotten sind aufeinandergetroffen"
        );
        assert!(summary.ships_lost > 0);
        assert_eq!(
            summary.ships_lost as usize,
            summary
                .polities
                .iter()
                .map(|polity| polity.ships_lost)
                .sum::<usize>()
        );
        // Die Befehle stehen im Protokoll; der Friede kam am genannten Tag.
        let log = sim.command_log();
        let declared = log
            .entries
            .iter()
            .filter(|entry| matches!(entry.command, SimCommand::DeclareWar { .. }))
            .count();
        assert_eq!(declared, 2);
        let peace = log
            .entries
            .iter()
            .find(|entry| matches!(entry.command, SimCommand::MakePeace { .. }))
            .expect("der Friedensschluss");
        assert_eq!(peace.tick, 700 * TICKS_PER_DAY);
        assert_eq!(peace.outcome, CommandOutcome::Applied);
        assert!(
            !sim.world()
                .resource::<crate::diplomacy::Diplomacy>()
                .at_war(PolityId(0), PolityId(1))
        );

        // In Jahresschritten oder am Stück, mit oder ohne Leertakte: derselbe Verlauf.
        let mut exact = Headless::new(
            data.clone(),
            SimConfig {
                seed: 4,
                player: None,
            },
        );
        script.run_until(&mut exact, 2 * DAYS_PER_YEAR, true);
        assert_eq!(exact.clock(), sim.clock());
        assert_eq!(exact.hash(), sim.hash());
        // Ohne Drehbuch ist es der gewöhnliche Lauf.
        let (plain, _) = run_scripted(&data, 4, 200, &WarScript::default());
        assert_eq!(plain.hash(), run_ai_game(&data, 4, 200).hash());
    }

    /// Steht eine Flotte im Gefecht, gibt es das Gefecht auch, und umgekehrt.
    fn fleets_match_battles(sim: &Headless) -> Result<(), String> {
        let world = sim.world();
        let fleets = world.resource::<Fleets>();
        let battles = world.resource::<crate::fleet::Battles>();
        for fleet in &fleets.list {
            let entry = battles.of_fleet(fleet.id).map(|(battle, _)| battle.id);
            if fleet.in_battle != entry.is_some() || fleet.battle != entry {
                return Err(format!(
                    "Takt {}: {} im Gefecht {} ({:?}), geführt in {entry:?}",
                    sim.clock().tick,
                    fleet.name,
                    fleet.in_battle,
                    fleet.battle
                ));
            }
        }
        check_invariants(world).map_err(|error| format!("Takt {}: {error}", sim.clock().tick))
    }

    /// Spielt `days` Tage und prüft nach jedem Tag, solange ein Gefecht läuft nach jedem
    /// Takt, dass Flotten und Gefechte zueinander passen. Gibt die Zahl der Gefechte der
    /// Partie zurück.
    fn play_watching_battles(sim: &mut Headless, days: u64, seed: u64) -> u32 {
        for _ in 0..days {
            if needs_fine_ticks(sim.world()) {
                for _ in 0..TICKS_PER_DAY {
                    sim.tick();
                    assert_eq!(fleets_match_battles(sim), Ok(()), "Seed {seed}");
                }
            } else {
                sim.run_days(1);
                assert_eq!(fleets_match_battles(sim), Ok(()), "Seed {seed}");
            }
        }
        sim.world().resource::<crate::fleet::Battles>().next_id
    }

    #[test]
    fn a_fleet_in_battle_always_has_its_battle_in_every_war() {
        let data = data();
        // Der Grenzkrieg der Sweeps: bis zur Kriegserklärung am Stück, dann sechs
        // Kriegsjahre unter Beobachtung.
        let start = 25 * DAYS_PER_YEAR;
        let script = WarScript::new(border_war());
        let mut fought = 0;
        // Vier Partien, in denen es in diesen Jahren zu Gefechten kommt.
        for seed in [1, 13, 18, 21] {
            let mut sim = Headless::new(data.clone(), SimConfig { seed, player: None });
            script.run_until(&mut sim, start + 1, false);
            let count = play_watching_battles(&mut sim, 6 * DAYS_PER_YEAR, seed);
            println!("Grenzkrieg, Seed {seed}: {count} Gefechte");
            fought += count;
        }
        assert!(fought >= 3, "nur {fought} Gefechte in vier Grenzkriegen");
        // Kriege, in denen das Drehbuch die Flotten gegen den Sitz des Gegners führt: mehr
        // Gefechte, auch mit drei Parteien, mit Rückzügen und Verlusten.
        let script = WarScript::new(vec![
            ScriptedEvent::rest_military(),
            ScriptedEvent::war(40, PolityId(0), PolityId(1)),
            ScriptedEvent::war(40, PolityId(2), PolityId(1)),
        ]);
        let mut fought = 0;
        for seed in 0..8 {
            let mut sim = Headless::new(data.clone(), SimConfig { seed, player: None });
            script.run_until(&mut sim, 41, false);
            let count = play_watching_battles(&mut sim, 2 * DAYS_PER_YEAR, seed);
            println!("Seed {seed}: {count} Gefechte");
            fought += count;
        }
        assert!(
            fought >= 8,
            "nur {fought} Gefechte in acht Kriegen nach Drehbuch"
        );
    }

    #[test]
    fn a_finished_battle_stays_readable_for_a_few_days() {
        use crate::fleet::Battles;
        let data = data();
        let script = WarScript::new(vec![
            ScriptedEvent::rest_military(),
            ScriptedEvent::war(40, PolityId(0), PolityId(1)),
            ScriptedEvent::war(40, PolityId(2), PolityId(1)),
        ]);
        let mut sim = Headless::new(
            data,
            SimConfig {
                seed: 4,
                player: None,
            },
        );
        let mut running: Vec<crate::ids::BattleId> = Vec::new();
        // Tag des Endes je Gefecht, sobald es aus den laufenden verschwunden ist.
        let mut ended: BTreeMap<crate::ids::BattleId, u64> = BTreeMap::new();
        for day in 1..=2 * DAYS_PER_YEAR {
            script.run_until(&mut sim, day, false);
            let battles = sim.world().resource::<Battles>();
            for id in running.drain(..) {
                if battles.get(id).is_none() {
                    // Eben beendet: Der letzte Stand und die Bilanz sind beide lesbar.
                    let last = battles.concluded(id).expect("der letzte Stand");
                    let report = battles.report(id).expect("die Bilanz");
                    assert!(last.finished && last.id == id && !last.ships.is_empty());
                    assert_eq!(last.combatants.len(), report.sides.len());
                    assert_eq!(report.system, last.system);
                    ended.insert(id, report.ended / TICKS_PER_DAY);
                }
            }
            running.extend(battles.active.iter().map(|battle| battle.id));
            // Nach wenigen Tagen ist der letzte Stand fort, die Bilanz bleibt.
            for (id, end) in &ended {
                // Am Stichtag selbst entscheidet, ob der Tagesanfang schon gerechnet ist.
                let kept = battles.concluded(*id).is_some();
                let note = format!("Gefecht {id}, beendet am Tag {end}, heute Tag {day}");
                assert!(kept || day >= end + Battles::CONCLUDED_DAYS, "{note}");
                assert!(!kept || day <= end + Battles::CONCLUDED_DAYS, "{note}");
            }
            assert!(battles.concluded.len() <= Battles::CONCLUDED);
            assert_eq!(check_invariants(sim.world()), Ok(()));
        }
        assert!(!ended.is_empty(), "kein Gefecht ging zu Ende");
        // Der aufbewahrte Stand übersteht Speichern und Laden.
        let text = sim.save().to_ron().unwrap();
        let loaded = Headless::from_save(
            sim.world().resource::<GameData>().clone(),
            SaveGame::from_ron(&text).unwrap(),
        );
        assert_eq!(loaded.hash(), sim.hash());
    }

    #[test]
    fn a_script_orders_ships_and_a_programme_replaces_what_a_war_costs() {
        let data = data();
        let corvette = data.design_index("dolch").unwrap();
        let (a, b) = (PolityId(0), PolityId(1));
        let start: usize = data
            .balance
            .military
            .starting_fleet_ids
            .iter()
            .map(|(_, count)| *count as usize)
            .sum();
        let script = WarScript::new(vec![
            // Rüstung und Flotten führt hier das Drehbuch, nicht die KI.
            ScriptedEvent::rest_military(),
            // Jede Gemeinschaft bestellt eine Korvette bei ihrer Hauptwelt …
            ScriptedEvent {
                day: 30,
                act: ScriptedAct::Order {
                    who: None,
                    ships: vec![(corvette, 1)],
                    yard: YardChoice::Capital,
                },
            },
            // … die beiden Kriegsparteien halten ihre Marine auf der Stärke der Startflotte.
            ScriptedEvent {
                day: 45,
                act: ScriptedAct::Navy {
                    who: Some(a),
                    factor: 1.0,
                },
            },
            ScriptedEvent {
                day: 45,
                act: ScriptedAct::Navy {
                    who: Some(b),
                    factor: 1.0,
                },
            },
            ScriptedEvent::war(40, a, b),
            ScriptedEvent::peace(3 * DAYS_PER_YEAR, a, b),
        ]);
        let (sim, violation) = run_scripted(&data, 4, 6 * DAYS_PER_YEAR, &script);
        assert_eq!(violation, None);
        let summary = summarize(&sim);
        assert!(summary.ships_lost > 0, "der Krieg hat Schiffe gekostet");
        let log = sim.command_log();
        let orders: Vec<_> = log
            .entries
            .iter()
            .filter(|entry| matches!(entry.command, SimCommand::BuildShip { .. }))
            .collect();
        // Die Bestellung für alle am genannten Tag, eine je Gemeinschaft …
        let first: Vec<_> = orders
            .iter()
            .filter(|entry| entry.tick == 30 * TICKS_PER_DAY)
            .collect();
        assert_eq!(first.len(), summary.polities.len());
        assert!(
            first
                .iter()
                .all(|entry| entry.outcome == CommandOutcome::Applied)
        );
        // … und das Programm bestellt nur zum Monatsbeginn, nur für die beiden, und erst
        // nach den ersten Verlusten.
        let later: Vec<_> = orders
            .iter()
            .filter(|entry| entry.tick > 30 * TICKS_PER_DAY)
            .collect();
        assert!(!later.is_empty(), "das Programm hat nachbestellt");
        for entry in &later {
            assert!(entry.tick.is_multiple_of(DAYS_PER_MONTH * TICKS_PER_DAY));
            assert!(entry.polity == a || entry.polity == b);
        }
        // Am Ende steht bei beiden wieder die Marine, die das Programm verlangt: Schiffe
        // und offene Aufträge zusammen ergeben mindestens die Startflotte.
        for polity in [a, b] {
            let entry = &summary.polities[polity.index()];
            assert!(entry.ships_lost > 0);
            assert!(
                entry.ships + entry.orders >= start,
                "{}: {} Schiffe und {} Aufträge",
                entry.name,
                entry.ships,
                entry.orders
            );
        }
        // Wer nicht im Programm ist, hat genau die eine Korvette mehr.
        assert_eq!(summary.polities[3].ships, start + 1);

        // Mit jedem Takt gerechnet derselbe Verlauf.
        let mut exact = Headless::new(
            data.clone(),
            SimConfig {
                seed: 4,
                player: None,
            },
        );
        script.run_until(&mut exact, 6 * DAYS_PER_YEAR, true);
        assert_eq!(exact.hash(), sim.hash());
    }

    #[test]
    fn wrong_save_format_is_rejected() {
        let data = data();
        let sim = Headless::new(
            data,
            SimConfig {
                seed: 1,
                player: None,
            },
        );
        let mut save = sim.save();
        save.format = 99;
        let text = save.to_ron().unwrap();
        assert!(matches!(
            SaveGame::from_ron(&text),
            Err(SaveError::Format { found: 99 })
        ));
    }

    #[test]
    fn an_older_format_is_named_even_if_its_state_does_not_decode() {
        // Ein Spielstand eines älteren Formats, dessen Zustand heute nicht mehr lesbar ist:
        // Felder fehlen, eines trägt einen anderen Typ.
        let text = "(format:2,data_hash:7,state:(clock:\"früher ein Text\",fleets:[1,2,3]))";
        assert!(ron::from_str::<SaveGame>(text).is_err());
        assert!(matches!(
            SaveGame::from_ron(text),
            Err(SaveError::Format { found: 2 })
        ));
        // Dasselbe mit der heutigen Nummer scheitert am Zustand, nicht am Format.
        let current = text.replace("format:2", &format!("format:{SAVE_FORMAT}"));
        assert!(matches!(
            SaveGame::from_ron(&current),
            Err(SaveError::Decode(_))
        ));
        // Was gar kein Spielstand ist, bleibt unlesbar.
        assert!(matches!(
            SaveGame::from_ron("kein Spielstand"),
            Err(SaveError::Decode(_))
        ));
    }
}
