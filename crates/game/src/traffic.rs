//! Verkehr auf der Karte: Frachter und Flotten bewegen sich sichtbar entlang der Routen.

use crate::map::{MapCamera, Selection};
use crate::state::{AppState, ViewState};
use crate::ui::fleets::FleetState;
use bevy::prelude::*;
use sim::clock::{SimClock, TICKS_PER_DAY};
use sim::finance::{Finance, Stage};
use sim::fleet::{Battles, Fleets};
use sim::galaxy::Galaxy;
use sim::ids::{FleetId, PolityId, SystemId};
use sim::logistics::Logistics;
use sim::polity::Polities;
use sim::tech::Research;

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

/// Anteil des laufenden Tages, der schon vergangen ist (0..=1), aus der Zahl der bisher
/// gerechneten Takte.
///
/// Der Kern setzt Flotten und Frachter im ersten Takt eines Tages um ihre ganze
/// Tagesstrecke weiter. Danach steht in ihnen der Ort zu Tagesbeginn, und die Karte
/// zeichnet die Strecke dieses Tages nach. Ist der letzte Takt des Tages gerechnet, steht
/// die Anzeige am Ende der Tagesstrecke und damit genau dort, wo der nächste Takt den
/// Zustand hinsetzt: Die Bewegung läuft ohne Sprung über die Tagesgrenze.
pub fn day_fraction(tick: u64) -> f32 {
    if tick == 0 {
        // Noch kein Takt gerechnet: Die Partie steht an ihrem Anfang.
        return 0.0;
    }
    ((tick - 1) % TICKS_PER_DAY + 1) as f32 / TICKS_PER_DAY as f32
}

/// Position auf der Karte: im System oder anteilig auf der Route zum nächsten System.
/// `step` ist die Strecke, die an diesem Tag zurückgelegt wird; `day_fraction` sagt, wie
/// viel davon schon gefahren ist.
pub fn position_on_route(
    galaxy: &Galaxy,
    system: SystemId,
    next: Option<SystemId>,
    progress: f32,
    step: f32,
    day_fraction: f32,
) -> Vec2 {
    let from = Vec2::from(galaxy.system(system).pos);
    let Some(next) = next else {
        return from;
    };
    let to = Vec2::from(galaxy.system(next).pos);
    // Die Länge einer Route ist der Abstand ihrer beiden Systeme.
    let length = from.distance(to).max(1.0);
    let t = (progress + step / length * day_fraction).clamp(0.0, 1.0);
    // Leicht versetzt neben der Route, damit Gegenverkehr nicht übereinander liegt.
    let side = (to - from).perp().normalize_or_zero() * -2.2;
    from.lerp(to, t) + side
}

/// Was die Karte von einer Flotte zeigt: wo sie steht und was sie gerade tut.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FleetMarker {
    pub fleet: FleetId,
    pub owner: PolityId,
    pub at: Vec2,
    pub state: FleetState,
    /// Ziel des Marschs, solange die Flotte eine Route hat.
    pub target: Option<SystemId>,
}

/// Kleinster Abstand zweier Flotten, die im selben System liegen.
const BERTH_GAP: f32 = 15.0;

/// Die Markierungen aller Flotten. Wer marschiert, steht auf seiner Route, um die
/// Strecke vorgerückt, die er heute wirklich schafft ([`sim::fleet::Fleet::day_range`]:
/// Antriebstechnik hebt sie, ein leerer Tank drosselt sie). Wer in einem System liegt
/// oder dort kämpft, steht neben dem Stern; mehrere Flotten im Abstand `gap`
/// nebeneinander, damit im Gefecht beide Seiten zu sehen sind.
pub fn fleet_markers(
    data: &sim::GameData,
    galaxy: &Galaxy,
    fleets: &Fleets,
    battles: &Battles,
    research: &Research,
    day_fraction: f32,
    gap: f32,
) -> Vec<FleetMarker> {
    let mut berths = vec![0u16; galaxy.systems.len()];
    fleets
        .list
        .iter()
        .filter(|fleet| fleet.alive)
        .map(|fleet| {
            // Im Gefecht ruht der Marsch, auch wenn die Flotte noch eine Route trägt.
            let under_way = !fleet.route.is_empty() && !fleet.in_battle;
            let at = if under_way {
                position_on_route(
                    galaxy,
                    fleet.system,
                    fleet.route.first().copied(),
                    fleet.progress,
                    fleet.day_range(data, research),
                    day_fraction,
                )
            } else {
                let berth = &mut berths[fleet.system.index()];
                let offset = Vec2::new(11.0 + gap * *berth as f32, 9.0);
                *berth += 1;
                Vec2::from(galaxy.system(fleet.system).pos) + offset
            };
            FleetMarker {
                fleet: fleet.id,
                owner: fleet.owner,
                at,
                state: FleetState::of(fleet, battles),
                target: fleet.route.last().copied(),
            }
        })
        .collect()
}

fn draw_traffic(
    mut gizmos: Gizmos,
    data: Res<sim::GameData>,
    clock: Res<SimClock>,
    galaxy: Res<Galaxy>,
    polities: Res<Polities>,
    logistics: Res<Logistics>,
    fleets: Res<Fleets>,
    battles: Res<Battles>,
    research: Res<Research>,
    finance: Res<Finance>,
    selection: Res<Selection>,
    projection: Single<&Projection, With<MapCamera>>,
) {
    let scale = match &**projection {
        Projection::Orthographic(ortho) => ortho.scale,
        _ => 1.0,
    };
    let day_fraction = day_fraction(clock.tick);
    let color_of = |polity: PolityId| {
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
                // Die Fahrt der Frachter folgt der Technik ihrer Gemeinschaft.
                logistics.freighter_speed(house.polity, data.balance.logistics.speed),
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

    // Flotten: Dreiecke in der Farbe ihrer Gemeinschaft. Im Gefecht rot umrandet; wer sich
    // nach einem Abzug sammelt, ist blasser und trägt einen offenen gelben Bogen.
    let size = 6.5 * scale.clamp(0.6, 2.0);
    // Aus der Ferne sind die Markierungen größer; dann rücken sie auch weiter auseinander.
    let gap = (size * 2.6).max(BERTH_GAP);
    for marker in fleet_markers(
        &data,
        &galaxy,
        &fleets,
        &battles,
        &research,
        day_fraction,
        gap,
    ) {
        let at = marker.at;
        let regrouping = matches!(marker.state, FleetState::Regrouping { .. });
        let color = color_of(marker.owner).with_alpha(if regrouping { 0.55 } else { 1.0 });
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
        if marker.state.in_battle() {
            gizmos.circle_2d(at, size * 1.7, Color::srgb(1.0, 0.25, 0.2));
        } else if regrouping {
            gizmos.arc_2d(
                Isometry2d::new(at, Rot2::degrees(-45.0)),
                std::f32::consts::PI * 1.5,
                size * 1.7,
                Color::srgb(0.95, 0.78, 0.25),
            );
        }
        if let Some(target) = marker.target
            && selection.system == Some(target)
        {
            let to = Vec2::from(galaxy.system(target).pos);
            gizmos.line_2d(at, to, color.with_alpha(0.35));
        }
    }

    // Unruhe: Ein gelber Ring markiert Systeme mit unruhigen, ein orangeroter solche mit
    // autonomen Welten.
    for system in &galaxy.systems {
        let worst = system
            .planets
            .iter()
            .filter(|planet| galaxy.planet(**planet).owner.is_some())
            .map(|planet| finance.region(*planet).stage)
            .max();
        let color = match worst {
            Some(Stage::Restless) => Color::srgb(0.95, 0.78, 0.25),
            Some(Stage::Autonomous) => Color::srgb(1.0, 0.45, 0.15),
            _ => continue,
        };
        gizmos
            .circle_2d(Vec2::from(system.pos), 12.5, color)
            .resolution(24);
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
    use sim::command::SimCommand;
    use sim::harness::Headless;
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

    #[test]
    fn the_day_runs_from_its_first_tick_to_its_last_without_a_gap() {
        assert_eq!(day_fraction(0), 0.0);
        // Nach dem ersten Takt eines Tages ist ein Zwanzigstel vergangen, nach dem letzten
        // der ganze Tag.
        assert_eq!(day_fraction(1), 1.0 / TICKS_PER_DAY as f32);
        assert_eq!(day_fraction(TICKS_PER_DAY), 1.0);
        assert_eq!(day_fraction(TICKS_PER_DAY + 1), 1.0 / TICKS_PER_DAY as f32);
        for tick in 1..3 * TICKS_PER_DAY {
            let (now, next) = (day_fraction(tick), day_fraction(tick + 1));
            assert!(now > 0.0 && now <= 1.0);
            // Stetig bis zum Tagesende, dann beginnt der nächste Tag.
            assert!(next > now || (now == 1.0 && next < 0.1), "Takt {tick}");
        }
    }

    /// Die Markierungen der Flotten für den Zustand einer Partie ohne Fenster.
    fn markers_of(sim: &Headless) -> Vec<FleetMarker> {
        let world = sim.world();
        fleet_markers(
            world.resource::<sim::GameData>(),
            world.resource::<Galaxy>(),
            world.resource::<Fleets>(),
            world.resource::<Battles>(),
            world.resource::<Research>(),
            day_fraction(world.resource::<SimClock>().tick),
            BERTH_GAP,
        )
    }

    /// Eine Partie, in der die Flotte der ersten Gemeinschaft über drei Routen in ein
    /// anderes System marschiert; der Marschbefehl ist angenommen, der erste Tag gerechnet.
    fn marching(data: &sim::GameData) -> Headless {
        let mut sim = Headless::new(
            data.clone(),
            SimConfig {
                seed: 3,
                player: Some(PolityId(0)),
            },
        );
        let home = sim.world().resource::<Fleets>().list[0].system;
        let target = {
            let galaxy = sim.galaxy();
            galaxy
                .systems
                .iter()
                .map(|system| system.id)
                .find(|system| {
                    galaxy
                        .shortest_path(home, *system, |_| true)
                        .is_some_and(|path| path.len() == 4)
                })
                .expect("ein System drei Sprünge entfernt")
        };
        sim.issue(
            PolityId(0),
            SimCommand::MoveFleet {
                fleet: FleetId(0),
                to: target,
            },
        );
        sim.tick();
        let fleet = &sim.world().resource::<Fleets>().list[0];
        assert!(fleet.route.len() >= 2, "ein Weg über mehrere Routen");
        // Der Tank reicht für den ganzen Weg: Die Fahrt bleibt ungedrosselt.
        assert!(way_left(&sim) * data.balance.military.fuel_per_distance < 0.9);
        sim
    }

    /// Weg, den die Flotte bis zu ihrem Ziel noch vor sich hat, vom Stand des Kerns aus.
    fn way_left(sim: &Headless) -> f32 {
        let galaxy = sim.galaxy();
        let fleet = &sim.world().resource::<Fleets>().list[0];
        let mut path = vec![fleet.system];
        path.extend(&fleet.route);
        let first = galaxy.path_length(&path[..2.min(path.len())]);
        galaxy.path_length(&path) - fleet.progress * first
    }

    #[test]
    fn a_marching_fleet_advances_by_its_real_day_range_and_never_jumps_back() {
        let data = sim::GameData::load_workspace().unwrap();
        let mut sim = marching(&data);
        let (target, lanes) = {
            let fleet = &sim.world().resource::<Fleets>().list[0];
            (*fleet.route.last().unwrap(), fleet.route.len())
        };
        let goal = Vec2::from(sim.galaxy().system(target).pos);
        let range = {
            let world = sim.world();
            world.resource::<Fleets>().list[0].day_range(&data, world.resource::<Research>())
        };
        // Die Startflotte hat volle Tanks: Sie läuft die Fahrt ihrer Gemeinschaft.
        assert_eq!(
            range,
            sim::fleet::map_speed(&data, sim.research(), PolityId(0))
        );
        let per_tick = range / TICKS_PER_DAY as f32;
        // Was die Anzeige bis zum Ziel noch zu zeichnen hat: den Rest des Wegs, abzüglich
        // des Stücks, das sie dem Stand des Kerns an diesem Tag schon voraus ist.
        let ahead = way_left(&sim) - per_tick;

        let mut last = markers_of(&sim)[0].at;
        let mut drawn = 0.0f32;
        let mut arrived = None;
        for _ in 0..TICKS_PER_DAY * 400 {
            sim.tick();
            let tick = sim.clock().tick;
            let marker = markers_of(&sim)[0];
            if sim.world().resource::<Fleets>().list[0].route.is_empty() {
                arrived = Some(tick);
                // Am Ziel liegt die Flotte neben dem Stern.
                assert!(marker.at.distance(goal) < 20.0);
                assert_eq!(marker.state, FleetState::Stationed);
                break;
            }
            assert_eq!(marker.state, FleetState::Marching);
            assert_eq!(marker.target, Some(target));
            let step = marker.at.distance(last);
            // Je Takt ein Zwanzigstel der Tagesstrecke, dazu an einem System der kleine
            // Schwenk auf die nächste Route: nie ein Satz nach vorn oder zurück.
            assert!(
                step <= per_tick + 5.0,
                "Takt {tick}: Sprung um {step} bei {per_tick} je Takt"
            );
            drawn += step;
            last = marker.at;
        }
        let arrived = arrived.expect("die Flotte kommt an");
        // Die Anzeige hat genau den Weg gezeichnet, den die Flotte marschiert ist …
        assert!(
            (drawn - ahead).abs() < 5.0 * lanes as f32,
            "{drawn} gezeichnet, {ahead} marschiert"
        );
        // … in so vielen Tagen, wie die Tagesstrecke verlangt (an jedem System verfällt
        // der Rest eines Tages).
        let days = arrived as f32 / TICKS_PER_DAY as f32;
        let needed = (ahead + per_tick) / range;
        assert!(
            days >= needed && days <= needed + lanes as f32 + 1.0,
            "{days} Tage für {ahead} bei {range} je Tag"
        );
    }

    #[test]
    fn better_propulsion_moves_the_marker_further_each_day() {
        let data = sim::GameData::load_workspace().unwrap();
        let mut slow = marching(&data);
        let mut fast = marching(&data);
        // Dieselbe Partie, einmal mit der nächsten Antriebstechnik.
        let boost = (0..data.techs.len() as u16)
            .find(|tech| {
                !fast.research().knows(PolityId(0), *tech)
                    && data.techs[*tech as usize]
                        .effects
                        .iter()
                        .any(|effect| matches!(effect, sim::data::TechEffect::FleetSpeed(_)))
            })
            .expect("eine Technologie für die Fahrt der Flotten");
        fast.world_mut()
            .resource_mut::<Research>()
            .grant(&data, PolityId(0), boost);
        let start = markers_of(&slow)[0].at;
        for sim in [&mut slow, &mut fast] {
            for _ in 0..TICKS_PER_DAY / 2 {
                sim.tick();
            }
        }
        let (plain, boosted) = (markers_of(&slow)[0].at, markers_of(&fast)[0].at);
        assert!(
            boosted.distance(start) > plain.distance(start) * 1.05,
            "{plain:?} ohne, {boosted:?} mit besserem Antrieb"
        );
    }

    #[test]
    fn fleets_in_one_system_stand_side_by_side_and_show_battle_and_regrouping() {
        let data = sim::GameData::load_workspace().unwrap();
        let mut state = SimState::generate(
            &data,
            SimConfig {
                seed: 3,
                player: Some(PolityId(0)),
            },
        );
        state.galaxy.rebuild_index();
        // Die Flotte der zweiten Gemeinschaft liegt im System der ersten; die dritte
        // sammelt sich nach einem Abzug.
        let home = state.fleets.list[0].system;
        state.fleets.list[1].system = home;
        state.fleets.list[2].regroup_until = Some(90);
        let markers = |state: &SimState| {
            fleet_markers(
                &data,
                &state.galaxy,
                &state.fleets,
                &state.battles,
                &state.research,
                0.5,
                BERTH_GAP,
            )
        };
        let shown = markers(&state);
        assert_eq!(shown.len(), state.fleets.list.len());
        assert!(shown[0].at.distance(shown[1].at) >= BERTH_GAP - 0.01);
        assert_eq!(shown[0].state, FleetState::Stationed);
        assert_eq!(shown[2].state, FleetState::Regrouping { until: 90 });
        // Im Gefecht: Der Kern führt es an der Flotte; die Karte zeigt es und lässt die
        // Flotte am Ort, auch wenn sie noch eine Route trägt.
        let neighbor = state.galaxy.neighbors(home)[0].system;
        state.fleets.list[0].in_battle = true;
        state.fleets.list[0].route = vec![neighbor];
        let shown = markers(&state);
        assert!(shown[0].state.in_battle());
        assert!(
            shown[0]
                .at
                .distance(Vec2::from(state.galaxy.system(home).pos))
                < 20.0
        );
        assert_ne!(shown[0].at, shown[1].at);
    }
}
