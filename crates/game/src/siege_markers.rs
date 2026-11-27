//! Marken der Belagerungen auf der Galaxiekarte.
//!
//! Um jedes System mit einer Belagerung, die der Spieler sehen darf, liegt ein Ring aus
//! fünf Abschnitten: So viele leuchten, wie die Belagerung Phasen erreicht hat, in der
//! Farbe der Phase. Ruht sie (Orbit umstritten), ist der Ring grau und gestrichelt. Der
//! Ring liegt außerhalb des doppelten roten Rings der Blockade; beides bleibt lesbar. An
//! der Marke bietet die Oberfläche einen Knopf an, der das Fenster „Belagerung“ öffnet.

use crate::map::{MapCamera, PLANET_LOD_SCALE, planet_map_position};
use crate::state::{AppState, ViewState};
use crate::ui::UiState;
use crate::ui::siege::{RESTING, may_see};
use bevy::prelude::*;
use sim::data::Rgb;
use sim::fleet::Fleets;
use sim::galaxy::Galaxy;
use sim::ids::PlanetId;
use sim::polity::Polities;
use sim::siege::SiegeCard;
use sim::state::SimConfig;
use std::f32::consts::TAU;

/// Halbmesser des Rings um das System, außerhalb der Ringe für Unruhe und Blockade.
const RING_RADIUS: f32 = 21.5;
/// Farbe einer ruhenden Belagerung.
pub const RESTING_COLOR: Rgb = (0.66, 0.67, 0.72);

/// Farbe der Phase 1 bis 5: kühl im Anmarsch, heiß im Bodenkrieg, violett im Untergrund.
pub fn phase_color(phase_number: u8) -> Rgb {
    match phase_number {
        0 | 1 => (0.55, 0.78, 1.0),
        2 => (0.96, 0.86, 0.36),
        3 => (1.0, 0.62, 0.2),
        4 => (1.0, 0.36, 0.3),
        _ => (0.82, 0.5, 0.98),
    }
}

/// Eine Belagerung, wie die Karte sie markiert.
#[derive(Debug, Clone, PartialEq)]
pub struct SiegeMarker {
    pub planet: PlanetId,
    pub planet_name: String,
    pub system_name: String,
    /// Ort des Systems im Fenster (logische Pixel) und der wievielte Knopf an diesem
    /// System es ist (mehrere Welten eines Systems können belagert sein).
    pub at: Vec2,
    pub row: u32,
    pub phase_number: u8,
    pub phase_label: &'static str,
    /// Die Belagerung ruht: Der Orbit ist umstritten.
    pub resting: bool,
    /// Der Fall ist entschieden, die Übergabe steht aus.
    pub decided: bool,
    pub blockaded: bool,
    pub attacker: String,
    pub defender: String,
}

impl SiegeMarker {
    /// Farbe der Marke: die der Phase, grau, wenn die Belagerung ruht.
    pub fn color(&self) -> Rgb {
        if self.resting {
            RESTING_COLOR
        } else {
            phase_color(self.phase_number)
        }
    }

    /// Beschriftung des Knopfs an der Marke: Nummer und Name der Phase, bei einer
    /// ruhenden Belagerung mit dem Zusatz „ruht“.
    pub fn label(&self) -> String {
        if self.resting {
            format!("{} {} · ruht", self.phase_number, self.phase_label)
        } else {
            format!("{} {}", self.phase_number, self.phase_label)
        }
    }

    /// Schwebetext: die Belagerung in einem Satz und die Legende der Marke.
    pub fn tooltip(&self) -> String {
        let state = if self.decided {
            "entschieden, die Übergabe steht aus".to_string()
        } else if self.resting {
            RESTING.to_string()
        } else {
            "läuft".to_string()
        };
        let blockade = if self.blockaded {
            " Das System ist gesperrt (doppelter roter Ring)."
        } else {
            ""
        };
        format!(
            "Belagerung von {} im System {}: {} belagert {}. Phase {} von 5 ({}), {state}.\
             {blockade}\n\nMarke auf der Karte: Ring aus fünf Abschnitten um das System; so \
             viele leuchten, wie Phasen erreicht sind (1 Anmarsch, 2 Einschließung, \
             3 Beschuss, 4 Bodenkrieg, 5 Untergrund). Grau und gestrichelt: Die Belagerung \
             ruht, der Orbit ist umstritten.\n\nEin Klick öffnet das Fenster „Belagerung“ \
             bei dieser Welt.",
            self.planet_name,
            self.system_name,
            self.attacker,
            self.defender,
            self.phase_number,
            self.phase_label
        )
    }
}

/// Wo auf dem Bildschirm die Belagerungen stehen, die der Spieler sehen darf.
#[derive(Resource, Default, Debug, Clone, PartialEq)]
pub struct SiegeMarkers(pub Vec<SiegeMarker>);

/// Die Marke zu einer Karte des Kerns, noch ohne Ort im Fenster.
pub fn marker_of(galaxy: &Galaxy, polities: &Polities, card: &SiegeCard) -> SiegeMarker {
    SiegeMarker {
        planet: card.planet,
        planet_name: card.planet_name.clone(),
        system_name: galaxy.system(card.system).name.clone(),
        at: Vec2::ZERO,
        row: 0,
        phase_number: card.phase_number,
        phase_label: card.phase_label,
        resting: !card.control && card.handover.is_none(),
        decided: card.handover.is_some(),
        blockaded: card.blockaded,
        attacker: polities.get(card.attacker).name.clone(),
        defender: polities.get(card.defender).name.clone(),
    }
}

/// Die Belagerungen, die der Spieler sehen darf, in der Reihenfolge des Kerns.
fn seen<'a>(
    cards: &'a [SiegeCard],
    galaxy: &'a Galaxy,
    fleets: &'a Fleets,
    config: &'a SimConfig,
) -> impl Iterator<Item = &'a SiegeCard> {
    cards.iter().filter(move |card| {
        may_see(
            galaxy,
            fleets,
            config.player,
            card.attacker,
            card.defender,
            card.system,
        )
    })
}

/// Führt nach, wo die Marken der sichtbaren Belagerungen auf der Karte stehen. Die Karten
/// des Kerns liest die Oberfläche einmal je Tag und Lage; hier werden sie nur verortet.
fn locate(
    camera: Single<(&Camera, &GlobalTransform), With<MapCamera>>,
    ui_state: Res<UiState>,
    galaxy: Res<Galaxy>,
    polities: Res<Polities>,
    fleets: Res<Fleets>,
    config: Res<SimConfig>,
    mut markers: ResMut<SiegeMarkers>,
) {
    let (camera, global) = *camera;
    let mut found: Vec<SiegeMarker> = Vec::new();
    for card in seen(ui_state.siege.cache.cards(), &galaxy, &fleets, &config) {
        let system = galaxy.system(card.system);
        let Ok(at) = camera.world_to_viewport(global, Vec3::new(system.pos[0], system.pos[1], 0.0))
        else {
            continue;
        };
        let mut marker = marker_of(&galaxy, &polities, card);
        marker.at = at;
        marker.row = found
            .iter()
            .filter(|other| other.system_name == marker.system_name)
            .count() as u32;
        found.push(marker);
    }
    if markers.0 != found {
        markers.0 = found;
    }
}

fn clear(mut markers: ResMut<SiegeMarkers>) {
    markers.0.clear();
}

/// Die fünf Abschnitte des Rings: Anfangs- und Endwinkel, von oben im Uhrzeigersinn.
pub fn ring_segments() -> [(f32, f32); 5] {
    let span = TAU / 5.0;
    let gap = 0.16;
    [0, 1, 2, 3, 4].map(|index| {
        let start = TAU / 4.0 - index as f32 * span - gap / 2.0;
        (start, start - (span - gap))
    })
}

fn arc(gizmos: &mut Gizmos, center: Vec2, radius: f32, from: f32, to: f32, color: Color) {
    let steps = 8;
    let points = (0..=steps).map(|step| {
        let angle = from + (to - from) * step as f32 / steps as f32;
        center + Vec2::from_angle(angle) * radius
    });
    gizmos.linestrip_2d(points, color);
}

/// Zeichnet die Ringe um die Systeme und, aus der Nähe, einen Ring um die belagerte Welt.
fn draw(
    mut gizmos: Gizmos,
    ui_state: Res<UiState>,
    galaxy: Res<Galaxy>,
    fleets: Res<Fleets>,
    config: Res<SimConfig>,
    projection: Single<&Projection, With<MapCamera>>,
    time: Res<Time<Real>>,
) {
    let planets_visible = match &**projection {
        Projection::Orthographic(ortho) => ortho.scale < PLANET_LOD_SCALE,
        _ => true,
    };
    let pulse = 0.75 + 0.25 * (time.elapsed_secs() * 3.0).sin();
    let srgb = |color: Rgb, alpha: f32| Color::srgba(color.0, color.1, color.2, alpha);
    let mut rings = 0u32;
    let mut last_system = None;
    for card in seen(ui_state.siege.cache.cards(), &galaxy, &fleets, &config) {
        // Mehrere Belagerungen in einem System: je ein Ring, nach außen gestaffelt.
        rings = if last_system == Some(card.system) {
            rings + 1
        } else {
            0
        };
        last_system = Some(card.system);
        let center = Vec2::from(galaxy.system(card.system).pos);
        let radius = RING_RADIUS + 3.5 * rings as f32;
        let resting = !card.control && card.handover.is_none();
        let lit = if resting {
            srgb(RESTING_COLOR, 0.9)
        } else {
            srgb(phase_color(card.phase_number), pulse)
        };
        for (index, (from, to)) in ring_segments().into_iter().enumerate() {
            if index >= card.phase_number as usize {
                arc(
                    &mut gizmos,
                    center,
                    radius,
                    from,
                    to,
                    Color::srgba(0.5, 0.52, 0.6, 0.28),
                );
            } else if resting {
                // Gestrichelt: drei kurze Stücke je Abschnitt.
                for piece in 0..3 {
                    let step = (to - from) / 5.0;
                    let start = from + step * (piece as f32 * 2.0);
                    arc(&mut gizmos, center, radius, start, start + step, lit);
                }
            } else {
                arc(&mut gizmos, center, radius, from, to, lit);
                arc(&mut gizmos, center, radius + 0.9, from, to, lit);
            }
        }
        if planets_visible {
            gizmos
                .circle_2d(planet_map_position(&galaxy, card.planet), 3.4, lit)
                .resolution(20);
        }
    }
}

pub struct SiegeMarkersPlugin;

impl Plugin for SiegeMarkersPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SiegeMarkers>()
            .add_systems(OnExit(ViewState::Galaxy), clear)
            .add_systems(OnExit(AppState::InGame), clear)
            .add_systems(
                Update,
                (locate, draw)
                    .run_if(in_state(ViewState::Galaxy))
                    .run_if(in_state(AppState::InGame)),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::FocusSystem;
    use crate::testing::{TestGame, conquest};
    use crate::ui::siege as window;
    use sim::ids::PolityId;
    use sim::siege::{SiegeRefs, siege_cards};

    fn marker() -> SiegeMarker {
        let (data, state) = conquest::state(conquest::BOMBARDMENT);
        let cards = siege_cards(&SiegeRefs::of_state(state, data), state.clock.day());
        marker_of(&state.galaxy, &state.polities, &cards[0])
    }

    #[test]
    fn every_phase_and_a_resting_siege_look_different() {
        let mut colors: Vec<Rgb> = (1..=5).map(phase_color).collect();
        colors.push(RESTING_COLOR);
        for (index, color) in colors.iter().enumerate() {
            for other in &colors[index + 1..] {
                let apart = (color.0 - other.0).abs()
                    + (color.1 - other.1).abs()
                    + (color.2 - other.2).abs();
                assert!(
                    apart > 0.3,
                    "{color:?} und {other:?} sind kaum zu unterscheiden"
                );
            }
        }
        // Fünf Abschnitte, die einander nicht berühren und zusammen fast den Kreis füllen.
        let segments = ring_segments();
        let covered: f32 = segments.iter().map(|(from, to)| from - to).sum();
        assert!(covered > TAU * 0.8 && covered < TAU);
        for pair in segments.windows(2) {
            assert!(pair[0].1 > pair[1].0, "Lücke zwischen den Abschnitten");
        }
    }

    #[test]
    fn the_marker_names_phase_state_and_legend() {
        let mut marker = marker();
        assert_eq!(marker.phase_number, 3);
        assert_eq!(marker.label(), "3 Beschuss");
        assert_eq!(marker.color(), phase_color(3));
        assert!(!marker.resting);
        let tip = marker.tooltip();
        for expected in [
            "Phase 3 von 5 (Beschuss), läuft.",
            &marker.planet_name,
            &marker.attacker,
            &marker.defender,
            "Ring aus fünf Abschnitten",
            "Fenster „Belagerung“",
        ] {
            assert!(tip.contains(expected), "„{expected}“ fehlt in: {tip}");
        }
        assert_eq!(
            tip.contains("gesperrt"),
            marker.blockaded,
            "die Sperre des Systems steht dabei, wenn es gesperrt ist"
        );
        // Ruht die Belagerung, sagen es Knopf, Farbe und Schwebetext.
        marker.resting = true;
        assert_eq!(marker.label(), "3 Beschuss · ruht");
        assert_eq!(marker.color(), RESTING_COLOR);
        assert!(marker.tooltip().contains(RESTING));
    }

    #[test]
    fn the_map_marks_the_siege_and_a_click_on_the_marker_opens_it() {
        let path = conquest::save_file(conquest::BOMBARDMENT, Some(PolityId(0)), "marke");
        let mut game = TestGame::start(&format!("--load {}", path.display()));
        game.frames(6);
        let _ = std::fs::remove_file(&path);
        let expected = marker();
        let system = {
            let galaxy = game.resource::<Galaxy>();
            galaxy.planet(expected.planet).system
        };
        // Die Karte zum belagerten System führen: Dort steht die Marke im freien Teil.
        game.send(FocusSystem(system));
        game.frames(30);
        let markers = game.resource::<SiegeMarkers>().0.clone();
        assert_eq!(markers.len(), 1, "{markers:?}");
        let found = &markers[0];
        assert_eq!(found.planet, expected.planet);
        assert_eq!((found.phase_number, found.resting), (3, false));
        assert!(found.at.x > 0.0 && found.at.y > 0.0, "{found:?}");
        assert!(game.window_rect(window::TITLE).is_none());
        game.click_on(&format!("{}: {}", window::TITLE, found.planet_name));
        game.frames(3);
        assert!(game.window_rect(window::TITLE).is_some());
        assert_eq!(
            game.resource::<UiState>().siege.chosen,
            Some(expected.planet)
        );
        // Wer nichts mit der Belagerung zu tun hat und weder Flotte noch Welt im System
        // hat, sieht auch keine Marke.
        let bystander = {
            let (_, state) = conquest::state(conquest::BOMBARDMENT);
            state
                .polities
                .list
                .iter()
                .map(|polity| polity.id)
                .find(|id| {
                    !may_see(
                        &state.galaxy,
                        &state.fleets,
                        Some(*id),
                        PolityId(0),
                        state
                            .galaxy
                            .planet(expected.planet)
                            .owner
                            .expect("Besitzer"),
                        system,
                    )
                })
                .expect("eine unbeteiligte Gemeinschaft")
        };
        let path = conquest::save_file(conquest::BOMBARDMENT, Some(bystander), "marke_fremd");
        let mut game = TestGame::start(&format!("--load {}", path.display()));
        game.frames(6);
        let _ = std::fs::remove_file(&path);
        game.send(FocusSystem(system));
        game.frames(30);
        assert!(game.resource::<SiegeMarkers>().0.is_empty());
    }
}
