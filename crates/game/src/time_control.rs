//! Pausierbare Echtzeit mit Geschwindigkeitsstufen.
//!
//! Die Simulation läuft in `FixedUpdate` mit festen 20 Takten je Sekunde Spielzeit.
//! Schneller oder langsamer wird nur die virtuelle Uhr; die Rechnung je Takt bleibt
//! dieselbe, der Verlauf also auch.

use crate::state::AppState;
use bevy::prelude::*;
use bevy_egui::input::egui_wants_any_keyboard_input;
use std::time::Duration;

/// Wählbare Stufen als Vielfaches der Echtzeit (1× = ein Tag je Sekunde).
pub const SPEEDS: [f32; 6] = [0.5, 1.0, 2.0, 5.0, 10.0, 25.0];
pub const SPEED_LABELS: [&str; 6] = ["½×", "1×", "2×", "5×", "10×", "25×"];

#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct SimSpeed {
    pub paused: bool,
    pub level: usize,
}

impl Default for SimSpeed {
    fn default() -> Self {
        Self {
            paused: true,
            level: 1,
        }
    }
}

impl SimSpeed {
    pub fn factor(&self) -> f32 {
        SPEEDS[self.level.min(SPEEDS.len() - 1)]
    }

    pub fn faster(&mut self) {
        self.level = (self.level + 1).min(SPEEDS.len() - 1);
    }

    pub fn slower(&mut self) {
        self.level = self.level.saturating_sub(1);
    }
}

pub struct TimeControlPlugin;

impl Plugin for TimeControlPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Time::<Fixed>::from_hz(sim::TICKS_PER_SECOND as f64))
            .init_resource::<SimSpeed>()
            .add_systems(
                Update,
                (
                    keyboard
                        .run_if(in_state(AppState::InGame))
                        .run_if(not(egui_wants_any_keyboard_input)),
                    apply_speed,
                )
                    .chain(),
            );
    }
}

fn keyboard(keys: Res<ButtonInput<KeyCode>>, mut speed: ResMut<SimSpeed>) {
    if keys.just_pressed(KeyCode::Space) {
        speed.paused = !speed.paused;
    }
    if keys.any_just_pressed([KeyCode::NumpadAdd, KeyCode::Equal, KeyCode::BracketRight]) {
        speed.faster();
    }
    if keys.any_just_pressed([KeyCode::NumpadSubtract, KeyCode::Minus, KeyCode::Slash]) {
        speed.slower();
    }
    for (level, key) in [
        KeyCode::Digit1,
        KeyCode::Digit2,
        KeyCode::Digit3,
        KeyCode::Digit4,
        KeyCode::Digit5,
        KeyCode::Digit6,
    ]
    .into_iter()
    .enumerate()
    {
        if keys.just_pressed(key) {
            speed.level = level;
            speed.paused = false;
        }
    }
}

/// Überträgt die gewählte Stufe auf die virtuelle Uhr.
fn apply_speed(speed: Res<SimSpeed>, mut time: ResMut<Time<Virtual>>) {
    if !speed.is_changed() {
        return;
    }
    let factor = speed.factor();
    time.set_relative_speed(factor);
    // Bevy kappt die *skalierte* Zeit je Frame bei `max_delta`. Der Deckel wächst deshalb
    // mit der Stufe (hohe Stufen brauchen viele Takte je Frame), bleibt aber endlich: Ein
    // Ruckler kostet dann Spielzeit statt eine Lawine nachzuholender Takte auszulösen.
    time.set_max_delta(Duration::from_secs_f32(max_delta_seconds(factor)));
    if speed.paused {
        time.pause();
    } else {
        time.unpause();
    }
}

/// Höchste Spielzeit, die ein einzelnes Frame nachholen darf.
fn max_delta_seconds(factor: f32) -> f32 {
    (factor * 0.05).max(0.25)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speed_levels_clamp() {
        let mut speed = SimSpeed::default();
        for _ in 0..20 {
            speed.faster();
        }
        assert_eq!(speed.factor(), 25.0);
        for _ in 0..20 {
            speed.slower();
        }
        assert_eq!(speed.factor(), 0.5);
        assert_eq!(SPEEDS.len(), SPEED_LABELS.len());
    }

    #[test]
    fn every_level_fits_into_the_frame_budget() {
        // Bei 60 Bildern je Sekunde muss jede Stufe ungekappt durchlaufen.
        for factor in SPEEDS {
            assert!(factor / 60.0 <= max_delta_seconds(factor));
        }
    }
}
