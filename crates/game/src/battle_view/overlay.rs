//! Linien in der Szene: Bezugsringe auf der Gefechtsebene, Auswahlring und die
//! zuschaltbare taktische Einblendung (Flottenanker, Reichweiten, Ziellinie).

use super::camera::{SelectedShip, ViewOptions};
use super::feed::BattleFeed;
use super::{BattleViewSet, Part, ViewFrame, ViewTimings, heading_rotation, to_world};
use bevy::prelude::*;
use std::f32::consts::FRAC_PI_2;

/// Eigene Gizmo-Gruppe: dünnere Linien als die der Galaxiekarte.
#[derive(Default, Reflect, GizmoConfigGroup)]
pub struct BattleGizmos;

pub(super) fn plugin(app: &mut App) {
    app.init_gizmo_group::<BattleGizmos>()
        .add_systems(Startup, configure)
        .add_systems(Update, draw_overlay.in_set(BattleViewSet::Overlay));
}

fn configure(mut store: ResMut<GizmoConfigStore>) {
    let (config, _) = store.config_mut::<BattleGizmos>();
    config.line.width = 1.4;
}

/// Kreis in der Gefechtsebene.
fn ring(
    gizmos: &mut Gizmos<BattleGizmos>,
    center: Vec3,
    radius: f32,
    color: Color,
    resolution: u32,
) {
    gizmos
        .circle(
            Isometry3d::new(center, Quat::from_rotation_x(-FRAC_PI_2)),
            radius,
            color,
        )
        .resolution(resolution);
}

fn draw_overlay(
    mut timings: ResMut<ViewTimings>,
    mut gizmos: Gizmos<BattleGizmos>,
    feed: Res<BattleFeed>,
    frame: Res<ViewFrame>,
    data: Res<sim::GameData>,
    selected: Res<SelectedShip>,
    options: Res<ViewOptions>,
    time: Res<Time<Real>>,
) {
    let _span = timings.span(Part::Overlay);
    let Some(battle) = feed.battle() else {
        return;
    };
    let balance = &data.balance.battle;
    let grid = Color::srgba(0.45, 0.6, 0.85, 0.07);
    let edge = Color::srgba(0.5, 0.68, 1.0, 0.22);

    // Bezugsringe: Sie zeigen, wo die Ebene liegt und wie groß das Feld ist.
    ring(&mut gizmos, Vec3::ZERO, balance.exit_radius, edge, 160);
    let mut radius = 600.0;
    while radius < balance.exit_radius - 100.0 {
        ring(&mut gizmos, Vec3::ZERO, radius, grid, 96);
        radius += 600.0;
    }
    let extent = balance.exit_radius;
    gizmos.line(
        Vec3::new(-extent, 0.0, 0.0),
        Vec3::new(extent, 0.0, 0.0),
        grid,
    );
    gizmos.line(
        Vec3::new(0.0, 0.0, -extent),
        Vec3::new(0.0, 0.0, extent),
        grid,
    );

    // Auswahlring, leicht pulsierend.
    let chosen = selected
        .0
        .and_then(|index| {
            Some((
                battle.ships.get(index as usize)?,
                frame.position(index as usize)?,
            ))
        })
        .filter(|(ship, _)| ship.status.in_play());
    if let Some((ship, position)) = chosen {
        let pulse = 1.0 + 0.05 * (time.elapsed_secs() * 4.0).sin();
        ring(
            &mut gizmos,
            position,
            (ship.radius * 1.35).max(7.0) * pulse,
            Color::srgba(1.0, 1.0, 1.0, 0.9),
            48,
        );
    }

    if !options.overlay {
        return;
    }

    // Aufstellungsraum der Parteien.
    ring(
        &mut gizmos,
        Vec3::ZERO,
        balance.spawn_radius,
        Color::srgba(0.5, 0.68, 1.0, 0.1),
        128,
    );

    // Flottenanker: Raute am Bezugspunkt der Formation, Pfeil in ihre Blickrichtung.
    for (side, combatant) in battle.combatants.iter().enumerate() {
        // Eine Partei ohne Schiffe auf dem Feld hat keinen Anker mehr.
        let present = battle
            .ships
            .iter()
            .any(|ship| ship.side as usize == side && ship.status.can_act());
        if !present {
            continue;
        }
        let (r, g, b) = feed.side_color(side);
        let alpha = if combatant.retreating { 0.35 } else { 0.8 };
        let color = Color::srgba(r, g, b, alpha);
        let anchor = to_world(combatant.anchor);
        let rotation = heading_rotation(combatant.facing);
        let forward = rotation * Vec3::X;
        let across = rotation * Vec3::Z;
        let size = 38.0;
        gizmos.lineloop(
            [
                anchor + forward * size,
                anchor + across * size * 0.6,
                anchor - forward * size,
                anchor - across * size * 0.6,
            ],
            color,
        );
        let tip = anchor + forward * 190.0;
        gizmos.line(anchor + forward * size, tip, color);
        gizmos.line(tip, tip - forward * 30.0 + across * 16.0, color);
        gizmos.line(tip, tip - forward * 30.0 - across * 16.0, color);
    }

    // Gewähltes Schiff: Reichweite der Hauptbewaffnung und Linie zum Ziel.
    if let Some((ship, position)) = chosen {
        let (r, g, b) = feed.side_color(ship.side as usize);
        ring(
            &mut gizmos,
            position,
            ship.main_range,
            Color::srgba(r, g, b, 0.28),
            96,
        );
        let target = ship.target.and_then(|target| {
            let other = battle.ships.get(target as usize)?;
            other
                .status
                .in_play()
                .then(|| frame.position(target as usize).map(|at| (other, at)))
                .flatten()
        });
        if let Some((other, at)) = target {
            let aim = Color::srgba(1.0, 0.72, 0.3, 0.75);
            gizmos.line(position, at, aim);
            ring(&mut gizmos, at, (other.radius * 1.25).max(6.0), aim, 32);
        }
    }
}
