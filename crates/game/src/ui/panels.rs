//! Die einzelnen Fenster und Leisten.

use super::format::{decimal, percent, population};
use super::{LeftTab, UiState, economy, finance, fleets};
use crate::history::PriceHistory;
use crate::map::Selection;
use crate::session::{Notice, SessionRequest, quicksave_path};
use crate::time_control::{SPEED_LABELS, SimSpeed};
use bevy_egui::egui::{self, Align, Align2, Color32, Layout, RichText};
use sim::chronicle::{Chronicle, ChronicleKind};
use sim::clock::{SimClock, date_of_tick};
use sim::command::SimCommand;
use sim::data::{GameData, Rgb};
use sim::diplomacy::Diplomacy;
use sim::economy::{Economy, to_credits};
use sim::finance::Finance;
use sim::fleet::Fleets;
use sim::galaxy::Galaxy;
use sim::ids::{PlanetId, SystemId};
use sim::logistics::Logistics;
use sim::polity::{Polities, can_claim};
use sim::state::SimConfig;

/// Lesender Blick auf die laufende Partie.
pub struct SimRefs<'a> {
    pub data: &'a GameData,
    pub galaxy: &'a Galaxy,
    pub polities: &'a Polities,
    pub clock: &'a SimClock,
    pub config: &'a SimConfig,
    pub chronicle: &'a Chronicle,
    pub economy: &'a Economy,
    pub finance: &'a Finance,
    pub logistics: &'a Logistics,
    pub fleets: &'a Fleets,
    pub diplomacy: &'a Diplomacy,
    pub history: &'a PriceHistory,
}

/// Was der Spieler in der Oberfläche ausgelöst hat.
#[derive(Clone, Debug)]
pub enum UiAction {
    Session(SessionRequest),
    TogglePause,
    SetSpeed(usize),
    Select(Selection),
    Focus(SystemId),
    /// Emittenten an der Börse zeigen.
    ShowIssuer(sim::ids::IssuerId),
    Command(SimCommand),
    Quit,
}

pub const ACCENT: Color32 = Color32::from_rgb(250, 180, 64);
pub const MUTED: Color32 = Color32::from_rgb(140, 150, 165);

pub fn apply_style(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = Color32::from_rgba_unmultiplied(14, 17, 26, 238);
    visuals.window_fill = Color32::from_rgba_unmultiplied(18, 22, 33, 245);
    visuals.selection.bg_fill = Color32::from_rgb(60, 90, 140);
    ctx.set_visuals(visuals);
}

pub fn color32(color: Rgb) -> Color32 {
    let byte = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    Color32::from_rgb(byte(color.0), byte(color.1), byte(color.2))
}

pub fn main_menu(
    root: &mut egui::Ui,
    state: &mut UiState,
    has_quicksave: bool,
    actions: &mut Vec<UiAction>,
) {
    egui::Window::new("hauptmenue")
        .title_bar(false)
        .resizable(false)
        .collapsible(false)
        .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
        .show(root.ctx(), |ui| {
            ui.set_width(380.0);
            ui.vertical_centered(|ui| {
                ui.add_space(10.0);
                ui.label(RichText::new("KONFLUENZ").size(40.0).strong().color(ACCENT));
                ui.label(RichText::new("Es kann nur eine Gemeinschaft geben.").italics());
                ui.add_space(18.0);
            });
            ui.horizontal(|ui| {
                ui.label("Seed");
                ui.add(egui::TextEdit::singleline(&mut state.seed_text).desired_width(140.0));
                if ui.button("Würfeln").clicked() {
                    state.seed_text = random_seed().to_string();
                }
            });
            ui.small("Gleicher Seed, gleiche Galaxie, gleicher Verlauf.");
            ui.add_space(10.0);
            let seed = state.seed_text.trim().parse::<u64>();
            ui.vertical_centered_justified(|ui| {
                if ui
                    .add_enabled(
                        seed.is_ok(),
                        egui::Button::new(RichText::new("Neue Partie").size(18.0)),
                    )
                    .on_disabled_hover_text("Der Seed muss eine ganze Zahl sein.")
                    .clicked()
                    && let Ok(seed) = seed
                {
                    actions.push(UiAction::Session(SessionRequest::NewGame { seed }));
                }
                if ui
                    .add_enabled(has_quicksave, egui::Button::new("Letzten Spielstand laden"))
                    .on_disabled_hover_text("Noch kein Spielstand vorhanden.")
                    .clicked()
                {
                    actions.push(UiAction::Session(SessionRequest::Load(quicksave_path())));
                }
                if ui
                    .button("Gefechtsübung")
                    .on_hover_text("Ein einzelnes Gefecht mit frei gewählten Flotten, in 3D.")
                    .clicked()
                {
                    state.practice_open = true;
                }
                if ui.button("Beenden").clicked() {
                    actions.push(UiAction::Quit);
                }
            });
            ui.add_space(6.0);
        });
}

fn random_seed() -> u64 {
    // Nur für den Vorschlag im Menü; die Partie selbst hängt allein am gewählten Seed.
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.subsec_nanos())
        .unwrap_or(1);
    (nanos % 1_000_000) as u64
}

pub fn in_game(
    root: &mut egui::Ui,
    sim: &SimRefs,
    state: &mut UiState,
    speed: &SimSpeed,
    selection: &Selection,
    actions: &mut Vec<UiAction>,
) {
    egui::Panel::top("kopfleiste").show(root, |ui| top_bar(ui, sim, state, speed, actions));
    egui::Panel::bottom("chronik")
        .resizable(true)
        .default_size(118.0)
        .size_range(60.0..=320.0)
        .show(root, |ui| chronicle(ui, sim));
    egui::Panel::left("auswahl")
        .resizable(true)
        .default_size(430.0)
        .size_range(300.0..=640.0)
        .show(root, |ui| left_panel(ui, sim, state, selection, actions));
    egui::Panel::right("gemeinschaften")
        .resizable(true)
        .default_size(290.0)
        .size_range(220.0..=420.0)
        .show(root, |ui| polities_panel(ui, sim, actions));

    let ctx = root.ctx().clone();
    let shown_planet = shown_planet(sim, selection);
    economy::chains_window(&ctx, sim, &mut state.chains_open, shown_planet);
    economy::flows_window(&ctx, sim, &mut state.flows_open, None);
    fleets::fleets_window(&ctx, sim, &mut state.fleets_open, selection, actions);
    finance::exchange_window(&ctx, sim, state, actions);
    finance::treasury_window(&ctx, sim, state, actions);
    if state.menu_open {
        pause_menu(root.ctx(), state, actions);
    }
    if state.help_open {
        help_window(root.ctx(), &mut state.help_open, false);
    }
}

/// Was von der Oberfläche der Partie bleibt, solange die Schlachtansicht gezeigt wird.
pub fn battle_menu(root: &mut egui::Ui, state: &mut UiState, actions: &mut Vec<UiAction>) {
    if state.menu_open {
        pause_menu(root.ctx(), state, actions);
    }
}

fn top_bar(
    ui: &mut egui::Ui,
    sim: &SimRefs,
    state: &mut UiState,
    speed: &SimSpeed,
    actions: &mut Vec<UiAction>,
) {
    ui.horizontal(|ui| {
        ui.add_space(4.0);
        match sim.config.player {
            Some(player) => {
                let polity = sim.polities.get(player);
                ui.label(
                    RichText::new(&polity.name)
                        .strong()
                        .color(color32(polity.color)),
                );
                ui.separator();
                ui.label(format!(
                    "{} Planeten · {} · Staatskasse {}",
                    sim.galaxy.planets_of(player).count(),
                    population(sim.galaxy.population_of(player)),
                    decimal(to_credits(sim.economy.treasury(player)), 0)
                ));
                if sim.diplomacy.is_at_war(player) {
                    ui.label(
                        RichText::new("IM KRIEG")
                            .color(Color32::from_rgb(235, 110, 95))
                            .strong(),
                    );
                }
            }
            None => {
                ui.label(RichText::new("Beobachter").strong());
            }
        }
        ui.separator();
        ui.label(
            RichText::new(sim.clock.date().to_string())
                .monospace()
                .strong(),
        );
        let pause_label = if speed.paused {
            "▶ Weiter"
        } else {
            "⏸ Pause"
        };
        if ui
            .selectable_label(speed.paused, pause_label)
            .on_hover_text("Leertaste")
            .clicked()
        {
            actions.push(UiAction::TogglePause);
        }
        for (level, label) in SPEED_LABELS.iter().enumerate() {
            let active = !speed.paused && speed.level == level;
            if ui
                .selectable_label(active, *label)
                .on_hover_text(format!("Taste {}", level + 1))
                .clicked()
            {
                actions.push(UiAction::SetSpeed(level));
            }
        }
        if speed.paused {
            ui.label(RichText::new("PAUSIERT").color(ACCENT).strong());
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui.button("Menü").on_hover_text("Esc").clicked() {
                state.menu_open = !state.menu_open;
            }
            if ui.button("Hilfe").on_hover_text("F1").clicked() {
                state.help_open = !state.help_open;
            }
            ui.separator();
            ui.toggle_value(&mut state.fleets_open, "Flotten");
            ui.toggle_value(&mut state.treasury_open, "Staat");
            ui.toggle_value(&mut state.exchange_open, "Börse");
            ui.toggle_value(&mut state.chains_open, "Ketten");
            ui.toggle_value(&mut state.flows_open, "Warenflüsse");
            ui.separator();
            let free = sim
                .galaxy
                .planets
                .iter()
                .filter(|planet| planet.owner.is_none())
                .count();
            ui.label(
                RichText::new(format!(
                    "{free} von {} Planeten frei",
                    sim.galaxy.planets.len()
                ))
                .color(if free == 0 { ACCENT } else { MUTED }),
            );
        });
    });
}

fn chronicle(ui: &mut egui::Ui, sim: &SimRefs) {
    ui.label(RichText::new("Chronik").strong());
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .stick_to_bottom(true)
        .show(ui, |ui| {
            for entry in &sim.chronicle.entries {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(date_of_tick(entry.tick).to_string())
                            .monospace()
                            .color(MUTED),
                    );
                    let color = match entry.polity {
                        Some(polity) => color32(sim.polities.get(polity).color),
                        None => chronicle_color(entry.kind),
                    };
                    ui.label(RichText::new(&entry.text).color(color));
                });
            }
        });
}

fn chronicle_color(kind: ChronicleKind) -> Color32 {
    match kind {
        ChronicleKind::War => Color32::from_rgb(230, 90, 80),
        ChronicleKind::End => ACCENT,
        _ => Color32::from_rgb(200, 205, 215),
    }
}

/// Die Welt, deren Einzelheiten gezeigt werden: die gewählte, sonst die erste des Systems.
fn shown_planet(sim: &SimRefs, selection: &Selection) -> Option<PlanetId> {
    let system = sim.galaxy.system(selection.system?);
    selection
        .planet
        .filter(|id| system.planets.contains(id))
        .or(system.planets.first().copied())
}

fn left_panel(
    ui: &mut egui::Ui,
    sim: &SimRefs,
    state: &mut UiState,
    selection: &Selection,
    actions: &mut Vec<UiAction>,
) {
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.selectable_value(&mut state.tab, LeftTab::Overview, "Übersicht");
        ui.selectable_value(&mut state.tab, LeftTab::Market, "Markt");
        ui.selectable_value(&mut state.tab, LeftTab::Production, "Produktion");
        ui.selectable_value(&mut state.tab, LeftTab::Ownership, "Besitz");
    });
    ui.separator();
    match (state.tab, selection.system) {
        (LeftTab::Market, Some(system)) => {
            ui.heading(&sim.galaxy.system(system).name);
            economy::market_tab(ui, sim, state, system);
        }
        (LeftTab::Production, Some(system)) => {
            planet_picker(ui, sim, system, selection, actions);
            if let Some(planet) = shown_planet(sim, selection) {
                economy::production_tab(ui, sim, planet);
            }
        }
        (LeftTab::Ownership, Some(system)) => {
            planet_picker(ui, sim, system, selection, actions);
            if let Some(planet) = shown_planet(sim, selection) {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        finance::ownership_tab(ui, sim, planet, actions);
                    });
            }
        }
        _ => selection_panel(ui, sim, selection, actions),
    }
}

/// Kurze Planetenleiste für die Reiter, die sich auf eine Welt beziehen.
fn planet_picker(
    ui: &mut egui::Ui,
    sim: &SimRefs,
    system: SystemId,
    selection: &Selection,
    actions: &mut Vec<UiAction>,
) {
    let shown = shown_planet(sim, selection);
    ui.horizontal_wrapped(|ui| {
        for planet_id in &sim.galaxy.system(system).planets {
            let planet = sim.galaxy.planet(*planet_id);
            if ui
                .selectable_label(shown == Some(*planet_id), &planet.name)
                .clicked()
            {
                actions.push(UiAction::Select(Selection {
                    system: Some(system),
                    planet: Some(*planet_id),
                }));
            }
        }
    });
}

fn selection_panel(
    ui: &mut egui::Ui,
    sim: &SimRefs,
    selection: &Selection,
    actions: &mut Vec<UiAction>,
) {
    let Some(system_id) = selection.system else {
        ui.add_space(6.0);
        ui.label(RichText::new("Kein System gewählt").strong());
        ui.label("Linksklick auf ein System der Karte wählt es aus.");
        return;
    };
    let galaxy = sim.galaxy;
    let system = galaxy.system(system_id);
    let star = &sim.data.galaxy.stars[system.star as usize];
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.heading(&system.name);
        if ui.small_button("zentrieren").clicked() {
            actions.push(UiAction::Focus(system_id));
        }
    });
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new(&star.name).color(color32(star.color)));
        ui.label(format!("· Netzlage {}", percent(system.centrality)))
            .on_hover_text("Anteil der kürzesten Wege durch die Galaxie, die hier durchlaufen.");
        if system.chokepoint {
            ui.label(RichText::new("· Engpass").color(ACCENT).strong())
                .on_hover_text(
                    "Brückenkopf zwischen zwei Clustern. Wer ihn hält, kontrolliert den Durchgang.",
                );
        }
    });
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new("Routen:").color(MUTED));
        for neighbor in galaxy.neighbors(system_id) {
            let other = galaxy.system(neighbor.system);
            let lane = galaxy.lane(neighbor.lane);
            let mut text = RichText::new(&other.name);
            if lane.bridge {
                text = text.color(ACCENT);
            }
            if ui
                .link(text)
                .on_hover_text(format!("Länge {}", decimal(lane.length as f64, 0)))
                .clicked()
            {
                actions.push(UiAction::Select(Selection {
                    system: Some(neighbor.system),
                    planet: None,
                }));
                actions.push(UiAction::Focus(neighbor.system));
            }
        }
    });
    ui.separator();

    egui::Grid::new("planeten")
        .striped(true)
        .num_columns(6)
        .show(ui, |ui| {
            for title in ["Planet", "Typ", "Besitzer", "Bevölkerung", "Bewohnb.", "g"] {
                ui.label(RichText::new(title).color(MUTED).small());
            }
            ui.end_row();
            for planet_id in &system.planets {
                let planet = galaxy.planet(*planet_id);
                let kind = &sim.data.planet_kinds[planet.kind as usize];
                if ui
                    .selectable_label(selection.planet == Some(*planet_id), &planet.name)
                    .clicked()
                {
                    actions.push(UiAction::Select(Selection {
                        system: Some(system_id),
                        planet: Some(*planet_id),
                    }));
                }
                ui.label(RichText::new(&kind.name).color(color32(kind.color)));
                match planet.owner {
                    Some(owner) => {
                        let polity = sim.polities.get(owner);
                        ui.label(RichText::new(&polity.short).color(color32(polity.color)));
                    }
                    None => {
                        ui.label(RichText::new("frei").color(MUTED));
                    }
                }
                if planet.owner.is_some() {
                    ui.label(population(planet.population));
                } else {
                    ui.label(RichText::new("–").color(MUTED));
                }
                ui.label(percent(planet.habitability));
                ui.label(decimal(planet.gravity as f64, 2));
                ui.end_row();
            }
        });

    let planet_id = selection
        .planet
        .filter(|id| system.planets.contains(id))
        .or_else(|| {
            // Ohne ausdrückliche Wahl zeigt die Tafel die erste Welt des Systems.
            system.planets.first().copied()
        });
    if let Some(planet_id) = planet_id {
        ui.separator();
        planet_details(ui, sim, planet_id, actions);
    }
}

fn planet_details(
    ui: &mut egui::Ui,
    sim: &SimRefs,
    planet_id: PlanetId,
    actions: &mut Vec<UiAction>,
) {
    let planet = sim.galaxy.planet(planet_id);
    let kind = &sim.data.planet_kinds[planet.kind as usize];
    ui.label(RichText::new(format!("{} · {}", planet.name, kind.name)).strong());
    egui::Grid::new("planet_details")
        .num_columns(2)
        .show(ui, |ui| {
            let mut row = |label: &str, value: String| {
                ui.label(RichText::new(label).color(MUTED));
                ui.label(value);
                ui.end_row();
            };
            row(
                "Schwerkraft",
                format!("{} g", decimal(planet.gravity as f64, 2)),
            );
            row("Atmosphäre", planet.atmosphere.label().to_string());
            row(
                "Klima",
                format!(
                    "{} °C im Mittel, {} Wasser",
                    planet.temperature.round() as i32,
                    percent(planet.water)
                ),
            );
            row("Bewohnbarkeit", percent(planet.habitability));
            row("Tragfähigkeit", population(planet.capacity));
            if planet.owner.is_some() {
                row(
                    "Bevölkerung",
                    format!(
                        "{} ({} ausgelastet)",
                        population(planet.population),
                        percent((planet.population / planet.capacity) as f32)
                    ),
                );
            }
        });
    ui.add_space(4.0);
    ui.label(RichText::new("Vorkommen").color(MUTED));
    if planet.deposits.is_empty() {
        ui.label("keine");
    }
    egui::Grid::new("vorkommen").num_columns(3).show(ui, |ui| {
        for deposit in &planet.deposits {
            ui.label(&sim.data.good(deposit.good).name);
            ui.add(
                egui::ProgressBar::new((deposit.richness / 2.4).clamp(0.04, 1.0))
                    .desired_width(90.0)
                    .desired_height(8.0)
                    .fill(ACCENT),
            );
            ui.label(RichText::new(decimal(deposit.richness as f64, 2)).color(MUTED));
            ui.end_row();
        }
    });

    if planet.owner.is_none()
        && let Some(player) = sim.config.player
    {
        ui.add_space(8.0);
        let verdict = can_claim(
            sim.galaxy,
            sim.polities,
            &sim.data.balance.colonization,
            sim.economy.treasury(player),
            player,
            planet_id,
            sim.clock.day(),
        );
        let button = egui::Button::new(RichText::new("Kolonie gründen").strong());
        if ui.add_enabled(verdict.is_ok(), button).clicked() {
            actions.push(UiAction::Command(SimCommand::ClaimPlanet {
                planet: planet_id,
            }));
        }
        match verdict {
            Ok(()) => {
                ui.small(format!(
                    "{} Siedler ziehen von der Hauptwelt um. Die Gründung kostet {} Credits.",
                    population(sim.data.balance.colonization.settlers),
                    decimal(sim.data.balance.colonization.cost, 0)
                ));
            }
            Err(reason) => {
                ui.label(RichText::new(format!("Nicht möglich: {reason}.")).color(MUTED));
            }
        }
    }
}

fn polities_panel(ui: &mut egui::Ui, sim: &SimRefs, actions: &mut Vec<UiAction>) {
    ui.add_space(4.0);
    ui.label(RichText::new("Gemeinschaften").strong());
    let total: f64 = sim
        .galaxy
        .planets
        .iter()
        .map(|planet| planet.population)
        .sum();
    egui::Grid::new("gemeinschaften")
        .striped(true)
        .num_columns(5)
        .show(ui, |ui| {
            for title in ["", "Planeten", "Bevölkerung", "Anteil", ""] {
                ui.label(RichText::new(title).color(MUTED).small());
            }
            ui.end_row();
            for polity in &sim.polities.list {
                let mut name = RichText::new(&polity.name).color(color32(polity.color));
                if sim.config.player == Some(polity.id) {
                    name = name.strong();
                }
                if !polity.alive {
                    name = name.strikethrough();
                }
                if ui.link(name).on_hover_text(&polity.motto).clicked()
                    && let Some(capital) = polity.capital
                {
                    let system = sim.galaxy.planet(capital).system;
                    actions.push(UiAction::Select(Selection {
                        system: Some(system),
                        planet: Some(capital),
                    }));
                    actions.push(UiAction::Focus(system));
                }
                let people = sim.galaxy.population_of(polity.id);
                ui.label(sim.galaxy.planets_of(polity.id).count().to_string());
                ui.label(population(people));
                ui.label(percent((people / total.max(1.0)) as f32));
                match sim.config.player {
                    Some(player) if player != polity.id && polity.alive => {
                        if sim.diplomacy.at_war(player, polity.id) {
                            if ui
                                .small_button(RichText::new("Frieden").color(ACCENT))
                                .on_hover_text("Der Krieg endet sofort.")
                                .clicked()
                            {
                                actions.push(UiAction::Command(SimCommand::MakePeace {
                                    target: polity.id,
                                }));
                            }
                        } else if ui
                            .small_button("Krieg")
                            .on_hover_text(
                                "Krieg erklären. Erst im Krieg wirken Blockade und Kaperfahrt.",
                            )
                            .clicked()
                        {
                            actions.push(UiAction::Command(SimCommand::DeclareWar {
                                target: polity.id,
                            }));
                        }
                    }
                    _ => {
                        ui.label("");
                    }
                }
                ui.end_row();
            }
        });
}

fn pause_menu(ctx: &egui::Context, state: &mut UiState, actions: &mut Vec<UiAction>) {
    egui::Window::new("Menü")
        .collapsible(false)
        .resizable(false)
        .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.set_width(260.0);
            ui.vertical_centered_justified(|ui| {
                if ui.button("Weiter").clicked() {
                    state.menu_open = false;
                }
                if ui.button("Speichern (F5)").clicked() {
                    actions.push(UiAction::Session(SessionRequest::Save(quicksave_path())));
                }
                if ui
                    .add_enabled(quicksave_path().is_file(), egui::Button::new("Laden (F9)"))
                    .clicked()
                {
                    actions.push(UiAction::Session(SessionRequest::Load(quicksave_path())));
                }
                if ui.button("Zum Hauptmenü").clicked() {
                    actions.push(UiAction::Session(SessionRequest::QuitToMenu));
                }
                if ui.button("Spiel beenden").clicked() {
                    actions.push(UiAction::Quit);
                }
            });
        });
}

/// Tasten der Galaxiekarte.
const MAP_KEYS: [(&str, &str); 9] = [
    ("Linksklick", "System wählen"),
    ("Rechte/mittlere Maustaste ziehen", "Karte verschieben"),
    ("W A S D / Pfeile", "Karte verschieben"),
    ("Mausrad", "Zoomen"),
    ("Leertaste", "Pause"),
    ("1 – 6", "Geschwindigkeit ½× bis 25×"),
    ("+ / −", "schneller / langsamer"),
    ("F5 / F9", "Schnellspeichern / Schnellladen"),
    ("Esc", "Menü"),
];

/// Tasten der Schlachtansicht.
pub const BATTLE_KEYS: [(&str, &str); 16] = [
    ("W A S D", "Kamera vor, zurück, seitwärts"),
    ("E / Q", "Kamera steigen / sinken"),
    ("Rechte Maustaste halten", "Umsehen"),
    (
        "M",
        "Umsehen ein- und ausschalten, ohne die Maustaste zu halten",
    ),
    ("Umschalt links", "schneller fliegen"),
    ("Mausrad", "Fluggeschwindigkeit ändern"),
    (
        "Ziffernblock 7 / 1 / 3",
        "Blick von oben / von vorn / von der Seite (mit Strg: Gegenseite)",
    ),
    ("U", "Übersicht: alle Schiffe auf dem Feld im Blick"),
    ("G", "Zum Schwerpunkt des Gefechts"),
    ("Linksklick", "Schiff wählen (ins Leere: Auswahl aufheben)"),
    (
        "F",
        "Gewähltes Schiff heranholen (ohne Auswahl: Gefechtsschwerpunkt)",
    ),
    ("V", "Gewähltes Schiff verfolgen"),
    (
        "T",
        "Taktische Einblendung: Flottenanker, Reichweiten, Ziellinie",
    ),
    ("N", "Namen an nahen Schiffen"),
    (
        "Leertaste, 1 – 6, + / −",
        "Pause und Geschwindigkeit wie in der Partie",
    ),
    ("Esc", "Gefechtsübung verlassen (in der Partie: Menü)"),
];

/// Hilfefenster. In der Schlachtansicht (`battle`) zeigt es deren Bedienung.
pub fn help_window(ctx: &egui::Context, open: &mut bool, battle: bool) {
    let keys: &[(&str, &str)] = if battle { &BATTLE_KEYS } else { &MAP_KEYS };
    egui::Window::new("Hilfe")
        .open(open)
        .resizable(false)
        .show(ctx, |ui| {
            egui::Grid::new("hilfe")
                .num_columns(2)
                .striped(true)
                .show(ui, |ui| {
                    for (keys, what) in keys {
                        ui.label(RichText::new(*keys).strong());
                        ui.label(*what);
                        ui.end_row();
                    }
                });
            ui.add_space(6.0);
            if battle {
                ui.label("Kleine Markierungen stehen für Schiffe, die aus der Ferne kaum zu sehen sind:");
                ui.label("Dreieck = Jäger und Korvette, Raute = Fregatte, Pfeil = Kreuzer,");
                ui.label("Sechseck = Schlachtschiff, Rechteck = Träger. Hohl = auf dem Rückzug, grau = treibt.");
                ui.label("Ein Klick auf ein Schiff zeigt Rumpf, Schild, Panzerung und jedes Modul.");
            } else {
                ui.label("Gelbe Routen sind Brücken zwischen Clustern, Rauten markieren Engpässe.");
                ui.label("Ringe um ein System: gelb = unruhige Welt, orange = autonom, rot doppelt = blockiert.");
                ui.label("Reiter „Besitz“ zeigt, warum eine Welt zum Reich hält oder nicht.");
            }
        });
}

pub fn notice(root: &mut egui::Ui, notice: &Notice) {
    if notice.seconds_left <= 0.0 || notice.text.is_empty() {
        return;
    }
    egui::Area::new("hinweis".into())
        .anchor(Align2::CENTER_TOP, [0.0, 44.0])
        .interactable(false)
        .show(root.ctx(), |ui| {
            let color = if notice.is_error {
                Color32::from_rgb(235, 110, 95)
            } else {
                Color32::WHITE
            };
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_max_width(520.0);
                ui.label(RichText::new(&notice.text).color(color));
            });
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim::state::SimState;

    struct Fixture {
        data: GameData,
        state: SimState,
    }

    fn fixture() -> Fixture {
        let data = GameData::load_workspace().unwrap();
        let mut state = SimState::generate(
            &data,
            SimConfig {
                seed: 3,
                player: Some(sim::ids::PolityId(0)),
            },
        );
        state.galaxy.rebuild_index();
        Fixture { data, state }
    }

    impl Fixture {
        fn refs<'a>(&'a self, history: &'a PriceHistory) -> SimRefs<'a> {
            SimRefs {
                data: &self.data,
                galaxy: &self.state.galaxy,
                polities: &self.state.polities,
                clock: &self.state.clock,
                config: &self.state.config,
                chronicle: &self.state.chronicle,
                economy: &self.state.economy,
                finance: &self.state.finance,
                logistics: &self.state.logistics,
                fleets: &self.state.fleets,
                diplomacy: &self.state.diplomacy,
                history,
            }
        }
    }

    /// Zeichnet ein Frame ohne Fenster und Grafikkarte.
    fn frame(mut draw: impl FnMut(&mut egui::Ui)) {
        let ctx = egui::Context::default();
        apply_style(&ctx);
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1600.0, 900.0),
            )),
            ..Default::default()
        };
        let mut output = ctx.run_ui(input, |ui| draw(ui));
        // Ohne Renderer nimmt niemand die Schriftatlas-Textur ab; ausdrücklich verwerfen.
        output.textures_delta.clear();
    }

    #[test]
    fn main_menu_draws_without_a_window() {
        let mut state = UiState::default();
        let mut actions = Vec::new();
        frame(|ui| main_menu(ui, &mut state, false, &mut actions));
        assert!(
            actions.is_empty(),
            "ohne Eingabe darf nichts ausgelöst werden"
        );
    }

    #[test]
    fn game_screen_draws_for_every_system_and_tab() {
        let fixture = fixture();
        let mut history = PriceHistory::default();
        history.record(0, &fixture.state.economy);
        history.record(5, &fixture.state.economy);
        let refs = fixture.refs(&history);
        let speed = SimSpeed::default();
        for tab in [
            LeftTab::Overview,
            LeftTab::Market,
            LeftTab::Production,
            LeftTab::Ownership,
        ] {
            let mut state = UiState {
                menu_open: true,
                help_open: true,
                flows_open: true,
                chains_open: true,
                fleets_open: true,
                exchange_open: true,
                exchange_all: true,
                treasury_open: true,
                issuer: fixture.state.finance.issuers.keys().next().copied(),
                tab,
                ..Default::default()
            };
            for system in &fixture.state.galaxy.systems {
                for planet in std::iter::once(None).chain(system.planets.iter().copied().map(Some))
                {
                    let selection = Selection {
                        system: Some(system.id),
                        planet,
                    };
                    let mut actions = Vec::new();
                    frame(|ui| in_game(ui, &refs, &mut state, &speed, &selection, &mut actions));
                    assert!(
                        actions.is_empty(),
                        "ohne Eingabe darf nichts ausgelöst werden"
                    );
                }
            }
            let mut actions = Vec::new();
            frame(|ui| {
                in_game(
                    ui,
                    &refs,
                    &mut state,
                    &speed,
                    &Selection::default(),
                    &mut actions,
                )
            });
        }
    }

    #[test]
    fn notice_draws_and_expires() {
        let mut text = Notice::default();
        text.error("Etwas ging schief");
        frame(|ui| notice(ui, &text));
        text.seconds_left = 0.0;
        frame(|ui| notice(ui, &text));
    }
}
