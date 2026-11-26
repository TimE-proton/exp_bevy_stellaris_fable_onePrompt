//! Zusammenhalt: Warum eine Region beim Reich bleibt oder geht.
//!
//! Gerechnet wird je Planet aus der Besitzlage. Es bindet, wenn die Einheimischen am
//! übrigen Reich beteiligt sind und der Besitz an ihren Betrieben breit gestreut ist. Es
//! trennt, wenn das Zentrum alles hält, die Gewinne einseitig abfließen oder Fremde die
//! Mehrheit haben. Krieg, Mangel, Steuern und Enteignungen kommen hinzu.

use super::shares::seat_of_issuer;
use super::{Finance, polity_of_holder, region_of_holder};
use crate::chronicle::{Chronicle, ChronicleKind};
use crate::clock::DAYS_PER_YEAR;
use crate::data::{CohesionBalance, GameData};
use crate::economy::{Economy, to_credits};
use crate::fleet::Fleets;
use crate::galaxy::Galaxy;
use crate::ids::{HolderId, IssuerId, PlanetId, PolityId};
use crate::logistics::Logistics;
use crate::polity::{Polities, Polity};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Stage {
    #[default]
    Loyal,
    /// Unruhig: Kurse leiden, noch keine Folgen für den Staat.
    Restless,
    /// Autonom: Die Region zahlt keine Steuern mehr an das Zentrum.
    Autonomous,
}

impl Stage {
    pub fn label(self) -> &'static str {
        match self {
            Stage::Loyal => "loyal",
            Stage::Restless => "unruhig",
            Stage::Autonomous => "autonom",
        }
    }
}

/// Wem die Betriebe einer Region gehören, als Anteile am Marktwert.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Ownership {
    /// Marktwert aller Betriebe der Region in Credits.
    pub value: f64,
    /// Elite und Fonds der Region selbst.
    pub local: f64,
    /// Staat sowie Elite und Fonds der Hauptwelt.
    pub center: f64,
    /// Übrige Regionen desselben Reichs, je Planet.
    pub regions: BTreeMap<PlanetId, f64>,
    /// Halter fremder Gemeinschaften, je Gemeinschaft.
    pub foreign: BTreeMap<PolityId, f64>,
}

impl Ownership {
    /// Streuung 0..1: Eins minus Herfindahl-Index über die Haltergruppen.
    pub fn dispersion(&self) -> f64 {
        let mut hhi = self.local * self.local + self.center * self.center;
        hhi += self
            .regions
            .values()
            .map(|share| share * share)
            .sum::<f64>();
        hhi += self
            .foreign
            .values()
            .map(|share| share * share)
            .sum::<f64>();
        (1.0 - hhi).clamp(0.0, 1.0)
    }

    /// Größter Anteil einer einzelnen fremden Gemeinschaft.
    pub fn largest_foreign(&self) -> Option<(PolityId, f64)> {
        let mut best: Option<(PolityId, f64)> = None;
        for (polity, share) in &self.foreign {
            if best.is_none_or(|(_, most)| *share > most) {
                best = Some((*polity, *share));
            }
        }
        best
    }
}

/// Betriebe, die einer Region zugerechnet werden.
fn issuers_of_region<'a>(
    finance: &'a Finance,
    galaxy: &'a Galaxy,
    logistics: &'a Logistics,
    capitals: &'a [Option<PlanetId>],
    planet: PlanetId,
) -> impl Iterator<Item = (IssuerId, &'a super::Issuer)> + 'a {
    finance.issuers.iter().filter_map(move |(id, issuer)| {
        let local = match id {
            IssuerId::Firm { planet: seat, .. } => *seat == planet,
            IssuerId::Kontor(_) => seat_of_issuer(galaxy, logistics, capitals, *id) == Some(planet),
            // Handelshäuser gehören dem ganzen Reich, keiner einzelnen Region.
            IssuerId::House(_) => false,
        };
        local.then_some((*id, issuer))
    })
}

pub fn ownership_of_region(
    finance: &Finance,
    galaxy: &Galaxy,
    logistics: &Logistics,
    capitals: &[Option<PlanetId>],
    planet: PlanetId,
) -> Ownership {
    let mut ownership = Ownership::default();
    let Some(owner) = galaxy.planet(planet).owner else {
        return ownership;
    };
    let capital = capitals.get(owner.index()).copied().flatten();
    for (_, issuer) in issuers_of_region(finance, galaxy, logistics, capitals, planet) {
        for (holder, shares) in &issuer.holdings {
            let value = *shares as f64 * issuer.price;
            ownership.value += value;
            let home = region_of_holder(*holder);
            match polity_of_holder(galaxy, *holder) {
                Some(polity) if polity != owner => {
                    *ownership.foreign.entry(polity).or_default() += value
                }
                // Halter einer verlassenen Welt zählen zu niemandem; ihr Anteil ist Streubesitz.
                None => *ownership.regions.entry(home.unwrap_or(planet)).or_default() += value,
                Some(_) if home == Some(planet) => ownership.local += value,
                Some(_) if matches!(holder, HolderId::State(_)) || home == capital => {
                    ownership.center += value;
                }
                Some(_) => *ownership.regions.entry(home.unwrap_or(planet)).or_default() += value,
            }
        }
    }
    if ownership.value > 0.0 {
        let value = ownership.value;
        ownership.local /= value;
        ownership.center /= value;
        ownership
            .regions
            .values_mut()
            .for_each(|share| *share /= value);
        ownership
            .foreign
            .values_mut()
            .for_each(|share| *share /= value);
    }
    ownership
}

/// Die Größen, aus denen sich der Zusammenhalt einer Region ergibt. Rohwerte; die Punkte
/// dazu liefert [`CohesionFactors::contributions`].
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CohesionFactors {
    /// Anteil des Vermögens der Einheimischen, der im übrigen Reich angelegt ist (0..1).
    pub interlock: f64,
    /// Streuung des Besitzes an den Betrieben der Region (0..1).
    pub dispersion: f64,
    pub local_share: f64,
    pub center_share: f64,
    pub foreign_share: f64,
    pub foreign_polity: Option<PolityId>,
    /// Netto abfließende Dividenden im Verhältnis zum Einkommen vor Ort (0..).
    pub drain: f64,
    /// Deckung des Lebensnotwendigen (0..1).
    pub supply: f64,
    pub weariness: f64,
    pub tax: f64,
    pub blockaded: bool,
    pub is_capital: bool,
    /// Sprünge bis zur Hauptwelt.
    pub distance: f64,
    /// Jahre unter dem heutigen Besitzer.
    pub years: f64,
    pub expropriation: f64,
    /// Besatzung nach einer Eroberung (0..1) und wie weit stationierte Truppen sie
    /// auffangen (0..1); Kriegsschäden, solange ihr Verursacher die Welt hält (0..1).
    /// Spiegel aus [`Region`], dort von der Belagerung geschrieben.
    pub occupation: f64,
    pub garrison_cover: f64,
    pub war_damage: f64,
    /// Wert, auf den der Zusammenhalt zustrebt.
    pub target: f64,
}

impl CohesionFactors {
    /// Beiträge in Punkten, mit Beschriftung. Ihre Summe plus Grundwert ist das Ziel.
    pub fn contributions(&self, balance: &CohesionBalance) -> Vec<(&'static str, f64)> {
        let center = if self.is_capital {
            0.0
        } else {
            -balance.center * ((self.center_share - 0.5) / 0.5).clamp(0.0, 1.0)
        };
        let honeymoon =
            balance.honeymoon * (1.0 - self.years / balance.honeymoon_years).clamp(0.0, 1.0);
        vec![
            (
                "Verflechtung mit dem Reich",
                balance.interlock * self.interlock,
            ),
            (
                "Streuung des Besitzes",
                balance.dispersion * self.dispersion,
            ),
            ("Besitz im Zentrum", center),
            (
                "Abfluss der Gewinne",
                -balance.drain * (self.drain / balance.drain_scale).clamp(0.0, 1.0),
            ),
            (
                "Fremder Besitz",
                -balance.foreign * ((self.foreign_share - 0.25) / 0.5).clamp(0.0, 1.0),
            ),
            (
                "Versorgung",
                -balance.supply * (1.0 - self.supply).clamp(0.0, 1.0),
            ),
            ("Kriegsmüdigkeit", -balance.weariness * self.weariness),
            ("Steuern", -balance.tax * (self.tax - balance.tax_neutral)),
            (
                "Blockade",
                if self.blockaded {
                    -balance.blockade
                } else {
                    0.0
                },
            ),
            (
                "Hauptwelt",
                if self.is_capital {
                    balance.capital
                } else {
                    0.0
                },
            ),
            (
                "Entfernung",
                -(balance.distance * self.distance).min(balance.distance_cap),
            ),
            // Unter Besatzung kommt keine Aufbruchstimmung auf.
            ("Aufbruchstimmung", honeymoon * (1.0 - self.occupation)),
            ("Enteignungen", -balance.expropriation * self.expropriation),
            (
                "Besatzung",
                -balance.occupation
                    * self.occupation
                    * (1.0 - balance.occupation_hold * self.garrison_cover),
            ),
            ("Kriegsschäden", -balance.war_damage * self.war_damage),
        ]
    }
}

/// Zielwert des Zusammenhalts aus den Faktoren.
pub fn cohesion_target(factors: &CohesionFactors, balance: &CohesionBalance) -> f64 {
    let sum: f64 = factors
        .contributions(balance)
        .iter()
        .map(|(_, points)| points)
        .sum();
    (balance.base + sum).clamp(0.0, 100.0)
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Region {
    /// Zusammenhalt 0..100.
    pub cohesion: f64,
    pub stage: Stage,
    /// Monate in Folge unter der Schwelle zur nächsten Eskalation.
    pub months_below: u32,
    /// Monate in Folge über der Schwelle zur Beruhigung.
    pub months_above: u32,
    pub factors: CohesionFactors,
    /// Dividenden des laufenden Monats: an Auswärtige gezahlt, von auswärts erhalten, im
    /// Ort geblieben.
    pub dividends_out: f64,
    pub dividends_in: f64,
    pub dividends_local: f64,
    /// Geglätteter Nettoabfluss im Verhältnis zum Einkommen vor Ort.
    pub drain: f64,
    /// Besitzer beim letzten Monatsabschluss und Tag, seit dem er es ist.
    pub owner_seen: Option<PolityId>,
    pub since_day: u64,
    /// Spiegelwerte der Belagerung; nur sie schreibt sie. `besieged`: Ein Datensatz
    /// besteht. `occupation`: Besatzung nach einer Eroberung (0..1). `garrison_cover`:
    /// Anteil der Garnison, die eine besetzte Welt ruhig hielte (0..1). `war_damage`:
    /// Kriegsschäden, soweit der heutige Besitzer sie verursacht hat (0..1).
    pub besieged: bool,
    pub occupation: f64,
    pub garrison_cover: f64,
    pub war_damage: f64,
}

/// Abspaltung einer Region.
#[derive(Clone, Debug, PartialEq)]
pub struct Secession {
    pub planet: PlanetId,
    pub from: PolityId,
    /// Übertritt zu einer bestehenden Gemeinschaft oder `None` für die Unabhängigkeit.
    pub joins: Option<PolityId>,
}

/// Monatlicher Abschluss: Faktoren neu bestimmen, Zusammenhalt nachführen, Stufen
/// wechseln. Gibt die fälligen Abspaltungen zurück; ausgeführt werden sie von
/// [`apply_secession`].
pub fn update_cohesion(
    finance: &mut Finance,
    galaxy: &Galaxy,
    economy: &mut Economy,
    logistics: &Logistics,
    fleets: &Fleets,
    polities: &Polities,
    data: &GameData,
    day: u64,
) -> Vec<Secession> {
    let balance = &data.balance.finance.cohesion;
    let capitals: Vec<Option<PlanetId>> = polities.list.iter().map(|p| p.capital).collect();
    let mut secessions = Vec::new();

    // Vermögen der Einheimischen: insgesamt und davon im übrigen Reich angelegt.
    let planet_count = galaxy.planets.len();
    let mut wealth = vec![0.0f64; planet_count];
    let mut abroad_in_realm = vec![0.0f64; planet_count];
    for (id, issuer) in &finance.issuers {
        let seat = seat_of_issuer(galaxy, logistics, &capitals, *id);
        let issuer_polity = super::polity_of_issuer(galaxy, logistics, *id);
        for (holder, shares) in &issuer.holdings {
            let Some(home) = region_of_holder(*holder) else {
                continue;
            };
            let value = *shares as f64 * issuer.price;
            wealth[home.index()] += value;
            if seat != Some(home)
                && issuer_polity.is_some()
                && issuer_polity == galaxy.planet(home).owner
            {
                abroad_in_realm[home.index()] += value;
            }
        }
    }
    for bond in &finance.bonds {
        if let Some(home) = region_of_holder(bond.holder) {
            let value = to_credits(bond.principal);
            wealth[home.index()] += value;
            // Wer dem eigenen Staat Geld leiht, hängt an dessen Bestand.
            if galaxy.planet(home).owner == Some(bond.issuer) {
                abroad_in_realm[home.index()] += value;
            }
        }
    }

    for planet in &galaxy.planets {
        let index = planet.id.index();
        let owner = planet.owner;
        // Besitzwechsel: Die Uhr beginnt neu.
        if finance.regions[index].owner_seen != owner {
            let region = &mut finance.regions[index];
            let fresh = region.owner_seen.is_none();
            region.owner_seen = owner;
            region.since_day = day;
            region.stage = Stage::Loyal;
            region.months_below = 0;
            region.months_above = 0;
            if fresh {
                region.cohesion = balance.base + balance.honeymoon;
            }
            economy.planets[index].tax_exempt = false;
        }
        let Some(owner) = owner else {
            continue;
        };
        if planet.population <= 0.0 {
            continue;
        }
        let ownership = ownership_of_region(finance, galaxy, logistics, &capitals, planet.id);
        let capital = capitals.get(owner.index()).copied().flatten();
        let is_capital = capital == Some(planet.id);
        let local = &economy.planets[index];

        // Verflechtung zählt nur, soweit die Einheimischen überhaupt Vermögen haben, das
        // sich mit dem Wert ihrer Betriebe messen kann.
        let weight = (wealth[index] / (0.3 * ownership.value).max(1.0)).min(1.0);
        let interlock = if wealth[index] > 0.0 {
            weight * abroad_in_realm[index] / wealth[index]
        } else {
            0.0
        };

        let region = &mut finance.regions[index];
        let income = local.income * 30.0 + region.dividends_local + region.dividends_in;
        let drain_now = (region.dividends_out - region.dividends_in).max(0.0) / income.max(1.0);
        region.drain += 0.2 * (drain_now - region.drain);
        region.dividends_out = 0.0;
        region.dividends_in = 0.0;
        region.dividends_local = 0.0;

        let distance = capital
            .map(|capital| {
                let hops = galaxy.hops_from(galaxy.planet(capital).system)[planet.system.index()];
                if hops == u32::MAX { 10.0 } else { hops as f64 }
            })
            .unwrap_or(0.0);
        let (foreign_polity, foreign_share) = match ownership.largest_foreign() {
            Some((polity, share)) => (Some(polity), share),
            None => (None, 0.0),
        };
        let state = &finance.polities[owner.index()];
        let factors = CohesionFactors {
            interlock,
            dispersion: ownership.dispersion(),
            local_share: ownership.local,
            center_share: ownership.center,
            foreign_share,
            foreign_polity,
            drain: region.drain,
            supply: local.essentials_met(&data.balance.economy),
            weariness: state.war_weariness,
            tax: economy.tax_rates[owner.index()],
            blockaded: fleets.is_blockaded(planet.system),
            is_capital,
            distance,
            years: (day.saturating_sub(region.since_day)) as f64 / DAYS_PER_YEAR as f64,
            expropriation: state.expropriation_risk,
            occupation: region.occupation,
            garrison_cover: region.garrison_cover,
            war_damage: region.war_damage,
            target: 0.0,
        };
        let target = cohesion_target(&factors, balance);
        region.cohesion += balance.adjust * (target - region.cohesion);
        region.factors = CohesionFactors { target, ..factors };

        // Stufen mit Verweildauern und Hysterese: kein Hin und Her an der Schwelle.
        let cohesion = region.cohesion;
        match region.stage {
            Stage::Loyal => {
                if cohesion < balance.restless_below {
                    region.stage = Stage::Restless;
                    region.months_below = 0;
                    region.months_above = 0;
                }
            }
            Stage::Restless => {
                if cohesion >= balance.loyal_above {
                    region.stage = Stage::Loyal;
                    region.months_below = 0;
                } else if cohesion < balance.autonomy_below && !is_capital {
                    region.months_below += 1;
                    if region.months_below >= balance.autonomy_months {
                        region.stage = Stage::Autonomous;
                        region.months_below = 0;
                        region.months_above = 0;
                        economy.planets[index].tax_exempt = true;
                    }
                } else {
                    region.months_below = 0;
                }
            }
            Stage::Autonomous => {
                if cohesion >= balance.recover_above {
                    region.months_above += 1;
                    region.months_below = 0;
                    if region.months_above >= balance.recover_months {
                        region.stage = Stage::Restless;
                        region.months_above = 0;
                        economy.planets[index].tax_exempt = false;
                    }
                } else {
                    region.months_above = 0;
                    let pulled = foreign_share >= balance.foreign_pull;
                    if cohesion < balance.secession_below || pulled {
                        region.months_below += 1;
                    } else {
                        region.months_below = 0;
                    }
                    if region.months_below >= balance.secession_months {
                        region.months_below = 0;
                        let joins = foreign_polity.filter(|polity| {
                            pulled && polities.list.get(polity.index()).is_some_and(|p| p.alive)
                        });
                        secessions.push(Secession {
                            planet: planet.id,
                            from: owner,
                            joins,
                        });
                    }
                }
            }
        }
    }
    secessions
}

/// Führt eine Abspaltung aus. Entsteht eine neue Gemeinschaft, wird sie angelegt; ist die
/// Obergrenze erreicht, bleibt die Region autonom und die Frist beginnt neu.
pub fn apply_secession(
    secession: &Secession,
    finance: &mut Finance,
    galaxy: &mut Galaxy,
    economy: &mut Economy,
    logistics: &Logistics,
    polities: &mut Polities,
    chronicle: &mut Chronicle,
    data: &GameData,
    tick: u64,
) -> Option<PolityId> {
    let balance = &data.balance.finance.cohesion;
    let planet = secession.planet;
    let index = planet.index();
    let old = secession.from;
    if galaxy.planet(planet).owner != Some(old) {
        return None;
    }
    let planet_name = galaxy.planet(planet).name.clone();
    let system = galaxy.planet(planet).system;
    let new_owner = match secession.joins {
        Some(target) => {
            chronicle.push(
                tick,
                ChronicleKind::Cohesion,
                Some(target),
                Some(system),
                format!(
                    "{} sagt sich von {} los und tritt zu {} über, das dort die Mehrheit hält.",
                    planet_name,
                    polities.get(old).name,
                    polities.get(target).name
                ),
            );
            target
        }
        None => {
            let alive = polities.alive().count();
            // Ein eigener Staat braucht genug Menschen; kleine Kolonien bleiben autonom.
            if alive >= balance.max_polities as usize
                || galaxy.planet(planet).population < balance.independence_min_population
            {
                return None;
            }
            let id = PolityId::from_index(polities.list.len());
            let parent = polities.get(old).clone();
            // Die Farbe des Mutterreichs, aufgehellt.
            let lighten = |channel: f32| channel + (1.0 - channel) * 0.45;
            polities.list.push(Polity {
                id,
                name: format!("Freistaat {planet_name}"),
                short: "Freistaat".into(),
                motto: "Was wir erarbeiten, bleibt hier.".into(),
                color: (
                    lighten(parent.color.0),
                    lighten(parent.color.1),
                    lighten(parent.color.2),
                ),
                capital: Some(planet),
                alive: true,
                next_claim_day: 0,
            });
            economy.treasuries.push(0);
            economy.tax_rates.push(economy.tax_rates[old.index()]);
            economy.treasury_targets.push(0);
            finance.polities.push(super::PolityFinance {
                bond_yield: data.balance.finance.base_yield,
                ..Default::default()
            });
            chronicle.push(
                tick,
                ChronicleKind::Cohesion,
                Some(id),
                Some(system),
                format!(
                    "{} sagt sich von {} los und ruft den Freistaat aus.",
                    planet_name, parent.name
                ),
            );
            id
        }
    };
    galaxy.planet_mut(planet).owner = Some(new_owner);
    economy.planets[index].tax_exempt = false;

    // Die Abtrünnigen ziehen die Hälfte der Anteile des alten Staates an ihren Betrieben ein.
    let capitals: Vec<Option<PlanetId>> = polities.list.iter().map(|p| p.capital).collect();
    let local_issuers: Vec<IssuerId> =
        issuers_of_region(finance, galaxy, logistics, &capitals, planet)
            .map(|(id, _)| id)
            .collect();
    for id in local_issuers {
        if let Some(issuer) = finance.issuers.get_mut(&id) {
            let seized = issuer.held_by(HolderId::State(old)) / 2;
            let taken = issuer.remove(HolderId::State(old), seized);
            issuer.add(HolderId::Public(planet), taken);
            issuer.shock *= 0.85;
        }
    }
    let day = tick / crate::clock::TICKS_PER_DAY;
    let region = &mut finance.regions[index];
    region.owner_seen = Some(new_owner);
    region.since_day = day;
    region.stage = Stage::Loyal;
    region.months_below = 0;
    region.months_above = 0;
    region.cohesion = balance.base + balance.honeymoon;
    finance.secessions += 1;
    Some(new_owner)
}
