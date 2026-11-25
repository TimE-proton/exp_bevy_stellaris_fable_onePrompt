//! Beziehungen zwischen den Gemeinschaften.
//!
//! Stand Meilenstein 2: nur Krieg und Frieden. Verträge, Vertrauen und verborgene
//! Dispositionen folgen in Meilenstein 6.

use crate::chronicle::{Chronicle, ChronicleKind};
use crate::ids::PolityId;
use crate::polity::Polities;
use bevy_app::prelude::*;
use bevy_ecs::prelude::Resource;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Resource, Clone, Debug, Default, Serialize, Deserialize)]
pub struct Diplomacy {
    /// Kriege als geordnete Paare (kleinere ID zuerst).
    pub wars: BTreeSet<(PolityId, PolityId)>,
}

fn pair(a: PolityId, b: PolityId) -> (PolityId, PolityId) {
    if a <= b { (a, b) } else { (b, a) }
}

impl Diplomacy {
    pub fn at_war(&self, a: PolityId, b: PolityId) -> bool {
        a != b && self.wars.contains(&pair(a, b))
    }

    pub fn enemies_of(&self, polity: PolityId) -> impl Iterator<Item = PolityId> + '_ {
        self.wars.iter().filter_map(move |(a, b)| {
            if *a == polity {
                Some(*b)
            } else if *b == polity {
                Some(*a)
            } else {
                None
            }
        })
    }

    pub fn is_at_war(&self, polity: PolityId) -> bool {
        self.enemies_of(polity).next().is_some()
    }
}

pub fn declare_war(
    diplomacy: &mut Diplomacy,
    polities: &Polities,
    chronicle: &mut Chronicle,
    tick: u64,
    aggressor: PolityId,
    target: PolityId,
) -> Result<(), String> {
    check_pair(polities, aggressor, target)?;
    if !diplomacy.wars.insert(pair(aggressor, target)) {
        return Err("es herrscht bereits Krieg".into());
    }
    chronicle.push(
        tick,
        ChronicleKind::War,
        Some(aggressor),
        None,
        format!(
            "{} erklärt {} den Krieg.",
            polities.get(aggressor).name,
            polities.get(target).name
        ),
    );
    Ok(())
}

pub fn make_peace(
    diplomacy: &mut Diplomacy,
    polities: &Polities,
    chronicle: &mut Chronicle,
    tick: u64,
    a: PolityId,
    b: PolityId,
) -> Result<(), String> {
    check_pair(polities, a, b)?;
    if !diplomacy.wars.remove(&pair(a, b)) {
        return Err("es herrscht kein Krieg".into());
    }
    chronicle.push(
        tick,
        ChronicleKind::Diplomacy,
        Some(a),
        None,
        format!(
            "{} und {} schließen Frieden.",
            polities.get(a).name,
            polities.get(b).name
        ),
    );
    Ok(())
}

fn check_pair(polities: &Polities, a: PolityId, b: PolityId) -> Result<(), String> {
    if a == b {
        return Err("eine Gemeinschaft kann sich nicht selbst meinen".into());
    }
    for id in [a, b] {
        if !polities
            .list
            .get(id.index())
            .is_some_and(|polity| polity.alive)
        {
            return Err("Gemeinschaft besteht nicht mehr".into());
        }
    }
    Ok(())
}

/// Diplomatie. Greift bisher nur über Befehle ein.
pub struct DiplomacyPlugin;

impl Plugin for DiplomacyPlugin {
    fn build(&self, _app: &mut App) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn war_is_symmetric() {
        let mut diplomacy = Diplomacy::default();
        diplomacy.wars.insert(pair(PolityId(3), PolityId(1)));
        assert!(diplomacy.at_war(PolityId(1), PolityId(3)));
        assert!(diplomacy.at_war(PolityId(3), PolityId(1)));
        assert!(!diplomacy.at_war(PolityId(1), PolityId(2)));
        assert!(!diplomacy.at_war(PolityId(1), PolityId(1)));
        assert_eq!(
            diplomacy.enemies_of(PolityId(3)).collect::<Vec<_>>(),
            vec![PolityId(1)]
        );
    }
}
