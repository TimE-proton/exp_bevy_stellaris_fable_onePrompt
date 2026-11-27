//! Nachfolge: die eine Stelle, an der eine Welt von einer Gemeinschaft zu einer anderen
//! wechselt, an der nach Spielbeginn eine Gemeinschaft entsteht und an der eine endet.
//!
//! Vier Vorgänge, alle auf [`Succession`]:
//! - [`Succession::spawn`] gründet eine Gemeinschaft. Nur hier wachsen die Listen je
//!   Gemeinschaft.
//! - [`Succession::transfer_planet`] überträgt eine Welt. Verliert eine Gemeinschaft dabei
//!   ihre Hauptwelt, zieht diese um; verliert sie ihre letzte Welt, endet sie.
//! - [`Succession::absorb`] beendet eine Gemeinschaft; eine andere tritt ihr Erbe an.
//! - [`Succession::subordinate`] unterstellt eine Gemeinschaft einer anderen.
//!
//! Jeder Vorgang prüft zuerst und ändert dann: Ein abgelehnter Aufruf lässt nichts zurück.
//! Wo eine Struktur je Gemeinschaft oder je Planet zerlegt oder gefüllt wird, geschieht das
//! erschöpfend (ohne `..`): Ein später ergänztes Feld lässt diese Datei nicht mehr
//! übersetzen, statt beim Besitzwechsel vergessen zu werden.
//!
//! Blockaden bestimmt wie bisher der nächste Tagesanfang neu.

use crate::ai::{AiState, MilitaryPlan};
use crate::chronicle::{Chronicle, ChronicleKind};
use crate::clock::TICKS_PER_DAY;
use crate::data::{CohesionBalance, GameData, Rgb};
use crate::diplomacy::{Diplomacy, NEUTRAL_CULTURE, Standing, drop_truces_of, drop_wars_of};
use crate::economy::{Budget, Economy, Money, PlanetEconomy};
use crate::endgame::{Endgame, Path, PlayerFate};
use crate::finance::{
    Bond, CohesionFactors, Finance, PolityFinance, Region, Stage, issuers_of_region, move_holdings,
    pass_holdings, polity_of_issuer,
};
use crate::fleet::{Battles, Fleets, succession as fleet_succession};
use crate::galaxy::{Galaxy, Planet};
use crate::ids::{HolderId, IssuerId, PlanetId, PolityId};
use crate::logistics::{Logistics, TradeHouse};
use crate::polity::{Polities, Polity};
use crate::state::{SimConfig, SimState};
use crate::tech::Research;
use bevy_ecs::prelude::*;
use bevy_ecs::system::SystemParam;

/// Alles, was ein Besitzwechsel, eine Gründung oder ein Ende berührt, als veränderliche
/// Leihgabe. Gebaut aus den Resources eines Systems ([`SuccessionParams::borrow`]), aus
/// dem Befehlskontext (`CommandContext::succession`), aus einem Zustandsabbild
/// ([`Succession::of_state`]) oder für eine ganze Welt (`Headless::succession`).
pub struct Succession<'a> {
    pub data: &'a GameData,
    pub config: &'a mut SimConfig,
    pub galaxy: &'a mut Galaxy,
    pub polities: &'a mut Polities,
    pub economy: &'a mut Economy,
    pub finance: &'a mut Finance,
    pub logistics: &'a mut Logistics,
    pub fleets: &'a mut Fleets,
    pub battles: &'a mut Battles,
    pub research: &'a mut Research,
    pub diplomacy: &'a mut Diplomacy,
    pub endgame: &'a mut Endgame,
    pub chronicle: &'a mut Chronicle,
    pub ai: &'a mut AiState,
}

/// Dasselbe als Systemparameter: Ein System nimmt `mut params: SuccessionParams` und
/// ruft `params.borrow()`.
#[derive(SystemParam)]
pub struct SuccessionParams<'w> {
    pub data: Res<'w, GameData>,
    pub config: ResMut<'w, SimConfig>,
    pub galaxy: ResMut<'w, Galaxy>,
    pub polities: ResMut<'w, Polities>,
    pub economy: ResMut<'w, Economy>,
    pub finance: ResMut<'w, Finance>,
    pub logistics: ResMut<'w, Logistics>,
    pub fleets: ResMut<'w, Fleets>,
    pub battles: ResMut<'w, Battles>,
    pub research: ResMut<'w, Research>,
    pub diplomacy: ResMut<'w, Diplomacy>,
    pub endgame: ResMut<'w, Endgame>,
    pub chronicle: ResMut<'w, Chronicle>,
    pub ai: ResMut<'w, AiState>,
}

impl SuccessionParams<'_> {
    pub fn borrow(&mut self) -> Succession<'_> {
        Succession {
            data: &self.data,
            config: &mut self.config,
            galaxy: &mut self.galaxy,
            polities: &mut self.polities,
            economy: &mut self.economy,
            finance: &mut self.finance,
            logistics: &mut self.logistics,
            fleets: &mut self.fleets,
            battles: &mut self.battles,
            research: &mut self.research,
            diplomacy: &mut self.diplomacy,
            endgame: &mut self.endgame,
            chronicle: &mut self.chronicle,
            ai: &mut self.ai,
        }
    }
}

/// Womit eine neue Gemeinschaft gegründet wird.
#[derive(Clone, Debug, PartialEq)]
pub struct Founding {
    pub name: String,
    pub short: String,
    pub motto: String,
    pub color: Rgb,
    /// Welt der Gemeinschaft, aus der die neue hervorgeht; sie wird ihre Hauptwelt. Der
    /// Aufrufer überträgt sie im selben Zug mit [`Succession::transfer_planet`].
    pub capital: PlanetId,
}

impl Founding {
    /// Ein Freistaat, den eine abtrünnige Region ausruft: benannt nach seiner Welt, in der
    /// aufgehellten Farbe des Reichs, das er verlässt.
    pub fn free_state(parent: &Polity, planet: &Planet) -> Self {
        let lighten = |channel: f32| channel + (1.0 - channel) * 0.45;
        Self {
            name: format!("Freistaat {}", planet.name),
            short: "Freistaat".into(),
            motto: "Was wir erarbeiten, bleibt hier.".into(),
            color: (
                lighten(parent.color.0),
                lighten(parent.color.1),
                lighten(parent.color.2),
            ),
            capital: planet.id,
        }
    }
}

/// Bedingungen, zu denen eine Welt den Besitzer wechselt.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TransferTerms {
    /// Wer den Anteil `state_share` dessen erhält, was der alte Staat an den Betrieben
    /// der Welt hält.
    pub state_shares_to: HolderId,
    /// 0..1, je Betrieb auf ganze Anteile abgerundet.
    pub state_share: f64,
    /// Zusammenhalt (0..100) und Stufe, mit denen die Region unter der neuen
    /// Gemeinschaft beginnt.
    pub cohesion: f64,
    pub stage: Stage,
    /// Faktor auf die Stimmung der Kurse aller Betriebe der Welt (1 = keine Wirkung).
    pub shock: f64,
    /// War die Welt die Hauptwelt: Anteil (0..1) dessen, was ihre Elite im verbleibenden
    /// Gebiet der alten Gemeinschaft hält und an Geld besitzt, der an die Elite der neuen
    /// Hauptwelt geht.
    pub elite_flight: f64,
    /// Weg, auf dem die Welt wechselt. `Own` für die Gründungswelt einer neuen
    /// Gemeinschaft.
    pub path: Path,
}

impl TransferTerms {
    /// Eine Region sagt sich los: Die Hälfte dessen, was der alte Staat an ihren Betrieben
    /// hält, geht an ihren Fonds, die Kurse geben nach, und unter der neuen Gemeinschaft
    /// beginnt sie loyal und mit Aufbruchstimmung.
    pub fn secession(planet: PlanetId, balance: &CohesionBalance, path: Path) -> Self {
        Self {
            state_shares_to: HolderId::Public(planet),
            state_share: 0.5,
            cohesion: balance.base + balance.honeymoon,
            stage: Stage::Loyal,
            shock: 0.85,
            elite_flight: 0.0,
            path,
        }
    }
}

/// Was eine Übertragung nach sich zog.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Transfer {
    /// Die Gemeinschaft, der die Welt gehörte.
    pub from: PolityId,
    /// Neue Hauptwelt der alten Gemeinschaft, wenn die übertragene ihre Hauptwelt war und
    /// ihr noch eine Welt bleibt.
    pub new_capital: Option<PlanetId>,
    /// Es war ihre letzte Welt: Die alte Gemeinschaft hat geendet.
    pub eliminated: bool,
}

/// Was beim Ende einer Gemeinschaft an die Erbin ging.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Absorbed {
    /// Welten, die den Besitzer gewechselt haben, aufsteigend.
    pub planets: Vec<PlanetId>,
    /// Übergegangene Staatskasse.
    pub treasury: Money,
    /// Flotten, die übergegangen sind (friedlicher Weg) oder aufgelöst wurden (Zwang).
    pub fleets: usize,
}

fn living(polities: &Polities, polity: PolityId) -> Result<&Polity, String> {
    match polities.list.get(polity.index()) {
        Some(found) if found.alive => Ok(found),
        Some(found) => Err(format!("{} besteht nicht mehr", found.name)),
        None => Err("Gemeinschaft unbekannt".into()),
    }
}

/// Bringt eine Liste je Gemeinschaft auf `count` Einträge und hängt den der neuen an.
fn grow<T: Clone>(list: &mut Vec<T>, count: usize, neutral: T, entry: T) {
    list.resize(count, neutral);
    list.push(entry);
}

impl<'a> Succession<'a> {
    /// Leihgabe aus einem Zustandsabbild, für Tests und Werkzeuge ohne Welt.
    pub fn of_state(state: &'a mut SimState, data: &'a GameData) -> Self {
        Succession {
            data,
            config: &mut state.config,
            galaxy: &mut state.galaxy,
            polities: &mut state.polities,
            economy: &mut state.economy,
            finance: &mut state.finance,
            logistics: &mut state.logistics,
            fleets: &mut state.fleets,
            battles: &mut state.battles,
            research: &mut state.research,
            diplomacy: &mut state.diplomacy,
            endgame: &mut state.endgame,
            chronicle: &mut state.chronicle,
            ai: &mut state.ai,
        }
    }

    /// Gründet eine Gemeinschaft, die aus `parent` hervorgeht, und gibt ihre Nummer
    /// zurück. Die einzige Stelle, an der die Listen je Gemeinschaft wachsen.
    ///
    /// Die Neue übernimmt Steuersatz und Kultur von `parent` und alles, was diese an
    /// Technik beherrscht und begonnen hat (auch deren Bild von den anderen). Sie beginnt
    /// ohne Staatskasse, ohne Schulden, ohne Flotte und ohne Handelshaus, frei und im
    /// Frieden mit allen. Ihre Hauptwelt ist `founding.capital`; die Welt gehört in diesem
    /// Augenblick noch `parent` und wird vom Aufrufer im selben Zug übertragen.
    pub fn spawn(
        &mut self,
        parent: PolityId,
        founding: Founding,
        tick: u64,
    ) -> Result<PolityId, String> {
        let Succession {
            data,
            config: _,
            galaxy,
            polities,
            economy,
            finance,
            logistics,
            fleets,
            battles,
            research,
            diplomacy,
            endgame,
            chronicle: _,
            // Der Plan der Neuen entsteht, sobald ihre Führung zum ersten Mal plant.
            ai: _,
        } = self;
        let Founding {
            name,
            short,
            motto,
            color,
            capital,
        } = founding;

        // Erst prüfen …
        let origin = living(polities, parent)?;
        let Some(seat) = galaxy.planets.get(capital.index()) else {
            return Err("Planet unbekannt".into());
        };
        if seat.owner != Some(parent) {
            return Err(format!("{} gehört nicht zu {}", seat.name, origin.name));
        }
        if name.trim().is_empty() {
            return Err("eine Gemeinschaft braucht einen Namen".into());
        }

        // … dann ändern.
        let count = polities.list.len();
        let id = PolityId::from_index(count);
        let day = tick / TICKS_PER_DAY;
        polities.list.push(Polity {
            id,
            name,
            short,
            motto,
            color,
            capital: Some(capital),
            alive: true,
            next_claim_day: 0,
        });

        let Economy {
            markets: _,
            planets: _,
            treasuries,
            tax_rates,
            treasury_targets,
            initial_money: _,
            minted: _,
            tech_yield,
            state_income,
            state_income_running,
            wage_bill_running,
            wage_tax_running,
            budgets,
            staff,
        } = &mut **economy;
        let usual_tax = data.balance.economy.income_tax;
        let tax = tax_rates.get(parent.index()).copied().unwrap_or(usual_tax);
        grow(treasuries, count, 0, 0);
        grow(tax_rates, count, usual_tax, tax);
        grow(treasury_targets, count, 0, 0);
        // Den Ertragsfaktor liefert gleich die geerbte Forschung.
        grow(tech_yield, count, 1.0, 1.0);
        grow(state_income, count, 0, 0);
        grow(state_income_running, count, 0, 0);
        grow(wage_bill_running, count, 0, 0);
        grow(wage_tax_running, count, 0, 0);
        // Der Haushalt der Neuen wird am nächsten Monatsersten eröffnet; bis dahin gilt für
        // jeden Anspruch nur ihre Kasse. Im Staatsdienst steht bei ihr noch niemand.
        grow(budgets, count, Budget::default(), Budget::default());
        grow(staff, count, 0, 0);

        let Finance {
            issuers: _,
            elites: _,
            regions: _,
            polities: states,
            bonds: _,
            secessions: _,
        } = &mut **finance;
        let fresh = PolityFinance {
            expropriation_risk: 0.0,
            default_memory: 0.0,
            war_weariness: 0.0,
            bond_yield: data.balance.finance.base_yield,
            revenue: 0.0,
            last_treasury: 0,
            defaults: 0,
        };
        grow(states, count, fresh.clone(), fresh);

        fleet_succession::size_for_polities(fleets, battles, count + 1);

        let Diplomacy {
            wars: _,
            // Eine Waffenruhe gilt zwischen zweien, die Frieden schlossen; die Neue beginnt
            // ohne.
            truces: _,
            standing,
            culture,
        } = &mut **diplomacy;
        let inherited = culture
            .get(parent.index())
            .copied()
            .unwrap_or(NEUTRAL_CULTURE);
        let free = Standing {
            overlord: None,
            voluntary: false,
            since_day: day,
        };
        grow(standing, count, Standing::default(), free);
        grow(culture, count, NEUTRAL_CULTURE, inherited);

        let Logistics {
            houses: _,
            freighters: _,
            flows: _,
            captured: _,
            tech_speed,
        } = &mut **logistics;
        grow(tech_speed, count, 1.0, 1.0);

        // Forschung: Einträge, Einblick und Bilder wachsen mit dem Erbe.
        research.inherit(data, parent, id);
        let Research {
            polities: _,
            intel: _,
            beliefs: _,
        } = &**research;
        // Das Ende der Partie führt nichts je Gemeinschaft.
        let Endgame {
            path: _,
            absorbed_pop: _,
            gap: _,
            seat: _,
            peak_population: _,
            months_below: _,
            free_states: _,
            player: _,
            outcome: _,
        } = &**endgame;

        // Werke und Frachter der Neuen rechnen vom selben Takt an mit der geerbten Technik.
        research.publish_effects(economy, logistics);
        Ok(id)
    }

    /// Prüft, ob [`Succession::transfer_planet`] die Welt übertragen würde, ohne etwas zu
    /// ändern, und gibt die Gemeinschaft zurück, der sie gehört. `to: None` steht für
    /// eine Gemeinschaft, die erst gegründet wird.
    pub fn check_transfer(
        &self,
        planet: PlanetId,
        to: Option<PolityId>,
        terms: &TransferTerms,
    ) -> Result<PolityId, String> {
        let Some(world) = self.galaxy.planets.get(planet.index()) else {
            return Err("Planet unbekannt".into());
        };
        let Some(from) = world.owner else {
            return Err(format!("{} hat keinen Besitzer", world.name));
        };
        living(self.polities, from)?;
        if let Some(to) = to {
            let target = living(self.polities, to)?;
            if to == from {
                return Err(format!("{} gehört bereits zu {}", world.name, target.name));
            }
        }
        let TransferTerms {
            state_shares_to,
            state_share,
            cohesion,
            stage: _,
            shock,
            elite_flight,
            path: _,
        } = terms;
        match state_shares_to {
            HolderId::State(polity) => {
                living(self.polities, *polity)?;
            }
            HolderId::Elite(seat) | HolderId::Public(seat) => {
                if seat.index() >= self.galaxy.planets.len() {
                    return Err("Empfänger der Staatsanteile: Planet unbekannt".into());
                }
            }
        }
        let share = |value: f64| (0.0..=1.0).contains(&value);
        if !share(*state_share) || !share(*elite_flight) {
            return Err("Anteile müssen zwischen 0 und 1 liegen".into());
        }
        if !shock.is_finite() || *shock <= 0.0 {
            return Err("der Faktor auf die Kurse muss positiv sein".into());
        }
        if !cohesion.is_finite() {
            return Err("der Zusammenhalt muss eine Zahl sein".into());
        }
        Ok(from)
    }

    /// Überträgt eine Welt an die Gemeinschaft `to`.
    ///
    /// - Besitzer, Steuerpflicht und die Buchführung der Region (Besitzer, Tag, Stufe,
    ///   Monatszähler, Zusammenhalt) folgen den Bedingungen.
    /// - Der Anteil `state_share` dessen, was der alte Staat an den Betrieben der Welt
    ///   hält, geht an `state_shares_to`; die Stimmung ihrer Kurse wird mit `shock`
    ///   multipliziert.
    /// - Für das Ende der Partie werden Weg, Bevölkerung und kultureller Abstand zwischen
    ///   alter und neuer Gemeinschaft festgehalten, wie sie in diesem Augenblick sind.
    /// - Bauaufträge der alten Gemeinschaft bei der Werft der Welt verfallen ohne
    ///   Erstattung. Ihre Handelshäuser, deren Heimathafen in dem System lag, ziehen in das
    ///   System ihrer Hauptwelt, wenn ihr dort keine Welt bleibt.
    /// - War es die Hauptwelt, wird die bevölkerungsreichste verbleibende Welt Hauptwelt
    ///   (bei Gleichstand die mit der kleineren Nummer); die Handelshäuser ziehen mit; die
    ///   Region ist höchstens unruhig und zahlt Steuern; der Anteil `elite_flight` dessen,
    ///   was die Elite der alten Hauptwelt an Betrieben im verbleibenden Gebiet hält
    ///   (Handelshäuser eingeschlossen), und ihres Geldes geht an die Elite der neuen.
    /// - War es die letzte Welt, endet die alte Gemeinschaft im selben Zug; `to` tritt ihr
    ///   Erbe auf dem Weg der Bedingungen an (siehe [`Succession::absorb`]).
    pub fn transfer_planet(
        &mut self,
        planet: PlanetId,
        to: PolityId,
        terms: TransferTerms,
        tick: u64,
    ) -> Result<Transfer, String> {
        let from = self.check_transfer(planet, Some(to), &terms)?;
        let TransferTerms {
            state_shares_to,
            state_share,
            cohesion,
            stage,
            shock,
            elite_flight,
            path,
        } = terms;
        let Succession {
            data,
            config: _,
            galaxy,
            polities,
            economy,
            finance,
            logistics,
            fleets,
            battles: _,
            research: _,
            diplomacy,
            endgame,
            chronicle,
            ai: _,
        } = self;
        let index = planet.index();
        let day = tick / TICKS_PER_DAY;
        let was_capital = polities.get(from).capital == Some(planet);
        // Abstand und Bevölkerung, wie sie in diesem Augenblick sind.
        let gap = 1.0 - diplomacy.affinity(from, to);

        let Planet {
            id: _,
            system,
            name: _,
            kind: _,
            orbit: _,
            gravity: _,
            atmosphere: _,
            temperature: _,
            water: _,
            habitability: _,
            capacity: _,
            population,
            deposits: _,
            owner,
        } = galaxy.planet_mut(planet);
        *owner = Some(to);
        let (system, population) = (*system, *population);

        let PlanetEconomy {
            capital: _,
            output: _,
            utilization: _,
            utilization_trend: _,
            roc: _,
            firms: _,
            households: _,
            wage: _,
            employment: _,
            labor_demand: _,
            needs_met: _,
            income: _,
            income_today: _,
            founded_day: _,
            tax_exempt,
            // Was stillsteht, steht unter jedem Besitzer still.
            disruption: _,
        } = &mut economy.planets[index];
        // Nur eine autonome Region führt keine Steuern ab.
        *tax_exempt = stage == Stage::Autonomous;

        let Region {
            cohesion: bond,
            stage: standing,
            months_below,
            months_above,
            factors: _,
            dividends_out: _,
            dividends_in: _,
            dividends_local: _,
            drain: _,
            owner_seen,
            since_day,
            // Spiegel der Belagerung: Sie schreibt sie selbst neu, sobald sie den
            // Besitzwechsel sieht (Aufhebung am selben Takt, Abgleich im Monatsschritt).
            besieged: _,
            occupation: _,
            garrison_cover: _,
            war_damage: _,
        } = &mut finance.regions[index];
        *bond = cohesion.clamp(0.0, 100.0);
        *standing = stage;
        *months_below = 0;
        *months_above = 0;
        *owner_seen = Some(to);
        *since_day = day;

        endgame.tag(planet, path, population, gap);
        endgame.seat[index] = false;

        // Die Betriebe der Welt, wie sie unter dem neuen Besitzer zugeordnet sind.
        let capitals: Vec<Option<PlanetId>> = polities.list.iter().map(|p| p.capital).collect();
        let local: Vec<IssuerId> = issuers_of_region(finance, galaxy, logistics, &capitals, planet)
            .map(|(id, _)| id)
            .collect();
        move_holdings(
            finance,
            &local,
            HolderId::State(from),
            state_shares_to,
            state_share,
        );
        for id in &local {
            if let Some(issuer) = finance.issuers.get_mut(id) {
                issuer.shock *= shock;
            }
        }

        // Die bevölkerungsreichste Welt, die der alten Gemeinschaft bleibt.
        let remaining = galaxy
            .planets_of(from)
            .max_by(|a, b| a.population.total_cmp(&b.population).then(b.id.cmp(&a.id)))
            .map(|world| world.id);
        let Some(heartland) = remaining else {
            // Es war ihre letzte Welt: Die Gemeinschaft endet. Auf friedlichem Weg gehen
            // auch die Aufträge ihrer Werft über; unter Zwang verfallen sie.
            if path == Path::Forced {
                fleet_succession::cancel_orders_at(
                    fleets, galaxy, chronicle, data, planet, from, tick,
                );
            }
            // Tritt sie mit ihrer letzten Welt bei, macht ihr Ende diese zum Sitz.
            self.conclude(from, to, path, tick);
            return Ok(Transfer {
                from,
                new_capital: None,
                eliminated: true,
            });
        };

        fleet_succession::cancel_orders_at(fleets, galaxy, chronicle, data, planet, from, tick);

        let new_capital = was_capital.then_some(heartland);
        let home = polities
            .get(from)
            .capital
            .filter(|_| !was_capital)
            .unwrap_or(heartland);
        let home_system = galaxy.planet(home).system;
        // Handelshäuser folgen der Hauptwelt; ein Haus, dessen Gemeinschaft im System
        // seines Heimathafens keine Welt mehr hält, zieht ebenfalls dorthin.
        if was_capital || !galaxy.owns_planet_in(from, system) {
            for house in &mut logistics.houses {
                if house.polity == from && house.home == system {
                    house.home = home_system;
                }
            }
        }

        if let Some(new_capital) = new_capital {
            polities.get_mut(from).capital = Some(new_capital);
            // Eine Hauptwelt ist höchstens unruhig und zahlt Steuern.
            let region = &mut finance.regions[new_capital.index()];
            if region.stage == Stage::Autonomous {
                region.stage = Stage::Restless;
                region.months_below = 0;
                region.months_above = 0;
            }
            economy.planets[new_capital.index()].tax_exempt = false;

            // Ein Teil der alten Elite geht mit ihrem Besitz in die neue Hauptwelt. Was
            // bleibt, gehört fortan Haltern der Gemeinschaft, die die alte Hauptwelt hat.
            let (old_elite, new_elite) = (HolderId::Elite(planet), HolderId::Elite(new_capital));
            let remaining_issuers: Vec<IssuerId> = finance
                .issuers
                .keys()
                .copied()
                .filter(|id| polity_of_issuer(galaxy, logistics, *id) == Some(from))
                .collect();
            move_holdings(
                finance,
                &remaining_issuers,
                old_elite,
                new_elite,
                elite_flight,
            );
            let cash = (finance.elites[index] as f64 * elite_flight) as Money;
            finance.elites[index] -= cash;
            finance.elites[new_capital.index()] += cash;

            let loser = polities.get(from);
            chronicle.push(
                tick,
                ChronicleKind::Cohesion,
                Some(from),
                Some(home_system),
                format!(
                    "{} verliert die Hauptwelt {}. Neue Hauptwelt ist {}.",
                    loser.name,
                    galaxy.planet(planet).name,
                    galaxy.planet(new_capital).name
                ),
            );
        }
        Ok(Transfer {
            from,
            new_capital,
            eliminated: false,
        })
    }

    /// Die Gemeinschaft `from` endet, `into` tritt ihr Erbe an.
    ///
    /// - Ihre Welten wechseln den Besitzer und behalten den Stand ihrer Region (Stufe,
    ///   Zusammenhalt, Fristen, Steuerfreiheit). Weg der Welten: `Path::Joined` vermerkt
    ///   nur Welten, die eigen oder beigetreten waren; eine genommene Welt bleibt durch
    ///   jede spätere Vereinigung genommen. `Path::Forced` vermerkt alle.
    /// - Auf friedlichem Weg wird die Hauptwelt zum Sitz, und Flotten wie Bauaufträge
    ///   gehen über. Unter Zwang werden Flotten und Aufträge aufgelöst, auch im Gefecht,
    ///   auf dem Marsch und im Dock.
    /// - Handelshäuser samt Frachtern und Ladung, Staatskasse, staatliche Anteile (auch
    ///   angebotene) und Anleihen gehen über; was die Erbin danach sich selbst schuldete,
    ///   ist getilgt. Die übernommene Kasse zählt bei ihr nicht als Einnahme des Monats.
    /// - Die Forschung wird vereinigt ([`Research::absorb`]).
    /// - Die Kriege der Beendeten enden. Wer ihr unterstand, untersteht dem Haupt der Erbin.
    /// - War sie die Gemeinschaft des Spielers, ist er ausgeschieden; die Partie läuft
    ///   ohne ihn weiter.
    pub fn absorb(
        &mut self,
        from: PolityId,
        into: PolityId,
        path: Path,
        tick: u64,
    ) -> Result<Absorbed, String> {
        let ending = living(self.polities, from)?;
        living(self.polities, into)?;
        if from == into {
            return Err(format!(
                "{} kann nicht in sich selbst aufgehen",
                ending.name
            ));
        }
        if path == Path::Own {
            return Err(
                "eine Gemeinschaft endet durch Beitritt oder unter Zwang, nicht als eigene".into(),
            );
        }
        Ok(self.conclude(from, into, path, tick))
    }

    /// Das Ende einer Gemeinschaft, nachdem geprüft ist. `Path::Own` kommt nur vor, wenn
    /// ihre letzte Welt eine neue Gemeinschaft gegründet hat: Dann erbt diese friedlich,
    /// und zu vermerken ist nichts.
    fn conclude(&mut self, from: PolityId, into: PolityId, path: Path, tick: u64) -> Absorbed {
        let Succession {
            data,
            config,
            galaxy,
            polities,
            economy,
            finance,
            logistics,
            fleets,
            battles,
            research,
            diplomacy,
            endgame,
            chronicle,
            ai,
        } = self;
        let day = tick / TICKS_PER_DAY;
        let forced = path == Path::Forced;
        let gap = 1.0 - diplomacy.affinity(from, into);
        let old_capital = polities.get(from).capital;

        // Welten: neuer Besitzer, alter Stand der Region.
        let mut planets = Vec::new();
        for index in 0..galaxy.planets.len() {
            let Planet {
                id,
                system: _,
                name: _,
                kind: _,
                orbit: _,
                gravity: _,
                atmosphere: _,
                temperature: _,
                water: _,
                habitability: _,
                capacity: _,
                population,
                deposits: _,
                owner,
            } = &mut galaxy.planets[index];
            if *owner != Some(from) {
                continue;
            }
            *owner = Some(into);
            let (id, population) = (*id, *population);
            match path {
                Path::Forced => {
                    endgame.tag(id, Path::Forced, population, gap);
                    endgame.seat[index] = false;
                }
                Path::Joined if endgame.path_of(id) != Path::Forced => {
                    endgame.tag(id, Path::Joined, population, gap);
                }
                Path::Joined | Path::Own => {}
            }
            // An der Wirtschaft der Welt ändert das Ende nichts: Auch die Steuerfreiheit
            // einer autonomen Region bleibt.
            let PlanetEconomy {
                capital: _,
                output: _,
                utilization: _,
                utilization_trend: _,
                roc: _,
                firms: _,
                households: _,
                wage: _,
                employment: _,
                labor_demand: _,
                needs_met: _,
                income: _,
                income_today: _,
                founded_day: _,
                tax_exempt: _,
                disruption: _,
            } = &economy.planets[index];
            planets.push(id);
        }
        // Die Hauptwelt einer Gemeinschaft, die beitritt, bleibt als Sitz kenntlich. Gründet
        // ihre letzte Welt eine neue Gemeinschaft, ist niemand beigetreten.
        if path == Path::Joined
            && let Some(capital) = old_capital
            && galaxy.planet(capital).owner == Some(into)
        {
            endgame.seat[capital.index()] = true;
        }
        // Die Regionen behalten Stufe, Zusammenhalt und Fristen. Der Monatsabschluss soll
        // den Wechsel nicht als neuen Besitzer lesen (das setzte sie zurück), und nichts
        // soll mehr die Beendete nennen.
        for (index, region) in finance.regions.iter_mut().enumerate() {
            let owner = galaxy.planets[index].owner;
            let Region {
                cohesion: _,
                stage: _,
                months_below: _,
                months_above: _,
                factors,
                dividends_out: _,
                dividends_in: _,
                dividends_local: _,
                drain: _,
                owner_seen,
                since_day: _,
                besieged: _,
                occupation: _,
                garrison_cover: _,
                war_damage: _,
            } = region;
            if *owner_seen == Some(from) {
                *owner_seen = owner;
            }
            let CohesionFactors {
                interlock: _,
                dispersion: _,
                local_share: _,
                center_share: _,
                foreign_share: _,
                foreign_polity,
                drain: _,
                supply: _,
                weariness: _,
                tax: _,
                blockaded: _,
                is_capital: _,
                distance: _,
                years: _,
                expropriation: _,
                target: _,
                occupation: _,
                garrison_cover: _,
                war_damage: _,
            } = factors;
            if *foreign_polity == Some(from) {
                *foreign_polity = Some(into).filter(|heir| owner != Some(*heir));
            }
        }

        let Polity {
            id: _,
            name: _,
            short: _,
            motto: _,
            color: _,
            capital,
            alive,
            next_claim_day: _,
        } = polities.get_mut(from);
        *capital = None;
        *alive = false;

        // Handelshäuser; Frachter und Ladung hängen am Haus.
        for house in &mut logistics.houses {
            let TradeHouse {
                id: _,
                polity,
                name: _,
                cash: _,
                home: _,
                profit: _,
                profit_today: _,
            } = house;
            if *polity == from {
                *polity = into;
            }
        }

        // Die militärische Führung der Beendeten plant nicht mehr: Ihr Plan ist leer, ihre
        // Unternehmungen sind fort, und keine ihrer Flotten ist mehr für jemanden
        // vorgemerkt. Übergehende Flotten führt fortan die Erbin nach ihrem eigenen Plan.
        // Stehen bleibt nur der Zähler der begonnenen Angriffe: Geschichte.
        let AiState {
            default_mode: _,
            modes,
            plans,
            reserved,
        } = &mut **ai;
        modes.remove(&from);
        if let Some(plan) = plans.get_mut(from.index()) {
            *plan = MilitaryPlan {
                offensives: plan.offensives,
                ..MilitaryPlan::default()
            };
        }
        for fleet in fleets.list.iter().filter(|fleet| fleet.owner == from) {
            reserved.remove(&fleet.id);
        }

        let moved_fleets = if forced {
            fleet_succession::dissolve_fleets(fleets, battles, data, from, into, tick).0
        } else {
            fleet_succession::pass_fleets(fleets, battles, polities, data, from, into, tick)
        };

        // Anleihen: Schuldner und Gläubiger heißen neu; was die Erbin sich nun selbst
        // schuldete, ist getilgt.
        let (old_state, new_state) = (HolderId::State(from), HolderId::State(into));
        for bond in &mut finance.bonds {
            let Bond {
                issuer,
                holder,
                principal: _,
                rate: _,
                matures_on: _,
            } = bond;
            if *issuer == from {
                *issuer = into;
            }
            if *holder == old_state {
                *holder = new_state;
            }
        }
        finance
            .bonds
            .retain(|bond| !(bond.issuer == into && bond.holder == new_state));
        pass_holdings(finance, old_state, new_state);

        // Staatskasse und Einnahmenrechnung.
        let Economy {
            markets: _,
            planets: _,
            treasuries,
            tax_rates: _,
            treasury_targets,
            initial_money: _,
            minted: _,
            tech_yield: _,
            state_income,
            state_income_running,
            wage_bill_running,
            wage_tax_running,
            budgets,
            staff,
        } = &mut **economy;
        let treasury = std::mem::take(&mut treasuries[from.index()]);
        treasuries[into.index()] += treasury;
        treasury_targets[from.index()] = 0;
        // Die laufenden Einnahmen der Beendeten sind fortan die der Erbin, samt Lohnsumme
        // und Lohnsteuer des laufenden Monats: Daran bemessen sich deren Forschungsetat und
        // ihr nächster Haushaltsabschluss.
        for income in [
            state_income,
            state_income_running,
            wage_bill_running,
            wage_tax_running,
        ] {
            let inherited = std::mem::take(&mut income[from.index()]);
            income[into.index()] += inherited;
        }
        // Wer im Dienst der Beendeten stand, steht mit seinem Geld fortan im Dienst der
        // Erbin und kauft an deren Sitz ein: Am Sitz der Beendeten gäbe es niemand mehr aus.
        let purse = std::mem::take(&mut staff[from.index()]);
        staff[into.index()] += purse;
        // Der Haushalt der Beendeten ist geschlossen: Keine Bewilligung bleibt offen, kein
        // Anspruch gibt mehr etwas aus. Die übernommene Kasse erscheint im nächsten
        // Abschluss der Erbin unter den übrigen Bewegungen, nicht als Einnahme.
        budgets[from.index()] = Budget::default();

        let PolityFinance {
            expropriation_risk,
            default_memory,
            war_weariness,
            bond_yield,
            revenue,
            last_treasury,
            // Die Zahl der Zahlungsausfälle bleibt als Geschichte stehen.
            defaults: _,
        } = &mut finance.polities[from.index()];
        let revenue = std::mem::take(revenue);
        *expropriation_risk = 0.0;
        *default_memory = 0.0;
        *war_weariness = 0.0;
        *bond_yield = data.balance.finance.base_yield;
        *last_treasury = 0;
        let heir = &mut finance.polities[into.index()];
        heir.revenue += revenue;
        // Die übernommene Kasse ist keine Einnahme des Monats.
        heir.last_treasury += treasury;

        research.absorb(data, from, into);
        research.publish_effects(economy, logistics);

        // Kriege und Waffenruhen enden, Unterstellte gehen an das Haupt der Erbin.
        drop_wars_of(diplomacy, from);
        drop_truces_of(diplomacy, from);
        let free = Standing {
            overlord: None,
            voluntary: false,
            since_day: day,
        };
        let wards: Vec<PolityId> = diplomacy.wards_of(from).collect();
        diplomacy.standing[from.index()] = free;
        if wards.contains(&into) {
            // Die Erbin unterstand der Beendeten: Jetzt ist sie frei.
            diplomacy.standing[into.index()] = free;
        }
        let head = diplomacy.head(into);
        for ward in wards {
            if ward != into {
                diplomacy.standing[ward.index()].overlord = Some(head);
            }
        }

        let SimConfig { seed: _, player } = &mut **config;
        if *player == Some(from) {
            *player = None;
            endgame.player = PlayerFate::Out;
        }

        fleet_succession::sync_hostility(battles, diplomacy);

        let (ended, heir) = (&polities.get(from).name, &polities.get(into).name);
        let worlds = match planets.len() {
            1 => "eine Welt".to_string(),
            count => format!("{count} Welten"),
        };
        let text = match path {
            // Die letzte Welt ist schon übertragen: Zu übernehmen bleibt nur die Kasse.
            Path::Forced if planets.is_empty() => format!(
                "{ended} hat die letzte Welt verloren und endet. {heir} übernimmt die \
                 Staatskasse; die Flotten von {ended} sind aufgelöst."
            ),
            Path::Forced => format!(
                "{ended} endet: {heir} übernimmt {worlds} und die Staatskasse. Die Flotten \
                 von {ended} sind aufgelöst."
            ),
            Path::Joined if !planets.is_empty() => {
                format!("{ended} geht in {heir} auf: {worlds}, Flotten und Staatskasse gehen über.")
            }
            Path::Joined | Path::Own => format!(
                "{ended} hält keine Welt mehr und endet. {heir} tritt das Erbe an: Flotten \
                 und Staatskasse gehen über."
            ),
        };
        chronicle.push(
            tick,
            if forced {
                ChronicleKind::War
            } else {
                ChronicleKind::Diplomacy
            },
            Some(into),
            old_capital.map(|capital| galaxy.planet(capital).system),
            text,
        );
        Absorbed {
            planets,
            treasury,
            fleets: moved_fleets,
        }
    }

    /// Unterstellt die Gemeinschaft `ward` dem Oberhaupt `lord`. Sie behält Welten,
    /// Staatskasse und Flotten. Beide müssen leben und frei sein.
    ///
    /// - Wer `ward` unterstand, untersteht fortan `lord`: Ketten gibt es nicht.
    /// - Die eigenen Kriege von `ward` enden; fortan teilt sie die Kriege ihres Haupts.
    /// - Ihre Welten werden vermerkt: gewollt als beigetreten (eine genommene Welt bleibt
    ///   genommen), erzwungen alle als genommen.
    /// - War sie die Gemeinschaft des Spielers, ist er ausgeschieden.
    pub fn subordinate(
        &mut self,
        ward: PolityId,
        lord: PolityId,
        voluntary: bool,
        tick: u64,
    ) -> Result<(), String> {
        let Succession {
            data: _,
            config,
            galaxy,
            polities,
            economy: _,
            finance: _,
            logistics: _,
            fleets: _,
            battles,
            research: _,
            diplomacy,
            endgame,
            chronicle,
            ai: _,
        } = self;
        let (lesser, greater) = (living(polities, ward)?, living(polities, lord)?);
        if ward == lord {
            return Err(format!(
                "{} kann sich nicht sich selbst unterstellen",
                lesser.name
            ));
        }
        for (polity, role) in [(lesser, "bereits"), (greater, "selbst")] {
            if let Some(overlord) = diplomacy.overlord_of(polity.id) {
                return Err(format!(
                    "{} untersteht {role} {}",
                    polity.name,
                    polities.get(overlord).name
                ));
            }
        }

        let day = tick / TICKS_PER_DAY;
        let gap = 1.0 - diplomacy.affinity(ward, lord);
        for world in galaxy.planets_of(ward) {
            if !voluntary {
                endgame.tag(world.id, Path::Forced, world.population, gap);
            } else if endgame.path_of(world.id) != Path::Forced {
                endgame.tag(world.id, Path::Joined, world.population, gap);
            }
        }

        drop_wars_of(diplomacy, ward);
        let wards: Vec<PolityId> = diplomacy.wards_of(ward).collect();
        for other in wards {
            diplomacy.standing[other.index()].overlord = Some(lord);
        }
        diplomacy.standing[ward.index()] = Standing {
            overlord: Some(lord),
            voluntary,
            since_day: day,
        };

        let SimConfig { seed: _, player } = &mut **config;
        if *player == Some(ward) {
            *player = None;
            endgame.player = PlayerFate::Out;
        }

        fleet_succession::sync_hostility(battles, diplomacy);

        let system = lesser.capital.map(|capital| galaxy.planet(capital).system);
        let (ward_name, lord_name) = (&lesser.name, &greater.name);
        let (kind, opening) = if voluntary {
            (
                ChronicleKind::Diplomacy,
                format!("{ward_name} unterstellt sich {lord_name}."),
            )
        } else {
            (
                ChronicleKind::War,
                format!("{ward_name} wird {lord_name} unterstellt."),
            )
        };
        let text = format!(
            "{opening} Welten, Flotten und Staatskasse behält {ward_name}; über Krieg und \
             Frieden entscheidet {lord_name}."
        );
        chronicle.push(tick, kind, Some(lord), system, text);
        Ok(())
    }
}

/// Invarianten der Nachfolge. Ein Verstoß ist ein Programmfehler.
///
/// - Alle Listen je Gemeinschaft sind gleich lang, alle Listen je Planet des Spielendes
///   so lang wie die der Planeten; eine Welt ohne Besitzer trägt keinen Weg.
/// - Jede lebende Gemeinschaft hat eine Hauptwelt und besitzt sie.
/// - Eine beendete Gemeinschaft hat nichts mehr: keine Welt, Hauptwelt, Flotte, kein
///   Handelshaus, keinen Bauauftrag, keinen Krieg, keine Waffenruhe, keine Anleihe als
///   Schuldnerin oder Gläubigerin, keine Anteile oder Angebote als Staat, keine
///   Staatskasse, kein Geld im Staatsdienst, keine offene Bewilligung im Haushalt, keinen
///   Plan der militärischen Führung, kein Oberhaupt und niemanden, der ihr untersteht.
///   Auch sonst nennt sie nichts im Zustand.
/// - Ein Oberhaupt lebt und ist frei. Der Spieler führt, wenn überhaupt, eine lebende,
///   freie Gemeinschaft.
///
/// Geschichte darf Beendete nennen: Chronik, Befehlsprotokoll, noch wartende Befehle,
/// Bilanzen beendeter Gefechte, aufgelöste Flotten und Zähler über die ganze Partie.
pub fn check(world: &World) -> Result<(), String> {
    let config = world.resource::<SimConfig>();
    let galaxy = world.resource::<Galaxy>();
    let polities = world.resource::<Polities>();
    let economy = world.resource::<Economy>();
    let finance = world.resource::<Finance>();
    let logistics = world.resource::<Logistics>();
    let fleets = world.resource::<Fleets>();
    let battles = world.resource::<Battles>();
    let research = world.resource::<Research>();
    let diplomacy = world.resource::<Diplomacy>();
    let endgame = world.resource::<Endgame>();
    let ai = world.resource::<AiState>();

    let count = polities.list.len();
    let mut lengths = vec![
        ("Staatskassen", economy.treasuries.len()),
        ("Steuersätze", economy.tax_rates.len()),
        ("Rücklagen", economy.treasury_targets.len()),
        ("Ertragsfaktoren", economy.tech_yield.len()),
        ("Staatseinnahmen", economy.state_income.len()),
        (
            "laufende Staatseinnahmen",
            economy.state_income_running.len(),
        ),
        ("laufende Lohnsummen", economy.wage_bill_running.len()),
        ("laufende Lohnsteuer", economy.wage_tax_running.len()),
        ("Haushalte", economy.budgets.len()),
        ("Kassen des Staatsdienstes", economy.staff.len()),
        ("Finanzlagen", finance.polities.len()),
        ("Forschungsstände", research.polities.len()),
        ("Einblick", research.intel.len()),
        ("Bilder der Forschung", research.beliefs.len()),
        ("Fahrtfaktoren", logistics.tech_speed.len()),
        ("Stellungen", diplomacy.standing.len()),
        ("Kulturen", diplomacy.culture.len()),
    ];
    lengths.extend(fleet_succession::per_polity_lengths(fleets, battles));
    lengths.extend(research.intel.iter().map(|row| ("Einblick", row.len())));
    lengths.extend(
        research
            .beliefs
            .iter()
            .map(|row| ("Bilder der Forschung", row.len())),
    );
    if let Some((name, length)) = lengths.iter().find(|(_, length)| *length != count) {
        return Err(format!(
            "Liste je Gemeinschaft ({name}) hat {length} Einträge bei {count} Gemeinschaften"
        ));
    }
    let planets = galaxy.planets.len();
    for (name, length) in [
        ("Wege", endgame.path.len()),
        ("Bevölkerung beim Wechsel", endgame.absorbed_pop.len()),
        ("kultureller Abstand", endgame.gap.len()),
        ("Sitze", endgame.seat.len()),
    ] {
        if length != planets {
            return Err(format!(
                "Liste je Planet ({name}) hat {length} Einträge bei {planets} Planeten"
            ));
        }
    }
    for world in &galaxy.planets {
        let index = world.id.index();
        let untouched = endgame.path[index] == Path::Own
            && endgame.absorbed_pop[index] == 0.0
            && endgame.gap[index] == 0.0;
        if !untouched && (world.owner.is_none() || endgame.path[index] == Path::Own) {
            return Err(format!(
                "{}: eigen oder ohne Besitzer, trägt aber die Spur eines Wechsels",
                world.name
            ));
        }
        if world.owner.is_none() && endgame.seat[index] {
            return Err(format!("{}: Sitz ohne Besitzer", world.name));
        }
        if let Some(owner) = world.owner
            && !polities
                .list
                .get(owner.index())
                .is_some_and(|polity| polity.alive)
        {
            return Err(format!(
                "{} gehört {owner}, einer Gemeinschaft, die geendet hat",
                world.name
            ));
        }
    }

    let ended = |polity: PolityId| {
        !polities
            .list
            .get(polity.index())
            .is_some_and(|found| found.alive)
    };
    for (index, polity) in polities.list.iter().enumerate() {
        if polity.id.index() != index {
            return Err(format!("{}: Nummer passt nicht zum Platz", polity.name));
        }
        let name = &polity.name;
        if polity.alive {
            let Some(capital) = polity.capital else {
                return Err(format!("{name} lebt ohne Hauptwelt"));
            };
            if galaxy
                .planets
                .get(capital.index())
                .and_then(|world| world.owner)
                != Some(polity.id)
            {
                return Err(format!("{name} besitzt die eigene Hauptwelt nicht"));
            }
            continue;
        }
        // Beendet: Es darf nichts geblieben sein.
        if polity.capital.is_some() {
            return Err(format!("{name} hat geendet, führt aber eine Hauptwelt"));
        }
        if economy.treasuries[index] != 0 {
            return Err(format!("{name} hat geendet, hält aber eine Staatskasse"));
        }
        if economy.staff[index] != 0 {
            return Err(format!(
                "{name} hat geendet, aber ihr Staatsdienst hält noch Geld"
            ));
        }
        if economy.budgets[index]
            .grants
            .values()
            .any(|grant| *grant != 0)
        {
            return Err(format!(
                "{name} hat geendet, aber ihr Haushalt hält noch eine Bewilligung"
            ));
        }
        if ai.modes.contains_key(&polity.id)
            || ai
                .plans
                .get(index)
                .is_some_and(|plan| !plan.operations.is_empty() || plan.station.is_some())
        {
            return Err(format!(
                "{name} hat geendet, aber ihre militärische Führung plant noch"
            ));
        }
        if diplomacy.standing[index].overlord.is_some() {
            return Err(format!("{name} hat geendet, untersteht aber noch"));
        }
        if logistics
            .houses
            .iter()
            .any(|house| house.polity == polity.id)
        {
            return Err(format!("{name} hat geendet, führt aber ein Handelshaus"));
        }
    }
    if let Some(anomaly) = fleet_succession::reference_to_ended(fleets, battles, polities) {
        return Err(anomaly);
    }
    if let Some((a, b)) = diplomacy
        .wars
        .iter()
        .find(|(a, b)| ended(*a) || ended(*b) || !diplomacy.is_free(*a) || !diplomacy.is_free(*b))
    {
        return Err(format!(
            "Krieg zwischen {a} und {b}: Eine von beiden hat geendet oder ist nicht frei"
        ));
    }
    if let Some((a, b)) = diplomacy
        .truces
        .keys()
        .find(|(a, b)| ended(*a) || ended(*b))
    {
        return Err(format!(
            "Waffenruhe zwischen {a} und {b}: Eine von beiden hat geendet"
        ));
    }
    if let Some(fleet) = ai
        .reserved
        .iter()
        .find(|id| fleets.get(**id).is_none_or(|fleet| !fleet.alive))
    {
        return Err(format!(
            "Flotte {fleet} ist vorgemerkt, besteht aber nicht mehr"
        ));
    }
    for (index, standing) in diplomacy.standing.iter().enumerate() {
        let Some(overlord) = standing.overlord else {
            continue;
        };
        if ended(overlord) || !diplomacy.is_free(overlord) || overlord.index() == index {
            return Err(format!(
                "{} untersteht {overlord}, das nicht lebt oder nicht frei ist",
                polities.list[index].name
            ));
        }
    }
    for bond in &finance.bonds {
        let held_by_ended = matches!(bond.holder, HolderId::State(polity) if ended(polity));
        if ended(bond.issuer) || held_by_ended {
            return Err(format!(
                "Anleihe von {} bei {:?}: Eine Seite hat geendet",
                bond.issuer, bond.holder
            ));
        }
    }
    for (id, issuer) in &finance.issuers {
        let stale = issuer
            .holdings
            .keys()
            .chain(issuer.offered.keys())
            .find(|holder| matches!(holder, HolderId::State(polity) if ended(*polity)));
        if let Some(holder) = stale {
            return Err(format!(
                "{id:?}: Anteile bei {holder:?}, einem Staat, der geendet hat"
            ));
        }
    }
    for (index, region) in finance.regions.iter().enumerate() {
        if region.owner_seen.is_some_and(ended) || region.factors.foreign_polity.is_some_and(ended)
        {
            return Err(format!(
                "Region {}: nennt eine Gemeinschaft, die geendet hat",
                galaxy.planets[index].name
            ));
        }
    }
    if let Some(player) = config.player
        && (ended(player) || !diplomacy.is_free(player))
    {
        return Err(format!(
            "der Spieler führt {player}, das nicht lebt oder nicht frei ist"
        ));
    }
    if let Some(survivor) = endgame.outcome.and_then(|outcome| outcome.survivor)
        && ended(survivor)
    {
        return Err(format!("die Überlebende {survivor} hat geendet"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Eine frische Partie als Zustandsabbild, dazu eine Welt, um die Prüfung laufen zu
    /// lassen.
    fn fresh(seed: u64) -> (GameData, SimState) {
        let data = GameData::load_workspace().unwrap();
        let state = SimState::generate(&data, SimConfig { seed, player: None });
        (data, state)
    }

    fn checked(data: &GameData, state: &SimState) -> Result<(), String> {
        let mut world = World::new();
        world.insert_resource(data.clone());
        state.clone().install(&mut world);
        check(&world)
    }

    /// Länge jeder Liste je Gemeinschaft.
    fn lengths(state: &SimState) -> Vec<usize> {
        let SimState {
            polities,
            economy,
            finance,
            research,
            logistics,
            fleets,
            battles,
            diplomacy,
            ..
        } = state;
        let mut lengths = vec![
            polities.list.len(),
            economy.treasuries.len(),
            economy.tax_rates.len(),
            economy.treasury_targets.len(),
            economy.tech_yield.len(),
            economy.state_income.len(),
            economy.state_income_running.len(),
            finance.polities.len(),
            research.polities.len(),
            research.intel.len(),
            research.beliefs.len(),
            logistics.tech_speed.len(),
            fleets.arrears.len(),
            battles.ships_lost.len(),
            diplomacy.standing.len(),
            diplomacy.culture.len(),
        ];
        lengths.extend(research.intel.iter().map(Vec::len));
        lengths.extend(research.beliefs.iter().map(Vec::len));
        lengths
    }

    #[test]
    fn a_fresh_game_keeps_every_rule() {
        let (data, state) = fresh(2);
        assert_eq!(checked(&data, &state), Ok(()));
        let count = state.polities.list.len();
        assert!(lengths(&state).iter().all(|length| *length == count));
        assert_eq!(state.endgame.path.len(), state.galaxy.planets.len());
        assert!(state.endgame.path.iter().all(|path| *path == Path::Own));
    }

    #[test]
    fn a_new_community_gets_one_entry_in_every_list_and_what_it_inherits() {
        let (data, mut state) = fresh(2);
        let parent = PolityId(1);
        let count = state.polities.list.len();
        // Eine Kolonie von Hand: Zu Spielbeginn hält jede Gemeinschaft nur ihre Hauptwelt.
        let colony = state
            .galaxy
            .planets
            .iter()
            .find(|planet| planet.owner.is_none())
            .unwrap()
            .id;
        state.galaxy.planet_mut(colony).owner = Some(parent);
        state.galaxy.planet_mut(colony).population = 25.0;
        state.economy.tax_rates[parent.index()] = 0.23;
        state.economy.treasury_targets[parent.index()] = 4_000;
        state.finance.polities[parent.index()].war_weariness = 40.0;
        let tech = data.tech_index("antrieb_2").unwrap();
        state.research.grant(&data, parent, tech);
        let founding =
            Founding::free_state(state.polities.get(parent), state.galaxy.planet(colony));
        assert!(founding.name.starts_with("Freistaat "));
        assert_eq!(founding.capital, colony);
        let tick = 77 * TICKS_PER_DAY;

        let mut succession = Succession::of_state(&mut state, &data);
        let child = succession.spawn(parent, founding.clone(), tick).unwrap();
        let terms = TransferTerms::secession(colony, &data.balance.finance.cohesion, Path::Own);
        let transfer = succession
            .transfer_planet(colony, child, terms, tick)
            .unwrap();
        assert_eq!(
            transfer,
            Transfer {
                from: parent,
                new_capital: None,
                eliminated: false
            }
        );

        // Genau ein Eintrag mehr, in jeder Liste.
        assert_eq!(child, PolityId::from_index(count));
        assert!(lengths(&state).iter().all(|length| *length == count + 1));
        assert_eq!(checked(&data, &state), Ok(()));
        let born = state.polities.get(child);
        assert_eq!((&born.name, &born.short), (&founding.name, &founding.short));
        assert_eq!((born.capital, born.alive), (Some(colony), true));
        assert_eq!(state.galaxy.planet(colony).owner, Some(child));
        // Geerbt: Steuersatz, Kultur, Technik. Nicht geerbt: Kasse, Rücklage, Lasten.
        let index = child.index();
        assert_eq!(state.economy.tax_rates[index], 0.23);
        assert_eq!(state.economy.treasuries[index], 0);
        assert_eq!(state.economy.treasury_targets[index], 0);
        assert_eq!(
            state.diplomacy.culture[index],
            state.diplomacy.culture[parent.index()]
        );
        assert_eq!(
            state.diplomacy.standing[index],
            Standing {
                overlord: None,
                voluntary: false,
                since_day: 77
            }
        );
        assert!(state.research.knows(child, tech));
        assert_eq!(state.research.known(child), state.research.known(parent));
        // Werke und Frachter rechnen sofort mit der geerbten Technik.
        let effects = state.research.effects(child);
        assert!(effects.fleet_speed > 1.0);
        assert_eq!(state.economy.tech_yield[index], effects.yield_factor);
        assert_eq!(state.logistics.tech_speed[index], effects.freighter_speed);
        let finance = &state.finance.polities[index];
        assert_eq!(finance.war_weariness, 0.0);
        assert_eq!(finance.bond_yield, data.balance.finance.base_yield);
        assert_eq!(state.fleets.arrears[index], 0.0);
        assert_eq!(state.battles.ships_lost[index], 0);
        // Die Mutter behält, was sie hatte.
        assert_eq!(state.economy.treasury_targets[parent.index()], 4_000);
        assert_eq!(state.finance.polities[parent.index()].war_weariness, 40.0);
        assert!(state.polities.get(parent).alive);
    }

    #[test]
    fn a_secession_passes_todays_terms() {
        let (data, _) = fresh(2);
        let balance = &data.balance.finance.cohesion;
        let planet = PlanetId(5);
        let terms = TransferTerms::secession(planet, balance, Path::Joined);
        assert_eq!(
            terms,
            TransferTerms {
                state_shares_to: HolderId::Public(planet),
                state_share: 0.5,
                cohesion: balance.base + balance.honeymoon,
                stage: Stage::Loyal,
                shock: 0.85,
                elite_flight: 0.0,
                path: Path::Joined,
            }
        );
    }
}
