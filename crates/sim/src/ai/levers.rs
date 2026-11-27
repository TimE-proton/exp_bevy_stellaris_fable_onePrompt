//! Die Hebel der KI: die einzige Stelle, an der sie Flotten-, Werft-, Forschungs- und
//! Belagerungsbefehle erteilt.
//!
//! Jeder Hebel vergleicht den gewünschten mit dem bestehenden Zustand und stellt nur dann
//! einen Befehl ein, wenn sich etwas ändern soll und der Befehl nach den Regeln angenommen
//! würde. Er gibt zurück, ob er einen Befehl erteilt hat. Die Belagerung
//! ([`super::siege`]) steuert über dieselben Hebel; spätere Planer (Kriegsziele,
//! Diplomatie) tun es ebenso, statt eigene Befehle zu erteilen.

use crate::battle::{Doctrine, Formation};
use crate::command::{CommandQueue, CommandSource, SimCommand};
use crate::data::{GameData, ShipClass};
use crate::diplomacy::Diplomacy;
use crate::economy::Economy;
use crate::fleet::{Battles, BuildOrder, Fleet, Fleets, Stance, supply_base, yard_slots};
use crate::galaxy::Galaxy;
use crate::ids::{PlanetId, PolityId, ShipId, SystemId};
use crate::polity::Polities;
use crate::siege::{DefensePlan, PlanetDefense, Posture, SiegeOrders, Sieges};
use crate::tech::PolityResearch;

/// Die Hand einer Gemeinschaft an den Hebeln: Sie erteilt Befehle in deren Namen.
pub struct Hand<'a> {
    queue: &'a mut CommandQueue,
    polity: PolityId,
}

/// Ziel eines Marschbefehls, den eine Flotte gerade ausführt; sonst das System, in dem
/// sie liegt.
pub fn destination(fleet: &Fleet) -> SystemId {
    fleet.route.last().copied().unwrap_or(fleet.system)
}

/// Hat die Gemeinschaft in diesem System eine Werft (für Instandsetzung und Stilllegung)?
pub fn has_yard(
    data: &GameData,
    galaxy: &Galaxy,
    economy: &Economy,
    polities: &Polities,
    fleets: &Fleets,
    polity: PolityId,
    system: SystemId,
) -> bool {
    galaxy.planets_in(system).any(|planet| {
        planet.owner == Some(polity)
            && yard_slots(data, galaxy, economy, polities, fleets, planet.id) > 0
    })
}

/// Eigene Werften, die heute einen freien Bauplatz haben und arbeiten können: im eigenen
/// Gebiet ([`supply_base`]), nicht blockiert, ohne feindliche Flotte im System. Je Welt
/// die Zahl der freien Plätze, aufsteigend nach Welt.
pub fn free_yards(
    data: &GameData,
    galaxy: &Galaxy,
    economy: &Economy,
    polities: &Polities,
    fleets: &Fleets,
    diplomacy: &Diplomacy,
    polity: PolityId,
) -> Vec<(PlanetId, u32)> {
    let limit = data.balance.military.yard_queue;
    galaxy
        .planets_of(polity)
        .filter(|planet| {
            supply_base(galaxy, diplomacy, polity, planet.system)
                && !fleets.is_blockaded(planet.system)
                && !fleets
                    .docked_in(planet.system)
                    .any(|fleet| diplomacy.at_war(fleet.owner, polity))
        })
        .filter_map(|planet| {
            let slots = yard_slots(data, galaxy, economy, polities, fleets, planet.id);
            let queued = fleets.orders_at(planet.id).count() as u32;
            let free = slots.min(limit).saturating_sub(queued);
            (free > 0).then_some((planet.id, free))
        })
        .collect()
}

impl<'a> Hand<'a> {
    pub fn new(queue: &'a mut CommandQueue, polity: PolityId) -> Self {
        Self { queue, polity }
    }

    fn issue(&mut self, command: SimCommand) -> bool {
        self.queue.push(self.polity, CommandSource::Ai, command);
        true
    }

    /// Schickt eine Flotte in ein System, wenn sie nicht schon dort liegt oder dorthin
    /// unterwegs ist. Wer kämpft oder sich sammelt, nimmt keinen Marschbefehl an.
    pub fn move_fleet(&mut self, fleet: &Fleet, to: SystemId) -> bool {
        if !fleet.is_ready() || destination(fleet) == to {
            return false;
        }
        self.issue(SimCommand::MoveFleet {
            fleet: fleet.id,
            to,
        })
    }

    /// Gibt einer Flotte ihren Auftrag (Wache, Blockade, Kaperfahrt, Geleitschutz).
    pub fn set_stance(&mut self, fleet: &Fleet, stance: Stance) -> bool {
        if fleet.stance == stance {
            return false;
        }
        self.issue(SimCommand::SetStance {
            fleet: fleet.id,
            stance,
        })
    }

    /// Stehende Befehle einer Flotte für Gefechte.
    pub fn set_orders(&mut self, fleet: &Fleet, formation: Formation, doctrine: Doctrine) -> bool {
        if fleet.formation == formation && fleet.doctrine == doctrine {
            return false;
        }
        self.issue(SimCommand::SetFleetOrders {
            fleet: fleet.id,
            formation,
            doctrine,
        })
    }

    /// Vereinigt zwei eigene Flotten, die einsatzbereit und ohne Marschbefehl im selben
    /// System liegen.
    pub fn merge(&mut self, from: &Fleet, into: &Fleet) -> bool {
        let fits = |fleet: &Fleet| fleet.is_ready() && fleet.route.is_empty();
        if from.id == into.id || !fits(from) || !fits(into) || from.system != into.system {
            return false;
        }
        self.issue(SimCommand::MergeFleets {
            from: from.id,
            into: into.id,
        })
    }

    /// Befiehlt einer Flotte den Rückzug aus dem Gefecht, in dem sie steht.
    pub fn retreat(&mut self, fleet: &Fleet, battles: &Battles) -> bool {
        let Some((battle, side)) = battles.of_fleet(fleet.id) else {
            return false;
        };
        if !fleet.in_battle || battle.combatants[side].retreating || battle.finished {
            return false;
        }
        self.issue(SimCommand::Retreat { fleet: fleet.id })
    }

    /// Bestellt bei der Werft einer Welt ein Schiff der Klasse; den Entwurf wählt die
    /// Werft. Der Aufrufer nimmt die Welt aus [`free_yards`] und prüft, dass die
    /// Gemeinschaft einen Entwurf der Klasse beherrscht.
    pub fn build_class(&mut self, planet: PlanetId, class: ShipClass) -> bool {
        self.issue(SimCommand::BuildClass { planet, class })
    }

    /// Streicht einen eigenen Bauauftrag.
    pub fn cancel_build(&mut self, order: &BuildOrder) -> bool {
        if order.polity != self.polity {
            return false;
        }
        self.issue(SimCommand::CancelBuild { order: order.id })
    }

    /// Legt Schiffe einer Flotte still. Sie muss ohne Marschbefehl und außerhalb eines
    /// Gefechts in einem System mit eigener Werft liegen (`at_yard`, siehe [`has_yard`]).
    pub fn lay_up(&mut self, fleet: &Fleet, ships: Vec<ShipId>, at_yard: bool) -> bool {
        if ships.is_empty()
            || !at_yard
            || fleet.in_battle
            || !fleet.route.is_empty()
            || ships.iter().any(|ship| fleet.ship(*ship).is_none())
        {
            return false;
        }
        self.issue(SimCommand::LayUpShips {
            fleet: fleet.id,
            ships,
        })
    }

    /// Lässt eine Flotte einen Planeten in ihrem System belagern.
    pub fn begin_siege(&mut self, sieges: &Sieges, fleet: &Fleet, planet: PlanetId) -> bool {
        if sieges.assign.get(&fleet.id) == Some(&planet) || !fleet.is_docked() {
            return false;
        }
        self.issue(SimCommand::BeginSiege {
            fleet: fleet.id,
            planet,
        })
    }

    /// Stehende Befehle einer eigenen Belagerung.
    pub fn siege_orders(&mut self, sieges: &Sieges, planet: PlanetId, orders: SiegeOrders) -> bool {
        let Some(siege) = sieges.siege(planet) else {
            return false;
        };
        if siege.attacker != self.polity
            || siege.orders == orders
            || orders.focus as usize >= siege.zones.len()
        {
            return false;
        }
        self.issue(SimCommand::SetSiegeOrders { planet, orders })
    }

    /// Hebt eine eigene Belagerung auf.
    pub fn lift_siege(&mut self, planet: PlanetId) -> bool {
        self.issue(SimCommand::LiftSiege { planet })
    }

    /// Schifft Reguläre einer eigenen Welt auf eine Flotte in deren System ein.
    pub fn embark(&mut self, fleet: &Fleet, planet: PlanetId, strength: f64, days: u16) -> bool {
        if strength.is_nan() || strength <= 0.0 || fleet.in_battle || !fleet.is_docked() {
            return false;
        }
        self.issue(SimCommand::EmbarkTroops {
            fleet: fleet.id,
            planet,
            strength,
            supply_days: days,
        })
    }

    /// Setzt eingeschiffte Verbände einer Flotte an Land.
    pub fn land(&mut self, fleet: &Fleet, planet: PlanetId, strength: f64) -> bool {
        if strength.is_nan() || strength <= 0.0 || !fleet.is_docked() {
            return false;
        }
        self.issue(SimCommand::LandTroops {
            fleet: fleet.id,
            planet,
            strength,
        })
    }

    /// Haltung der Garnison einer eigenen Welt und ob ihre Geschütze schweigen.
    pub fn garrison_orders(
        &mut self,
        defense: &PlanetDefense,
        planet: PlanetId,
        posture: Posture,
        hold_fire: bool,
    ) -> bool {
        if defense.posture == posture && defense.hold_fire == hold_fire {
            return false;
        }
        self.issue(SimCommand::SetGarrisonOrders {
            planet,
            posture,
            hold_fire,
        })
    }

    /// Übergibt eine belagerte eigene Welt.
    pub fn capitulate(&mut self, planet: PlanetId) -> bool {
        self.issue(SimCommand::Capitulate { planet })
    }

    /// Setzt das Ausbauziel der Verteidigung einer eigenen Welt.
    pub fn defense_plan(
        &mut self,
        defense: &PlanetDefense,
        planet: PlanetId,
        plan: DefensePlan,
    ) -> bool {
        if defense.plan == plan {
            return false;
        }
        self.issue(SimCommand::SetDefensePlan { planet, plan })
    }

    /// Setzt den Anteil der Staatseinnahmen, der in die Forschung geht.
    pub fn research_share(&mut self, entry: &PolityResearch, share: f64, max: f64) -> bool {
        let share = share.clamp(0.0, max);
        if (share - entry.budget_share).abs() <= 1e-9 {
            return false;
        }
        self.issue(SimCommand::SetResearchBudget { share })
    }

    /// Setzt das Forschungsziel; `None` gibt die Wahl an die Automatik zurück.
    pub fn research_target(&mut self, entry: &PolityResearch, tech: Option<u16>) -> bool {
        let unchanged = match tech {
            Some(tech) => !entry.auto && entry.target == Some(tech),
            None => entry.auto,
        };
        if unchanged {
            return false;
        }
        self.issue(SimCommand::SetResearchTarget { tech })
    }
}
