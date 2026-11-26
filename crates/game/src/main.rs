//! KONFLUENZ – Bevy-App. Zeigt den Simulationskern an und schickt ihm Befehle.

mod args;
mod battle_view;
mod history;
mod map;
mod pointer;
mod selftest;
mod session;
mod state;
#[cfg(test)]
mod testing;
mod time_control;
mod traffic;
mod ui;

use bevy::log::{DEFAULT_FILTER, LogPlugin};
use bevy::prelude::*;
use bevy::window::PresentMode;
use bevy_egui::EguiPlugin;

fn main() -> AppExit {
    let args = match args::Args::parse(std::env::args().skip(1)) {
        Ok(args) => args,
        Err(message) => {
            eprintln!("{message}\n\n{}", args::USAGE);
            return AppExit::error();
        }
    };
    if args.help {
        println!("{}", args::USAGE);
        return AppExit::Success;
    }
    let data = match sim::GameData::load_located(args.data_dir.as_deref()) {
        Ok(data) => data,
        Err(error) => {
            eprintln!("Fehler in den Spieldaten: {error}");
            return AppExit::error();
        }
    };

    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: "KONFLUENZ".into(),
                    resolution: (1600, 900).into(),
                    present_mode: PresentMode::AutoVsync,
                    ..default()
                }),
                ..default()
            })
            .set(LogPlugin {
                // Die Freiflugsteuerung der Engine meldet beim Start ihre Tastenbelegung auf
                // Englisch; die gültige steht in der Hilfe des Spiels.
                filter: format!("{DEFAULT_FILTER},bevy_camera_controller=warn"),
                ..default()
            }),
    )
    .add_plugins(EguiPlugin::default());
    add_game(&mut app, data, args);
    app.run()
}

/// Das Spiel selbst, ohne Engine und Fenster. Die Systemtests setzen es auf eine Engine
/// ohne Grafikkarte; so prüfen sie dieselben Plugins in derselben Reihenfolge.
fn add_game(app: &mut App, data: sim::GameData, args: args::Args) {
    app.insert_resource(ClearColor(Color::srgb(0.015, 0.018, 0.03)))
        .insert_resource(data)
        .insert_resource(args)
        // Der Kern: dieselbe Plugin-Gruppe wie im Headless-Programm.
        .add_plugins(sim::SimPlugins)
        .add_plugins((
            state::StatePlugin,
            time_control::TimeControlPlugin,
            pointer::PointerPlugin,
            session::SessionPlugin,
            map::GalaxyViewPlugin,
            traffic::TrafficPlugin,
            history::HistoryPlugin,
            battle_view::BattleViewPlugin,
            ui::UiPlugin,
            selftest::SelftestPlugin,
        ));
}
