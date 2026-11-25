//! Die eine Quelle der Schlachtansicht.
//!
//! Wem ein Gefecht gehört (der Gefechtsübung, später der Partie), der legt hier nach jedem
//! Takt dessen Zustand ab. Die Ansicht liest nur diese Resource; Befehle des Spielers
//! gehen als [`BattleOrder`] zurück an den Besitzer. So hängt die Ansicht an keinem
//! bestimmten Spielzustand.

use bevy::prelude::*;
use sim::battle::math::wrap_angle;
use sim::battle::{Battle, BattleShip, Beam, Doctrine, Formation, Impact, V2};
use sim::data::Rgb;

/// Wie lange ein Schildtreffer nachleuchtet (Sekunden Gefechtszeit).
pub const SHIELD_FLASH_SECONDS: f32 = 0.45;

/// Befehl des Spielers an eine Partei des gezeigten Gefechts.
#[derive(Message, Clone, Copy, Debug, PartialEq)]
pub enum BattleOrder {
    /// Formation und Doktrin setzen.
    SetOrders {
        side: usize,
        formation: Formation,
        doctrine: Doctrine,
    },
    /// Alle Schiffe der Partei verlassen das Feld.
    Retreat { side: usize },
}

/// Wunsch des Spielers an den Besitzer des Gefechts, der nicht das Gefecht selbst betrifft.
#[derive(Message, Clone, Copy, Debug, PartialEq, Eq)]
pub enum BattleViewRequest {
    /// Die Ansicht verlassen.
    Leave,
    /// Dasselbe Gefecht von vorn (nur wenn [`FeedInfo::can_restart`]).
    Restart,
}

/// Lage eines Schiffs in der Gefechtsebene.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Pose {
    pub pos: V2,
    pub heading: f32,
}

impl Pose {
    pub fn of(ship: &BattleShip) -> Self {
        Self {
            pos: ship.pos,
            heading: ship.heading,
        }
    }

    /// Zwischenlage auf dem Weg nach `to` (`t` in 0..1). Der Kurs dreht auf dem kurzen Weg,
    /// auch über die Naht bei ±π.
    pub fn lerp(self, to: Pose, t: f32) -> Pose {
        Pose {
            pos: self.pos + (to.pos - self.pos) * t,
            heading: lerp_angle(self.heading, to.heading, t),
        }
    }
}

/// Winkel zwischen `from` und `to` auf dem kurzen Weg, Ergebnis in (-π, π].
pub fn lerp_angle(from: f32, to: f32, t: f32) -> f32 {
    wrap_angle(from + wrap_angle(to - from) * t)
}

/// Eine Partei, wie die Ansicht sie beschriftet und einfärbt.
#[derive(Clone, Debug, PartialEq)]
pub struct SideInfo {
    pub name: String,
    pub color: Rgb,
}

/// Was der Besitzer eines Gefechts der Ansicht dazu mitgibt.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FeedInfo {
    /// Überschrift, etwa „Gefechtsübung“ oder der Name des Systems.
    pub title: String,
    /// Je Partei Name und Farbe, in der Reihenfolge von `Battle::combatants`.
    pub sides: Vec<SideInfo>,
    /// Partei, die der Spieler führt; `None` für Zuschauer.
    pub player_side: Option<usize>,
    /// Bietet die Bilanz „Neu starten“ an?
    pub can_restart: bool,
}

/// Nachleuchten eines Schildtreffers an einem Schiff.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ShieldFlash {
    /// Gefechtszeit, zu der der Schimmer erlischt.
    pub until: f32,
    /// Richtung in der Gefechtsebene, aus der der Treffer kam.
    pub angle: f32,
}

#[derive(Resource, Default)]
pub struct BattleFeed {
    /// Zustand nach dem letzten Takt.
    battle: Option<Battle>,
    /// Lage jedes Schiffs einen Takt davor (gleiche Indizes; neue Schiffe fehlen noch).
    previous: Vec<Pose>,
    /// Zählt hoch, sobald ein anderes Gefecht gezeigt wird. Die Szene baut sich dann neu.
    pub epoch: u32,
    pub info: FeedInfo,
    /// Je Schiff das letzte Aufleuchten seines Schilds.
    shield_flash: Vec<ShieldFlash>,
    /// Lebensdauer jedes Strahls und Einschlags bei seiner Entstehung, parallel zu
    /// `battle.beams` und `battle.impacts`. Der Kern führt nur die Restzeit.
    beam_life: Vec<f32>,
    impact_life: Vec<f32>,
}

impl BattleFeed {
    /// Das gezeigte Gefecht, falls es eines gibt.
    pub fn battle(&self) -> Option<&Battle> {
        self.battle.as_ref()
    }

    pub fn is_active(&self) -> bool {
        self.battle.is_some()
    }

    /// Zeigt ab jetzt ein anderes Gefecht.
    pub fn begin(&mut self, battle: &Battle, info: FeedInfo) {
        self.epoch = self.epoch.wrapping_add(1);
        self.info = info;
        self.previous = battle.ships.iter().map(Pose::of).collect();
        self.shield_flash = vec![ShieldFlash::default(); battle.ships.len()];
        self.beam_life = battle.beams.iter().map(|beam| beam.ttl).collect();
        self.impact_life = battle.impacts.iter().map(|impact| impact.ttl).collect();
        self.battle = Some(battle.clone());
    }

    /// Übernimmt den Zustand des Gefechts. Aufzurufen nach jedem Takt und nach jedem
    /// Befehl, der es verändert. Ohne vorheriges [`BattleFeed::begin`] geschieht nichts.
    pub fn refresh(&mut self, battle: &Battle) {
        let Some(held) = self.battle.as_mut() else {
            return;
        };
        if battle.tick != held.tick {
            // Ein Takt ist vergangen: Der bisherige Zustand wird zur Ausgangslage der
            // Interpolation, und was sich nur aus dem Vergleich zweier Takte ergibt
            // (Schildtreffer, Lebensdauern der Effekte), wird hier festgehalten.
            self.previous.clear();
            self.previous.extend(held.ships.iter().map(Pose::of));
            self.shield_flash
                .resize(battle.ships.len(), ShieldFlash::default());
            for (index, (old, new)) in held.ships.iter().zip(&battle.ships).enumerate() {
                if new.shield + 0.05 < old.shield && new.status.in_play() {
                    self.shield_flash[index] = ShieldFlash {
                        until: battle.time + SHIELD_FLASH_SECONDS,
                        angle: threat_angle(battle, new),
                    };
                }
            }
            carry_lives(
                &mut self.beam_life,
                &held.beams,
                &battle.beams,
                |beam| beam.ttl,
                same_beam,
            );
            carry_lives(
                &mut self.impact_life,
                &held.impacts,
                &battle.impacts,
                |impact| impact.ttl,
                same_impact,
            );
        }
        copy_state(held, battle);
    }

    /// Es gibt kein Gefecht mehr zu zeigen.
    pub fn clear(&mut self) {
        self.battle = None;
        self.previous.clear();
        self.shield_flash.clear();
        self.beam_life.clear();
        self.impact_life.clear();
        self.info = FeedInfo::default();
    }

    /// Lage des Schiffs `index` zwischen dem vorletzten und dem letzten Takt
    /// (`alpha` = 0 ist der vorletzte, 1 der letzte).
    pub fn pose(&self, index: usize, alpha: f32) -> Pose {
        let Some(ship) = self.battle.as_ref().and_then(|b| b.ships.get(index)) else {
            return Pose::default();
        };
        let now = Pose::of(ship);
        match self.previous.get(index) {
            Some(before) => before.lerp(now, alpha.clamp(0.0, 1.0)),
            // Im letzten Takt gestartet (Bordjäger): Es gibt noch keine frühere Lage.
            None => now,
        }
    }

    /// Kurswechsel des Schiffs im letzten Takt (Bogenmaß, links positiv).
    pub fn turn(&self, index: usize) -> f32 {
        match (
            self.battle.as_ref().and_then(|b| b.ships.get(index)),
            self.previous.get(index),
        ) {
            (Some(ship), Some(before)) => wrap_angle(ship.heading - before.heading),
            _ => 0.0,
        }
    }

    pub fn shield_flash(&self, index: usize) -> ShieldFlash {
        self.shield_flash.get(index).copied().unwrap_or_default()
    }

    /// Lebensdauer des Strahls `index` bei seiner Entstehung.
    pub fn beam_life(&self, index: usize) -> f32 {
        life_or_ttl(&self.beam_life, index, || {
            let beam = self.battle.as_ref()?.beams.get(index)?;
            Some(beam.ttl)
        })
    }

    /// Lebensdauer des Einschlags `index` bei seiner Entstehung.
    pub fn impact_life(&self, index: usize) -> f32 {
        life_or_ttl(&self.impact_life, index, || {
            let impact = self.battle.as_ref()?.impacts.get(index)?;
            Some(impact.ttl)
        })
    }

    pub fn side_name(&self, side: usize) -> &str {
        self.info
            .sides
            .get(side)
            .map_or("Unbekannt", |info| info.name.as_str())
    }

    pub fn side_color(&self, side: usize) -> Rgb {
        self.info
            .sides
            .get(side)
            .map_or((0.7, 0.7, 0.7), |info| info.color)
    }
}

fn life_or_ttl(lives: &[f32], index: usize, ttl: impl FnOnce() -> Option<f32>) -> f32 {
    lives
        .get(index)
        .copied()
        .or_else(ttl)
        .unwrap_or(0.1)
        .max(1e-3)
}

/// Richtung, aus der ein Schiff vermutlich beschossen wird: sein Ziel, sonst der
/// Schwerpunkt der Gegner.
fn threat_angle(battle: &Battle, ship: &BattleShip) -> f32 {
    let toward = ship
        .target
        .and_then(|target| battle.ships.get(target as usize))
        .map(|target| target.pos)
        .or_else(|| {
            battle
                .combatants
                .get(ship.side as usize)
                .map(|combatant| combatant.enemy_center)
        })
        .unwrap_or(V2::ZERO);
    (toward - ship.pos).angle()
}

fn same_beam(old: &Beam, new: &Beam) -> bool {
    old.from == new.from && old.to == new.to && old.kind == new.kind
}

fn same_impact(old: &Impact, new: &Impact) -> bool {
    old.pos == new.pos && old.size == new.size
}

/// Führt die anfängliche Lebensdauer der Effekte über einen Takt hinweg mit.
///
/// Der Kern verdichtet seine Effektlisten stabil und hängt Neues hinten an. Was in der
/// neuen Liste an gleicher Stelle im Raum steht und nicht jünger geworden ist, hat also
/// überlebt und behält seine Lebensdauer; alles dahinter ist neu und bringt seine
/// Restzeit als Lebensdauer mit.
fn carry_lives<T>(
    lives: &mut Vec<f32>,
    old: &[T],
    new: &[T],
    ttl: impl Fn(&T) -> f32,
    same: impl Fn(&T, &T) -> bool,
) {
    if lives.len() != old.len() {
        // Der Besitzer hat Takte übersprungen: Was da ist, zählt ab jetzt.
        lives.clear();
        lives.extend(old.iter().map(&ttl));
    }
    let mut kept = 0;
    for (index, before) in old.iter().enumerate() {
        let Some(after) = new.get(kept) else {
            break;
        };
        if same(before, after) && ttl(after) <= ttl(before) {
            lives[kept] = lives[index];
            kept += 1;
        }
    }
    lives.truncate(kept);
    lives.extend(new[kept..].iter().map(&ttl));
}

/// Kopiert den sichtbaren Zustand und verwendet dabei den vorhandenen Speicher wieder:
/// Bei schnellem Vorlauf geschieht das viele Male je Frame.
fn copy_state(held: &mut Battle, source: &Battle) {
    held.id = source.id;
    held.system = source.system;
    held.seed = source.seed;
    held.tick = source.tick;
    held.time = source.time;
    held.finished = source.finished;
    held.timed_out = source.timed_out;
    held.combatants.clone_from(&source.combatants);
    held.hostile.clone_from(&source.hostile);
    held.shots.clone_from(&source.shots);
    held.beams.clone_from(&source.beams);
    held.impacts.clone_from(&source.impacts);
    held.wrecks.clone_from(&source.wrecks);

    held.ships.truncate(source.ships.len());
    for (target, ship) in held.ships.iter_mut().zip(&source.ships) {
        // Die Modulliste behält ihren Speicher; alle übrigen Felder sind schlichte Werte.
        let mut modules = std::mem::take(&mut target.modules);
        modules.clear();
        modules.extend_from_slice(&ship.modules);
        *target = BattleShip { modules, ..*ship };
    }
    let known = held.ships.len();
    held.ships.extend(source.ships[known..].iter().cloned());
}

#[cfg(test)]
pub mod testing {
    //! Gefechte für die Tests der Ansicht.

    use super::*;
    use sim::GameData;
    use sim::battle::matchup::{self, Side};

    pub fn data() -> GameData {
        GameData::load_workspace().expect("Spieldaten des Workspace")
    }

    pub fn info(sides: usize) -> FeedInfo {
        let palette = [(0.35, 0.65, 1.0), (1.0, 0.45, 0.3), (0.5, 0.9, 0.4)];
        FeedInfo {
            title: "Test".into(),
            sides: (0..sides)
                .map(|side| SideInfo {
                    name: format!("Partei {}", side + 1),
                    color: palette[side % palette.len()],
                })
                .collect(),
            player_side: Some(0),
            can_restart: true,
        }
    }

    /// Ein gemischtes Gefecht, `ticks` Takte weit gerechnet; der Feed hat jeden Takt gesehen.
    pub fn stepped(data: &GameData, fleets: [&str; 2], ticks: u32) -> (Battle, BattleFeed) {
        let sides: Vec<Side> = fleets
            .iter()
            .map(|fleet| Side::parse(data, fleet).expect("Flottenliste"))
            .collect();
        let mut battle = matchup::build(data, 11, &sides);
        let mut feed = BattleFeed::default();
        feed.begin(&battle, info(2));
        for _ in 0..ticks {
            battle.step(data);
            feed.refresh(&battle);
        }
        (battle, feed)
    }

    pub const MIXED: [&str; 2] = [
        "bastion:1,speer:3,waechter:3,dolch:3,horst:1",
        "bastion:2,speer:2,waechter:4,dolch:2",
    ];
}

#[cfg(test)]
mod tests {
    use super::testing::*;
    use super::*;
    use std::f32::consts::PI;

    #[test]
    fn angles_interpolate_the_short_way_across_the_seam() {
        // Von 170° nach −170° sind es 20° über die Naht.
        let (from, to) = (170f32.to_radians(), -170f32.to_radians());
        let half = lerp_angle(from, to, 0.5);
        assert!((half.abs() - PI).abs() < 1e-4, "Mitte liegt bei ±180°, nicht {half}");
        let quarter = lerp_angle(from, to, 0.25);
        assert!((quarter - 175f32.to_radians()).abs() < 1e-4);
        let three_quarters = lerp_angle(from, to, 0.75);
        assert!((three_quarters + 175f32.to_radians()).abs() < 1e-4);
        // Und zurück, ebenfalls über die Naht.
        assert!((lerp_angle(to, from, 0.25) + 175f32.to_radians()).abs() < 1e-4);
        // Enden und gewöhnliche Fälle.
        assert!((lerp_angle(0.3, 1.1, 0.0) - 0.3).abs() < 1e-6);
        assert!((lerp_angle(0.3, 1.1, 1.0) - 1.1).abs() < 1e-6);
        assert!((lerp_angle(-0.4, 0.4, 0.5)).abs() < 1e-6);
        // Das Ergebnis bleibt immer im Bereich (-π, π].
        for step in 0..=20 {
            let angle = lerp_angle(3.0, -3.0, step as f32 / 20.0);
            assert!(angle > -PI - 1e-6 && angle <= PI + 1e-6);
        }
    }

    #[test]
    fn poses_interpolate_position_and_heading() {
        let a = Pose {
            pos: V2::new(10.0, -4.0),
            heading: 3.0,
        };
        let b = Pose {
            pos: V2::new(20.0, 6.0),
            heading: -3.0,
        };
        let mid = a.lerp(b, 0.5);
        assert!((mid.pos.x - 15.0).abs() < 1e-5 && (mid.pos.y - 1.0).abs() < 1e-5);
        // 3,0 und −3,0 liegen 0,283 rad auseinander, über die Naht hinweg.
        assert!((mid.heading.abs() - PI).abs() < 1e-4);
        assert_eq!(a.lerp(b, 0.0), a);
        let end = a.lerp(b, 1.0);
        assert!((end.pos.x - b.pos.x).abs() < 1e-5 && (end.heading - b.heading).abs() < 1e-5);
    }

    #[test]
    fn feed_keeps_the_previous_tick_for_interpolation() {
        let data = data();
        let (mut battle, mut feed) = stepped(&data, MIXED, 40);
        let before: Vec<Pose> = battle.ships.iter().map(Pose::of).collect();
        battle.step(&data);
        feed.refresh(&battle);
        let held = feed.battle().unwrap();
        assert_eq!(held.tick, battle.tick);
        assert_eq!(held.ships.len(), battle.ships.len());
        for (index, ship) in battle.ships.iter().enumerate() {
            let now = Pose::of(ship);
            assert_eq!(feed.pose(index, 1.0).pos, now.pos);
            if let Some(old) = before.get(index) {
                assert_eq!(feed.pose(index, 0.0).pos, old.pos);
                let mid = feed.pose(index, 0.5).pos;
                assert!((mid.x - 0.5 * (old.pos.x + now.pos.x)).abs() < 1e-3);
            } else {
                // Frisch gestartete Jäger stehen sofort an ihrem Platz.
                assert_eq!(feed.pose(index, 0.0).pos, now.pos);
            }
        }
        // Ein Befehl ohne Takt verschiebt die Ausgangslage nicht.
        let anchor = feed.pose(0, 0.0);
        battle.set_orders(&data, 0, Formation::Wedge, Doctrine::Aggressive);
        feed.refresh(&battle);
        assert_eq!(feed.pose(0, 0.0), anchor);
        assert_eq!(feed.battle().unwrap().combatants[0].formation, Formation::Wedge);
    }

    #[test]
    fn feed_copy_matches_the_battle_tick_by_tick() {
        let data = data();
        let (mut battle, mut feed) = stepped(&data, MIXED, 0);
        let epoch = feed.epoch;
        for _ in 0..600 {
            battle.step(&data);
            feed.refresh(&battle);
            let held = feed.battle().unwrap();
            assert_eq!(held.ships.len(), battle.ships.len());
            assert_eq!(held.shots.len(), battle.shots.len());
            assert_eq!(held.beams.len(), battle.beams.len());
            assert_eq!(held.wrecks.len(), battle.wrecks.len());
            for (copy, ship) in held.ships.iter().zip(&battle.ships) {
                assert_eq!(copy.pos, ship.pos);
                assert_eq!(copy.status, ship.status);
                assert_eq!(copy.hull, ship.hull);
                assert_eq!(copy.modules, ship.modules);
            }
            for side in 0..2 {
                assert_eq!(held.active_by_class(side), battle.active_by_class(side));
                assert_eq!(
                    held.fighting_value(&data, side),
                    battle.fighting_value(&data, side)
                );
            }
        }
        assert_eq!(feed.epoch, epoch, "dasselbe Gefecht, dieselbe Epoche");
        assert!(battle.wrecks.len() > 2, "das Gefecht hat Verluste gebracht");
        feed.begin(&battle, info(2));
        assert_eq!(feed.epoch, epoch + 1);
        feed.clear();
        assert!(!feed.is_active());
    }

    #[test]
    fn effect_lives_are_tracked_from_creation() {
        let data = data();
        let (mut battle, mut feed) = stepped(&data, MIXED, 0);
        let (mut beams_seen, mut impacts_seen, mut flashes) = (0, 0, 0);
        for _ in 0..900 {
            battle.step(&data);
            feed.refresh(&battle);
            for (index, beam) in battle.beams.iter().enumerate() {
                let life = feed.beam_life(index);
                assert!(life + 1e-5 >= beam.ttl, "Lebensdauer {life} unter Restzeit {}", beam.ttl);
                // Kein Strahl lebt länger als eine halbe Sekunde.
                assert!(life <= 0.5);
                beams_seen += 1;
            }
            for (index, impact) in battle.impacts.iter().enumerate() {
                let life = feed.impact_life(index);
                assert!(life + 1e-5 >= impact.ttl);
                // Wer älter als ein Takt ist, hat seine anfängliche Lebensdauer behalten.
                if impact.ttl + 0.15 < life {
                    impacts_seen += 1;
                }
            }
            flashes += (0..battle.ships.len())
                .filter(|index| feed.shield_flash(*index).until > battle.time)
                .count();
        }
        assert!(beams_seen > 100, "nur {beams_seen} Strahlen gesehen");
        assert!(impacts_seen > 20, "nur {impacts_seen} gealterte Einschläge gesehen");
        assert!(flashes > 20, "Schilde wurden getroffen, also leuchten sie auf");
    }

    #[test]
    fn lives_survive_compaction() {
        // Drei Effekte, der mittlere erlischt, hinten kommt einer dazu.
        let old = [(1.0f32, 0.3f32), (2.0, 0.1), (3.0, 0.5)];
        let new = [(1.0f32, 0.2f32), (3.0, 0.4), (4.0, 0.25)];
        let mut lives = vec![0.3, 0.9, 1.2];
        carry_lives(&mut lives, &old, &new, |item| item.1, |a, b| a.0 == b.0);
        assert_eq!(lives, vec![0.3, 1.2, 0.25]);
        // Alles erloschen, nichts Neues.
        carry_lives(&mut lives, &new, &[], |item: &(f32, f32)| item.1, |a, b| a.0 == b.0);
        assert!(lives.is_empty());
        // An gleicher Stelle neu entstanden (Restzeit größer): gilt als neu.
        let mut lives = vec![0.9];
        carry_lives(&mut lives, &[(1.0f32, 0.1f32)], &[(1.0, 0.3)], |item| item.1, |a, b| {
            a.0 == b.0
        });
        assert_eq!(lives, vec![0.3]);
    }
}
