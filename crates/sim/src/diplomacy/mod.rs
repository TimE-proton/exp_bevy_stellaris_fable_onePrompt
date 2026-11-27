//! Beziehungen zwischen den Gemeinschaften.
//!
//! Krieg und Frieden, wer wem untersteht, und wie nah sich zwei Gemeinschaften kulturell
//! sind. Verträge, Vertrauen und verborgene Dispositionen folgen in Meilenstein 6.
//!
//! Begriffe: Eine Gemeinschaft ist *frei*, solange sie kein Oberhaupt über sich hat. Ihr
//! *Haupt* ist das Oberhaupt, sonst sie selbst. Ein *Verbund* ist ein Haupt mit allen, die
//! ihm unterstehen. Kriege bestehen nur zwischen Häuptern; wer untersteht, teilt die
//! Kriege seines Haupts, und im selben Verbund führt niemand Krieg gegeneinander.
//!
//! Nach einem Friedensschluss gilt zwischen den beiden eine Waffenruhe
//! (`military.truce_days`): So lange nimmt keiner dem anderen gegenüber eine neue
//! Kriegserklärung vor. Frieden und sofort wieder Krieg wäre sonst ein Mittel, ein
//! laufendes Gefecht von vorn zu beginnen oder eine Sammelfrist zu umgehen.

use crate::chronicle::{Chronicle, ChronicleKind};
use crate::clock::TICKS_PER_DAY;
use crate::data::GameData;
use crate::ids::PolityId;
use crate::polity::Polities;
use bevy_app::prelude::*;
use bevy_ecs::prelude::Resource;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Stellung einer Gemeinschaft: frei oder einem Oberhaupt unterstellt.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Standing {
    /// Oberhaupt; `None` = frei.
    pub overlord: Option<PolityId>,
    /// Die Unterstellung war gewollt, nicht erzwungen.
    pub voluntary: bool,
    /// Tag, seit dem die Stellung gilt.
    pub since_day: u64,
}

/// Kultur ohne Vorlage: die Mitte in allen drei Zügen.
pub const NEUTRAL_CULTURE: [f32; 3] = [0.5; 3];

#[derive(Resource, Clone, Debug, Default, Serialize, Deserialize)]
pub struct Diplomacy {
    /// Kriege als geordnete Paare (kleinere ID zuerst), nur zwischen Häuptern.
    pub wars: BTreeSet<(PolityId, PolityId)>,
    /// Waffenruhen nach einem Friedensschluss: je Paar (kleinere ID zuerst) der erste
    /// Tag, an dem wieder ein Krieg erklärt werden kann.
    #[serde(default)]
    pub truces: BTreeMap<(PolityId, PolityId), u64>,
    /// Stellung je Gemeinschaft, in der Reihenfolge von `Polities::list`. Fehlt ein
    /// Eintrag, gilt die Gemeinschaft als frei.
    #[serde(default)]
    pub standing: Vec<Standing>,
    /// Gelebte Kultur je Gemeinschaft: Offenheit, Gemeinsinn, Wehrhaftigkeit, je 0..1.
    #[serde(default)]
    pub culture: Vec<[f32; 3]>,
}

fn pair(a: PolityId, b: PolityId) -> (PolityId, PolityId) {
    if a <= b { (a, b) } else { (b, a) }
}

impl Diplomacy {
    /// Oberhaupt einer Gemeinschaft; `None`, wenn sie frei ist.
    pub fn overlord_of(&self, polity: PolityId) -> Option<PolityId> {
        self.standing
            .get(polity.index())
            .and_then(|standing| standing.overlord)
    }

    /// Haupt einer Gemeinschaft: ihr Oberhaupt, sonst sie selbst.
    pub fn head(&self, polity: PolityId) -> PolityId {
        self.overlord_of(polity).unwrap_or(polity)
    }

    /// Frei ist, wer niemandem untersteht.
    pub fn is_free(&self, polity: PolityId) -> bool {
        self.overlord_of(polity).is_none()
    }

    /// Gemeinschaften, die `polity` unterstehen, aufsteigend.
    pub fn wards_of(&self, polity: PolityId) -> impl Iterator<Item = PolityId> + '_ {
        self.standing
            .iter()
            .enumerate()
            .filter(move |(_, standing)| standing.overlord == Some(polity))
            .map(|(index, _)| PolityId::from_index(index))
    }

    /// Verbund einer Gemeinschaft: ihr Haupt zuerst, dann alle, die ihm unterstehen,
    /// aufsteigend.
    pub fn bloc(&self, polity: PolityId) -> Vec<PolityId> {
        let head = self.head(polity);
        std::iter::once(head).chain(self.wards_of(head)).collect()
    }

    /// Kultur einer Gemeinschaft; ohne Eintrag die neutrale Mitte.
    pub fn culture_of(&self, polity: PolityId) -> [f32; 3] {
        self.culture
            .get(polity.index())
            .copied()
            .unwrap_or(NEUTRAL_CULTURE)
    }

    /// Kulturelle Nähe zweier Gemeinschaften, 0..1: eins minus dem Abstand ihrer Kulturen,
    /// gemessen am größten möglichen. 1 = gleich, 0 = entgegengesetzt in allen drei Zügen.
    pub fn affinity(&self, a: PolityId, b: PolityId) -> f32 {
        let (first, second) = (self.culture_of(a), self.culture_of(b));
        let distance = first
            .iter()
            .zip(&second)
            .map(|(x, y)| (x - y) * (x - y))
            .sum::<f32>()
            .sqrt();
        (1.0 - distance / 3.0f32.sqrt()).clamp(0.0, 1.0)
    }

    /// Herrscht zwischen den Verbünden der beiden Krieg? Wer untersteht, teilt die Kriege
    /// seines Haupts; im selben Verbund herrscht nie Krieg.
    pub fn at_war(&self, a: PolityId, b: PolityId) -> bool {
        let (a, b) = (self.head(a), self.head(b));
        a != b && self.wars.contains(&pair(a, b))
    }

    /// Alle Gemeinschaften, mit denen `polity` im Krieg liegt: die verfeindeten Häupter,
    /// jedes gefolgt von denen, die ihm unterstehen.
    pub fn enemies_of(&self, polity: PolityId) -> impl Iterator<Item = PolityId> + '_ {
        let own = self.head(polity);
        self.wars
            .iter()
            .filter_map(move |(a, b)| {
                if *a == own {
                    Some(*b)
                } else if *b == own {
                    Some(*a)
                } else {
                    None
                }
            })
            .flat_map(move |enemy| std::iter::once(enemy).chain(self.wards_of(enemy)))
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

/// Beziehungen zu Spielbeginn: Alle sind frei, niemand führt Krieg, und jede Gemeinschaft
/// lebt die Kultur ihrer Vorlage aus `polities.ron`.
pub fn setup_diplomacy(data: &GameData, polities: &Polities) -> Diplomacy {
    Diplomacy {
        wars: BTreeSet::new(),
        truces: BTreeMap::new(),
        standing: vec![Standing::default(); polities.list.len()],
        culture: polities
            .list
            .iter()
            .map(|polity| {
                // Wie beim Startwissen: Die Gemeinschaft trägt den Namen ihrer Vorlage.
                data.polities
                    .iter()
                    .find(|def| def.name == polity.name)
                    .map_or(NEUTRAL_CULTURE, |def| {
                        [def.culture.0, def.culture.1, def.culture.2]
                    })
            })
            .collect(),
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
    check_pair(diplomacy, polities, aggressor, target)?;
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
    check_pair(diplomacy, polities, a, b)?;
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

/// Beendet alle Kriege einer Gemeinschaft, ohne Friedensschluss: wenn sie endet oder sich
/// einem Oberhaupt unterstellt. Sonst blieben ihre Gegner für immer im Krieg mit
/// niemandem. Gibt die bisherigen Gegner zurück, aufsteigend.
pub fn drop_wars_of(diplomacy: &mut Diplomacy, polity: PolityId) -> Vec<PolityId> {
    let mut former = Vec::new();
    diplomacy.wars.retain(|(a, b)| {
        if *a == polity {
            former.push(*b);
            false
        } else if *b == polity {
            former.push(*a);
            false
        } else {
            true
        }
    });
    former.sort_unstable();
    former
}

/// Hebt alle Waffenruhen einer Gemeinschaft auf: wenn sie endet. Mit einer Gemeinschaft,
/// die es nicht mehr gibt, ruht nichts.
pub fn drop_truces_of(diplomacy: &mut Diplomacy, polity: PolityId) {
    diplomacy
        .truces
        .retain(|(a, b), _| *a != polity && *b != polity);
}

/// Krieg und Frieden sind Sache freier, lebender Gemeinschaften.
fn check_pair(
    diplomacy: &Diplomacy,
    polities: &Polities,
    a: PolityId,
    b: PolityId,
) -> Result<(), String> {
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
    for id in [a, b] {
        if let Some(overlord) = diplomacy.overlord_of(id) {
            let head = polities
                .list
                .get(overlord.index())
                .map_or("einem Oberhaupt", |polity| polity.name.as_str());
            return Err(format!(
                "{} untersteht {head}: Über Krieg und Frieden entscheidet das Oberhaupt",
                polities.get(id).name
            ));
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

    /// Beziehungen unter `count` freien Gemeinschaften, ohne Kriege.
    fn free(count: usize) -> Diplomacy {
        Diplomacy {
            wars: BTreeSet::new(),
            truces: BTreeMap::new(),
            standing: vec![Standing::default(); count],
            culture: vec![NEUTRAL_CULTURE; count],
        }
    }

    fn subordinate(diplomacy: &mut Diplomacy, ward: u32, lord: u32) {
        diplomacy.standing[ward as usize].overlord = Some(PolityId(lord));
    }

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

    #[test]
    fn without_entries_everyone_is_free_and_their_own_head() {
        // Von Hand gebaute Lagen (etwa in Tests der Flotten) führen keine Stellungen.
        let diplomacy = Diplomacy::default();
        let far = PolityId(7);
        assert!(diplomacy.is_free(far));
        assert_eq!(diplomacy.head(far), far);
        assert_eq!(diplomacy.bloc(far), vec![far]);
        assert_eq!(diplomacy.wards_of(far).count(), 0);
        assert_eq!(diplomacy.culture_of(far), NEUTRAL_CULTURE);
        assert!(!diplomacy.is_at_war(far));
    }

    #[test]
    fn wards_share_the_wars_of_their_head() {
        let mut diplomacy = free(6);
        let id = PolityId;
        // 1 und 4 unterstehen 0, 3 untersteht 2; 5 bleibt für sich.
        subordinate(&mut diplomacy, 1, 0);
        subordinate(&mut diplomacy, 4, 0);
        subordinate(&mut diplomacy, 3, 2);
        assert_eq!(diplomacy.head(id(1)), id(0));
        assert_eq!(diplomacy.head(id(0)), id(0));
        assert!(!diplomacy.is_free(id(4)) && diplomacy.is_free(id(5)));
        assert_eq!(
            diplomacy.wards_of(id(0)).collect::<Vec<_>>(),
            vec![id(1), id(4)]
        );
        assert_eq!(diplomacy.bloc(id(4)), vec![id(0), id(1), id(4)]);
        assert_eq!(diplomacy.bloc(id(2)), vec![id(2), id(3)]);
        assert_eq!(diplomacy.bloc(id(5)), vec![id(5)]);

        // Krieg zwischen den Häuptern 0 und 2: Alle sechs Paare über die Verbünde hinweg
        // liegen im Krieg, kein Paar innerhalb eines Verbunds.
        diplomacy.wars.insert(pair(id(0), id(2)));
        for ours in [0, 1, 4] {
            for theirs in [2, 3] {
                assert!(
                    diplomacy.at_war(id(ours), id(theirs)),
                    "{ours} gegen {theirs}"
                );
                assert!(diplomacy.at_war(id(theirs), id(ours)));
            }
        }
        for (a, b) in [(0, 1), (0, 4), (1, 4), (2, 3)] {
            assert!(!diplomacy.at_war(id(a), id(b)), "{a} und {b}: ein Verbund");
        }
        for member in 0..5 {
            assert!(diplomacy.is_at_war(id(member)));
            assert!(!diplomacy.at_war(id(member), id(5)));
        }
        assert!(!diplomacy.is_at_war(id(5)));
        // Feinde eines Unterstellten sind die seines Haupts, samt deren Unterstellten.
        assert_eq!(
            diplomacy.enemies_of(id(4)).collect::<Vec<_>>(),
            vec![id(2), id(3)]
        );
        assert_eq!(
            diplomacy.enemies_of(id(3)).collect::<Vec<_>>(),
            vec![id(0), id(1), id(4)]
        );
    }

    #[test]
    fn dropping_wars_leaves_no_trace_of_the_community() {
        let mut diplomacy = free(4);
        let id = PolityId;
        diplomacy.wars.insert(pair(id(2), id(0)));
        diplomacy.wars.insert(pair(id(2), id(3)));
        diplomacy.wars.insert(pair(id(0), id(1)));
        assert_eq!(drop_wars_of(&mut diplomacy, id(2)), vec![id(0), id(3)]);
        assert!(!diplomacy.is_at_war(id(2)) && !diplomacy.is_at_war(id(3)));
        assert!(diplomacy.at_war(id(0), id(1)), "andere Kriege bleiben");
        assert_eq!(drop_wars_of(&mut diplomacy, id(2)), vec![]);
        assert_eq!(diplomacy.wars.len(), 1);
    }

    #[test]
    fn affinity_measures_the_distance_of_cultures() {
        let mut diplomacy = free(4);
        diplomacy.culture[0] = [0.0, 0.0, 0.0];
        diplomacy.culture[1] = [1.0, 1.0, 1.0];
        diplomacy.culture[2] = [0.2, 0.5, 0.9];
        diplomacy.culture[3] = [0.2, 0.5, 0.4];
        let id = PolityId;
        assert_eq!(diplomacy.affinity(id(2), id(2)), 1.0);
        assert!(diplomacy.affinity(id(0), id(1)).abs() < 1e-6);
        // Symmetrisch, und ein Abstand von 0,5 in einem Zug kostet 0,5 / √3.
        let near = diplomacy.affinity(id(2), id(3));
        assert_eq!(near, diplomacy.affinity(id(3), id(2)));
        assert!((near - (1.0 - 0.5 / 3.0f32.sqrt())).abs() < 1e-6);
        // Wer keinen Eintrag hat, gilt als Mitte.
        assert_eq!(
            diplomacy.affinity(id(9), id(0)),
            1.0 - (0.75f32).sqrt() / 3.0f32.sqrt()
        );
    }

    #[test]
    fn the_shipped_templates_give_every_community_its_culture() {
        let data = GameData::load_workspace().unwrap();
        let mut galaxy = crate::galaxy::generate_galaxy(&data, 3);
        let polities = crate::polity::found_polities(&data, &mut galaxy, 3);
        let diplomacy = setup_diplomacy(&data, &polities);
        assert_eq!(diplomacy.standing.len(), polities.list.len());
        assert_eq!(diplomacy.culture.len(), polities.list.len());
        for polity in &polities.list {
            let template = data
                .polities
                .iter()
                .find(|def| def.name == polity.name)
                .unwrap();
            let (o, g, w) = template.culture;
            assert_eq!(diplomacy.culture_of(polity.id), [o, g, w]);
            assert!(diplomacy.is_free(polity.id));
        }
        // Keine zwei Gemeinschaften der Partie sind kulturell gleich.
        for a in &polities.list {
            for b in &polities.list {
                assert_eq!(diplomacy.affinity(a.id, b.id) == 1.0, a.id == b.id);
            }
        }
    }
}
