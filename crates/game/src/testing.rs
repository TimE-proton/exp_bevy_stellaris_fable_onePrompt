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
use bevy::window::{
    CursorGrabMode, CursorMoved, CursorOptions, ExitCondition, PrimaryWindow, WindowEvent,
    WindowResized,
};
use bevy::winit::WinitPlugin;
use bevy_egui::egui::accesskit::{Node, Role};
use bevy_egui::{EguiContext, EguiOutput, EguiPlugin, PrimaryEguiContext, egui};
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
        game.frames(2);
        // Die Oberfläche führt ihren Baum der Bedienelemente mit, wie für Vorleseprogramme
        // (das Fenstersystem schaltet ihn sonst ein). Daran finden die Tests Knöpfe nach
        // ihrer Beschriftung.
        game.egui().enable_accesskit();
        game.frames(1);
        game
    }

    pub fn frames(&mut self, count: usize) {
        for _ in 0..count {
            self.app.update();
        }
    }

    /// Lässt Frames laufen, bis das Spiel sich beenden will, und liefert, womit. Bricht
    /// nach `limit` Frames ab.
    pub fn run_to_exit(&mut self, limit: usize) -> AppExit {
        for _ in 0..limit {
            self.app.update();
            if let Some(exit) = self.app.should_exit() {
                return exit;
            }
        }
        panic!("nach {limit} Frames hat sich das Spiel nicht beendet");
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

    /// Schickt eine Eingabe so, wie das Fenstersystem sie schickt: einmal als eigene
    /// Nachricht (daraus liest die Engine Tasten und Mausknöpfe) und einmal im Sammelstrom
    /// aller Fensterereignisse (daraus liest die Oberfläche).
    fn input<M: Message + Clone + Into<WindowEvent>>(&mut self, message: M) {
        self.send(message.clone());
        self.send::<WindowEvent>(message.into());
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
        let mut entity = self.world_mut().entity_mut(window);
        let mut state = entity.get_mut::<Window>().expect("Fenster");
        let from = state.cursor_position();
        state.set_cursor_position(Some(to));
        self.input(CursorMoved {
            window,
            position: to,
            delta: from.map(|from| to - from),
        });
        if let Some(from) = from {
            // Rohe Mausbewegung, aus der die Engine Züge und das Umsehen liest.
            self.send(MouseMotion { delta: to - from });
        }
    }

    pub fn press(&mut self, button: MouseButton) {
        let window = self.window();
        self.input(MouseButtonInput {
            button,
            state: ButtonState::Pressed,
            window,
        });
    }

    pub fn release(&mut self, button: MouseButton) {
        let window = self.window();
        self.input(MouseButtonInput {
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
        self.wheel(lines);
        self.frames(2);
    }

    /// Dreht das Mausrad um `lines` Rasten, wo der Zeiger gerade steht.
    pub fn wheel(&mut self, lines: f32) {
        let window = self.window();
        self.input(MouseWheel {
            unit: MouseScrollUnit::Line,
            x: 0.0,
            y: lines,
            window,
            phase: TouchPhase::Moved,
        });
    }

    /// Drückt eine Taste und lässt sie im nächsten Frame wieder los.
    pub fn key(&mut self, key_code: KeyCode) {
        self.key_with(&[], key_code);
    }

    /// Wie [`TestGame::key`], mit gehaltenen Zusatztasten.
    pub fn key_with(&mut self, held: &[KeyCode], key_code: KeyCode) {
        for key in held {
            self.key_event(*key, ButtonState::Pressed);
        }
        self.frames(1);
        self.key_event(key_code, ButtonState::Pressed);
        self.frames(1);
        self.key_event(key_code, ButtonState::Released);
        for key in held {
            self.key_event(*key, ButtonState::Released);
        }
        self.frames(2);
    }

    /// Eine Taste geht nieder oder wird losgelassen.
    pub fn key_event(&mut self, key_code: KeyCode, state: ButtonState) {
        let window = self.window();
        self.input(KeyboardInput {
            key_code,
            logical_key: Key::Unidentified(bevy::input::keyboard::NativeKey::Unidentified),
            state,
            text: None,
            repeat: false,
            window,
        });
    }

    /// Zieht das Fenster auf eine andere Größe (logische Pixel), wie das Fenstersystem es
    /// meldet.
    pub fn resize(&mut self, size: Vec2) {
        let window = self.window();
        let mut entity = self.world_mut().entity_mut(window);
        entity
            .get_mut::<Window>()
            .expect("Fenster")
            .resolution
            .set(size.x, size.y);
        self.input(WindowResized {
            window,
            width: size.x,
            height: size.y,
        });
    }

    /// Ist der Mauszeiger gefangen (Umsehen in der Schlacht)?
    pub fn pointer_grabbed(&mut self) -> bool {
        let window = self.window();
        self.world()
            .get::<CursorOptions>(window)
            .is_some_and(|options| options.grab_mode != CursorGrabMode::None)
    }

    /// Mitte des ersten Bedienelements, auf das `wanted` zutrifft, wie es zuletzt gezeichnet
    /// wurde.
    fn element(&mut self, wanted: impl Fn(&Node) -> bool) -> Option<Vec2> {
        let world = self.world_mut();
        let output = world
            .query_filtered::<&EguiOutput, With<PrimaryEguiContext>>()
            .single(world)
            .ok()?;
        let tree = output.platform_output.accesskit_update.as_ref()?;
        tree.nodes
            .iter()
            .map(|(_, node)| node)
            .find(|node| wanted(node))
            .and_then(|node| node.bounds())
            .map(|bounds| {
                Vec2::new(
                    ((bounds.x0 + bounds.x1) * 0.5) as f32,
                    ((bounds.y0 + bounds.y1) * 0.5) as f32,
                )
            })
    }

    /// Mitte des Knopfs mit genau dieser Beschriftung, wie er zuletzt gezeichnet wurde;
    /// `None`, wenn es keinen gibt oder er gesperrt ist.
    pub fn button(&mut self, label: &str) -> Option<Vec2> {
        self.element(|node| {
            node.role() == Role::Button && node.label() == Some(label) && !node.is_disabled()
        })
    }

    /// Mitte des ersten Textes mit genau diesem Wortlaut, wie er zuletzt gezeichnet wurde.
    /// Auch Zeilen einer Liste, die gerade aus dem sichtbaren Teil gerollt sind, haben ihren
    /// Ort: An ihm lässt sich ablesen, wie weit die Liste gerollt ist.
    pub fn text(&mut self, text: &str) -> Option<Vec2> {
        self.element(|node| node.role() == Role::Label && node.value() == Some(text))
    }

    /// Alle Texte der Oberfläche mit ihrem Ort, wie sie zuletzt gezeichnet wurden.
    pub fn labels(&mut self) -> Vec<(String, egui::Rect)> {
        let world = self.world_mut();
        let Ok(output) = world
            .query_filtered::<&EguiOutput, With<PrimaryEguiContext>>()
            .single(world)
        else {
            return Vec::new();
        };
        let Some(tree) = output.platform_output.accesskit_update.as_ref() else {
            return Vec::new();
        };
        tree.nodes
            .iter()
            .map(|(_, node)| node)
            .filter(|node| node.role() == Role::Label)
            .filter_map(|node| {
                let bounds = node.bounds()?;
                let rect = egui::Rect::from_min_max(
                    egui::pos2(bounds.x0 as f32, bounds.y0 as f32),
                    egui::pos2(bounds.x1 as f32, bounds.y1 as f32),
                );
                Some((node.value()?.to_string(), rect))
            })
            .collect()
    }

    /// Klickt auf den Knopf mit dieser Beschriftung.
    pub fn click_on(&mut self, label: &str) {
        let at = self
            .button(label)
            .unwrap_or_else(|| panic!("kein Knopf „{label}“ zu sehen"));
        self.click(at);
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

/// Größe der Galaxie, die ein Seed ergibt: Systeme und Planeten.
fn galaxy_size(data: &sim::GameData, seed: u64) -> (usize, usize) {
    let galaxy = sim::galaxy::generate_galaxy(data, seed);
    (galaxy.systems.len(), galaxy.planets.len())
}

/// Zwei Seeds, deren Galaxien verschieden groß sind: der kleinste und der größte unter
/// den ersten acht. Wer prüft, was beim Wechsel der Partie mit Kennungen der alten
/// geschieht, braucht genau das: Kennungen der großen liegen außerhalb der kleinen.
pub fn small_and_large_galaxy(data: &sim::GameData) -> (u64, u64) {
    let sizes: Vec<(u64, (usize, usize))> = (1..=8)
        .map(|seed| (seed, galaxy_size(data, seed)))
        .collect();
    let (small, least) = *sizes.iter().min_by_key(|(_, size)| *size).unwrap();
    let (large, most) = *sizes.iter().max_by_key(|(_, size)| *size).unwrap();
    assert!(
        least.0 < most.0 && least.1 < most.1,
        "die Galaxien der Seeds unterscheiden sich nicht mehr in Systemen und Planeten: {sizes:?}"
    );
    (small, large)
}

/// Schreibt den Spielstand einer frischen Partie mit diesem Seed in eine Datei, die nur
/// dieser Testlauf kennt, und liefert ihren Pfad. Wer sie anlegt, räumt sie wieder ab.
pub fn save_of_new_game(data: &sim::GameData, seed: u64, name: &str) -> std::path::PathBuf {
    use sim::state::{SAVE_FORMAT, SaveGame, SimConfig, SimState};
    let path =
        std::env::temp_dir().join(format!("konfluenz_test_{}_{name}.ron", std::process::id()));
    let state = SimState::generate(
        data,
        SimConfig {
            seed,
            player: Some(sim::ids::PolityId(0)),
        },
    );
    SaveGame {
        format: SAVE_FORMAT,
        data_hash: data.hash,
        state,
    }
    .write(&path)
    .expect("Spielstand schreiben");
    path
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
