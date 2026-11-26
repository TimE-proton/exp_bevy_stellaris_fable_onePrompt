//! Das Ende einer Partie.
//!
//! Jede Welt trägt, auf welchem Weg sie zu ihrer heutigen Gemeinschaft kam: von ihr selbst
//! besiedelt, beigetreten oder genommen. Einmal im Monat wird gezählt. Bleibt nur noch ein
//! souveräner Verbund, endet die Partie: als Metamorphose, wenn mehr Menschen beigetreten
//! sind, als genommen wurden, sonst als Auslöschung. Bricht die Bevölkerung der Galaxie
//! zusammen oder ist niemand mehr souverän, endet sie ohne Überlebende.
//!
//! Die Entscheidung trifft eine reine Funktion ([`classify`]) über eine Zählung
//! ([`Census`]); geschrieben wird das Ergebnis genau einmal ([`judge`]). Danach steht die
//! Simulation still.

use crate::chronicle::{Chronicle, ChronicleKind};
use crate::clock::{SimClock, TICKS_PER_DAY, month_start};
use crate::data::{EndgameBalance, GameData};
use crate::diplomacy::Diplomacy;
use crate::galaxy::Galaxy;
use crate::ids::{PlanetId, PolityId};
use crate::polity::Polities;
use crate::{SimSet, sim_schedule};
use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use serde::{Deserialize, Serialize};

/// Weg, auf dem eine Welt zu ihrer heutigen Gemeinschaft kam.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Path {
    /// Von ihr besiedelt, oder mit ihr gegründet.
    #[default]
    Own,
    /// Beigetreten: Übertritt einer Region, Vereinigung, gewollte Unterstellung.
    Joined,
    /// Genommen: gegen den Willen der Gemeinschaft, der sie gehörte.
    Forced,
}

impl Path {
    pub fn label(self) -> &'static str {
        match self {
            Path::Own => "eigen",
            Path::Joined => "beigetreten",
            Path::Forced => "genommen",
        }
    }
}

/// Was aus der vom Menschen geführten Gemeinschaft geworden ist.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlayerFate {
    /// Führt eine freie Gemeinschaft (oder niemand spielt).
    #[default]
    Active,
    /// In einer Vereinigung aufgegangen, die eine andere Gemeinschaft führt.
    Merged,
    /// Ausgeschieden: Die Gemeinschaft hat geendet oder untersteht einer anderen.
    Out,
}

impl PlayerFate {
    pub fn label(self) -> &'static str {
        match self {
            PlayerFate::Active => "im Spiel",
            PlayerFate::Merged => "aufgegangen",
            PlayerFate::Out => "ausgeschieden",
        }
    }
}

/// Die beiden Enden einer Partie.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Ending {
    /// Die Galaxie ist überwiegend durch Beitritt eins geworden.
    Metamorphosis,
    /// Eine Gemeinschaft bleibt allein zurück, oder keine.
    Extinction,
}

impl Ending {
    pub fn label(self) -> &'static str {
        match self {
            Ending::Metamorphosis => "Metamorphose",
            Ending::Extinction => "Auslöschung",
        }
    }
}

/// Ausgang der Partie. Wird genau einmal geschrieben.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Outcome {
    pub ending: Ending,
    /// Die eine souveräne Gemeinschaft, die bleibt; `None`, wenn keine überdauert.
    pub survivor: Option<PolityId>,
    /// Tag, an dem die Partie endete.
    pub day: u64,
    /// Bevölkerung (Mio.) der beigetretenen und der genommenen Welten, jeweils im
    /// Augenblick ihres Wechsels: im Verbund der Überlebenden, ohne sie in der ganzen
    /// Galaxie.
    pub joined: f64,
    pub forced: f64,
}

impl Outcome {
    /// Jahre seit Spielbeginn, nach denen die Partie endete.
    pub fn year(&self) -> u64 {
        self.day / crate::clock::DAYS_PER_YEAR
    }
}

#[derive(Resource, Clone, Debug, Default, Serialize, Deserialize)]
pub struct Endgame {
    /// Je Planet: Weg zur heutigen Gemeinschaft.
    pub path: Vec<Path>,
    /// Je Planet: Bevölkerung (Mio.) im Augenblick des Wechsels; 0 für eigene Welten.
    /// Spätere Wanderung ändert daran nichts.
    pub absorbed_pop: Vec<f64>,
    /// Je Planet: kultureller Abstand (0..1) zwischen alter und neuer Gemeinschaft im
    /// Augenblick des Wechsels.
    pub gap: Vec<f32>,
    /// Je Planet: einstige Hauptwelt einer beigetretenen Gemeinschaft.
    pub seat: Vec<bool>,
    /// Höchststand der Bevölkerung aller in der Partie.
    pub peak_population: f64,
    /// Monate in Folge, in denen die Bevölkerung unter der Schwelle des Zusammenbruchs lag.
    pub months_below: u32,
    /// Gemeinschaften, die in dieser Partie durch Abspaltung entstanden sind.
    pub free_states: u32,
    pub player: PlayerFate,
    pub outcome: Option<Outcome>,
}

impl Endgame {
    /// Ausgangslage: Jede Welt ist eigen, nichts ist entschieden.
    pub fn new(planets: usize) -> Self {
        Self {
            path: vec![Path::Own; planets],
            absorbed_pop: vec![0.0; planets],
            gap: vec![0.0; planets],
            seat: vec![false; planets],
            peak_population: 0.0,
            months_below: 0,
            free_states: 0,
            player: PlayerFate::Active,
            outcome: None,
        }
    }

    /// Ist die Partie zu Ende?
    pub fn ended(&self) -> bool {
        self.outcome.is_some()
    }

    pub fn path_of(&self, planet: PlanetId) -> Path {
        self.path.get(planet.index()).copied().unwrap_or_default()
    }

    pub fn is_seat(&self, planet: PlanetId) -> bool {
        self.seat.get(planet.index()).copied().unwrap_or(false)
    }

    /// Vermerkt, auf welchem Weg eine Welt eben ihre Gemeinschaft gewechselt hat. Eine
    /// eigene Welt trägt weder Bevölkerung noch Abstand.
    pub fn tag(&mut self, planet: PlanetId, path: Path, population: f64, gap: f32) {
        // Erschöpfend zerlegt: Ein neues Feld je Planet fällt hier auf.
        let Endgame {
            path: paths,
            absorbed_pop,
            gap: gaps,
            seat: _,
            peak_population: _,
            months_below: _,
            free_states: _,
            player: _,
            outcome: _,
        } = self;
        let index = planet.index();
        let own = path == Path::Own;
        paths[index] = path;
        absorbed_pop[index] = if own { 0.0 } else { population.max(0.0) };
        gaps[index] = if own { 0.0 } else { gap.clamp(0.0, 1.0) };
    }

    /// Eine Welt ohne Besitzer trägt keine Geschichte: Wer sie neu besiedelt, dem ist sie
    /// eigen.
    pub fn clear_planet(&mut self, planet: PlanetId) {
        let Endgame {
            path,
            absorbed_pop,
            gap,
            seat,
            peak_population: _,
            months_below: _,
            free_states: _,
            player: _,
            outcome: _,
        } = self;
        let index = planet.index();
        path[index] = Path::Own;
        absorbed_pop[index] = 0.0;
        gap[index] = 0.0;
        seat[index] = false;
    }
}

/// Ein freier Verbund in der Zählung.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BlocCensus {
    /// Die freie Gemeinschaft an der Spitze.
    pub head: PolityId,
    /// Bevölkerung (Mio.) aller Welten des Verbunds heute.
    pub population: f64,
    /// Bevölkerung der beigetretenen bzw. genommenen Welten des Verbunds, jeweils im
    /// Augenblick ihres Wechsels (`Endgame::absorbed_pop`).
    pub joined: f64,
    pub forced: f64,
}

/// Zählung, über die [`classify`] entscheidet: reine Daten, in Tests von Hand gebaut.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Census {
    /// Bevölkerung (Mio.) aller besiedelten Welten.
    pub population: f64,
    /// Höchststand der Partie.
    pub peak: f64,
    /// Monate in Folge unter der Schwelle des Zusammenbruchs, der laufende eingerechnet.
    pub months_below: u32,
    /// Je freie, lebende Gemeinschaft ihr Verbund. Wer untersteht, zählt bei seinem Haupt.
    pub blocs: Vec<BlocCensus>,
}

/// Urteil über eine Zählung.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Verdict {
    pub ending: Ending,
    pub survivor: Option<PolityId>,
    pub joined: f64,
    pub forced: f64,
}

/// Souverän ist ein freier Verbund, der Menschen hat und mindestens den Mindestanteil an
/// der Bevölkerung aller hält.
pub fn is_sovereign(bloc: &BlocCensus, total: f64, levers: &EndgameBalance) -> bool {
    bloc.population > 0.0 && bloc.population >= levers.sovereign_min_share * total
}

/// Ist die Bevölkerung zusammengebrochen: unter dem Anteil `collapse_share` ihres
/// Höchststands, und das seit `collapse_months` Monaten in Folge?
pub fn has_collapsed(census: &Census, levers: &EndgameBalance) -> bool {
    census.months_below >= levers.collapse_months
        && census.population < levers.collapse_share * census.peak
}

/// Entscheidet, ob die Partie zu Ende ist. Rein und für jede Zählung definiert; die erste
/// zutreffende Regel gilt:
///
/// 1. Die Bevölkerung ist zusammengebrochen, oder kein Verbund ist mehr souverän:
///    Auslöschung ohne Überlebende.
/// 2. Genau ein Verbund ist souverän: Er überdauert. Metamorphose, wenn in ihm mehr
///    Menschen beigetreten sind, als genommen wurden; sonst (auch bei Gleichstand)
///    Auslöschung. Freie Gemeinschaften unter der Schwelle zählen nicht.
///
/// Sonst geht die Partie weiter.
pub fn classify(census: &Census, levers: &EndgameBalance) -> Option<Verdict> {
    let mut sovereigns = census
        .blocs
        .iter()
        .filter(|bloc| is_sovereign(bloc, census.population, levers));
    let first = sovereigns.next();
    let Some(only) = first.filter(|_| !has_collapsed(census, levers)) else {
        return Some(Verdict {
            ending: Ending::Extinction,
            survivor: None,
            joined: census.blocs.iter().map(|bloc| bloc.joined).sum(),
            forced: census.blocs.iter().map(|bloc| bloc.forced).sum(),
        });
    };
    if sovereigns.next().is_some() {
        return None;
    }
    Some(Verdict {
        ending: if only.joined > only.forced {
            Ending::Metamorphosis
        } else {
            Ending::Extinction
        },
        survivor: Some(only.head),
        joined: only.joined,
        forced: only.forced,
    })
}

/// Bevölkerung (Mio.) aller Welten lebender Gemeinschaften.
pub fn total_population(galaxy: &Galaxy, polities: &Polities) -> f64 {
    galaxy
        .planets
        .iter()
        .filter(|planet| {
            planet
                .owner
                .and_then(|owner| polities.list.get(owner.index()))
                .is_some_and(|polity| polity.alive)
        })
        // Von null an gezählt: Die Summe über nichts wäre sonst minus null.
        .fold(0.0, |sum, planet| sum + planet.population)
}

/// Zählt die Galaxie: je freie, lebende Gemeinschaft ihr Verbund in ID-Reihenfolge.
/// Höchststand und Monatszähler stammen aus dem Zustand.
pub fn census(
    galaxy: &Galaxy,
    polities: &Polities,
    diplomacy: &Diplomacy,
    endgame: &Endgame,
) -> Census {
    let mut blocs: Vec<BlocCensus> = polities
        .alive()
        .filter(|polity| diplomacy.is_free(polity.id))
        .map(|polity| BlocCensus {
            head: polity.id,
            population: 0.0,
            joined: 0.0,
            forced: 0.0,
        })
        .collect();
    for planet in &galaxy.planets {
        let Some(owner) = planet.owner else {
            continue;
        };
        let head = diplomacy.head(owner);
        // Die Häupter stehen aufsteigend: Eine Welt, deren Haupt nicht lebt, zählt nirgends.
        let Ok(position) = blocs.binary_search_by_key(&head, |bloc| bloc.head) else {
            continue;
        };
        let bloc = &mut blocs[position];
        bloc.population += planet.population;
        let absorbed = endgame
            .absorbed_pop
            .get(planet.id.index())
            .copied()
            .unwrap_or(0.0);
        match endgame.path_of(planet.id) {
            Path::Own => {}
            Path::Joined => bloc.joined += absorbed,
            Path::Forced => bloc.forced += absorbed,
        }
    }
    Census {
        population: total_population(galaxy, polities),
        peak: endgame.peak_population,
        months_below: endgame.months_below,
        blocs,
    }
}

/// Monatliche Prüfung, nach allem anderen: Höchststand und Monatszähler fortschreiben,
/// zählen, entscheiden. Fällt ein Urteil, wird der Ausgang geschrieben, genau einmal und
/// zusammen mit einem Eintrag in der Chronik. Gibt den eben geschriebenen Ausgang zurück.
pub fn judge(
    endgame: &mut Endgame,
    galaxy: &Galaxy,
    polities: &Polities,
    diplomacy: &Diplomacy,
    chronicle: &mut Chronicle,
    levers: &EndgameBalance,
    tick: u64,
) -> Option<Outcome> {
    if endgame.ended() {
        return None;
    }
    let total = total_population(galaxy, polities);
    endgame.peak_population = endgame.peak_population.max(total);
    endgame.months_below = if total < levers.collapse_share * endgame.peak_population {
        endgame.months_below.saturating_add(1)
    } else {
        0
    };
    let census = census(galaxy, polities, diplomacy, endgame);
    let verdict = classify(&census, levers)?;
    let outcome = Outcome {
        ending: verdict.ending,
        survivor: verdict.survivor,
        day: tick / TICKS_PER_DAY,
        joined: verdict.joined,
        forced: verdict.forced,
    };
    endgame.outcome = Some(outcome);
    let sums = format!(
        "Beigetreten sind Welten mit {:.0} Mio. Einwohnern, genommen wurden Welten mit {:.0} Mio.",
        outcome.joined, outcome.forced
    );
    let label = outcome.ending.label();
    let text = match (outcome.ending, outcome.survivor) {
        (Ending::Metamorphosis, Some(survivor)) => format!(
            "{label}: Die Galaxie ist in {} aufgegangen. {sums}",
            polities.get(survivor).name
        ),
        (_, Some(survivor)) => format!(
            "{label}: {} bleibt als einzige souveräne Gemeinschaft zurück. {sums}",
            polities.get(survivor).name
        ),
        (_, None) if has_collapsed(&census, levers) => format!(
            "{label}: Die Bevölkerung der Galaxie ist zusammengebrochen, keine Gemeinschaft \
             überdauert. {sums}"
        ),
        (_, None) => {
            format!("{label}: Keine souveräne Gemeinschaft besteht mehr. {sums}")
        }
    };
    chronicle.push(tick, ChronicleKind::End, outcome.survivor, None, text);
    Some(outcome)
}

/// Das Ende der Partie: die monatliche Prüfung.
pub struct EndgamePlugin;

impl Plugin for EndgamePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            sim_schedule(),
            judge_month.in_set(SimSet::Victory).run_if(month_start),
        );
    }
}

fn judge_month(
    data: Res<GameData>,
    clock: Res<SimClock>,
    galaxy: Res<Galaxy>,
    polities: Res<Polities>,
    diplomacy: Res<Diplomacy>,
    mut endgame: ResMut<Endgame>,
    mut chronicle: ResMut<Chronicle>,
) {
    judge(
        &mut endgame,
        &galaxy,
        &polities,
        &diplomacy,
        &mut chronicle,
        &data.balance.endgame,
        clock.tick,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::{Domain, Rng};

    fn levers() -> EndgameBalance {
        EndgameBalance {
            max_free_states: 6,
            sovereign_min_share: 0.02,
            collapse_share: 0.30,
            collapse_months: 24,
        }
    }

    fn bloc(head: u32, population: f64, joined: f64, forced: f64) -> BlocCensus {
        BlocCensus {
            head: PolityId(head),
            population,
            joined,
            forced,
        }
    }

    /// Zählung in guten Zeiten: Bevölkerung am Höchststand.
    fn thriving(blocs: Vec<BlocCensus>) -> Census {
        let population = blocs.iter().map(|bloc| bloc.population).sum();
        Census {
            population,
            peak: population,
            months_below: 0,
            blocs,
        }
    }

    fn verdict(ending: Ending, survivor: Option<u32>, joined: f64, forced: f64) -> Option<Verdict> {
        Some(Verdict {
            ending,
            survivor: survivor.map(PolityId),
            joined,
            forced,
        })
    }

    #[test]
    fn classify_is_total() {
        let levers = levers();
        use Ending::{Extinction, Metamorphosis};

        // Zwei Souveräne: Die Partie geht weiter, gleich wie ihre Welten zu ihnen kamen.
        let two = thriving(vec![bloc(0, 600.0, 300.0, 0.0), bloc(1, 400.0, 0.0, 200.0)]);
        assert_eq!(classify(&two, &levers), None);

        // Genau einer: Mehr beigetreten als genommen ist die Metamorphose …
        let joined = thriving(vec![bloc(3, 1000.0, 500.0, 300.0)]);
        assert_eq!(
            classify(&joined, &levers),
            verdict(Metamorphosis, Some(3), 500.0, 300.0)
        );
        // … mehr genommen die Auslöschung, mit der Überlebenden benannt …
        let forced = thriving(vec![bloc(3, 1000.0, 300.0, 500.0)]);
        assert_eq!(
            classify(&forced, &levers),
            verdict(Extinction, Some(3), 300.0, 500.0)
        );
        // … und der Gleichstand ebenso: auch der einer Gemeinschaft, zu der nie jemand kam.
        let tie = thriving(vec![bloc(3, 1000.0, 400.0, 400.0)]);
        assert_eq!(
            classify(&tie, &levers),
            verdict(Extinction, Some(3), 400.0, 400.0)
        );
        let alone = thriving(vec![bloc(2, 1000.0, 0.0, 0.0)]);
        assert_eq!(
            classify(&alone, &levers),
            verdict(Extinction, Some(2), 0.0, 0.0)
        );

        // Wer untersteht, zählt bei seinem Haupt: Der Verbund ist ein Eintrag, und die
        // Welten der Unterstellten gehen nach ihrem Weg in seine Summen ein. Hier bringt
        // eine gewollte Unterstellung (400 beigetreten) die Mehrheit gegen 350 genommene.
        let with_wards = thriving(vec![bloc(0, 1500.0, 150.0 + 400.0, 350.0)]);
        assert_eq!(
            classify(&with_wards, &levers),
            verdict(Metamorphosis, Some(0), 550.0, 350.0)
        );
        // Dieselbe Unterstellung erzwungen kehrt das Urteil um.
        let with_forced_wards = thriving(vec![bloc(0, 1500.0, 150.0, 350.0 + 400.0)]);
        assert_eq!(
            classify(&with_forced_wards, &levers),
            verdict(Extinction, Some(0), 150.0, 750.0)
        );

        // Freie Gemeinschaften unter der Schwelle (2 % von 1000) zählen nicht: weder
        // verhindern sie das Ende, noch gehen ihre Summen in das Urteil ein.
        let rumps = thriving(vec![
            bloc(0, 15.0, 90.0, 0.0),
            bloc(1, 970.0, 200.0, 100.0),
            bloc(4, 15.0, 0.0, 70.0),
        ]);
        assert_eq!(
            classify(&rumps, &levers),
            verdict(Metamorphosis, Some(1), 200.0, 100.0)
        );
        // Genau an der Schwelle ist eine Gemeinschaft souverän: Dann sind es zwei.
        let at_floor = thriving(vec![bloc(0, 20.0, 0.0, 0.0), bloc(1, 980.0, 200.0, 100.0)]);
        assert_eq!(classify(&at_floor, &levers), None);

        // Niemand ist souverän (alle unter der Schwelle, oder es gibt niemanden mehr):
        // Auslöschung ohne Überlebende, mit den Summen der ganzen Galaxie.
        let crumbs = Census {
            population: 1000.0,
            peak: 1000.0,
            months_below: 0,
            blocs: vec![bloc(0, 10.0, 5.0, 0.0), bloc(1, 12.0, 0.0, 7.0)],
        };
        assert_eq!(
            classify(&crumbs, &levers),
            verdict(Extinction, None, 5.0, 7.0)
        );
        assert_eq!(
            classify(&Census::default(), &levers),
            verdict(Extinction, None, 0.0, 0.0)
        );
        let deserted = Census {
            population: 0.0,
            peak: 5000.0,
            months_below: 3,
            blocs: vec![bloc(0, 0.0, 0.0, 0.0), bloc(1, 0.0, 0.0, 0.0)],
        };
        assert_eq!(
            classify(&deserted, &levers),
            verdict(Extinction, None, 0.0, 0.0)
        );

        // Zusammenbruch: unter 30 % des Höchststands, 24 Monate in Folge. Er geht jeder
        // anderen Regel vor, auch wenn noch mehrere Souveräne bestehen oder genau einer.
        let mut collapsed = Census {
            population: 290.0,
            peak: 1000.0,
            months_below: 24,
            blocs: vec![bloc(0, 150.0, 40.0, 0.0), bloc(1, 140.0, 0.0, 30.0)],
        };
        assert_eq!(
            classify(&collapsed, &levers),
            verdict(Extinction, None, 40.0, 30.0)
        );
        collapsed.blocs.truncate(1);
        collapsed.blocs[0].population = 290.0;
        assert_eq!(
            classify(&collapsed, &levers),
            verdict(Extinction, None, 40.0, 0.0)
        );
        // Einen Monat zu früh gilt er noch nicht: Dann greift nur die Regel für die eine
        // Souveräne, zu der hier 40 Mio. beigetreten sind und niemand genommen wurde, …
        collapsed.months_below = 23;
        assert_eq!(
            classify(&collapsed, &levers),
            verdict(Metamorphosis, Some(0), 40.0, 0.0)
        );
        // … und eine Bevölkerung an der Schwelle ist nicht darunter.
        let mut shrunk = Census {
            population: 300.0,
            peak: 1000.0,
            months_below: 60,
            blocs: vec![bloc(0, 150.0, 0.0, 0.0), bloc(1, 150.0, 0.0, 0.0)],
        };
        assert_eq!(classify(&shrunk, &levers), None);
        shrunk.population = 299.9;
        assert_eq!(
            classify(&shrunk, &levers),
            verdict(Extinction, None, 0.0, 0.0)
        );
    }

    /// Eine Zahl, wie sie in einer Zählung stehen könnte, auch eine unbrauchbare.
    fn any_amount(rng: &mut Rng) -> f64 {
        match rng.range_usize(0, 12) {
            0 => 0.0,
            1 => f64::NAN,
            2 => f64::INFINITY,
            3 => -rng.f64() * 1000.0,
            4 => f64::MIN_POSITIVE,
            5 => -0.0,
            _ => rng.f64() * 10f64.powi(rng.range_usize(0, 6) as i32),
        }
    }

    #[test]
    fn classify_never_panics_and_gives_one_coherent_verdict() {
        let mut seen = [0usize; 4];
        for case in 0..20_000u64 {
            let mut rng = Rng::stream(77, Domain::Polity, case, 0);
            let levers = EndgameBalance {
                max_free_states: rng.range_usize(0, 10) as u32,
                sovereign_min_share: [0.02, 0.0, 0.5, 1.0, f64::NAN][rng.range_usize(0, 5)],
                collapse_share: [0.3, 0.0, 0.99, f64::NAN][rng.range_usize(0, 4)],
                collapse_months: [24, 0, 1, u32::MAX][rng.range_usize(0, 4)],
            };
            let blocs: Vec<BlocCensus> = (0..rng.range_usize(0, 7))
                .map(|head| BlocCensus {
                    head: PolityId(head as u32),
                    population: any_amount(&mut rng),
                    joined: any_amount(&mut rng),
                    forced: any_amount(&mut rng),
                })
                .collect();
            // Meist eine stimmige Summe, manchmal irgendeine Zahl.
            let population = if rng.chance(0.7) {
                blocs
                    .iter()
                    .map(|bloc| bloc.population)
                    .filter(|population| population.is_finite() && *population > 0.0)
                    .sum()
            } else {
                any_amount(&mut rng)
            };
            let census = Census {
                population,
                peak: if rng.chance(0.5) {
                    population
                } else {
                    any_amount(&mut rng)
                },
                months_below: [0, 1, 23, 24, 500, u32::MAX][rng.range_usize(0, 6)],
                blocs,
            };

            let result = classify(&census, &levers);
            let sovereign: Vec<&BlocCensus> = census
                .blocs
                .iter()
                .filter(|bloc| is_sovereign(bloc, census.population, &levers))
                .collect();
            let collapsed = has_collapsed(&census, &levers);
            match result {
                None => {
                    seen[0] += 1;
                    assert!(sovereign.len() >= 2 && !collapsed, "{census:?}");
                }
                Some(found) => match found.survivor {
                    None => {
                        seen[1] += 1;
                        assert!(sovereign.is_empty() || collapsed, "{census:?}");
                        assert_eq!(found.ending, Ending::Extinction);
                    }
                    Some(survivor) => {
                        assert!(!collapsed, "{census:?}");
                        assert_eq!(sovereign.len(), 1, "{census:?}");
                        let only = sovereign[0];
                        assert_eq!(only.head, survivor);
                        let wanted = if only.joined > only.forced {
                            seen[2] += 1;
                            Ending::Metamorphosis
                        } else {
                            seen[3] += 1;
                            Ending::Extinction
                        };
                        assert_eq!(found.ending, wanted, "{census:?}");
                    }
                },
            }

            // Die Reihenfolge der Verbünde ändert das Urteil nicht. (Verglichen wird ohne
            // die Summen: Sie hängen bei unbrauchbaren Zahlen an der Reihenfolge.)
            let key = |verdict: Option<Verdict>| verdict.map(|v| (v.ending, v.survivor));
            let mut shuffled = census.clone();
            rng.shuffle(&mut shuffled.blocs);
            assert_eq!(key(classify(&shuffled, &levers)), key(result), "{census:?}");
            // Eine freie Gemeinschaft unter der Schwelle mehr oder weniger ebenso wenig,
            // solange die Bevölkerung aller dieselbe bleibt.
            let mut with_rump = census.clone();
            with_rump.blocs.push(BlocCensus {
                head: PolityId(99),
                population: 0.0,
                joined: 0.0,
                forced: 0.0,
            });
            assert_eq!(
                key(classify(&with_rump, &levers)),
                key(result),
                "{census:?}"
            );
        }
        // Die erzeugten Zählungen decken jeden Ausgang ab.
        assert!(seen.iter().all(|count| *count > 50), "{seen:?}");
    }

    #[test]
    fn tags_record_how_a_world_came_and_clear_for_own_worlds() {
        let mut endgame = Endgame::new(4);
        assert!(endgame.path.iter().all(|path| *path == Path::Own));
        assert!(!endgame.ended());
        let planet = PlanetId(2);
        endgame.tag(planet, Path::Forced, 250.0, 0.4);
        assert_eq!(endgame.path_of(planet), Path::Forced);
        assert_eq!((endgame.absorbed_pop[2], endgame.gap[2]), (250.0, 0.4));
        endgame.seat[2] = true;
        // Der Weg ändert nichts am Sitz; ein Abstand außerhalb der Grenzen wird gestutzt.
        endgame.tag(planet, Path::Joined, 300.0, 1.7);
        assert_eq!((endgame.absorbed_pop[2], endgame.gap[2]), (300.0, 1.0));
        assert!(endgame.is_seat(planet));
        // Eine eigene Welt trägt weder Bevölkerung noch Abstand.
        endgame.tag(planet, Path::Own, 300.0, 0.3);
        assert_eq!(endgame.path_of(planet), Path::Own);
        assert_eq!((endgame.absorbed_pop[2], endgame.gap[2]), (0.0, 0.0));
        endgame.tag(planet, Path::Forced, 80.0, 0.2);
        endgame.clear_planet(planet);
        assert_eq!(endgame.path_of(planet), Path::Own);
        assert_eq!(endgame.absorbed_pop[2], 0.0);
        assert!(!endgame.is_seat(planet));
        // Die Nachbarn bleiben unberührt.
        assert_eq!(endgame.absorbed_pop, vec![0.0; 4]);
        assert_eq!(Ending::Metamorphosis.label(), "Metamorphose");
        assert_eq!(Ending::Extinction.label(), "Auslöschung");
    }
}
