//! KONFLUENZ – Bevy-App. Zeigt den Simulationskern an und schickt ihm Befehle.

mod args;
mod history;
mod map;
mod selftest;
mod session;
mod state;
mod time_control;
mod traffic;
mod ui;

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

    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "KONFLUENZ".into(),
                resolution: (1600, 900).into(),
                present_mode: PresentMode::AutoVsync,
                ..default()
            }),
            ..default()
        }))
        .add_plugins(EguiPlugin::default())
        .insert_resource(ClearColor(Color::srgb(0.015, 0.018, 0.03)))
        .insert_resource(data)
        .insert_resource(args)
        // Der Kern: dieselbe Plugin-Gruppe wie im Headless-Programm.
        .add_plugins(sim::SimPlugins)
        .add_plugins((
            state::StatePlugin,
            time_control::TimeControlPlugin,
            session::SessionPlugin,
            map::GalaxyViewPlugin,
            traffic::TrafficPlugin,
            history::HistoryPlugin,
            ui::UiPlugin,
            selftest::SelftestPlugin,
        ))
        .run()
}
