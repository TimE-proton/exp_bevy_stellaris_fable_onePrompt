//! Verkehr auf der Karte: Frachter und Flotten bewegen sich sichtbar entlang der Routen.

use crate::map::{MapCamera, Selection};
use crate::state::{AppState, ViewState};
use bevy::prelude::*;
use sim::clock::{SimClock, TICKS_PER_DAY};
use sim::fleet::Fleets;
use sim::galaxy::Galaxy;
use sim::ids::SystemId;
use sim::logistics::Logistics;
use sim::polity::Polities;

pub struct TrafficPlugin;

impl Plugin for TrafficPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            draw_traffic
                .run_if(in_state(AppState::InGame))
                .run_if(in_state(ViewState::Galaxy)),
        );
    }
}

/// Position auf der Karte: im System oder anteilig auf der Route zum nächsten System.
/// `day_fraction` glättet die Bewegung zwischen zwei Tagesschritten.
pub fn position_on_route(
    galaxy: &Galaxy,
    system: SystemId,
    next: Option<SystemId>,
    progress: f32,
    speed: f32,
    day_fraction: f32,
) -> Vec2 {
    let from = Vec2::from(galaxy.system(system).pos);
    let Some(next) = next else {
        return from;
    };
    let to = Vec2::from(galaxy.system(next).pos);
    let length = from.distance(to).max(1.0);
    let t = (progress + speed / length * day_fraction).clamp(0.0, 1.0);
    // Leicht versetzt neben der Route, damit Gegenverkehr nicht übereinander liegt.
    let side = (to - from).perp().normalize_or_zero() * -2.2;
    from.lerp(to, t) + side
}

fn draw_traffic(
    mut gizmos: Gizmos,
    data: Res<sim::GameData>,
    clock: Res<SimClock>,
    galaxy: Res<Galaxy>,
    polities: Res<Polities>,
    logistics: Res<Logistics>,
    fleets: Res<Fleets>,
    selection: Res<Selection>,
    projection: Single<&Projection, With<MapCamera>>,
) {
    let scale = match &**projection {
        Projection::Orthographic(ortho) => ortho.scale,
        _ => 1.0,
    };
    let day_fraction = (clock.tick % TICKS_PER_DAY) as f32 / TICKS_PER_DAY as f32;
    let color_of = |polity: sim::ids::PolityId| {
        let (r, g, b) = polities.get(polity).color;
        Color::srgb(r, g, b)
    };

    // Frachter: kleine Punkte, beladen heller. Aus großer Entfernung ausgeblendet.
    if scale < 2.2 {
        let radius = (1.4 * scale.max(0.5)).min(2.4);
        for freighter in logistics
            .freighters
            .iter()
            .filter(|f| f.alive && !f.route.is_empty())
        {
            let house = &logistics.houses[freighter.house.index()];
            let at = position_on_route(
                &galaxy,
                freighter.system,
                freighter.route.first().copied(),
                freighter.progress,
                data.balance.logistics.speed,
                day_fraction,
            );
            let alpha = if freighter.cargo.is_some() {
                0.95
            } else {
                0.35
            };
            gizmos
                .circle_2d(at, radius, color_of(house.polity).with_alpha(alpha))
                .resolution(6);
        }
    }

    // Flotten: Dreiecke in der Farbe ihrer Gemeinschaft, im Gefecht rot umrandet.
    for fleet in fleets.list.iter().filter(|fleet| fleet.alive) {
        let moving = !fleet.route.is_empty() && !fleet.in_battle;
        let at = position_on_route(
            &galaxy,
            fleet.system,
            fleet.route.first().copied(),
            fleet.progress,
            data.balance.fleets.speed,
            if moving { day_fraction } else { 0.0 },
        ) + if fleet.route.is_empty() {
            Vec2::new(11.0, 9.0)
        } else {
            Vec2::ZERO
        };
        let size = 6.5 * scale.clamp(0.6, 2.0);
        let color = color_of(fleet.owner);
        for shrink in [1.0, 0.72, 0.44] {
            let s = size * shrink;
            gizmos.linestrip_2d(
                [
                    at + Vec2::new(0.0, s),
                    at + Vec2::new(-s * 0.85, -s * 0.6),
                    at + Vec2::new(s * 0.85, -s * 0.6),
                    at + Vec2::new(0.0, s),
                ],
                color,
            );
        }
        if fleet.in_battle {
            gizmos.circle_2d(at, size * 1.7, Color::srgb(1.0, 0.25, 0.2));
        }
        if let Some(target) = fleet.route.last()
            && selection.system == Some(*target)
        {
            let to = Vec2::from(galaxy.system(*target).pos);
            gizmos.line_2d(at, to, color.with_alpha(0.35));
        }
    }

    // Blockierte Systeme: doppelter roter Ring.
    for system in &fleets.blockaded {
        let pos = Vec2::from(galaxy.system(*system).pos);
        let red = Color::srgb(1.0, 0.2, 0.15);
        gizmos.circle_2d(pos, 15.0, red);
        gizmos.circle_2d(pos, 17.5, red);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim::state::{SimConfig, SimState};

    #[test]
    fn route_position_interpolates_between_systems() {
        let data = sim::GameData::load_workspace().unwrap();
        let mut state = SimState::generate(
            &data,
            SimConfig {
                seed: 2,
                player: None,
            },
        );
        state.galaxy.rebuild_index();
        let galaxy = &state.galaxy;
        let lane = &galaxy.lanes[0];
        let (a, b) = (
            Vec2::from(galaxy.system(lane.a).pos),
            Vec2::from(galaxy.system(lane.b).pos),
        );
        assert_eq!(position_on_route(galaxy, lane.a, None, 0.0, 10.0, 0.0), a);
        let start = position_on_route(galaxy, lane.a, Some(lane.b), 0.0, 10.0, 0.0);
        let middle = position_on_route(galaxy, lane.a, Some(lane.b), 0.5, 10.0, 0.0);
        let end = position_on_route(galaxy, lane.a, Some(lane.b), 1.0, 10.0, 0.0);
        assert!(start.distance(a) < 3.0 && end.distance(b) < 3.0);
        assert!((middle.distance(a) - middle.distance(b)).abs() < 0.5);
        // Innerhalb eines Tages rückt die Anzeige weiter vor.
        let later = position_on_route(galaxy, lane.a, Some(lane.b), 0.5, 10.0, 0.9);
        assert!(later.distance(b) < middle.distance(b));
    }
}
