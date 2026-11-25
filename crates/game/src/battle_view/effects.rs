//! Geschosse, Strahlen, Einschläge und Schildschimmer.
//!
//! Der Kern verdichtet diese Listen jeden Takt, feste Indizes gibt es nicht. Deshalb
//! kommen die Effekte aus Vorräten wiederverwendbarer Entities: Jedes Frame werden die
//! ersten Plätze eines Vorrats neu belegt und die übrigen ausgeblendet. Ein Vorrat wächst
//! bis zu seiner Obergrenze und schrumpft nie; angelegt oder entfernt wird im Dauerbetrieb
//! nichts. Alle Plätze eines Vorrats teilen Mesh und Material, die Engine zeichnet sie
//! gebündelt.

use super::feed::{BattleFeed, SHIELD_FLASH_SECONDS};
use super::scene::{
    BattleAssets, BattleVisual, FLASH_LEVELS, SHIELD_LEVELS, SideAssets, rebuild_on_epoch,
};
use super::{BattleViewSet, Part, ViewFrame, ViewTimings, heading_rotation, to_world};
use bevy::prelude::*;
use sim::battle::BeamKind;
use sim::data::WeaponKind;

/// Vorräte je Partei.
const TRACER: usize = 0;
const MISSILE: usize = 1;
const BEAM_LIGHT: usize = 2;
const BEAM_HEAVY: usize = 3;
const BEAM_PD: usize = 4;
const PER_SIDE: usize = 5;

/// Obergrenzen. Was darüber hinausgeht, wird in diesem Frame nicht gezeigt.
const CAP_TRACERS: usize = 6000;
const CAP_MISSILES: usize = 1500;
const CAP_BEAMS: usize = 1500;
const CAP_FLASHES: usize = 700;
const CAP_SHIELDS: usize = 400;

/// So viele Pixel ist ein Geschoss oder Strahl mindestens dick, damit ein Gefecht auch
/// aus der Ferne zu lesen ist.
const MIN_PIXELS: f32 = 1.4;

/// Platz in einem Vorrat.
#[derive(Component, Clone, Copy, Debug)]
pub struct PoolSlot {
    pool: u16,
    index: u32,
}

/// Ein Vorrat gleichartiger Effekte.
pub struct Pool {
    mesh: Handle<Mesh>,
    material: Handle<StandardMaterial>,
    cap: usize,
    /// Angelegte Plätze.
    spawned: usize,
    /// Belegung dieses Frames: Platz `i` zeigt `items[i]`.
    items: Vec<Transform>,
}

impl Pool {
    fn new(mesh: &Handle<Mesh>, material: &Handle<StandardMaterial>, cap: usize) -> Self {
        Self {
            mesh: mesh.clone(),
            material: material.clone(),
            cap,
            spawned: 0,
            items: Vec::new(),
        }
    }

    fn push(&mut self, transform: Transform) {
        if self.items.len() < self.cap {
            self.items.push(transform);
        }
    }
}

#[derive(Resource, Default)]
pub struct EffectPools {
    epoch: Option<u32>,
    sides: usize,
    pools: Vec<Pool>,
}

/// Was die Ansicht zuletzt an Effekten gezeigt hat.
#[derive(Resource, Default, Debug, Clone, Copy, PartialEq, Eq)]
pub struct EffectStats {
    /// Geschosse und Lenkwaffen in diesem Frame.
    pub shots: usize,
    /// Höchststand seit Beginn des Gefechts.
    pub peak_shots: usize,
    pub beams: usize,
    pub peak_beams: usize,
    /// Angelegte Plätze aller Vorräte.
    pub entities: usize,
}

impl EffectPools {
    /// Vorräte für ein neues Gefecht: je Partei fünf, dazu Einschläge und Schildschimmer.
    pub fn rebuild(&mut self, assets: &BattleAssets, sides: &SideAssets) {
        self.epoch = sides.epoch;
        self.sides = sides.sides.len();
        self.pools.clear();
        for side in &sides.sides {
            self.pools
                .push(Pool::new(&assets.tracer, &side.tracer, CAP_TRACERS));
            self.pools
                .push(Pool::new(&assets.missile, &side.missile, CAP_MISSILES));
            self.pools
                .push(Pool::new(&assets.beam, &side.beam_light, CAP_BEAMS));
            self.pools
                .push(Pool::new(&assets.beam, &side.beam_heavy, CAP_BEAMS));
            self.pools
                .push(Pool::new(&assets.beam, &side.beam_pd, CAP_BEAMS));
        }
        for material in &assets.flash_materials {
            self.pools
                .push(Pool::new(&assets.flash, material, CAP_FLASHES));
        }
        for material in &assets.shield_materials {
            self.pools
                .push(Pool::new(&assets.shield, material, CAP_SHIELDS));
        }
    }

    fn side_pool(&mut self, side: usize, kind: usize) -> Option<&mut Pool> {
        (side < self.sides).then(|| &mut self.pools[side * PER_SIDE + kind])
    }

    fn flash_pool(&mut self, level: usize) -> &mut Pool {
        let index = self.sides * PER_SIDE + level.min(FLASH_LEVELS - 1);
        &mut self.pools[index]
    }

    fn shield_pool(&mut self, level: usize) -> &mut Pool {
        let index = self.sides * PER_SIDE + FLASH_LEVELS + level.min(SHIELD_LEVELS - 1);
        &mut self.pools[index]
    }

    /// Angelegte Plätze aller Vorräte.
    pub fn spawned(&self) -> usize {
        self.pools.iter().map(|pool| pool.spawned).sum()
    }

    /// Obergrenze aller Vorräte zusammen.
    pub fn capacity(&self) -> usize {
        self.pools.iter().map(|pool| pool.cap).sum()
    }

    /// Verteilt alle Effekte dieses Frames auf die Vorräte. Reine Rechnung: liest den Feed
    /// und den Blick der Kamera, legt nichts an.
    pub fn collect(&mut self, feed: &BattleFeed, frame: &ViewFrame) -> EffectStats {
        for pool in &mut self.pools {
            pool.items.clear();
        }
        let mut stats = EffectStats::default();
        let Some(battle) = feed.battle() else {
            return stats;
        };
        if self.pools.is_empty() {
            return stats;
        }
        let tick = frame.tick_seconds.max(1e-3);
        // Die Anzeige liegt zwischen zwei Takten noch ein Stück vor dem letzten Takt.
        let back = (1.0 - frame.alpha) * tick;
        let eye = frame.projector.eye;
        let pixel = frame.projector.pixel();
        let thin = |at: Vec3| MIN_PIXELS * pixel * at.distance(eye);

        for shot in &battle.shots {
            let position = to_world(shot.pos - shot.vel * back);
            let rotation = heading_rotation(shot.vel.angle());
            let least = thin(position);
            let power = shot.damage.max(0.3).sqrt();
            let (kind, scale) = match shot.kind {
                WeaponKind::Missile => {
                    let size = (1.3 + 0.17 * power).max(least * 1.6);
                    (MISSILE, Vec3::splat(size))
                }
                _ => {
                    let width = (0.3 + 0.13 * power).max(least);
                    let length = (5.0 + 2.2 * power).max(width * 5.0);
                    (TRACER, Vec3::new(length, width, width))
                }
            };
            if let Some(pool) = self.side_pool(shot.side as usize, kind) {
                pool.push(Transform {
                    translation: position,
                    rotation,
                    scale,
                });
                stats.shots += 1;
            }
        }

        // Nach dem Ende lässt der Kern Strahlen und Einschläge stehen; sie gelten als
        // erloschen.
        if !battle.finished {
            for (index, beam) in battle.beams.iter().enumerate() {
                let progress = progress(feed.beam_life(index), beam.ttl, frame.alpha, tick);
                let from = to_world(beam.from);
                let mut to = to_world(beam.to);
                let span = to - from;
                let reach = span.length();
                if reach < 1e-3 {
                    continue;
                }
                let direction = span / reach;
                if !beam.hit && beam.kind != BeamKind::PointDefense {
                    // Ein Fehlschuss endet nicht neben dem Ziel, er geht daran vorbei.
                    to += direction * (reach * 0.3 + 90.0);
                }
                let rotation = heading_rotation((beam.to - beam.from).angle());
                let (kind, start, length, width) = match beam.kind {
                    BeamKind::Light => (BEAM_LIGHT, from, to.distance(from), 0.8),
                    BeamKind::Heavy => (BEAM_HEAVY, from, to.distance(from), 2.4),
                    BeamKind::PointDefense => {
                        // Leuchtspur: ein kurzes Stück, das vom Schützen zum Ziel läuft.
                        let piece = (reach * 0.4).min(55.0);
                        let start = from + direction * ((reach - piece) * progress);
                        (BEAM_PD, start, piece, 0.45)
                    }
                };
                let fade = if beam.kind == BeamKind::PointDefense {
                    1.0
                } else {
                    (1.0 - progress).powf(0.6)
                };
                let middle = start + direction * (length * 0.5);
                let width = (width * fade).max(thin(middle) * fade.max(0.35));
                if let Some(pool) = self.side_pool(beam.side as usize, kind) {
                    pool.push(Transform {
                        translation: start,
                        rotation,
                        scale: Vec3::new(length, width, width),
                    });
                    stats.beams += 1;
                }
            }

            for (index, impact) in battle.impacts.iter().enumerate() {
                let progress = progress(feed.impact_life(index), impact.ttl, frame.alpha, tick);
                let position = to_world(impact.pos);
                // Schnell aufblähen, dann langsamer weiterwachsen und verblassen.
                let swell = 1.0 - (1.0 - progress) * (1.0 - progress);
                let radius = (impact.size * (0.3 + 0.7 * swell)).max(thin(position) * 1.5);
                let level = (progress * FLASH_LEVELS as f32) as usize;
                self.flash_pool(level).push(Transform {
                    translation: position,
                    rotation: Quat::IDENTITY,
                    scale: Vec3::splat(radius),
                });
            }

            for (index, ship) in battle.ships.iter().enumerate() {
                let flash = feed.shield_flash(index);
                let left = flash.until - frame.time;
                if left <= 0.0 || !ship.status.in_play() {
                    continue;
                }
                let Some((position, _)) = frame.poses.get(index) else {
                    continue;
                };
                let faded = 1.0 - (left / SHIELD_FLASH_SECONDS).clamp(0.0, 1.0);
                let level = (faded * SHIELD_LEVELS as f32) as usize;
                let radius = ship.radius * 1.12;
                self.shield_pool(level).push(Transform {
                    translation: *position,
                    rotation: heading_rotation(flash.angle),
                    scale: Vec3::new(radius, radius * 0.5, radius),
                });
            }
        }
        stats
    }
}

/// Wie weit ein Effekt durch sein Leben ist (0..1). Der Kern zählt die Restzeit in ganzen
/// Takten herunter; zwischen den Takten schreibt `alpha` sie fort.
fn progress(life: f32, ttl: f32, alpha: f32, tick: f32) -> f32 {
    let ticks = (life / tick - 0.01).ceil().max(1.0);
    (((life - ttl).max(0.0) + alpha * tick) / (ticks * tick)).clamp(0.0, 1.0)
}

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<EffectPools>()
        .init_resource::<EffectStats>()
        .add_systems(
            Update,
            show_effects
                .in_set(BattleViewSet::Scene)
                .after(rebuild_on_epoch),
        );
}

fn show_effects(
    mut timings: ResMut<ViewTimings>,
    mut commands: Commands,
    feed: Res<BattleFeed>,
    frame: Res<ViewFrame>,
    assets: Res<BattleAssets>,
    sides: Res<SideAssets>,
    mut pools: ResMut<EffectPools>,
    mut stats: ResMut<EffectStats>,
    mut slots: Query<(&PoolSlot, &mut Transform, &mut Visibility)>,
) {
    let _span = timings.span(Part::Effects);
    if pools.epoch != sides.epoch {
        // Neues Gefecht: Die alten Plätze sind mit der Szene verschwunden.
        pools.rebuild(&assets, &sides);
        *stats = EffectStats::default();
    }
    let shown = pools.collect(&feed, &frame);
    assign(&mut commands, &mut pools, &mut slots);
    *stats = EffectStats {
        peak_shots: stats.peak_shots.max(shown.shots),
        peak_beams: stats.peak_beams.max(shown.beams),
        entities: pools.spawned(),
        ..shown
    };
}

/// Überträgt die Belegung der Vorräte auf ihre Entities: belegte Plätze bekommen ihre
/// Lage und werden sichtbar, freie werden ausgeblendet, fehlende angelegt.
fn assign(
    commands: &mut Commands,
    pools: &mut EffectPools,
    slots: &mut Query<(&PoolSlot, &mut Transform, &mut Visibility)>,
) {
    for (slot, mut transform, mut visibility) in slots.iter_mut() {
        let Some(pool) = pools.pools.get(slot.pool as usize) else {
            continue;
        };
        match pool.items.get(slot.index as usize) {
            Some(item) => {
                *transform = *item;
                if *visibility != Visibility::Inherited {
                    *visibility = Visibility::Inherited;
                }
            }
            None => {
                if *visibility != Visibility::Hidden {
                    *visibility = Visibility::Hidden;
                }
            }
        }
    }
    for (number, pool) in pools.pools.iter_mut().enumerate() {
        while pool.spawned < pool.items.len() {
            commands.spawn((
                BattleVisual,
                PoolSlot {
                    pool: number as u16,
                    index: pool.spawned as u32,
                },
                Mesh3d(pool.mesh.clone()),
                MeshMaterial3d(pool.material.clone()),
                pool.items[pool.spawned],
            ));
            pool.spawned += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::feed::testing::{MIXED, data, info, stepped};
    use super::super::scene::{SideMaterials, testing::assets};
    use super::*;
    use bevy::ecs::system::RunSystemOnce;
    use sim::battle::matchup::{self, Side};

    fn frame_for(feed: &BattleFeed, alpha: f32) -> ViewFrame {
        let battle = feed.battle().unwrap();
        ViewFrame {
            alpha,
            time: battle.time - (1.0 - alpha) * 0.1,
            tick_seconds: 0.1,
            poses: (0..battle.ships.len())
                .map(|index| {
                    let pose = feed.pose(index, alpha);
                    (to_world(pose.pos), pose.heading)
                })
                .collect(),
            projector: Default::default(),
        }
    }

    fn side_assets(materials: &mut Assets<StandardMaterial>, epoch: u32) -> SideAssets {
        SideAssets {
            epoch: Some(epoch),
            sides: vec![
                SideMaterials::build((0.3, 0.6, 1.0), materials),
                SideMaterials::build((1.0, 0.4, 0.3), materials),
            ],
        }
    }

    #[test]
    fn progress_runs_from_start_to_end() {
        // Ein Takt Lebensdauer: Der Effekt läuft innerhalb dieses einen Takts ab.
        assert_eq!(progress(0.1, 0.1, 0.0, 0.1), 0.0);
        assert!((progress(0.1, 0.1, 0.5, 0.1) - 0.5).abs() < 1e-5);
        // 0,12 s Lebensdauer sind zwei sichtbare Takte.
        assert!((progress(0.12, 0.12, 0.0, 0.1)).abs() < 1e-5);
        assert!((progress(0.12, 0.02, 0.0, 0.1) - 0.5).abs() < 1e-4);
        assert!(progress(0.12, 0.02, 0.99, 0.1) > 0.98);
        // Stetig und nie über 1.
        let mut last = 0.0;
        for (ttl, alpha) in [(0.3, 0.0), (0.3, 0.7), (0.2, 0.2), (0.1, 0.1), (0.1, 0.99)] {
            let value = progress(0.3, ttl, alpha, 0.1);
            assert!(value >= last && value <= 1.0);
            last = value;
        }
    }

    #[test]
    fn every_effect_of_the_battle_gets_a_slot() {
        let data = data();
        let (mut assets_and_stores, mut materials) = {
            let (assets, meshes, materials) = assets();
            ((assets, meshes), materials)
        };
        let sides = side_assets(&mut materials, 1);
        let mut pools = EffectPools::default();
        pools.rebuild(&assets_and_stores.0, &sides);
        assert_eq!(pools.pools.len(), 2 * PER_SIDE + FLASH_LEVELS + SHIELD_LEVELS);

        let (mut battle, mut feed) = stepped(&data, MIXED, 0);
        let (mut most_shots, mut most_beams, mut flashes, mut shields) = (0, 0, 0, 0);
        for step in 0..700 {
            battle.step(&data);
            feed.refresh(&battle);
            let alpha = (step % 4) as f32 * 0.25;
            let stats = pools.collect(&feed, &frame_for(&feed, alpha));
            if battle.finished {
                break;
            }
            // Jedes Geschoss und jeder Strahl des Gefechts ist einem Platz zugewiesen.
            assert_eq!(stats.shots, battle.shots.len());
            assert_eq!(stats.beams, battle.beams.len());
            let tracers: usize = (0..2).map(|side| pools.pools[side * PER_SIDE + TRACER].items.len()).sum();
            let missiles: usize = (0..2).map(|side| pools.pools[side * PER_SIDE + MISSILE].items.len()).sum();
            assert_eq!(tracers + missiles, battle.shots.len());
            let flash_items: usize = (0..FLASH_LEVELS).map(|level| pools.flash_pool(level).items.len()).sum();
            assert_eq!(flash_items, battle.impacts.len());
            most_shots = most_shots.max(stats.shots);
            most_beams = most_beams.max(stats.beams);
            flashes += flash_items;
            shields += (0..SHIELD_LEVELS).map(|level| pools.shield_pool(level).items.len()).sum::<usize>();
            for pool in &pools.pools {
                for item in &pool.items {
                    assert!(item.translation.is_finite() && item.scale.is_finite());
                    assert!(item.scale.min_element() > 0.0, "Effekt ohne Ausdehnung");
                    assert_eq!(item.translation.y, 0.0, "Effekte liegen in der Gefechtsebene");
                }
            }
        }
        assert!(most_shots > 10 && most_beams > 5, "{most_shots} Geschosse, {most_beams} Strahlen");
        assert!(flashes > 50 && shields > 20, "{flashes} Einschläge, {shields} Schildtreffer");
        let _ = &mut assets_and_stores;
    }

    #[test]
    fn tracers_point_along_their_flight_and_beams_reach_their_end() {
        let data = data();
        let (_, mut materials) = {
            let (assets, _, materials) = assets();
            (assets, materials)
        };
        let (assets, _, _) = assets();
        let sides = side_assets(&mut materials, 1);
        let mut pools = EffectPools::default();
        pools.rebuild(&assets, &sides);
        let (mut battle, mut feed) = stepped(&data, MIXED, 0);
        let mut checked = 0;
        for _ in 0..500 {
            battle.step(&data);
            feed.refresh(&battle);
            if battle.finished || battle.shots.is_empty() || battle.beams.is_empty() {
                continue;
            }
            // Am Ende des Takts (alpha = 1) steht jedes Geschoss genau an seinem Ort.
            pools.collect(&feed, &frame_for(&feed, 1.0));
            for side in 0..2 {
                let mut expected = battle
                    .shots
                    .iter()
                    .filter(|shot| shot.side as usize == side && shot.kind != WeaponKind::Missile);
                for item in &pools.pools[side * PER_SIDE + TRACER].items {
                    let shot = expected.next().expect("Geschoss zum Platz");
                    assert!(item.translation.distance(to_world(shot.pos)) < 1e-3);
                    let nose = item.rotation * Vec3::X;
                    let flight = to_world(shot.vel).normalize();
                    assert!(nose.dot(flight) > 0.999, "Leuchtspur quer zur Flugrichtung");
                    assert!(item.scale.x > item.scale.y * 3.0, "Leuchtspur ist gestreckt");
                    checked += 1;
                }
                // Jeder Laserstrahl beginnt beim Schützen und reicht bis zum Endpunkt;
                // ein Fehlschuss geht darüber hinaus.
                let mut beams = battle
                    .beams
                    .iter()
                    .filter(|beam| beam.side as usize == side && beam.kind == BeamKind::Light);
                for item in &pools.pools[side * PER_SIDE + BEAM_LIGHT].items {
                    let beam = beams.next().expect("Strahl zum Platz");
                    assert!(item.translation.distance(to_world(beam.from)) < 1e-3);
                    let end = item.translation + item.rotation * Vec3::X * item.scale.x;
                    let reach = to_world(beam.to).distance(to_world(beam.from));
                    if beam.hit {
                        assert!(end.distance(to_world(beam.to)) < 0.05);
                    } else {
                        assert!(item.scale.x > reach + 80.0, "Fehlschuss endet am Ziel");
                    }
                }
            }
        }
        assert!(checked > 100, "nur {checked} Leuchtspuren geprüft");
    }

    /// Welt ohne Fenster: nur das, was die Effekte brauchen.
    fn world_with(feed: BattleFeed) -> World {
        let mut world = World::new();
        let (assets, meshes, mut materials) = assets();
        let sides = side_assets(&mut materials, feed.epoch);
        world.insert_resource(assets);
        world.insert_resource(meshes);
        world.insert_resource(materials);
        world.insert_resource(sides);
        world.insert_resource(feed);
        world.insert_resource(ViewFrame::default());
        world.insert_resource(ViewTimings::default());
        world.insert_resource(EffectPools::default());
        world.insert_resource(EffectStats::default());
        world
    }

    fn slots(world: &mut World) -> (usize, usize) {
        let mut query = world.query::<(&PoolSlot, &Visibility)>();
        let all = query.iter(world).count();
        let visible = query
            .iter(world)
            .filter(|(_, visibility)| **visibility != Visibility::Hidden)
            .count();
        (all, visible)
    }

    #[test]
    fn pool_entities_are_reused_and_bounded() {
        let data = data();
        let sides: Vec<Side> = MIXED
            .iter()
            .map(|fleet| Side::parse(&data, fleet).unwrap())
            .collect();
        let mut battle = matchup::build(&data, 5, &sides);
        let mut feed = BattleFeed::default();
        feed.begin(&battle, info(2));
        let mut world = world_with(feed);

        let (mut high_water, mut shown_total, mut frames) = (0usize, 0usize, 0);
        for step in 0..900 {
            battle.step(&data);
            world.resource_mut::<BattleFeed>().refresh(&battle);
            if step % 3 != 0 {
                continue;
            }
            let frame = frame_for(world.resource::<BattleFeed>(), 0.5);
            world.insert_resource(frame);
            world.run_system_once(show_effects).unwrap();
            frames += 1;

            let (all, _) = slots(&mut world);
            let stats = *world.resource::<EffectStats>();
            let live = if battle.finished {
                battle.shots.len()
            } else {
                battle.shots.len() + battle.beams.len() + battle.impacts.len()
            };
            shown_total += live;
            // Angelegt wird nur, was je gleichzeitig gebraucht wurde: Die Zahl der Plätze
            // wächst höchstens auf den bisherigen Höchststand je Vorrat und fällt nie.
            assert!(all >= high_water, "Plätze wurden entfernt");
            assert_eq!(all, stats.entities);
            high_water = all;
            assert!(all <= world.resource::<EffectPools>().capacity());
        }
        // Über das ganze Gefecht wurden weit mehr Effekte gezeigt, als es Plätze gibt:
        // Sie werden wiederverwendet.
        assert!(frames > 100);
        assert!(shown_total > high_water * 5, "{shown_total} Effekte auf {high_water} Plätzen");
        assert!(high_water > 20 && high_water < 2000, "{high_water} Plätze");

        // Nach dem Gefecht (keine Geschosse mehr, Strahlen gelten als erloschen) ist jeder
        // Platz ausgeblendet, aber keiner entfernt.
        while !battle.finished {
            battle.step(&data);
        }
        world.resource_mut::<BattleFeed>().refresh(&battle);
        let frame = frame_for(world.resource::<BattleFeed>(), 1.0);
        world.insert_resource(frame);
        world.run_system_once(show_effects).unwrap();
        world.run_system_once(show_effects).unwrap();
        let (all, visible) = slots(&mut world);
        assert_eq!(all, high_water);
        assert_eq!(visible, 0);
    }

    #[test]
    fn pools_never_grow_past_their_cap() {
        let mut pool = Pool::new(&Handle::default(), &Handle::default(), 8);
        for _ in 0..100 {
            pool.push(Transform::IDENTITY);
        }
        assert_eq!(pool.items.len(), 8);
        // Ein neues Gefecht beginnt mit leeren Vorräten.
        let (assets, _, mut materials) = assets();
        let mut pools = EffectPools::default();
        pools.rebuild(&assets, &side_assets(&mut materials, 3));
        assert_eq!(pools.spawned(), 0);
        assert_eq!(pools.epoch, Some(3));
        assert_eq!(
            pools.capacity(),
            2 * (CAP_TRACERS + CAP_MISSILES + 3 * CAP_BEAMS)
                + FLASH_LEVELS * CAP_FLASHES
                + SHIELD_LEVELS * CAP_SHIELDS
        );
    }
}
