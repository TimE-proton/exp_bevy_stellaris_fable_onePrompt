//! Das ganze Spiel ohne Fenster und Grafikkarte, für Systemtests.
//!
//! Dieselben Plugins wie im Programm, nur ohne Fenstersystem und ohne Renderer. Eingaben
//! kommen als die Nachrichten herein, die sonst das Fenstersystem schickt; die Oberfläche
//! läuft in jedem Frame wirklich durch. So prüfen die Tests das Zusammenspiel der Systeme
//! im echten Zeitplan und nicht einzelne Funktionen an ihm vorbei.

use crate::args::Args;
use crate::state::AppState;
use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input::mouse::{MouseButtonInput, MouseMotion, MouseScrollUnit, MouseWheel};
use bevy::input::touch::TouchPhase;
use bevy::log::LogPlugin;
use bevy::prelude::*;
use bevy::render::RenderPlugin;
use bevy::render::settings::WgpuSettings;
use bevy::time::TimeUpdateStrategy;
use bevy::window::{CursorMoved, ExitCondition, PrimaryWindow};
use bevy::winit::WinitPlugin;
use bevy_egui::{EguiContext, EguiPlugin, PrimaryEguiContext, egui};
use std::time::Duration;

/// Größe des gedachten Fensters in logischen Pixeln.
pub const WINDOW: Vec2 = Vec2::new(1600.0, 900.0);
/// Echtzeit, die je Frame vergeht.
pub const FRAME: Duration = Duration::from_millis(20);

pub struct TestGame {
    pub app: App,
}

impl TestGame {
    /// Startet das Spiel wie von der Kommandozeile, etwa `"--practice --seed 3"`.
    pub fn start(command_line: &str) -> Self {
        let args = Args::parse(command_line.split_whitespace().map(String::from))
            .expect("gültige Kommandozeile");
        let data = sim::GameData::load_workspace().expect("Spieldaten des Workspace");
        let mut app = App::new();
        app.add_plugins(
            DefaultPlugins
                .build()
                .disable::<WinitPlugin>()
                // Protokoll und Abbruchsignal gibt es je Prozess nur einmal; die Tests
                // teilen sich einen Prozess.
                .disable::<LogPlugin>()
                .disable::<bevy::app::TerminalCtrlCHandlerPlugin>()
                .set(RenderPlugin {
                    render_creation: WgpuSettings {
                        backends: None,
                        ..default()
                    }
                    .into(),
                    ..default()
                })
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        resolution: (WINDOW.x as u32, WINDOW.y as u32).into(),
                        ..default()
                    }),
                    exit_condition: ExitCondition::DontExit,
                    ..default()
                }),
        )
        .add_plugins(EguiPlugin::default())
        // Jedes Frame dauert gleich lang: Die Tests hängen nicht an der Uhr des Rechners.
        .insert_resource(TimeUpdateStrategy::ManualDuration(FRAME));
        crate::add_game(&mut app, data, args);
        app.finish();
        app.cleanup();
        let mut game = Self { app };
        // Die ersten Frames legen Kameras und Oberfläche an und führen ins Hauptmenü.
        game.frames(3);
        game
    }

    pub fn frames(&mut self, count: usize) {
        for _ in 0..count {
            self.app.update();
        }
    }

    /// Lässt Frames laufen, bis `done` zutrifft; bricht nach `limit` Frames ab.
    pub fn frames_until(&mut self, limit: usize, mut done: impl FnMut(&mut World) -> bool) {
        for _ in 0..limit {
            if done(self.app.world_mut()) {
                return;
            }
            self.app.update();
        }
        assert!(
            done(self.app.world_mut()),
            "nach {limit} Frames nicht eingetreten"
        );
    }

    pub fn world(&self) -> &World {
        self.app.world()
    }

    pub fn world_mut(&mut self) -> &mut World {
        self.app.world_mut()
    }

    pub fn state(&self) -> AppState {
        *self.world().resource::<State<AppState>>().get()
    }

    pub fn resource<R: Resource>(&self) -> &R {
        self.world().resource::<R>()
    }

    /// Zahl der Entities mit der Komponente `C`.
    pub fn count<C: Component>(&mut self) -> usize {
        let world = self.world_mut();
        world.query::<&C>().iter(world).count()
    }

    pub fn send<M: Message>(&mut self, message: M) {
        self.world_mut().write_message(message);
    }

    fn window(&mut self) -> Entity {
        let world = self.world_mut();
        world
            .query_filtered::<Entity, With<PrimaryWindow>>()
            .single(world)
            .expect("Hauptfenster")
    }

    /// Bewegt den Mauszeiger an eine Stelle des Fensters (logische Pixel).
    pub fn move_pointer(&mut self, to: Vec2) {
        let window = self.window();
        let world = self.world_mut();
        let mut entity = world.entity_mut(window);
        let mut state = entity.get_mut::<Window>().expect("Fenster");
        let from = state.cursor_position();
        state.set_cursor_position(Some(to));
        world.write_message(CursorMoved {
            window,
            position: to,
            delta: from.map(|from| to - from),
        });
        if let Some(from) = from {
            world.write_message(MouseMotion { delta: to - from });
        }
    }

    pub fn press(&mut self, button: MouseButton) {
        let window = self.window();
        self.send(MouseButtonInput {
            button,
            state: ButtonState::Pressed,
            window,
        });
    }

    pub fn release(&mut self, button: MouseButton) {
        let window = self.window();
        self.send(MouseButtonInput {
            button,
            state: ButtonState::Released,
            window,
        });
    }

    /// Ein Klick, wie ihn ein Mensch macht: Zeiger hinbewegen, drücken, loslassen, jeweils
    /// in eigenen Frames.
    pub fn click(&mut self, at: Vec2) {
        self.move_pointer(at);
        self.frames(2);
        self.press(MouseButton::Left);
        self.frames(1);
        self.release(MouseButton::Left);
        self.frames(2);
    }

    /// Ein Klick ohne Anlauf: Zeigerbewegung und Tastendruck treffen im selben Frame ein
    /// (wie bei sehr schnellen Klicks oder langsamen Frames).
    pub fn click_at_once(&mut self, at: Vec2) {
        self.move_pointer(at);
        self.press(MouseButton::Left);
        self.frames(1);
        self.release(MouseButton::Left);
        self.frames(2);
    }

    /// Zieht mit gedrückter Taste von `from` um `by`, in mehreren Schritten.
    pub fn drag(&mut self, button: MouseButton, from: Vec2, by: Vec2) {
        self.move_pointer(from);
        self.frames(2);
        self.press(button);
        self.frames(1);
        for step in 1..=4 {
            self.move_pointer(from + by * (step as f32 / 4.0));
            self.frames(1);
        }
        self.release(button);
        self.frames(2);
    }

    /// Dreht das Mausrad um `lines` Rasten an der Stelle `at`.
    pub fn scroll(&mut self, at: Vec2, lines: f32) {
        self.move_pointer(at);
        self.frames(2);
        let window = self.window();
        self.send(MouseWheel {
            unit: MouseScrollUnit::Line,
            x: 0.0,
            y: lines,
            window,
            phase: TouchPhase::Moved,
        });
        self.frames(2);
    }

    /// Drückt eine Taste und lässt sie im nächsten Frame wieder los.
    pub fn key(&mut self, key_code: KeyCode) {
        self.key_with(&[], key_code);
    }

    /// Wie [`TestGame::key`], mit gehaltenen Zusatztasten.
    pub fn key_with(&mut self, held: &[KeyCode], key_code: KeyCode) {
        let window = self.window();
        let message = |key_code: KeyCode, state: ButtonState| KeyboardInput {
            key_code,
            logical_key: Key::Unidentified(bevy::input::keyboard::NativeKey::Unidentified),
            state,
            text: None,
            repeat: false,
            window,
        };
        for key in held {
            self.send(message(*key, ButtonState::Pressed));
        }
        self.frames(1);
        self.send(message(key_code, ButtonState::Pressed));
        self.frames(1);
        self.send(message(key_code, ButtonState::Released));
        for key in held {
            self.send(message(*key, ButtonState::Released));
        }
        self.frames(2);
    }

    /// Der Kontext der Oberfläche, etwa um nach einem Frame Fenster und Flächen abzufragen.
    pub fn egui(&mut self) -> egui::Context {
        let world = self.world_mut();
        world
            .query_filtered::<&mut EguiContext, With<PrimaryEguiContext>>()
            .single_mut(world)
            .expect("Kontext der Oberfläche")
            .get_mut()
            .clone()
    }

    /// Rechteck eines Fensters der Oberfläche (nach seinem Titel), wie es zuletzt gezeichnet
    /// wurde.
    pub fn window_rect(&mut self, title: &str) -> Option<egui::Rect> {
        let ctx = self.egui();
        // egui bildet die Kennung eines Fensters aus seinem Titel als `Option<&str>`.
        let id = egui::Id::new(Some(title));
        ctx.memory(|memory| {
            memory
                .areas()
                .is_visible(&egui::LayerId::new(egui::Order::Middle, id))
                .then(|| memory.area_rect(id))
                .flatten()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_game_reaches_the_main_menu_without_a_window() {
        let mut game = TestGame::start("");
        assert_eq!(game.state(), AppState::MainMenu);
        // Das Hauptmenü ist wirklich gezeichnet worden.
        assert!(game.window_rect("hauptmenue").is_some());
    }
}
