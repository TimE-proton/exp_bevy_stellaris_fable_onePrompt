//! Krieg: Ziel, Bilanz, zulässige Friedensbedingungen und das Diktat.
//!
//! Die Bilanz eines Krieges zählt aus Sicht des Angreifers, −100..100: genommene
//! Bevölkerung gegen die Ausgangsbevölkerung des Gegners, verlorener Flottenwert gegen
//! den eigenen bei Kriegsbeginn, und der Unterschied der Kriegsmüdigkeit. Wer vorn liegt,
//! darf mehr verlangen ([`terms_band`]). Nach `war_max_months` oder wenn beide erschöpft
//! sind, endet der Krieg von selbst ([`diktat`]).

use super::Diplomacy;
use super::proposals::PeaceTerms;
use super::relations::{War, WarAim, pair, side};
use crate::clock::{DAYS_PER_MONTH, TICKS_PER_DAY};
use crate::data::{DiplomacyLevers, GameData};
use crate::finance::Finance;
use crate::fleet::{Battles, Fleets};
use crate::galaxy::Galaxy;
use crate::ids::{PlanetId, PolityId};

/// Bauwert aller Flotten eines Verbunds in Credits.
pub fn bloc_fleet_value(
    data: &GameData,
    diplomacy: &Diplomacy,
    fleets: &Fleets,
    polity: PolityId,
) -> f64 {
    let bloc = diplomacy.bloc(polity);
    fleets
        .list
        .iter()
        .filter(|fleet| fleet.alive && bloc.contains(&fleet.owner))
        .flat_map(|fleet| &fleet.ships)
        .filter_map(|ship| data.designs.get(ship.design as usize))
        .map(|design| design.stats.value)
        .sum()
}

/// Bevölkerung eines Verbunds (Mio.).
pub fn bloc_population(galaxy: &Galaxy, diplomacy: &Diplomacy, polity: PolityId) -> f64 {
    diplomacy
        .bloc(polity)
        .into_iter()
        .map(|member| galaxy.population_of(member))
        .sum()
}

/// Ausgangslage eines Krieges, Angreifer zuerst.
pub fn figures(
    data: &GameData,
    galaxy: &Galaxy,
    diplomacy: &Diplomacy,
    fleets: &Fleets,
    attacker: PolityId,
    target: PolityId,
) -> super::WarFigures {
    super::WarFigures {
        population: [
            bloc_population(galaxy, diplomacy, attacker),
            bloc_population(galaxy, diplomacy, target),
        ],
        fleet: [
            bloc_fleet_value(data, diplomacy, fleets, attacker),
            bloc_fleet_value(data, diplomacy, fleets, target),
        ],
    }
}

/// Kriegsmüdigkeit des Hauptes einer Gemeinschaft, 0..100.
pub fn weariness(finance: &Finance, polity: PolityId) -> f64 {
    finance
        .polities
        .get(polity.index())
        .map_or(0.0, |state| state.war_weariness)
}

/// Bilanz des Krieges aus Sicht der Seite `side_index` (Stelle in den Feldern je Seite),
/// −100..100.
pub fn score_for(
    war: &War,
    side_index: usize,
    weariness: [f64; 2],
    levers: &DiplomacyLevers,
) -> f32 {
    let (me, foe) = (side_index, 1 - side_index);
    let share = |value: f64, base: f64| if base > 0.0 { value / base } else { 0.0 };
    let (w_pop, w_fleet, w_weary) = levers.score_weights;
    let taken = share(war.taken[me], war.pop0[foe]) - share(war.taken[foe], war.pop0[me]);
    let lost = share(war.lost[foe], war.fleet0[foe]) - share(war.lost[me], war.fleet0[me]);
    let weary = (weariness[foe] - weariness[me]) / 100.0;
    (w_pop as f64 * taken + w_fleet as f64 * lost.clamp(-1.0, 1.0) + w_weary as f64 * weary)
        .clamp(-100.0, 100.0) as f32
}

/// Vorsprung von `polity` im Krieg gegen `other` (über ihre Häupter), −100..100; 0 ohne
/// Krieg oder ohne bekannte Umstände.
pub fn lead_of(
    diplomacy: &Diplomacy,
    finance: &Finance,
    levers: &DiplomacyLevers,
    polity: PolityId,
    other: PolityId,
) -> f32 {
    let (a, b) = (diplomacy.head(polity), diplomacy.head(other));
    let Some(war) = diplomacy.war_between(a, b) else {
        return 0.0;
    };
    let mut wear = [0.0; 2];
    wear[side(a, b)] = weariness(finance, a);
    wear[side(b, a)] = weariness(finance, b);
    score_for(war, side(a, b), wear, levers)
}

/// Was der Sieger bei gegebenem Vorsprung verlangen darf.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TermsBand {
    /// Tribut und Abtretungen.
    pub concessions: bool,
    /// Höchster Anteil der Bevölkerung des Unterlegenen, der abgetreten werden darf.
    pub cede_share: f64,
    /// Unterwerfung.
    pub subjugation: bool,
}

/// Zulässige Forderungen des Siegers `victor` an `yielder`. Die Unterwerfung verlangt
/// einen Vorsprung ab `terms_subjugate` oder die eingenommene Hauptwelt des Gegners, und
/// ein Angreifer darf sie nur fordern, wenn sie sein Kriegsziel ist.
pub fn terms_band(
    war: Option<&War>,
    lead: f32,
    capital_taken: bool,
    victor: PolityId,
    levers: &DiplomacyLevers,
) -> TermsBand {
    let concessions = lead >= levers.terms_tribute;
    let aim_allows = war.is_none_or(|war| war.attacker != victor || war.aim == WarAim::Subjugation);
    TermsBand {
        concessions,
        cede_share: if concessions {
            (lead as f64 / 100.0).clamp(0.0, 1.0) * 0.5
        } else {
            0.0
        },
        subjugation: aim_allows && (lead >= levers.terms_subjugate || capital_taken),
    }
}

/// Liegen die Bedingungen im Band? `demand`: Der Sieger fordert sie (vom Unterlegenen
/// angeboten gilt jedes Angebot).
pub fn check_terms(
    galaxy: &Galaxy,
    terms: &PeaceTerms,
    band: TermsBand,
    yielder_population: f64,
    levers: &DiplomacyLevers,
) -> Result<(), String> {
    if terms.is_white() {
        return Ok(());
    }
    if !band.concessions {
        return Err(format!(
            "der Vorsprung reicht nur für einen Frieden ohne Bedingungen (ab {:.0} Punkten \
             Tribut und Abtretungen)",
            levers.terms_tribute
        ));
    }
    if terms.subjugate && !band.subjugation {
        return Err(format!(
            "für eine Unterwerfung braucht es {:.0} Punkte Vorsprung oder die Hauptwelt des \
             Gegners, und als Angreifer das Kriegsziel Unterwerfung",
            levers.terms_subjugate
        ));
    }
    let ceded: f64 = terms
        .cede
        .iter()
        .filter_map(|planet| galaxy.planets.get(planet.index()))
        .map(|planet| planet.population)
        .sum();
    if yielder_population > 0.0 && ceded > band.cede_share * yielder_population + 1e-9 {
        return Err(format!(
            "die Abtretung umfasst {:.0} % der Bevölkerung; der Vorsprung erlaubt höchstens {:.0} %",
            100.0 * ceded / yielder_population,
            100.0 * band.cede_share
        ));
    }
    Ok(())
}

/// Eine Welt ist im Krieg genommen worden: in die Bilanz des Krieges zwischen den
/// Häuptern von altem und neuem Besitzer.
pub fn record_taken(
    diplomacy: &mut Diplomacy,
    from: PolityId,
    to: PolityId,
    population: f64,
    planet: PlanetId,
    was_capital: bool,
) {
    let (loser, winner) = (diplomacy.head(from), diplomacy.head(to));
    if loser == winner || !diplomacy.wars.contains(&pair(loser, winner)) {
        return;
    }
    if let Some(war) = diplomacy
        .relations
        .get_mut(&pair(loser, winner))
        .and_then(|relation| relation.war.as_mut())
    {
        war.taken[side(winner, loser)] += population.max(0.0);
        war.conquered.push((winner, planet));
        if was_capital {
            war.took_capital[side(winner, loser)] = true;
        }
    }
}

/// Täglich: Verluste aus Gefechten, die seit der letzten Zählung endeten, in die
/// Bilanzen der Kriege.
pub fn record_battles(diplomacy: &mut Diplomacy, battles: &Battles, tick: u64) {
    let since = diplomacy.battles_counted_until;
    for report in &battles.reports {
        if report.ended < since || report.ended >= tick {
            continue;
        }
        for side_report in &report.sides {
            let loser = diplomacy.head(side_report.owner);
            for other in &report.sides {
                let foe = diplomacy.head(other.owner);
                if foe == loser || !diplomacy.wars.contains(&pair(loser, foe)) {
                    continue;
                }
                if let Some(war) = diplomacy
                    .relations
                    .get_mut(&pair(loser, foe))
                    .and_then(|relation| relation.war.as_mut())
                {
                    war.lost[side(loser, foe)] += side_report.lost_value as f64;
                }
                // Jede Partei wird nur einem Krieg angerechnet: dem mit dem ersten Gegner.
                break;
            }
        }
    }
    diplomacy.battles_counted_until = tick;
}

/// Monate, die ein Krieg schon dauert.
pub fn months(war: &War, tick: u64) -> u32 {
    ((tick / TICKS_PER_DAY).saturating_sub(war.since_day) / DAYS_PER_MONTH) as u32
}

/// Was das Diktat über einen Krieg verfügt.
#[derive(Clone, Debug, PartialEq)]
pub enum Diktat {
    /// Beide sind erschöpft, oder keiner liegt deutlich vorn: Frieden ohne Bedingungen.
    White,
    /// Der Vorsprung entscheidet: Der Unterlegene zahlt (und wird unterworfen, wenn das
    /// Band es erlaubt).
    Imposed { victor: PolityId, terms: PeaceTerms },
}

/// Monatlich: Endet dieser Krieg von selbst? Nach `war_max_months` oder bei voller Bilanz
/// diktiert der Vorsprung; sind dann beide erschöpft (`exhaustion`), oder liegt keiner
/// deutlich vorn, endet er ohne Sieger. Vorher endet kein Krieg von selbst: Die
/// Kriegsmüdigkeit wächst in jedem Krieg mit der Zeit und wirkt bis dahin über die
/// Abwägung der Antworten auf Friedensvorschläge.
pub fn diktat(
    war: &War,
    a: PolityId,
    b: PolityId,
    weariness: [f64; 2],
    capital_taken: [bool; 2],
    tick: u64,
    levers: &DiplomacyLevers,
) -> Option<Diktat> {
    let score = score_for(war, 0, weariness, levers);
    let timed_out = months(war, tick) >= levers.war_max_months;
    if !timed_out && score.abs() < 100.0 {
        return None;
    }
    if weariness[0] >= levers.exhaustion && weariness[1] >= levers.exhaustion {
        return Some(Diktat::White);
    }
    let (victor, yielder, lead, index) = if score >= 0.0 {
        (a, b, score, 0)
    } else {
        (b, a, -score, 1)
    };
    let band = terms_band(Some(war), lead, capital_taken[index], victor, levers);
    if !band.concessions {
        return Some(Diktat::White);
    }
    Some(Diktat::Imposed {
        victor,
        terms: PeaceTerms {
            yielder: Some(yielder),
            cede: Vec::new(),
            tribute_years: levers.terms_years,
            subjugate: band.subjugation,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn war() -> War {
        War {
            attacker: PolityId(0),
            aim: WarAim::Limited,
            since_day: 0,
            pop0: [1000.0, 1000.0],
            fleet0: [500.0, 500.0],
            taken: [0.0; 2],
            lost: [0.0; 2],
            conquered: Vec::new(),
            took_capital: [false; 2],
        }
    }

    #[test]
    fn score_is_antisymmetric_and_bounded() {
        let levers = crate::data::GameData::load_workspace()
            .unwrap()
            .diplomacy
            .levers;
        let mut war = war();
        war.taken = [400.0, 0.0];
        war.lost = [100.0, 400.0];
        let wear = [30.0, 60.0];
        let a = score_for(&war, 0, wear, &levers);
        let b = score_for(&war, 1, wear, &levers);
        assert!((a + b).abs() < 1e-4);
        assert!(a > 20.0, "{a}");
        war.taken = [5000.0, 0.0];
        assert_eq!(score_for(&war, 0, wear, &levers), 100.0);
    }

    #[test]
    fn the_band_widens_with_the_lead_and_the_aim() {
        let levers = crate::data::GameData::load_workspace()
            .unwrap()
            .diplomacy
            .levers;
        let mut limited = war();
        let attacker = PolityId(0);
        let defender = PolityId(1);
        let small = terms_band(Some(&limited), 10.0, false, attacker, &levers);
        assert!(!small.concessions && !small.subjugation);
        let mid = terms_band(Some(&limited), 30.0, false, attacker, &levers);
        assert!(mid.concessions && !mid.subjugation && mid.cede_share > 0.0);
        let big = terms_band(Some(&limited), 80.0, false, attacker, &levers);
        assert!(!big.subjugation, "begrenztes Ziel: keine Unterwerfung");
        assert!(terms_band(Some(&limited), 80.0, false, defender, &levers).subjugation);
        limited.aim = WarAim::Subjugation;
        assert!(terms_band(Some(&limited), 80.0, false, attacker, &levers).subjugation);
        assert!(terms_band(Some(&limited), 25.0, true, attacker, &levers).subjugation);
    }

    #[test]
    fn exhaustion_and_time_end_every_war() {
        let levers = crate::data::GameData::load_workspace()
            .unwrap()
            .diplomacy
            .levers;
        let war = war();
        let (a, b) = (PolityId(0), PolityId(1));
        let day = TICKS_PER_DAY * DAYS_PER_MONTH;
        assert_eq!(
            diktat(&war, a, b, [10.0, 10.0], [false; 2], day, &levers),
            None
        );
        // Erschöpfung allein beendet vor der Frist keinen Krieg, …
        assert_eq!(
            diktat(&war, a, b, [90.0, 90.0], [false; 2], day, &levers),
            None
        );
        let late = TICKS_PER_DAY * DAYS_PER_MONTH * levers.war_max_months as u64;
        assert_eq!(
            diktat(&war, a, b, [10.0, 10.0], [false; 2], late, &levers),
            Some(Diktat::White)
        );
        let mut lost = war.clone();
        lost.taken = [600.0, 0.0];
        match diktat(&lost, a, b, [10.0, 10.0], [false; 2], late, &levers) {
            Some(Diktat::Imposed { victor, terms }) => {
                assert_eq!(victor, a);
                assert_eq!(terms.yielder, Some(b));
                assert!(terms.tribute_years > 0);
                assert!(!terms.subjugate);
            }
            other => panic!("{other:?}"),
        }
        // … nach ihr aber entscheidet sie über den Vorsprung hinweg.
        match diktat(&lost, a, b, [90.0, 95.0], [false; 2], late, &levers) {
            Some(Diktat::White) => {}
            other => panic!("{other:?}"),
        }
    }
}
