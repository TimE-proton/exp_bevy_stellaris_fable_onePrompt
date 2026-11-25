//! Übungsgefechte außerhalb einer Partie: für Tests, zum Auswuchten der Schiffsdaten und
//! als Leistungsmessung. Derselbe Rechner wie in der Partie, nur ohne Welt darum.

use super::{Battle, CombatModifiers, CombatantSetup, Doctrine, Formation, ShipSpec};
use crate::data::GameData;
use crate::ids::{BattleId, PolityId, SystemId};

/// Eine Partei eines Übungsgefechts.
#[derive(Clone, Debug)]
pub struct Side {
    /// Entwurf und Anzahl.
    pub ships: Vec<(u16, u32)>,
    pub formation: Formation,
    pub doctrine: Doctrine,
    pub modifiers: CombatModifiers,
    /// Vorräte an Bord als Anteil (1 = voll).
    pub ammo: f32,
    pub fuel: f32,
    pub morale: Option<f32>,
}

impl Side {
    pub fn new(ships: Vec<(u16, u32)>) -> Self {
        Self {
            ships,
            formation: Formation::default(),
            doctrine: Doctrine::default(),
            modifiers: CombatModifiers::default(),
            ammo: 1.0,
            fuel: 1.0,
            morale: None,
        }
    }

    /// Liest eine Liste wie `speer:4,waechter:6`.
    pub fn parse(data: &GameData, text: &str) -> Result<Self, String> {
        let mut ships = Vec::new();
        for part in text
            .split(',')
            .map(str::trim)
            .filter(|part| !part.is_empty())
        {
            let (key, count) = match part.split_once(':') {
                Some((key, count)) => {
                    let count: u32 = count
                        .trim()
                        .parse()
                        .map_err(|_| format!("„{count}“ ist keine Anzahl (in „{part}“)"))?;
                    (key.trim(), count)
                }
                None => (part, 1),
            };
            let design = data.design_index(key).ok_or_else(|| {
                let known: Vec<&str> = data
                    .designs
                    .iter()
                    .map(|design| design.key.as_str())
                    .collect();
                format!(
                    "unbekannter Entwurf „{key}“; bekannt sind: {}",
                    known.join(", ")
                )
            })?;
            ships.push((design, count));
        }
        if ships.is_empty() {
            return Err("eine Partei braucht mindestens ein Schiff".into());
        }
        Ok(Self::new(ships))
    }

    /// So viele Schiffe eines Entwurfs, wie das Budget hergibt (mindestens eines).
    pub fn for_budget(data: &GameData, design: u16, budget: f64) -> Self {
        let value = data.designs[design as usize].stats.value.max(1.0);
        Self::new(vec![(design, ((budget / value).round() as u32).max(1))])
    }

    pub fn with_formation(mut self, formation: Formation) -> Self {
        self.formation = formation;
        self
    }

    pub fn with_doctrine(mut self, doctrine: Doctrine) -> Self {
        self.doctrine = doctrine;
        self
    }

    pub fn with_modifiers(mut self, modifiers: CombatModifiers) -> Self {
        self.modifiers = modifiers;
        self
    }

    pub fn with_supplies(mut self, ammo: f32, fuel: f32) -> Self {
        self.ammo = ammo;
        self.fuel = fuel;
        self
    }

    /// Bauwert der Partei.
    pub fn value(&self, data: &GameData) -> f64 {
        self.ships
            .iter()
            .map(|(design, count)| data.designs[*design as usize].stats.value * *count as f64)
            .sum()
    }

    pub fn ship_count(&self) -> u32 {
        self.ships.iter().map(|(_, count)| *count).sum()
    }

    fn specs(&self, data: &GameData) -> Vec<ShipSpec> {
        let mut specs = Vec::new();
        for (design, count) in &self.ships {
            for _ in 0..*count {
                let mut spec = ShipSpec::fresh(data, *design);
                spec.ammo = self.ammo;
                spec.fuel = self.fuel;
                spec.morale = self.morale;
                specs.push(spec);
            }
        }
        specs
    }
}

/// Bilanz einer Partei nach dem Gefecht.
#[derive(Clone, Debug, Default)]
pub struct SideOutcome {
    pub initial: [u32; 6],
    pub lost: [u32; 6],
    pub escaped: [u32; 6],
    /// Noch kämpfend auf dem Feld.
    pub holding: [u32; 6],
    pub initial_value: f32,
    pub lost_value: f32,
    /// Hält am Ende das Feld.
    pub holds_field: bool,
    pub retreated: bool,
}

impl SideOutcome {
    /// Anteil des Bauwerts, der verloren ging.
    pub fn loss_share(&self) -> f32 {
        if self.initial_value > 0.0 {
            self.lost_value / self.initial_value
        } else {
            0.0
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Outcome {
    pub ticks: u64,
    /// Gefechtszeit in Sekunden.
    pub time: f32,
    pub timed_out: bool,
    pub sides: Vec<SideOutcome>,
    /// Höchststände, für die Leistungsmessung.
    pub peak_ships: usize,
    pub peak_shots: usize,
}

impl Outcome {
    /// Sieger ist, wer als Einziger das Feld hält. Halten es mehrere oder niemand, gibt es
    /// keinen.
    pub fn winner(&self) -> Option<usize> {
        let mut holders = self
            .sides
            .iter()
            .enumerate()
            .filter(|(_, side)| side.holds_field);
        let first = holders.next()?;
        holders.next().is_none().then_some(first.0)
    }

    /// Wer das Gefecht nach Punkten für sich entschieden hat: der Sieger auf dem Feld,
    /// sonst die Seite mit dem deutlich kleineren Verlustanteil.
    pub fn better_side(&self) -> Option<usize> {
        if let Some(winner) = self.winner() {
            return Some(winner);
        }
        let (a, b) = (self.sides.first()?, self.sides.get(1)?);
        let (loss_a, loss_b) = (a.loss_share(), b.loss_share());
        if (loss_a - loss_b).abs() < 0.1 {
            None
        } else if loss_a < loss_b {
            Some(0)
        } else {
            Some(1)
        }
    }
}

/// Stellt ein Gefecht auf: Alle Parteien sind einander feind.
pub fn build(data: &GameData, seed: u64, sides: &[Side]) -> Battle {
    let mut battle = Battle::new(BattleId(0), SystemId(0), seed);
    for (index, side) in sides.iter().enumerate() {
        let mut setup = CombatantSetup::new(PolityId(index as u32), data);
        setup.formation = side.formation;
        setup.doctrine = side.doctrine;
        setup.modifiers = side.modifiers;
        if let Some(morale) = side.morale {
            setup.morale = morale;
        }
        battle.add_combatant(data, setup, &side.specs(data));
    }
    for a in 0..sides.len() {
        for b in (a + 1)..sides.len() {
            battle.set_hostile(a, b, true);
        }
    }
    battle
}

/// Bilanz eines (laufenden oder beendeten) Gefechts.
pub fn outcome_of(battle: &Battle) -> Outcome {
    let sides = battle
        .combatants
        .iter()
        .enumerate()
        .map(|(index, combatant)| {
            let holding = battle.active_by_class(index);
            SideOutcome {
                initial: combatant.initial,
                lost: combatant.lost,
                escaped: combatant.escaped,
                holding,
                initial_value: combatant.initial_value,
                lost_value: combatant.lost_value,
                holds_field: battle.is_fighting(index),
                retreated: combatant.retreating,
            }
        })
        .collect();
    Outcome {
        ticks: battle.tick,
        time: battle.time,
        timed_out: battle.timed_out,
        sides,
        peak_ships: battle.ships.len(),
        peak_shots: 0,
    }
}

/// Rechnet ein Gefecht bis zum Ende.
pub fn fight(data: &GameData, seed: u64, sides: &[Side]) -> Outcome {
    let mut battle = build(data, seed, sides);
    let mut peak_shots = 0;
    // Die Zeitgrenze beendet jedes Gefecht; die Schranke hier schützt nur vor Datenfehlern.
    let limit = (data.balance.battle.time_limit / data.balance.battle.dt) as u64 + 100;
    while !battle.finished && battle.tick < limit {
        battle.step(data);
        peak_shots = peak_shots.max(battle.shots.len());
    }
    let mut outcome = outcome_of(&battle);
    outcome.peak_shots = peak_shots;
    outcome
}
