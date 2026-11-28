//! Tafeln der Orbitansicht: Kopfleiste mit Zeitsteuerung und den Wegen zurück, links die
//! Phasen, Kennzahlen, Prognose und Meldungen, in der Szene Beschriftungen, dazu der
//! Hinweis auf eine ruhende Belagerung und das Fenster mit dem Ausgang.

use super::super::camera::BattleCamera;
use super::super::campaign::WATCH as WATCH_BATTLE;
use super::scene::{OrbitScene, OverviewRequest, PLANET_RADIUS, landing_direction, shield_share};
use super::{BACK, BattleAfterLeave, SiegeWorld, TITLE, TO_WINDOW, WatchedSiege};
use crate::pointer::UiCover;
use crate::state::ViewState;
use crate::time_control::SimSpeed;
use crate::ui::UiState;
use crate::ui::format::percent;
use crate::ui::panels::{self, ACCENT, MUTED, TimeClick, color32, detail as small, split_bar};
use crate::ui::siege::detail::defense_text;
use crate::ui::siege::{
    RESTING, days_text, forecast_text, phase_text, progress_of, resting_forecast, state_text, units,
};
use bevy::prelude::*;
use bevy_egui::EguiContexts;
use bevy_egui::egui::{self, Align2, Color32, RichText};
use sim::siege::{Phase, SiegeCard};

/// Überschriften und Knöpfe; Tests und README nennen sie.
pub const HEADING_NUMBERS: &str = "Lage";
pub const HEADING_EVENTS: &str = "Meldungen";
pub const RESULT_TITLE: &str = "Ausgang der Belagerung";
pub const KEEP_WATCHING: &str = "Szene weiter ansehen";
pub const OVERVIEW: &str = "Übersicht";
/// Über den Zahlen, sobald die Belagerung zu Ende ist: Sie zeigen den letzten Stand.
pub const LAST_STATE: &str = "Stand am Ende der Belagerung";

const BAD: Color32 = Color32::from_rgb(235, 110, 95);
const WARN: Color32 = Color32::from_rgb(240, 190, 80);

/// Was der Spieler in den Tafeln ausgelöst hat.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HudAction {
    Time(TimeClick),
    Back,
    ToWindow,
    WatchBattle(sim::ids::BattleId),
    Overview,
    Menu,
    Help,
    CloseResult,
}

/// Was der Tagesbericht des Kerns über den letzten Tag sagt, in einer Zeile; `None`, wenn
/// nichts geschah.
pub fn day_report(card: &SiegeCard) -> Option<String> {
    let report = &card.view.siege.report;
    let mut parts = Vec::new();
    if report.shield_absorbed > 0.0 {
        parts.push("der Schild fängt Beschuss ab".to_string());
    }
    if report.defender_bombed > 0.0 {
        parts.push(format!(
            "Beschuss kostet die Verteidiger {} Verbände",
            units(report.defender_bombed)
        ));
    }
    if report.attacker_lost > 0.0 || report.defender_lost > 0.0 {
        parts.push(format!(
            "Gefecht am Boden: Belagerer −{}, Verteidiger −{}",
            units(report.attacker_lost),
            units(report.defender_lost)
        ));
    }
    if report.attacker_stranded > 0.0 {
        parts.push(format!(
            "ohne Nachschub lösen sich {} Verbände der Landungstruppe auf",
            units(report.attacker_stranded)
        ));
    }
    if report.hull > 0.0 {
        parts.push("Gegenfeuer trifft die Flotte".to_string());
    }
    if report.fire_damage > 0.0 || report.ground_damage > 0.0 {
        parts.push(format!(
            "Schäden an der Welt: {}",
            percent((report.fire_damage + report.ground_damage) as f32)
        ));
    }
    (!parts.is_empty()).then(|| format!("Letzter Tag: {}.", parts.join("; ")))
}

/// Die fünf Phasen als schmale Leiste: Jede zeigt ihre Nummer, die laufende auch ihren
/// Namen; die Namen der anderen stehen im Schwebetext.
fn phase_bar(ui: &mut egui::Ui, card: &SiegeCard, ended: bool) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 3.0;
        for phase in [
            Phase::Approach,
            Phase::Investment,
            Phase::Bombardment,
            Phase::Ground,
            Phase::Underground,
        ] {
            let number = phase.number();
            let current = number == card.phase_number;
            let (fill, text) = if current && !ended {
                (
                    if card.control { ACCENT } else { WARN },
                    Color32::from_rgb(20, 20, 24),
                )
            } else if number <= card.phase_number {
                (
                    Color32::from_rgb(70, 78, 96),
                    Color32::from_rgb(210, 214, 222),
                )
            } else {
                (Color32::from_rgb(34, 39, 52), MUTED)
            };
            let label = if current {
                format!("{number} {}", phase.label())
            } else {
                number.to_string()
            };
            egui::Frame::new()
                .fill(fill)
                .inner_margin(egui::Margin::symmetric(8, 3))
                .corner_radius(3)
                .show(ui, |ui| {
                    ui.label(RichText::new(label).color(text).size(12.0))
                        .on_hover_text(format!("Phase {number} von 5: {}", phase.label()));
                });
        }
    });
}

fn top_bar(
    ui: &mut egui::Ui,
    world: &SiegeWorld,
    watched: &WatchedSiege,
    speed: &SimSpeed,
    actions: &mut Vec<HudAction>,
) {
    let card = &watched.card;
    // Beide Hälften der Leiste melden Klicks; gesammelt wird an einer Stelle.
    let clicks = std::cell::RefCell::new(Vec::new());
    let left = |ui: &mut egui::Ui| {
        ui.add_space(4.0);
        ui.label(RichText::new(TITLE).color(MUTED));
        ui.label(
            RichText::new(format!(
                "{} · {}",
                card.planet_name,
                world.galaxy.system(card.system).name
            ))
            .strong()
            .color(ACCENT),
        );
        ui.separator();
        ui.label(
            RichText::new(world.clock.date().to_string())
                .monospace()
                .strong(),
        );
        if let Some(click) = panels::time_controls(ui, speed) {
            clicks.borrow_mut().push(HudAction::Time(click));
        }
    };
    let right = |ui: &mut egui::Ui| {
        if ui.button("Menü").on_hover_text("Esc").clicked() {
            clicks.borrow_mut().push(HudAction::Menu);
        }
        if ui.button("Hilfe").on_hover_text("F1").clicked() {
            clicks.borrow_mut().push(HudAction::Help);
        }
        ui.separator();
        if ui
            .button(BACK)
            .on_hover_text("Verlässt die Orbitansicht. Die Belagerung läuft in der Partie weiter.")
            .clicked()
        {
            clicks.borrow_mut().push(HudAction::Back);
        }
        if ui
            .button(TO_WINDOW)
            .on_hover_text(
                "Zurück zur Karte, mit dem Fenster „Belagerung“ bei dieser Welt: Dort stehen \
                 alle Zahlen, die Prognose mit ihren Kosten und die Befehle.",
            )
            .clicked()
        {
            clicks.borrow_mut().push(HudAction::ToWindow);
        }
        if ui
            .button(OVERVIEW)
            .on_hover_text("Welt und Flotte im Blick (Taste U)")
            .clicked()
        {
            clicks.borrow_mut().push(HudAction::Overview);
        }
    };
    split_bar(ui, left, right);
    actions.extend(clicks.into_inner());
}

fn side_panel(ui: &mut egui::Ui, world: &SiegeWorld, watched: &WatchedSiege) {
    let card = &watched.card;
    let name = |polity: sim::ids::PolityId| {
        let polity = world.polities.get(polity);
        RichText::new(&polity.name).color(color32(polity.color))
    };
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                ui.label(name(card.attacker));
                ui.label(RichText::new("belagert").color(MUTED));
                ui.label(name(card.defender));
            });
            phase_bar(ui, card, watched.ended());
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(phase_text(card)).strong());
                let (text, color) = if watched.ended() {
                    ("beendet".to_string(), MUTED)
                } else if card.control || card.handover.is_some() {
                    (state_text(card), MUTED)
                } else {
                    (state_text(card), WARN)
                };
                ui.label(RichText::new(text).color(color));
            });
            ui.separator();
            ui.label(RichText::new(HEADING_NUMBERS).strong());
            if watched.ended() {
                ui.label(small(LAST_STATE).color(MUTED));
            }
            egui::Grid::new("orbit_lage").num_columns(2).show(ui, |ui| {
                let mut row = |label: &str, value: String| {
                    ui.label(RichText::new(label).color(MUTED));
                    ui.add(egui::Label::new(value).wrap());
                    ui.end_row();
                };
                row("Dauer", days_text(card.days as f64));
                row("Fortschritt", {
                    let progress = progress_of(card);
                    if card.phase == Phase::Underground {
                        progress.text
                    } else {
                        percent(progress.share)
                    }
                });
                row("Vorräte der Garnison", days_text(card.stores_days));
                row(
                    "Verteidiger",
                    format!(
                        "Miliz {} · Reserve {} · Untergrund {}",
                        units(card.troops.militia),
                        units(card.troops.reserve),
                        units(card.troops.deep)
                    ),
                );
                row(
                    "Landungstruppe",
                    if card.troops.army > 0.0 {
                        format!(
                            "{} Verbände am Boden, Nachschub {}",
                            units(card.troops.army),
                            days_text(card.army_supply_days)
                        )
                    } else {
                        format!("keine am Boden · {} an Bord", units(card.troops.embarked))
                    },
                );
                row(
                    "Flotte",
                    if watched.ships == 0 {
                        // Ohne Schiffe im Orbit gibt es keine Füllstände zu nennen.
                        fleet_text(watched)
                    } else {
                        format!(
                            "{} · Munition {} · Treibstoff {}",
                            fleet_text(watched),
                            percent(card.ammo as f32),
                            percent(card.fuel as f32)
                        )
                    },
                );
                row(
                    "Schild",
                    if card.defense.shield == 0 {
                        "keiner".to_string()
                    } else {
                        format!(
                            "Stufe {}, Ladung {}",
                            card.defense.shield,
                            percent(shield_share(&world.data, watched))
                        )
                    },
                );
            });
            ui.label(small(defense_text(card)));
            // Nach dem Ende gilt weder der Bericht des letzten Tages noch eine Prognose.
            if !watched.ended() {
                if let Some(report) = day_report(card) {
                    ui.label(small(report));
                }
                ui.separator();
                ui.label(RichText::new(crate::ui::siege::FORECAST).strong());
                if let Some(resting) = resting_forecast(card) {
                    ui.label(small(resting).color(WARN));
                } else {
                    for (course, forecast) in &card.forecasts {
                        let color = if forecast.falls {
                            Color32::from_rgb(120, 200, 140)
                        } else {
                            MUTED
                        };
                        ui.label(small(forecast_text(*course, forecast)).color(color));
                    }
                }
            }
            ui.separator();
            ui.label(RichText::new(HEADING_EVENTS).strong());
            if watched.events.is_empty() {
                ui.label(small(
                    "Noch nichts Neues, seit die Ansicht offen ist. Phasenwechsel, Verluste \
                     und Landungen erscheinen hier, sobald die Simulation sie meldet.",
                ));
            }
            for (date, text) in watched.events.iter().rev() {
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        RichText::new(date.to_string())
                            .monospace()
                            .color(MUTED)
                            .small(),
                    );
                    ui.label(small(text.clone()));
                });
            }
            ui.separator();
            ui.label(small(
                "Kamera wie in der Schlachtansicht: W A S D fliegen, E / Q steigen und sinken, \
                 rechte Maustaste halten zum Umsehen, Mausrad für das Tempo, U für die \
                 Übersicht. Die Befehle der eigenen Seite stehen im Fenster „Befehle“ unten \
                 rechts; alles Weitere zeigt das Fenster „Belagerung“ auf der Karte.",
            ));
        });
}

/// „12 Schiffe“ und, wenn die Szene im Maßstab zeigt, wie viele auf einen Rumpf kommen.
pub fn fleet_text(watched: &WatchedSiege) -> String {
    let ships = match watched.ships {
        0 => "keine Schiffe im Orbit".to_string(),
        1 => "1 Schiff".to_string(),
        count => format!("{count} Schiffe"),
    };
    if watched.layout.per_hull > 1 {
        format!(
            "{ships} (ein Rumpf in der Szene steht für {})",
            watched.layout.per_hull
        )
    } else {
        ships
    }
}

/// Ist ein Punkt der Szene von der Kamera aus zu sehen, oder liegt der Planet davor?
fn in_sight(eye: Vec3, point: Vec3) -> bool {
    let along = point - eye;
    let length = along.length();
    if length < 1.0 {
        return true;
    }
    let direction = along / length;
    // Nächster Punkt der Sichtlinie zum Mittelpunkt der Welt.
    let reach = (-eye).dot(direction).clamp(0.0, length);
    (eye + direction * reach).length() >= PLANET_RADIUS * 0.985
}

/// Beschriftungen in der Szene: Zonen, Flotte, Stationen. Sie stehen in der Ebene der
/// Szene, unter allen Tafeln und Fenstern.
fn scene_labels(
    root: &egui::Ui,
    watched: &WatchedSiege,
    scene: &OrbitScene,
    camera: (&Camera, &GlobalTransform),
) {
    let free = root.available_rect_before_wrap().shrink(6.0);
    let painter = root.painter().with_clip_rect(free);
    let eye = camera.1.translation();
    let label = |at: Vec3, text: String, color: Color32| {
        if !in_sight(eye, at) {
            return;
        }
        let Ok(spot) = camera.0.world_to_viewport(camera.1, at) else {
            return;
        };
        let pos = egui::pos2(spot.x, spot.y - 10.0);
        if !free.contains(pos) {
            return;
        }
        let font = egui::FontId::proportional(13.0);
        // Dunkle, halb durchsichtige Tafel und Schatten: Die Schrift steht vor hellen
        // Wolken, Eis und vor dem Himmel.
        let size = painter
            .layout_no_wrap(text.clone(), font.clone(), color)
            .size();
        let plate = Align2::CENTER_BOTTOM
            .anchor_size(pos, size)
            .expand2(egui::vec2(5.0, 2.0));
        painter.rect_filled(plate, 3.0, Color32::from_black_alpha(150));
        painter.text(
            pos + egui::vec2(1.0, 1.0),
            Align2::CENTER_BOTTOM,
            &text,
            font.clone(),
            Color32::from_black_alpha(220),
        );
        painter.text(pos, Align2::CENTER_BOTTOM, text, font, color);
    };
    // Farbe einer Gemeinschaft, aufgehellt: Die Schrift steht auch auf Flächen in genau
    // dieser Farbe.
    let light = |color: sim::data::Rgb| {
        let mix = |value: f32| value + (1.0 - value) * 0.6;
        color32((mix(color.0), mix(color.1), mix(color.2)))
    };
    let layout = &watched.layout;
    let ground_war = layout.landed || layout.zones.iter().any(|zone| zone.2);
    if let Some(spot) = scene.landing_spot() {
        label(spot, "Landungsboote".to_string(), light(layout.attacker));
    }
    if ground_war {
        let zones = &watched.card.view.siege.zones;
        for (zone, (direction, _)) in zones.iter().zip(&scene.zones) {
            let (state, color) = if zone.taken {
                ("genommen".to_string(), light(layout.attacker))
            } else {
                (
                    format!("Miliz {}", units(zone.militia)),
                    light(layout.defender),
                )
            };
            label(
                *direction * (PLANET_RADIUS * 1.01),
                format!("{}: {state}", zone.kind.label()),
                color,
            );
        }
    } else {
        label(
            landing_direction() * (PLANET_RADIUS * 1.01),
            "Landegebiet".to_string(),
            MUTED,
        );
    }
    if let Some(center) = scene.fleet_center() {
        let text = if layout.control {
            "Flotte des Belagerers".to_string()
        } else {
            format!("Flotte des Belagerers: {RESTING}")
        };
        label(
            center + center.normalize() * 190.0,
            text,
            light(layout.attacker),
        );
    }
    for (index, station) in scene.stations().into_iter().enumerate() {
        label(
            station + station.normalize() * 30.0,
            format!("Station {}", index + 1),
            light(layout.defender),
        );
    }
}

/// Hinweis über der Szene, solange die Belagerung ruht, mit dem Weg ins laufende Gefecht.
fn resting_banner(
    ctx: &egui::Context,
    free: egui::Rect,
    world: &SiegeWorld,
    watched: &WatchedSiege,
    actions: &mut Vec<HudAction>,
) {
    let card = &watched.card;
    if watched.ended() || card.control || card.handover.is_some() {
        return;
    }
    egui::Area::new("belagerung_ruht".into())
        .order(egui::Order::Middle)
        .fixed_pos(egui::pos2(free.center().x - 230.0, free.top() + 12.0))
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_width(460.0);
                ui.label(
                    RichText::new(format!("Die Belagerung {RESTING}"))
                        .color(WARN)
                        .strong(),
                );
                ui.label(small(if card.troops.army > 0.0 {
                    "Die Flotten des Belagerers stehen im Gefecht oder liegen nicht über \
                     der Welt: kein Feuer aus dem Orbit, kein Nachschub. Die Truppe am \
                     Boden kämpft ohne beides weiter."
                } else {
                    "Die Flotten des Belagerers stehen im Gefecht oder halten den Orbit \
                     nicht. Solange zehrt niemand an Vorräten und Garnison, und es fällt \
                     kein Schuss auf die Welt."
                }));
                if let Some(battle) = world.battle_at(card)
                    && ui
                        .button(RichText::new(WATCH_BATTLE).color(BAD))
                        .on_hover_text(
                            "Verlässt die Orbitansicht und zeigt das laufende Gefecht im \
                             System in der Schlachtansicht.",
                        )
                        .clicked()
                {
                    actions.push(HudAction::WatchBattle(battle));
                }
            });
        });
}

fn result_window(ctx: &egui::Context, watched: &WatchedSiege, actions: &mut Vec<HudAction>) {
    let Some(result) = &watched.result else {
        return;
    };
    if watched.result_closed {
        return;
    }
    egui::Window::new(RESULT_TITLE)
        .collapsible(false)
        .resizable(false)
        .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.set_width(440.0);
            let color = if result.fell { BAD } else { ACCENT };
            ui.label(RichText::new(&result.headline).color(color).strong());
            ui.add_space(4.0);
            if result.lines.is_empty() {
                ui.label(
                    "Die Simulation führt diese Belagerung nicht mehr. Näheres steht in der \
                     Chronik.",
                );
            }
            for line in &result.lines {
                ui.label(line);
            }
            ui.add_space(4.0);
            ui.label(small(format!(
                "Dauer: {}. Die Partie wurde dazu angehalten; „{BACK}“ führt zur Karte, die \
                 Leertaste lässt die Zeit weiterlaufen.",
                days_text(watched.card.days as f64)
            )));
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui.button(BACK).clicked() {
                    actions.push(HudAction::Back);
                }
                if ui.button(KEEP_WATCHING).clicked() {
                    actions.push(HudAction::CloseResult);
                }
            });
        });
}

pub(super) fn draw_hud(
    mut contexts: EguiContexts,
    world: SiegeWorld,
    watched: Option<ResMut<WatchedSiege>>,
    scene: Option<Res<OrbitScene>>,
    camera: Single<(&Camera, &GlobalTransform), With<BattleCamera>>,
    mut speed: ResMut<SimSpeed>,
    mut ui_state: ResMut<UiState>,
    mut cover: ResMut<UiCover>,
    mut next: ResMut<NextState<ViewState>>,
    mut after: ResMut<BattleAfterLeave>,
    mut overview: MessageWriter<OverviewRequest>,
) -> Result {
    let Some(mut watched) = watched else {
        return Ok(());
    };
    let ctx = contexts.ctx_mut()?;
    let mut root = UiCover::root(ctx, "orbitansicht");
    let mut actions = Vec::new();
    egui::Panel::top("orbit_kopf").show(&mut root, |ui| {
        top_bar(ui, &world, &watched, &speed, &mut actions)
    });
    egui::Panel::left("orbit_lage")
        .resizable(true)
        .default_size(350.0)
        .size_range(280.0..=520.0)
        .show(&mut root, |ui| side_panel(ui, &world, &watched));
    if let Some(scene) = &scene {
        scene_labels(&root, &watched, scene, *camera);
    }
    let free = root.available_rect_before_wrap();
    let ctx = root.ctx().clone();
    resting_banner(&ctx, free, &world, &watched, &mut actions);
    result_window(&ctx, &watched, &mut actions);
    if ui_state.help_open {
        panels::help_window(&ctx, &mut ui_state.help_open, true);
    }
    // Was Leiste und Tafel belegen, gehört nicht der Szene.
    cover.record(&root);
    for action in actions {
        match action {
            HudAction::Time(TimeClick::Toggle) => speed.paused = !speed.paused,
            HudAction::Time(TimeClick::Level(level)) => {
                speed.level = level;
                speed.paused = false;
            }
            HudAction::Back => next.set(ViewState::Galaxy),
            HudAction::ToWindow => {
                ui_state.siege.show(watched.planet);
                next.set(ViewState::Galaxy);
            }
            HudAction::WatchBattle(battle) => {
                after.0 = Some(battle);
                next.set(ViewState::Galaxy);
            }
            HudAction::Overview => {
                overview.write(OverviewRequest);
            }
            HudAction::Menu => ui_state.menu_open = !ui_state.menu_open,
            HudAction::Help => ui_state.help_open = !ui_state.help_open,
            HudAction::CloseResult => watched.result_closed = true,
        }
    }
    Ok(())
}
