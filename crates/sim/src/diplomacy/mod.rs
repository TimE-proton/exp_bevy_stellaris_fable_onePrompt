//! Beziehungen zwischen den Gemeinschaften.
//!
//! Stand Meilenstein 2: nur Krieg und Frieden. Verträge, Vertrauen und verborgene
//! Dispositionen folgen in Meilenstein 6.
//!
//! Nach einem Friedensschluss gilt zwischen den beiden eine Waffenruhe
//! (`military.truce_days`): So lange nimmt keiner dem anderen gegenüber eine neue
//! Kriegserklärung vor. Frieden und sofort wieder Krieg wäre sonst ein Mittel, ein
//! laufendes Gefecht von vorn zu beginnen oder eine Sammelfrist zu umgehen.

use crate::chronicle::{Chronicle, ChronicleKind};
use crate::clock::TICKS_PER_DAY;
use crate::ids::PolityId;
use crate::polity::Polities;
use bevy_app::prelude::*;
use bevy_ecs::prelude::Resource;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Resource, Clone, Debug, Default, Serialize, Deserialize)]
pub struct Diplomacy {
    /// Kriege als geordnete Paare (kleinere ID zuerst).
    pub wars: BTreeSet<(PolityId, PolityId)>,
    /// Waffenruhen nach einem Friedensschluss: je Paar (kleinere ID zuerst) der erste
    /// Tag, an dem wieder ein Krieg erklärt werden kann.
    #[serde(default)]
    pub truces: BTreeMap<(PolityId, PolityId), u64>,
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

    /// Erster Tag, an dem zwischen den beiden wieder ein Krieg erklärt werden kann, wenn
    /// nach einem Friedensschluss noch eine Waffenruhe gilt.
    pub fn truce_until(&self, a: PolityId, b: PolityId, day: u64) -> Option<u64> {
        self.truces
            .get(&pair(a, b))
            .copied()
            .filter(|until| *until > day)
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
    let day = tick / TICKS_PER_DAY;
    diplomacy.truces.retain(|_, until| *until > day);
    if diplomacy.at_war(aggressor, target) {
        return Err("es herrscht bereits Krieg".into());
    }
    if let Some(until) = diplomacy.truce_until(aggressor, target, day) {
        return Err(format!(
            "nach dem Friedensschluss gilt noch {} Tage Waffenruhe",
            until - day
        ));
    }
    diplomacy.wars.insert(pair(aggressor, target));
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

/// Schließt Frieden. Danach gilt `truce_days` Tage lang Waffenruhe zwischen den beiden.
pub fn make_peace(
    diplomacy: &mut Diplomacy,
    polities: &Polities,
    chronicle: &mut Chronicle,
    tick: u64,
    a: PolityId,
    b: PolityId,
    truce_days: u64,
) -> Result<(), String> {
    check_pair(polities, a, b)?;
    if !diplomacy.wars.remove(&pair(a, b)) {
        return Err("es herrscht kein Krieg".into());
    }
    let day = tick / TICKS_PER_DAY;
    diplomacy.truces.retain(|_, until| *until > day);
    if truce_days > 0 {
        diplomacy.truces.insert(pair(a, b), day + truce_days);
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
