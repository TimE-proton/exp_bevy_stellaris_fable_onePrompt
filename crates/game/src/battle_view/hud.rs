//! Tafeln der Schlachtansicht (egui): Kopfleiste mit Gefechtszeit und Zeitsteuerung, je
//! Partei eine Lagetafel mit Befehlen, die Tafel des gewählten Schiffs mit seinen Modulen,
//! die Bilanz am Ende und, im Bildschirmraum, die taktischen Markierungen.
//!
//! Wie in der übrigen Oberfläche sind die Tafeln reine Funktionen: Sie lesen den Feed,
//! zeichnen und geben Aktionen zurück. Erst das System am Ende wendet sie an.

use super::camera::{CameraRequest, Projector, SelectedShip, ViewOptions};
use super::feed::{BattleFeed, BattleOrder, BattleViewRequest};
use super::{Part, ViewFrame, ViewTimings};
use crate::time_control::{SPEED_LABELS, SimSpeed};
use crate::ui::UiState;
use crate::ui::format::{decimal, percent};
use crate::ui::panels::{self, ACCENT, MUTED, color32};
use bevy::prelude::*;
use bevy_egui::egui::{self, Align, Align2, Color32, Layout, RichText, Stroke};
use bevy_egui::{EguiContexts, egui::epaint::Shape};
use sim::GameData;
use sim::battle::matchup::{self, Outcome};
use sim::battle::{Battle, BattleShip, Doctrine, Formation, ShipStatus};
use sim::data::{ModuleKind, ShipClass};

const WARNING: Color32 = Color32::from_rgb(235, 110, 95);
const GOOD: Color32 = Color32::from_rgb(110, 200, 120);
const SHIELD: Color32 = Color32::from_rgb(90, 160, 255);
const ARMOR: Color32 = Color32::from_rgb(190, 170, 140);

/// Ab dieser Rumpflänge auf dem Bildschirm (Pixel) verschwindet die Markierung ganz;
/// darunter blendet sie sich ein.
const MARKER_GONE: f32 = 46.0;
const MARKER_FULL: f32 = 20.0;
/// Ab dieser Rumpflänge trägt ein Schiff seinen Namen (wenn Namen eingeschaltet sind).
const LABEL_FROM: f32 = 46.0;

/// Was die Tafeln lesen.
pub struct HudRefs<'a> {
    pub feed: &'a BattleFeed,
    pub data: &'a GameData,
    pub frame: &'a ViewFrame,
    pub speed: &'a SimSpeed,
    pub selected: Option<u32>,
    pub options: &'a ViewOptions,
}

/// Was der Spieler in den Tafeln ausgelöst hat.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum HudAction {
    Order(BattleOrder),
    Request(BattleViewRequest),
    Camera(CameraRequest),
    Select(Option<u32>),
    TogglePause,
    SetSpeed(usize),
    ToggleOverlay,
    ToggleLabels,
    ToggleFollow,
    ToggleSummary,
    ToggleHelp,
}

pub fn status_label(status: ShipStatus) -> &'static str {
    match status {
        ShipStatus::Active => "kämpft",
        ShipStatus::Routing => "auf dem Rückzug",
        ShipStatus::Escaped => "entkommen",
        ShipStatus::Disabled => "treibt ohne Energie",
        ShipStatus::Destroyed => "verloren",
    }
}

/// Zustand eines Moduls in Worten.
pub fn module_condition(hp: f32, full: f32) -> &'static str {
    if hp <= 0.0 {
        "ausgefallen"
    } else if hp < full * 0.999 {
        "beschädigt"
    } else {
        "intakt"
    }
}

/// Richtung eines Feuerwinkels in Worten (Grad vom Bug, Steuerbord positiv).
fn facing_label(facing: f32, arc: f32) -> &'static str {
    if arc >= 359.0 {
        return "rundum";
    }
    let facing = ((facing + 180.0).rem_euclid(360.0)) - 180.0;
    match facing {
        f if f.abs() <= 30.0 => "voraus",
        f if f.abs() >= 150.0 => "achteraus",
        f if (60.0..=120.0).contains(&f) => "Steuerbord",
        f if (-120.0..=-60.0).contains(&f) => "Backbord",
        f if f > 0.0 => "Steuerbord achtern",
        _ => "Backbord achtern",
    }
}

/// Gefechtszeit als mm:ss.
pub fn clock(seconds: f32) -> String {
    let total = seconds.max(0.0) as u32;
    format!("{:02}:{:02}", total / 60, total % 60)
}

/// Lage einer Partei, einmal je Frame aus dem Gefecht gezählt.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SideSummary {
    pub active: [u32; 6],
    /// Anteil des anfänglichen Kampfwerts, der noch kämpft.
    pub strength: f32,
    /// Mittlere Moral der Schiffe auf dem Feld, 0..1.
    pub morale: f32,
    /// Jäger, die noch im Hangar stehen.
    pub hangar: u32,
}

pub fn summarize(battle: &Battle, data: &GameData, side: usize) -> SideSummary {
    let mut summary = SideSummary {
        active: battle.active_by_class(side),
        ..Default::default()
    };
    let (mut morale, mut crews) = (0.0, 0);
    for ship in battle
        .ships
        .iter()
        .filter(|ship| ship.side as usize == side)
    {
        if ship.status.can_act() {
            morale += ship.morale;
            crews += 1;
        }
        if ship.status == ShipStatus::Active {
            summary.hangar += ship.hangar;
        }
    }
    summary.morale = if crews > 0 {
        (morale / crews as f32 / 100.0).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let initial = battle.combatants[side].initial_value.max(1.0);
    summary.strength = (battle.fighting_value(data, side) / initial).clamp(0.0, 1.0);
    summary
}

fn bar(fraction: f32, color: Color32, width: f32) -> egui::ProgressBar {
    egui::ProgressBar::new(fraction.clamp(0.0, 1.0))
        .desired_width(width)
        .desired_height(9.0)
        .fill(color)
}

/// Farbe eines Zustandsbalkens: grün, gelb, rot.
fn health_color(fraction: f32) -> Color32 {
    if fraction > 0.6 {
        GOOD
    } else if fraction > 0.3 {
        ACCENT
    } else {
        WARNING
    }
}

/// Zeichnet alle Tafeln der Schlachtansicht.
pub fn battle_hud(root: &mut egui::Ui, refs: &HudRefs, actions: &mut Vec<HudAction>) {
    let Some(battle) = refs.feed.battle() else {
        return;
    };
    markers(root.ctx(), refs, battle);
    egui::Panel::top("gefechtsleiste").show(root, |ui| top_bar(ui, refs, battle, actions));
    let ctx = root.ctx().clone();
    for side in 0..battle.combatants.len() {
        side_panel(&ctx, refs, battle, side, actions);
    }
    if let Some(ship) = refs
        .selected
        .and_then(|index| battle.ships.get(index as usize).map(|ship| (index, ship)))
    {
        inspector(&ctx, refs, battle, ship.0, ship.1, actions);
    }
    if battle.finished && refs.options.summary {
        summary(&ctx, refs, battle, actions);
    }
}

fn top_bar(ui: &mut egui::Ui, refs: &HudRefs, battle: &Battle, actions: &mut Vec<HudAction>) {
    let speed = refs.speed;
    ui.horizontal(|ui| {
        ui.add_space(4.0);
        ui.label(RichText::new(&refs.feed.info.title).strong().color(ACCENT));
        ui.separator();
        ui.label("Gefechtszeit");
        ui.label(RichText::new(clock(battle.time)).monospace().strong());
        if battle.finished {
            let text = if battle.timed_out {
                "ZEITGRENZE"
            } else {
                "BEENDET"
            };
            ui.label(RichText::new(text).color(ACCENT).strong());
        }
        ui.separator();
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
            actions.push(HudAction::TogglePause);
        }
        for (level, label) in SPEED_LABELS.iter().enumerate() {
            let active = !speed.paused && speed.level == level;
            if ui
                .selectable_label(active, *label)
                .on_hover_text(format!("Taste {}", level + 1))
                .clicked()
            {
                actions.push(HudAction::SetSpeed(level));
            }
        }
        if refs.feed.info.still {
            ui.label(RichText::new("STANDBILD").color(ACCENT).strong());
        } else if speed.paused && !battle.finished {
            let hint = if battle.tick == 0 {
                "PAUSIERT – Leertaste beginnt das Gefecht"
            } else {
                "PAUSIERT"
            };
            ui.label(RichText::new(hint).color(ACCENT).strong());
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui.button("Verlassen").clicked() {
                actions.push(HudAction::Request(BattleViewRequest::Leave));
            }
            if ui.button("Hilfe").on_hover_text("F1").clicked() {
                actions.push(HudAction::ToggleHelp);
            }
            ui.separator();
            if battle.finished
                && ui
                    .selectable_label(refs.options.summary, "Bilanz")
                    .on_hover_text("Bilanz des Gefechts ein- und ausblenden")
                    .clicked()
            {
                actions.push(HudAction::ToggleSummary);
            }
            if ui
                .selectable_label(refs.options.labels, "Namen")
                .on_hover_text("Namen an nahen Schiffen (N)")
                .clicked()
            {
                actions.push(HudAction::ToggleLabels);
            }
            if ui
                .selectable_label(refs.options.overlay, "Taktik")
                .on_hover_text("Flottenanker, Reichweiten und Ziellinien (T)")
                .clicked()
            {
                actions.push(HudAction::ToggleOverlay);
            }
            ui.separator();
            if ui
                .button("Gefecht")
                .on_hover_text("Zum Schwerpunkt des Gefechts (G)")
                .clicked()
            {
                actions.push(HudAction::Camera(CameraRequest::FocusFight));
            }
            if ui
                .button("Übersicht")
                .on_hover_text("Über das ganze Feld (U)")
                .clicked()
            {
                actions.push(HudAction::Camera(CameraRequest::Overview));
            }
        });
    });
}

/// Lagetafel einer Partei; für die Partei des Spielers mit Befehlen.
fn side_panel(
    ctx: &egui::Context,
    refs: &HudRefs,
    battle: &Battle,
    side: usize,
    actions: &mut Vec<HudAction>,
) {
    let combatant = &battle.combatants[side];
    let summary = summarize(battle, refs.data, side);
    let color = color32(refs.feed.side_color(side));
    // Die ersten beiden Parteien stehen links und rechts, weitere dazwischen.
    let (anchor, offset) = match side {
        0 => (Align2::LEFT_TOP, [8.0, 34.0]),
        1 => (Align2::RIGHT_TOP, [-8.0, 34.0]),
        more => (Align2::CENTER_TOP, [(more as f32 - 2.5) * 270.0, 34.0]),
    };
    egui::Window::new(format!("partei_{side}"))
        .title_bar(false)
        .resizable(false)
        .anchor(anchor, offset)
        .show(ctx, |ui| {
            ui.set_width(236.0);
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(refs.feed.side_name(side))
                        .strong()
                        .color(color),
                );
                if refs.feed.info.player_side == Some(side) {
                    ui.label(RichText::new("(du)").color(MUTED));
                }
                if combatant.retreating {
                    ui.label(RichText::new("RÜCKZUG").color(WARNING).strong());
                }
            });
            ui.label(
                RichText::new(format!(
                    "{} · {}",
                    combatant.formation.label(),
                    combatant.doctrine.label()
                ))
                .color(MUTED),
            );
            egui::Grid::new(("klassen", side))
                .striped(true)
                .num_columns(4)
                .show(ui, |ui| {
                    for title in ["Klasse", "kämpft", "verloren", "entkommen"] {
                        ui.label(RichText::new(title).color(MUTED).small());
                    }
                    ui.end_row();
                    for class in ShipClass::ALL {
                        let index = class.index();
                        if combatant.initial[index] == 0 {
                            continue;
                        }
                        ui.label(class.plural());
                        ui.label(summary.active[index].to_string());
                        let lost = combatant.lost[index];
                        let mut text = RichText::new(lost.to_string());
                        if lost > 0 {
                            text = text.color(WARNING);
                        }
                        ui.label(text);
                        ui.label(combatant.escaped[index].to_string());
                        ui.end_row();
                    }
                });
            if summary.hangar > 0 {
                ui.label(
                    RichText::new(format!("{} Jäger noch im Hangar", summary.hangar))
                        .color(MUTED)
                        .small(),
                );
            }
            egui::Grid::new(("lage", side))
                .num_columns(3)
                .show(ui, |ui| {
                    ui.label(RichText::new("Kampfwert").color(MUTED));
                    ui.add(bar(summary.strength, color, 110.0));
                    ui.label(percent(summary.strength));
                    ui.end_row();
                    ui.label(RichText::new("Moral").color(MUTED));
                    ui.add(bar(summary.morale, health_color(summary.morale), 110.0));
                    ui.label(percent(summary.morale));
                    ui.end_row();
                });
            if refs.feed.info.player_side == Some(side) {
                ui.separator();
                orders(ui, battle, side, actions);
            }
        });
}

/// Befehle des Spielers an seine Partei.
fn orders(ui: &mut egui::Ui, battle: &Battle, side: usize, actions: &mut Vec<HudAction>) {
    let combatant = &battle.combatants[side];
    let locked = battle.finished || combatant.retreating;
    ui.add_enabled_ui(!locked, |ui| {
        ui.label(RichText::new("Formation").color(MUTED).small());
        ui.horizontal(|ui| {
            for formation in Formation::ALL {
                if ui
                    .selectable_label(combatant.formation == formation, formation.label())
                    .clicked()
                    && combatant.formation != formation
                {
                    actions.push(HudAction::Order(BattleOrder::SetOrders {
                        side,
                        formation,
                        doctrine: combatant.doctrine,
                    }));
                }
            }
        });
        ui.label(RichText::new("Doktrin").color(MUTED).small());
        ui.horizontal(|ui| {
            for doctrine in Doctrine::ALL {
                if ui
                    .selectable_label(combatant.doctrine == doctrine, doctrine.label())
                    .clicked()
                    && combatant.doctrine != doctrine
                {
                    actions.push(HudAction::Order(BattleOrder::SetOrders {
                        side,
                        formation: combatant.formation,
                        doctrine,
                    }));
                }
            }
        });
        if ui
            .button(RichText::new("Rückzug befehlen").color(WARNING))
            .on_hover_text("Alle Schiffe verlassen das Feld. Nicht umkehrbar.")
            .clicked()
        {
            actions.push(HudAction::Order(BattleOrder::Retreat { side }));
        }
    });
}

/// Tafel des gewählten Schiffs: Zustand, Vorräte, Ziel und alle Module.
fn inspector(
    ctx: &egui::Context,
    refs: &HudRefs,
    battle: &Battle,
    index: u32,
    ship: &BattleShip,
    actions: &mut Vec<HudAction>,
) {
    let design = &refs.data.designs[ship.design as usize];
    let side_color = color32(refs.feed.side_color(ship.side as usize));
    egui::Window::new("schiff")
        .title_bar(false)
        .resizable(false)
        .default_height(640.0)
        .anchor(Align2::LEFT_BOTTOM, [8.0, -8.0])
        .show(ctx, |ui| {
            ui.set_width(330.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new(&design.name).strong().size(16.0));
                ui.label(RichText::new(ship.class.label()).color(MUTED));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui
                        .small_button("×")
                        .on_hover_text("Auswahl aufheben")
                        .clicked()
                    {
                        actions.push(HudAction::Select(None));
                    }
                });
            });
            ui.horizontal(|ui| {
                ui.label(RichText::new(refs.feed.side_name(ship.side as usize)).color(side_color));
                let mut status = RichText::new(status_label(ship.status));
                if ship.status != ShipStatus::Active {
                    status = status.color(WARNING);
                }
                ui.label(status);
            });
            ui.horizontal(|ui| {
                let in_play = ship.status.in_play();
                if ui
                    .add_enabled(in_play, egui::Button::new("Heranholen").small())
                    .on_hover_text("Kamera zum Schiff (F)")
                    .clicked()
                {
                    actions.push(HudAction::Camera(CameraRequest::FocusShip(index)));
                }
                if ui
                    .add_enabled(
                        in_play,
                        egui::Button::selectable(refs.options.follow, "Verfolgen").small(),
                    )
                    .on_hover_text("Die Kamera wandert mit dem Schiff mit (V)")
                    .clicked()
                {
                    actions.push(HudAction::ToggleFollow);
                }
            });
            ui.separator();

            egui::Grid::new("schiff_zustand")
                .num_columns(3)
                .show(ui, |ui| {
                    let row =
                        |ui: &mut egui::Ui, label: &str, value: f32, full: f32, color: Color32| {
                            let fraction = if full > 0.0 { value / full } else { 0.0 };
                            ui.label(RichText::new(label).color(MUTED));
                            ui.add(bar(fraction, color, 150.0));
                            if full > 0.0 {
                                ui.label(format!(
                                    "{} / {}",
                                    decimal(value.max(0.0) as f64, 0),
                                    decimal(full as f64, 0)
                                ));
                            } else {
                                ui.label(RichText::new("–").color(MUTED));
                            }
                            ui.end_row();
                        };
                    let hull = ship.hull / ship.hull_max.max(1e-6);
                    row(ui, "Rumpf", ship.hull, ship.hull_max, health_color(hull));
                    row(ui, "Schild", ship.shield, ship.shield_max, SHIELD);
                    for (face, label) in [
                        "Panzer Bug",
                        "Panzer Steuerbord",
                        "Panzer Heck",
                        "Panzer Backbord",
                    ]
                    .into_iter()
                    .enumerate()
                    {
                        row(ui, label, ship.armor[face], ship.armor_max[face], ARMOR);
                    }
                    row(ui, "Munition", ship.ammo, ship.ammo_max, ACCENT);
                    row(
                        ui,
                        "Treibstoff",
                        ship.fuel,
                        ship.fuel_max,
                        Color32::from_rgb(90, 200, 190),
                    );
                    row(
                        ui,
                        "Moral",
                        ship.morale,
                        100.0,
                        health_color(ship.morale / 100.0),
                    );
                });
            if ship.hangar > 0 {
                ui.label(format!("{} Jäger im Hangar", ship.hangar));
            }
            ui.horizontal(|ui| {
                ui.label(RichText::new("Ziel").color(MUTED));
                match ship.target.and_then(|target| {
                    battle
                        .ships
                        .get(target as usize)
                        .map(|other| (target, other))
                }) {
                    Some((target, other)) => {
                        let name = &refs.data.designs[other.design as usize].name;
                        let text = RichText::new(format!("{name} ({})", other.class.label()))
                            .color(color32(refs.feed.side_color(other.side as usize)));
                        if ui.link(text).on_hover_text("Ziel wählen").clicked() {
                            actions.push(HudAction::Select(Some(target)));
                        }
                    }
                    None => {
                        ui.label(RichText::new("keines").color(MUTED));
                    }
                }
            });
            ui.separator();

            ui.label(RichText::new("Module").strong());
            egui::ScrollArea::vertical()
                .max_height(190.0)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    egui::Grid::new("module")
                        .striped(true)
                        .num_columns(3)
                        .show(ui, |ui| {
                            for (slot, state) in design.modules.iter().zip(&ship.modules) {
                                let module = &refs.data.modules[slot.module_id as usize];
                                let condition = module_condition(state.hp, module.hp);
                                let fraction = (state.hp / module.hp.max(1e-6)).clamp(0.0, 1.0);
                                let mut name = RichText::new(&module.name);
                                if state.hp <= 0.0 {
                                    name = name.color(MUTED).strikethrough();
                                }
                                let label = ui.label(name);
                                if matches!(module.kind, ModuleKind::Weapon(_)) {
                                    label.on_hover_text(format!(
                                        "Feuerwinkel {}, {} Grad",
                                        facing_label(slot.facing, slot.arc),
                                        decimal(slot.arc as f64, 0)
                                    ));
                                }
                                ui.add(bar(fraction, health_color(fraction), 70.0));
                                let color = match condition {
                                    "intakt" => GOOD,
                                    "beschädigt" => ACCENT,
                                    _ => WARNING,
                                };
                                ui.label(RichText::new(condition).color(color));
                                ui.end_row();
                            }
                        });
                });
        });
}

/// Ergebnis in einem Satz.
pub fn outcome_text(feed: &BattleFeed, battle: &Battle, outcome: &Outcome) -> String {
    if battle.timed_out {
        return "Zeitgrenze erreicht: Beide Seiten lösen sich voneinander.".into();
    }
    match outcome.winner() {
        Some(winner) => format!("{} behauptet das Feld.", feed.side_name(winner)),
        None => "Keine Seite hält das Feld.".into(),
    }
}

/// Bilanz nach dem Ende des Gefechts.
fn summary(ctx: &egui::Context, refs: &HudRefs, battle: &Battle, actions: &mut Vec<HudAction>) {
    let outcome = matchup::outcome_of(battle);
    let mut open = true;
    egui::Window::new("Bilanz")
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .default_pos([ctx.content_rect().center().x - 260.0, 150.0])
        .show(ctx, |ui| {
            ui.set_min_width(500.0);
            ui.label(
                RichText::new(outcome_text(refs.feed, battle, &outcome))
                    .strong()
                    .size(16.0),
            );
            ui.label(
                RichText::new(format!("Dauer {} Minuten Gefechtszeit", clock(battle.time)))
                    .color(MUTED),
            );
            ui.add_space(4.0);
            for (side, result) in outcome.sides.iter().enumerate() {
                let color = color32(refs.feed.side_color(side));
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(refs.feed.side_name(side))
                            .strong()
                            .color(color),
                    );
                    let mut notes = Vec::new();
                    if result.holds_field {
                        notes.push("hält das Feld");
                    }
                    if result.retreated {
                        notes.push("Rückzug");
                    }
                    notes.push("Verlust am Bauwert");
                    ui.label(
                        RichText::new(format!(
                            "{} {}",
                            notes.join(" · "),
                            percent(result.loss_share())
                        ))
                        .color(MUTED),
                    );
                });
                egui::Grid::new(("bilanz", side))
                    .striped(true)
                    .num_columns(5)
                    .show(ui, |ui| {
                        for title in ["Klasse", "anfangs", "verloren", "entkommen", "im Feld"] {
                            ui.label(RichText::new(title).color(MUTED).small());
                        }
                        ui.end_row();
                        for class in ShipClass::ALL {
                            let index = class.index();
                            if result.initial[index] == 0 {
                                continue;
                            }
                            ui.label(class.plural());
                            ui.label(result.initial[index].to_string());
                            let mut lost = RichText::new(result.lost[index].to_string());
                            if result.lost[index] > 0 {
                                lost = lost.color(WARNING);
                            }
                            ui.label(lost);
                            ui.label(result.escaped[index].to_string());
                            ui.label(result.holding[index].to_string());
                            ui.end_row();
                        }
                    });
                ui.add_space(6.0);
            }
            ui.horizontal(|ui| {
                if refs.feed.info.can_restart
                    && ui
                        .button(RichText::new("Neu starten").size(15.0))
                        .on_hover_text("Dasselbe Gefecht noch einmal von vorn")
                        .clicked()
                {
                    actions.push(HudAction::Request(BattleViewRequest::Restart));
                }
                if ui.button(RichText::new("Zurück").size(15.0)).clicked() {
                    actions.push(HudAction::Request(BattleViewRequest::Leave));
                }
                if ui
                    .button("Feld ansehen")
                    .on_hover_text("Bilanz ausblenden; „Bilanz“ in der Kopfleiste holt sie zurück")
                    .clicked()
                {
                    actions.push(HudAction::ToggleSummary);
                }
            });
        });
    if !open {
        actions.push(HudAction::ToggleSummary);
    }
}

/// Umriss der Markierung einer Klasse (x voraus) und ihre halbe Größe in Pixeln.
pub fn marker_outline(class: ShipClass) -> (&'static [[f32; 2]], f32) {
    match class {
        ShipClass::Fighter => (&[[1.0, 0.0], [-0.8, 0.75], [-0.8, -0.75]], 3.2),
        ShipClass::Corvette => (&[[1.25, 0.0], [-0.9, 0.55], [-0.9, -0.55]], 5.2),
        ShipClass::Frigate => (&[[1.0, 0.0], [0.0, 0.8], [-1.0, 0.0], [0.0, -0.8]], 6.0),
        ShipClass::Cruiser => (
            &[
                [1.3, 0.0],
                [0.35, 0.62],
                [-1.0, 0.62],
                [-1.0, -0.62],
                [0.35, -0.62],
            ],
            7.2,
        ),
        ShipClass::Battleship => (
            &[
                [1.3, 0.0],
                [0.55, 0.85],
                [-1.0, 0.85],
                [-1.25, 0.0],
                [-1.0, -0.85],
                [0.55, -0.85],
            ],
            8.8,
        ),
        ShipClass::Carrier => (
            &[[1.15, 0.95], [-1.15, 0.95], [-1.15, -0.95], [1.15, -0.95]],
            8.8,
        ),
    }
}

/// Deckkraft der Markierung eines Schiffs, dessen Rumpf `pixels` lang erscheint: voll,
/// solange das Schiff winzig ist, ausgeblendet, sobald man den Rumpf selbst gut erkennt.
pub fn marker_alpha(pixels: f32) -> f32 {
    ((MARKER_GONE - pixels) / (MARKER_GONE - MARKER_FULL)).clamp(0.0, 1.0)
}

/// Eine taktische Markierung im Bildschirmraum.
#[derive(Clone, Debug, PartialEq)]
pub struct Marker {
    pub ship: u32,
    pub center: egui::Pos2,
    /// Ecken des Umrisses, bereits gedreht und verschoben.
    pub points: Vec<egui::Pos2>,
    pub alpha: f32,
    /// Erscheinende Rumpflänge in Pixeln.
    pub pixels: f32,
}

/// Berechnet die Markierungen aller Schiffe auf dem Feld, die im Bild liegen.
pub fn collect_markers(battle: &Battle, frame: &ViewFrame, projector: &Projector) -> Vec<Marker> {
    let mut markers = Vec::with_capacity(battle.ships.len());
    let pixel = projector.pixel();
    for (index, ship) in battle.ships.iter().enumerate() {
        if !ship.status.in_play() {
            continue;
        }
        let Some((position, heading)) = frame.poses.get(index) else {
            continue;
        };
        let Some((screen, depth)) = projector.project(*position) else {
            continue;
        };
        let margin = 40.0;
        if screen.x < -margin
            || screen.y < -margin
            || screen.x > projector.size.x + margin
            || screen.y > projector.size.y + margin
        {
            continue;
        }
        let pixels = 2.0 * ship.radius / (pixel * depth);
        let alpha = marker_alpha(pixels);
        let mut points = Vec::new();
        if alpha > 0.0 {
            // Der Bug der Markierung zeigt, wohin das Schiff auf dem Bildschirm fährt.
            let ahead =
                *position + super::heading_rotation(*heading) * Vec3::X * ship.radius.max(8.0);
            let direction = projector
                .project(ahead)
                .map(|(point, _)| point - screen)
                .filter(|delta| delta.length_squared() > 1e-6)
                .map_or(Vec2::X, |delta| delta.normalize());
            let (outline, size) = marker_outline(ship.class);
            points.extend(outline.iter().map(|corner| {
                let local = Vec2::new(corner[0], corner[1]) * size;
                let rotated = Vec2::new(
                    local.x * direction.x - local.y * direction.y,
                    local.x * direction.y + local.y * direction.x,
                );
                egui::pos2(screen.x + rotated.x, screen.y + rotated.y)
            }));
        }
        markers.push(Marker {
            ship: index as u32,
            center: egui::pos2(screen.x, screen.y),
            points,
            alpha,
            pixels,
        });
    }
    markers
}

/// Zeichnet Markierungen, Auswahlklammern und Namen auf die Hintergrundebene, also unter
/// alle Tafeln.
fn markers(ctx: &egui::Context, refs: &HudRefs, battle: &Battle) {
    let painter = ctx.layer_painter(egui::LayerId::background());
    let found = collect_markers(battle, refs.frame, &refs.frame.projector);
    let with_alpha = |color: Color32, alpha: f32| {
        Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), (alpha * 255.0) as u8)
    };
    let mut labels = 0;
    for marker in &found {
        let ship = &battle.ships[marker.ship as usize];
        let side = color32(refs.feed.side_color(ship.side as usize));
        if marker.alpha > 0.0 {
            let outline = Stroke::new(1.0, with_alpha(Color32::BLACK, marker.alpha * 0.7));
            let shape = match ship.status {
                // Fliehende Schiffe: nur der Umriss in Parteifarbe.
                ShipStatus::Routing => Shape::closed_line(
                    marker.points.clone(),
                    Stroke::new(1.3, with_alpha(side, marker.alpha)),
                ),
                // Treibende Schiffe: grau gefüllt.
                ShipStatus::Disabled => Shape::convex_polygon(
                    marker.points.clone(),
                    with_alpha(Color32::from_gray(110), marker.alpha * 0.8),
                    Stroke::new(1.0, with_alpha(side, marker.alpha)),
                ),
                _ => Shape::convex_polygon(
                    marker.points.clone(),
                    with_alpha(side, marker.alpha * 0.92),
                    outline,
                ),
            };
            painter.add(shape);
        }
        if refs.selected == Some(marker.ship) {
            // Klammern um das gewählte Schiff, egal wie groß es erscheint.
            let half = (marker.pixels * 0.5 + 6.0).clamp(12.0, 220.0);
            let arm = (half * 0.35).min(14.0);
            let stroke = Stroke::new(1.5, Color32::WHITE);
            for (sx, sy) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
                let corner = marker.center + egui::vec2(sx * half, sy * half);
                painter.line_segment([corner, corner - egui::vec2(sx * arm, 0.0)], stroke);
                painter.line_segment([corner, corner - egui::vec2(0.0, sy * arm)], stroke);
            }
        }
        // Im Standbild (Schaustellung) trägt jedes Schiff seinen Namen, sonst nur die nahen.
        let near = marker.pixels >= LABEL_FROM || refs.feed.info.still;
        if refs.options.labels && near && labels < 60 {
            labels += 1;
            let design = &refs.data.designs[ship.design as usize];
            painter.text(
                marker.center + egui::vec2(0.0, (marker.pixels * 0.36).min(150.0) + 12.0),
                Align2::CENTER_TOP,
                format!("{} · {}", design.name, ship.class.label()),
                egui::FontId::proportional(13.0),
                side,
            );
        }
    }
}

pub(super) fn draw_hud(
    mut timings: ResMut<ViewTimings>,
    mut contexts: EguiContexts,
    feed: Res<BattleFeed>,
    data: Res<GameData>,
    frame: Res<ViewFrame>,
    mut speed: ResMut<SimSpeed>,
    mut selected: ResMut<SelectedShip>,
    mut options: ResMut<ViewOptions>,
    mut ui_state: ResMut<UiState>,
    mut orders: MessageWriter<BattleOrder>,
    mut requests: MessageWriter<BattleViewRequest>,
    mut camera: MessageWriter<CameraRequest>,
) -> Result {
    let _span = timings.span(Part::Hud);
    let ctx = contexts.ctx_mut()?;
    let mut root = egui::Ui::new(
        ctx.clone(),
        "gefecht".into(),
        egui::UiBuilder::new()
            .layer_id(egui::LayerId::background())
            .max_rect(ctx.viewport_rect()),
    );
    let mut actions = Vec::new();
    let refs = HudRefs {
        feed: &feed,
        data: &data,
        frame: &frame,
        speed: &speed,
        selected: selected.0,
        options: &options,
    };
    battle_hud(&mut root, &refs, &mut actions);
    if ui_state.help_open {
        panels::help_window(root.ctx(), &mut ui_state.help_open, true);
    }
    for action in actions {
        match action {
            HudAction::Order(order) => {
                orders.write(order);
            }
            HudAction::Request(request) => {
                requests.write(request);
            }
            HudAction::Camera(request) => {
                camera.write(request);
            }
            HudAction::Select(ship) => selected.0 = ship,
            HudAction::TogglePause => speed.paused = !speed.paused,
            HudAction::SetSpeed(level) => {
                speed.level = level;
                speed.paused = false;
            }
            HudAction::ToggleOverlay => options.overlay = !options.overlay,
            HudAction::ToggleLabels => options.labels = !options.labels,
            HudAction::ToggleFollow => options.follow = !options.follow && selected.0.is_some(),
            HudAction::ToggleSummary => options.summary = !options.summary,
            HudAction::ToggleHelp => ui_state.help_open = !ui_state.help_open,
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::feed::testing::{MIXED, data, stepped};
    use super::super::to_world;
    use super::*;

    /// Zeichnet ein Frame ohne Fenster und Grafikkarte.
    fn frame(mut draw: impl FnMut(&mut egui::Ui)) {
        let ctx = egui::Context::default();
        panels::apply_style(&ctx);
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1600.0, 900.0),
            )),
            ..Default::default()
        };
        let mut output = ctx.run_ui(input, |ui| draw(ui));
        output.textures_delta.clear();
    }

    fn view_frame(feed: &BattleFeed, camera: &Transform) -> ViewFrame {
        let battle = feed.battle().unwrap();
        ViewFrame {
            alpha: 1.0,
            time: battle.time,
            tick_seconds: 0.1,
            poses: (0..battle.ships.len())
                .map(|index| {
                    let pose = feed.pose(index, 1.0);
                    (to_world(pose.pos), pose.heading)
                })
                .collect(),
            projector: Projector::new(camera, super::super::camera::FOV, Vec2::new(1600.0, 900.0)),
        }
    }

    #[test]
    fn hud_draws_for_a_running_battle_and_every_selection() {
        let data = data();
        let (battle, feed) = stepped(&data, MIXED, 450);
        assert!(
            !battle.finished && !battle.wrecks.is_empty(),
            "mitten im Gefecht"
        );
        let camera = super::super::camera::overview_transform(2400.0);
        let frame_data = view_frame(&feed, &camera);
        let speed = SimSpeed::default();
        for labels in [false, true] {
            let options = ViewOptions {
                overlay: true,
                labels,
                follow: labels,
                summary: true,
            };
            // Ohne Auswahl und mit jedem Schiff des Gefechts als Auswahl, auch mit
            // zerstörten und entkommenen.
            for selected in std::iter::once(None).chain((0..battle.ships.len() as u32).map(Some)) {
                let refs = HudRefs {
                    feed: &feed,
                    data: &data,
                    frame: &frame_data,
                    speed: &speed,
                    selected,
                    options: &options,
                };
                let mut actions = Vec::new();
                frame(|ui| battle_hud(ui, &refs, &mut actions));
                assert!(
                    actions.is_empty(),
                    "ohne Eingabe darf nichts ausgelöst werden"
                );
            }
        }
        // Ein Auswahlindex, den es nicht gibt, bringt nichts zum Absturz.
        let refs = HudRefs {
            feed: &feed,
            data: &data,
            frame: &frame_data,
            speed: &speed,
            selected: Some(99_999),
            options: &ViewOptions::default(),
        };
        frame(|ui| battle_hud(ui, &refs, &mut Vec::new()));
    }

    #[test]
    fn hud_draws_the_summary_after_the_battle() {
        let data = data();
        let (mut battle, mut feed) = stepped(&data, MIXED, 0);
        while !battle.finished {
            battle.step(&data);
            feed.refresh(&battle);
        }
        let camera = super::super::camera::overview_transform(2400.0);
        let frame_data = view_frame(&feed, &camera);
        let speed = SimSpeed {
            paused: false,
            level: 3,
        };
        let refs = HudRefs {
            feed: &feed,
            data: &data,
            frame: &frame_data,
            speed: &speed,
            selected: Some(0),
            options: &ViewOptions::default(),
        };
        let mut actions = Vec::new();
        frame(|ui| battle_hud(ui, &refs, &mut actions));
        assert!(actions.is_empty());
        // Auch mit ausgeblendeter Bilanz (zum Betrachten des Felds).
        let hidden = ViewOptions {
            summary: false,
            ..Default::default()
        };
        let refs = HudRefs {
            options: &hidden,
            ..refs
        };
        frame(|ui| battle_hud(ui, &refs, &mut actions));
        assert!(actions.is_empty());
        // Die Bilanz nennt den Ausgang und zählt je Partei, was der Kern gezählt hat.
        let outcome = matchup::outcome_of(&battle);
        let text = outcome_text(&feed, &battle, &outcome);
        match outcome.winner() {
            Some(winner) => assert!(text.contains(feed.side_name(winner))),
            None => assert!(text.contains("Keine Seite") || text.contains("Zeitgrenze")),
        }
        for (side, result) in outcome.sides.iter().enumerate() {
            let summary = summarize(&battle, &data, side);
            assert_eq!(summary.active, result.holding);
            for class in 0..6 {
                assert!(
                    result.lost[class] + result.escaped[class] + result.holding[class]
                        <= result.initial[class].max(
                            result.lost[class] + result.escaped[class] + result.holding[class]
                        )
                );
            }
        }
    }

    #[test]
    fn side_summary_counts_what_the_battle_holds() {
        let data = data();
        let (battle, _) = stepped(&data, MIXED, 0);
        for side in 0..2 {
            let summary = summarize(&battle, &data, side);
            // Vor dem ersten Schuss: voller Kampfwert, Moral auf dem Ausgangswert.
            assert!((summary.strength - 1.0).abs() < 1e-3);
            assert!((summary.morale - data.balance.battle.morale_start / 100.0).abs() < 1e-3);
            assert_eq!(summary.active, battle.active_by_class(side));
        }
        // Der Träger der ersten Partei hat seine Jäger noch an Bord.
        assert_eq!(summarize(&battle, &data, 0).hangar, 24);
        assert_eq!(summarize(&battle, &data, 1).hangar, 0);

        let (battle, _) = stepped(&data, MIXED, 500);
        let lost: u32 = battle
            .combatants
            .iter()
            .map(|c| c.lost.iter().sum::<u32>())
            .sum();
        assert!(lost > 0);
        let worn = (0..2)
            .map(|side| summarize(&battle, &data, side).strength)
            .fold(1.0, f32::min);
        assert!(worn < 0.9, "Verluste drücken den Kampfwert");
    }

    #[test]
    fn markers_fade_in_for_small_ships_and_point_along_the_course() {
        assert_eq!(marker_alpha(4.0), 1.0);
        assert_eq!(marker_alpha(MARKER_FULL), 1.0);
        assert_eq!(marker_alpha(MARKER_GONE), 0.0);
        assert_eq!(marker_alpha(200.0), 0.0);
        let half = marker_alpha((MARKER_FULL + MARKER_GONE) * 0.5);
        assert!((half - 0.5).abs() < 1e-5);
        // Je Klasse ein eigener Umriss; größere Klassen haben größere Markierungen.
        let mut last = 0.0;
        for class in ShipClass::ALL {
            let (outline, size) = marker_outline(class);
            assert!(outline.len() >= 3);
            assert!(size >= last);
            last = size;
        }
        assert_ne!(
            marker_outline(ShipClass::Battleship).0,
            marker_outline(ShipClass::Carrier).0
        );

        let data = data();
        let (battle, feed) = stepped(&data, MIXED, 60);
        // Aus der Übersicht über das ganze Feld ist jedes Schiff klein: Alle tragen eine
        // Markierung (die kleinen Klassen voll deckend), und sie liegt dort, wo das Schiff
        // im Bild steht.
        let camera = super::super::camera::overview_transform(2400.0);
        let frame_data = view_frame(&feed, &camera);
        let markers = collect_markers(&battle, &frame_data, &frame_data.projector);
        let in_play = battle
            .ships
            .iter()
            .filter(|ship| ship.status.in_play())
            .count();
        assert_eq!(markers.len(), in_play);
        for marker in &markers {
            let ship = &battle.ships[marker.ship as usize];
            assert!(
                marker.alpha > 0.8,
                "{:?} mit {} Pixeln",
                ship.class,
                marker.pixels
            );
            if !ship.class.is_capital() {
                assert_eq!(
                    marker.alpha, 1.0,
                    "{:?} mit {} Pixeln",
                    ship.class, marker.pixels
                );
            }
            let (outline, size) = marker_outline(ship.class);
            assert_eq!(marker.points.len(), outline.len());
            for point in &marker.points {
                assert!(point.distance(marker.center) <= size * 1.6 + 1e-3);
            }
            // Die Spitze (erste Ecke) liegt in Fahrtrichtung vor der Mitte.
            let (position, heading) = frame_data.poses[marker.ship as usize];
            let ahead = position + super::super::heading_rotation(heading) * Vec3::X * 60.0;
            let (ahead_screen, _) = frame_data.projector.project(ahead).unwrap();
            let course = (ahead_screen - Vec2::new(marker.center.x, marker.center.y)).normalize();
            let tip = marker.points[0] - marker.center;
            if ship.class != ShipClass::Carrier {
                assert!(course.dot(Vec2::new(tip.x, tip.y).normalize()) > 0.99);
            }
        }
        // Aus der Nähe verschwindet die Markierung: Der Rumpf selbst ist zu erkennen.
        let capital = battle
            .ships
            .iter()
            .position(|ship| ship.class == ShipClass::Battleship && ship.status.in_play())
            .unwrap();
        let near =
            Transform::from_translation(frame_data.poses[capital].0 + Vec3::new(0.0, 60.0, 110.0))
                .looking_at(frame_data.poses[capital].0, Vec3::Y);
        let close_frame = view_frame(&feed, &near);
        let close = collect_markers(&battle, &close_frame, &close_frame.projector);
        let own = close
            .iter()
            .find(|marker| marker.ship as usize == capital)
            .unwrap();
        assert_eq!(own.alpha, 0.0);
        assert!(own.points.is_empty());
        assert!(own.pixels > 200.0);
    }

    #[test]
    fn words_for_states_and_modules() {
        assert_eq!(module_condition(50.0, 50.0), "intakt");
        assert_eq!(module_condition(12.0, 50.0), "beschädigt");
        assert_eq!(module_condition(0.0, 50.0), "ausgefallen");
        assert_eq!(clock(0.0), "00:00");
        assert_eq!(clock(125.9), "02:05");
        assert_eq!(facing_label(0.0, 250.0), "voraus");
        assert_eq!(facing_label(90.0, 150.0), "Steuerbord");
        assert_eq!(facing_label(-90.0, 150.0), "Backbord");
        assert_eq!(facing_label(180.0, 240.0), "achteraus");
        assert_eq!(facing_label(-120.0, 220.0), "Backbord");
        assert_eq!(facing_label(0.0, 360.0), "rundum");
        for status in [
            ShipStatus::Active,
            ShipStatus::Routing,
            ShipStatus::Escaped,
            ShipStatus::Disabled,
            ShipStatus::Destroyed,
        ] {
            assert!(!status_label(status).is_empty());
        }
    }

    #[test]
    fn modules_of_a_damaged_ship_show_their_condition() {
        let data = data();
        let (battle, _) = stepped(&data, MIXED, 700);
        // Nach einem langen Gefecht gibt es Schiffe mit beschädigten und ausgefallenen
        // Modulen; die Tafel benennt jeden Zustand aus den Daten des Kerns.
        let (mut damaged, mut failed, mut intact) = (0, 0, 0);
        for ship in &battle.ships {
            let design = &data.designs[ship.design as usize];
            assert_eq!(design.modules.len(), ship.modules.len());
            for (slot, state) in design.modules.iter().zip(&ship.modules) {
                match module_condition(state.hp, data.modules[slot.module_id as usize].hp) {
                    "beschädigt" => damaged += 1,
                    "ausgefallen" => failed += 1,
                    _ => intact += 1,
                }
            }
        }
        assert!(
            damaged > 0 && failed > 0 && intact > 0,
            "{damaged} / {failed} / {intact}"
        );
    }
}
