//! Tafeln der Schlachtansicht (egui): Kopfleiste mit Gefechtszeit und Zeitsteuerung, je
//! Partei eine Lagetafel mit Befehlen, die Tafel des gewählten Schiffs mit seinen Modulen,
//! die Bilanz am Ende und, im Bildschirmraum, die taktischen Markierungen.
//!
//! Wie in der übrigen Oberfläche sind die Tafeln reine Funktionen: Sie lesen den Feed,
//! zeichnen und geben Aktionen zurück. Erst das System am Ende wendet sie an.

use super::camera::{CameraRequest, HudLayout, LAYOUT_GAP, Projector, SelectedShip, ViewOptions};
use super::feed::{BattleFeed, BattleOrder, BattleViewRequest};
use super::{Part, ViewFrame, ViewTimings};
use crate::pointer::UiCover;
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
/// Abstand der Tafeln von der Kopfleiste und vom Fensterrand.
const PANEL_GAP: f32 = 11.0;
const EDGE_GAP: f32 = 8.0;
/// Breite der Lagetafeln: so viel, wie die Tabelle der Klassen mit ihren fünf Spalten
/// braucht, wenn die Spalten im Abstand [`COLUMN_GAP`] stehen.
const SIDE_PANEL_WIDTH: f32 = 286.0;
const COLUMN_GAP: f32 = 6.0;
/// Weniger Höhe bekommt die Modulliste nie, auch nicht in sehr niedrigen Fenstern.
const MIN_MODULE_LIST: f32 = 60.0;

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

/// Verbleib der Schiffe einer Partei, je Klasse. Jedes Schiff, mit dem die Partei begann,
/// steht in genau einer Spalte: Die Spalten einer Klasse ergeben zusammen zu jeder Zeit
/// ihre Anfangszahl. Bordjäger zählen von Anfang an mit; solange sie an Bord sind, teilen
/// sie das Los ihres Trägers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Tally {
    /// Kämpft.
    pub fighting: [u32; 6],
    /// Auf dem Rückzug, aber noch auf dem Feld.
    pub withdrawing: [u32; 6],
    /// Treibt ohne Energie auf dem Feld.
    pub adrift: [u32; 6],
    pub lost: [u32; 6],
    pub escaped: [u32; 6],
}

impl Tally {
    pub fn of(battle: &Battle, side: usize) -> Self {
        let mut tally = Self {
            // Was verloren ging, zählt der Kern: Mit einem Träger gehen auch die Jäger in
            // seinem Hangar unter, und denen sieht man es am Wrack nicht mehr an.
            lost: battle.combatants[side].lost,
            ..Default::default()
        };
        let fighters = ShipClass::Fighter.index();
        for ship in battle
            .ships
            .iter()
            .filter(|ship| ship.side as usize == side)
        {
            let column = match ship.status {
                ShipStatus::Active => &mut tally.fighting,
                ShipStatus::Routing => &mut tally.withdrawing,
                ShipStatus::Disabled => &mut tally.adrift,
                ShipStatus::Escaped => &mut tally.escaped,
                ShipStatus::Destroyed => continue,
            };
            column[ship.class.index()] += 1;
            column[fighters] += ship.hangar;
        }
        tally
    }

    /// Schiffe einer Klasse, über alle Spalten.
    #[cfg(test)]
    pub fn total(&self, class: usize) -> u32 {
        self.fighting[class]
            + self.withdrawing[class]
            + self.adrift[class]
            + self.lost[class]
            + self.escaped[class]
    }

    /// Noch auf dem Feld und nicht ohne Energie: kämpfend oder auf dem Rückzug.
    pub fn under_way(&self, class: usize) -> u32 {
        self.fighting[class] + self.withdrawing[class]
    }
}

/// Lage einer Partei, einmal je Frame aus dem Gefecht gezählt.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SideSummary {
    pub tally: Tally,
    /// Anteil des anfänglichen Kampfwerts, der noch kämpft.
    pub strength: f32,
    /// Mittlere Moral der Schiffe auf dem Feld, 0..1.
    pub morale: f32,
    /// Jäger, die noch im Hangar eines Trägers auf dem Feld stehen.
    pub hangar: u32,
}

pub fn summarize(battle: &Battle, data: &GameData, side: usize) -> SideSummary {
    let mut summary = SideSummary {
        tally: Tally::of(battle, side),
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
        if ship.status.in_play() {
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

/// Eine Zahl der Tabellen: Null tritt zurück, damit ins Auge fällt, wo etwas steht.
fn count(ui: &mut egui::Ui, value: u32, color: Option<Color32>) {
    let text = RichText::new(value.to_string());
    ui.label(match (value, color) {
        (0, _) => text.color(MUTED),
        (_, Some(color)) => text.color(color),
        (_, None) => text,
    });
}

/// Kopfzeile einer Tabelle: kleine Überschriften, jede mit einer Erklärung.
fn column_titles(ui: &mut egui::Ui, titles: &[(&str, &str)]) {
    for (title, meaning) in titles {
        let label = ui.label(RichText::new(*title).color(MUTED).small());
        if !meaning.is_empty() {
            label.on_hover_text(*meaning);
        }
    }
    ui.end_row();
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

/// Rahmen der Tafeln über der Schlacht. Er deckt, wie alle Fenster der Oberfläche
/// (`panels::apply_style`): Sonst schienen Markierungen, Namen und Leuchtspuren in die
/// Zahlen.
fn panel_frame(ctx: &egui::Context) -> egui::Frame {
    egui::Frame::window(&ctx.global_style())
}

/// Zeichnet alle Tafeln der Schlachtansicht. Gibt zurück, was Kopfleiste, Lagetafeln und
/// die Tafel des gewählten Schiffs vom Bild belegen; die Kamera stellt bei der Übersicht
/// kein Schiff darunter.
pub fn battle_hud(
    root: &mut egui::Ui,
    refs: &HudRefs,
    actions: &mut Vec<HudAction>,
) -> Option<HudLayout> {
    let battle = refs.feed.battle()?;
    let screen = root.ctx().content_rect();
    // Platz, den eine Tafel an ihrem Bildrand belegt, mit Abstand. Aus ihrer Größe und
    // nicht aus ihrer Lage gerechnet: Ein neues Fenster steht im ersten Durchlauf noch
    // nicht an seinem Platz, seine Größe kennt es aber schon.
    let reserved = |rect: egui::Rect| rect.width() + EDGE_GAP + LAYOUT_GAP;
    markers(root.ctx(), refs, battle);
    let bar =
        egui::Panel::top("gefechtsleiste").show(root, |ui| top_bar(ui, refs, battle, actions));
    // Die Tafeln hängen unter der Kopfleiste, wie hoch sie auch ist: In schmalen Fenstern
    // hat sie zwei Zeilen.
    let top = bar.response.rect.bottom() + PANEL_GAP;
    let mut layout = HudLayout {
        top: bar.response.rect.bottom() - screen.top(),
        corner: Vec2::ZERO,
        column: None,
    };
    let ctx = root.ctx().clone();
    let mut panel_bottoms = [top - EDGE_GAP; 2];
    for side in 0..battle.combatants.len() {
        let rect = side_panel(&ctx, refs, battle, side, top, actions);
        if let (Some(bottom), Some(rect)) = (panel_bottoms.get_mut(side), rect) {
            *bottom = rect.bottom();
            layout.corner = layout.corner.max(Vec2::new(
                reserved(rect),
                top - screen.top() + rect.height() + LAYOUT_GAP,
            ));
        }
    }
    if let Some(ship) = refs
        .selected
        .and_then(|index| battle.ships.get(index as usize).map(|ship| (index, ship)))
    {
        // Die Tafel des Schiffs steht unter der Lagetafel ihrer Seite und darf den Raum
        // bis dorthin nutzen.
        let side = inspector_side(refs);
        let below = panel_bottoms[side] + EDGE_GAP;
        let rect = inspector(&ctx, refs, battle, ship.0, ship.1, below, actions);
        layout.column = rect.map(reserved);
    }
    if battle.finished && refs.options.summary {
        summary(&ctx, refs, battle, actions);
    }
    Some(layout)
}

/// Seite, auf der die Tafel des gewählten Schiffs steht (0 = links, 1 = rechts): nicht
/// dort, wo die höhere Lagetafel des Spielers mit ihren Befehlen steht.
fn inspector_side(refs: &HudRefs) -> usize {
    if refs.feed.info.player_side == Some(1) {
        0
    } else {
        1
    }
}

fn top_bar(ui: &mut egui::Ui, refs: &HudRefs, battle: &Battle, actions: &mut Vec<HudAction>) {
    let speed = refs.speed;
    // Beide Hälften der Leiste melden Aktionen.
    let actions = std::cell::RefCell::new(actions);
    let left = |ui: &mut egui::Ui| {
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
            actions.borrow_mut().push(HudAction::TogglePause);
        }
        for (level, label) in SPEED_LABELS.iter().enumerate() {
            let active = !speed.paused && speed.level == level;
            if ui
                .selectable_label(active, *label)
                .on_hover_text(format!("Taste {}", level + 1))
                .clicked()
            {
                actions.borrow_mut().push(HudAction::SetSpeed(level));
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
    };
    let right = |ui: &mut egui::Ui| {
        if ui.button("Verlassen").clicked() {
            actions
                .borrow_mut()
                .push(HudAction::Request(BattleViewRequest::Leave));
        }
        if ui.button("Hilfe").on_hover_text("F1").clicked() {
            actions.borrow_mut().push(HudAction::ToggleHelp);
        }
        ui.separator();
        if battle.finished
            && ui
                .selectable_label(refs.options.summary, "Bilanz")
                .on_hover_text("Bilanz des Gefechts ein- und ausblenden")
                .clicked()
        {
            actions.borrow_mut().push(HudAction::ToggleSummary);
        }
        if ui
            .selectable_label(refs.options.labels, "Namen")
            .on_hover_text("Namen an nahen Schiffen (N)")
            .clicked()
        {
            actions.borrow_mut().push(HudAction::ToggleLabels);
        }
        if ui
            .selectable_label(refs.options.overlay, "Taktik")
            .on_hover_text("Flottenanker, Reichweiten und Ziellinien (T)")
            .clicked()
        {
            actions.borrow_mut().push(HudAction::ToggleOverlay);
        }
        ui.separator();
        if ui
            .button("Gefecht")
            .on_hover_text("Zum Schwerpunkt des Gefechts (G)")
            .clicked()
        {
            actions
                .borrow_mut()
                .push(HudAction::Camera(CameraRequest::FocusFight));
        }
        if ui
            .button("Übersicht")
            .on_hover_text("Alle Schiffe auf dem Feld im Blick (U)")
            .clicked()
        {
            actions
                .borrow_mut()
                .push(HudAction::Camera(CameraRequest::Overview));
        }
        // Das Mausrad ändert die Fluggeschwindigkeit; ohne diese Anzeige sähe man davon
        // nichts.
        ui.label(
            RichText::new(format!("Flug {}×", decimal(refs.frame.fly_speed as f64, 1)))
                .color(MUTED),
        )
        .on_hover_text("Fluggeschwindigkeit der Kamera. Das Mausrad ändert sie.");
    };
    panels::split_bar(ui, left, right);
}

/// Lagetafel einer Partei; für die Partei des Spielers mit Befehlen. `top` ist ihre
/// Oberkante. Gibt zurück, wo die Tafel steht.
fn side_panel(
    ctx: &egui::Context,
    refs: &HudRefs,
    battle: &Battle,
    side: usize,
    top: f32,
    actions: &mut Vec<HudAction>,
) -> Option<egui::Rect> {
    let combatant = &battle.combatants[side];
    let summary = summarize(battle, refs.data, side);
    let color = color32(refs.feed.side_color(side));
    // Die ersten beiden Parteien stehen links und rechts, weitere dazwischen.
    let (anchor, offset) = match side {
        0 => (Align2::LEFT_TOP, [EDGE_GAP, top]),
        1 => (Align2::RIGHT_TOP, [-EDGE_GAP, top]),
        more => (Align2::CENTER_TOP, [(more as f32 - 2.5) * 270.0, top]),
    };
    egui::Window::new(format!("partei_{side}"))
        .title_bar(false)
        .resizable(false)
        .frame(panel_frame(ctx))
        .anchor(anchor, offset)
        .show(ctx, |ui| {
            ui.set_width(SIDE_PANEL_WIDTH);
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
            // Jede Zeile ergibt die Anfangszahl der Klasse: Auch wer sich zurückzieht oder
            // treibt, steht in einer Spalte. Die Spalten stehen dicht, damit die Tafel
            // schmal bleibt.
            let spacing = egui::vec2(COLUMN_GAP, ui.spacing().item_spacing.y);
            egui::Grid::new(("klassen", side))
                .striped(true)
                .num_columns(6)
                .min_col_width(0.0)
                .spacing(spacing)
                .show(ui, |ui| {
                    column_titles(
                        ui,
                        &[
                            ("Klasse", ""),
                            (
                                "kämpft",
                                "Kämpft; Jäger an Bord eines kämpfenden Trägers zählen mit.",
                            ),
                            ("Rückzug", "Auf dem Rückzug, aber noch auf dem Feld."),
                            ("treibt", "Treibt ohne Energie auf dem Feld."),
                            ("verloren", "Zerstört."),
                            ("entkommen", "Hat das Feld verlassen."),
                        ],
                    );
                    let tally = &summary.tally;
                    for class in ShipClass::ALL {
                        let index = class.index();
                        if combatant.initial[index] == 0 {
                            continue;
                        }
                        ui.label(class.plural());
                        ui.label(tally.fighting[index].to_string());
                        count(ui, tally.withdrawing[index], Some(ACCENT));
                        count(ui, tally.adrift[index], None);
                        count(ui, tally.lost[index], Some(WARNING));
                        count(ui, tally.escaped[index], None);
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
        })
        .map(|shown| shown.response.rect)
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

/// Wie viele Module eines Schiffs ausgefallen und wie viele beschädigt sind.
pub fn module_damage(data: &GameData, ship: &BattleShip) -> (usize, usize) {
    let design = &data.designs[ship.design as usize];
    let (mut failed, mut damaged) = (0, 0);
    for (slot, state) in design.modules.iter().zip(&ship.modules) {
        match module_condition(state.hp, data.modules[slot.module_id as usize].hp) {
            "ausgefallen" => failed += 1,
            "beschädigt" => damaged += 1,
            _ => {}
        }
    }
    (failed, damaged)
}

/// Tafel des gewählten Schiffs: Zustand, Vorräte, Ziel und alle Module. `below` ist die
/// Höhe im Fenster, unter der sie bleiben muss (die Lagetafel darüber). Gibt zurück, wo
/// die Tafel steht.
fn inspector(
    ctx: &egui::Context,
    refs: &HudRefs,
    battle: &Battle,
    index: u32,
    ship: &BattleShip,
    below: f32,
    actions: &mut Vec<HudAction>,
) -> Option<egui::Rect> {
    let design = &refs.data.designs[ship.design as usize];
    let side_color = color32(refs.feed.side_color(ship.side as usize));
    // Die Tafel steht unten auf der Seite, auf der nicht die (höhere) Lagetafel des Spielers
    // mit ihren Befehlen steht.
    let (anchor, offset) = if inspector_side(refs) == 0 {
        (Align2::LEFT_BOTTOM, [EDGE_GAP, -EDGE_GAP])
    } else {
        (Align2::RIGHT_BOTTOM, [-EDGE_GAP, -EDGE_GAP])
    };
    let frame = panel_frame(ctx);
    // So hoch darf die ganze Tafel werden; was nach Kopf und Zustandsbalken davon bleibt,
    // gehört der Modulliste.
    let room = ctx.content_rect().bottom() - EDGE_GAP - below - frame.total_margin().sum().y;
    egui::Window::new("schiff")
        .title_bar(false)
        .resizable(false)
        .default_height(640.0)
        .frame(frame)
        .anchor(anchor, offset)
        .show(ctx, |ui| {
            ui.set_width(330.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new(&design.name).strong().color(ACCENT));
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

            // Was ausgefallen ist, steht schon in der Überschrift: Die Liste selbst kann
            // länger sein, als die Tafel hoch ist.
            let (failed, damaged) = module_damage(refs.data, ship);
            ui.horizontal(|ui| {
                ui.label(RichText::new("Module").strong());
                ui.label(RichText::new(design.modules.len().to_string()).color(MUTED));
                if failed > 0 {
                    ui.label(RichText::new(format!("{failed} ausgefallen")).color(WARNING));
                }
                if damaged > 0 {
                    ui.label(RichText::new(format!("{damaged} beschädigt")).color(ACCENT));
                }
                if failed + damaged == 0 {
                    ui.label(RichText::new("alle intakt").color(MUTED));
                }
            });
            let list_height =
                (room - ui.min_rect().height() - ui.spacing().item_spacing.y).max(MIN_MODULE_LIST);
            // Ein Rollbalken, der immer zu sehen ist, sobald die Liste nicht ganz passt; der
            // übliche erscheint erst unter dem Mauszeiger.
            ui.spacing_mut().scroll = egui::style::ScrollStyle::solid();
            egui::ScrollArea::vertical()
                // Je Schiff eine eigene Rollstellung: Ein neu gewähltes beginnt oben. Die
                // Epoche gehört dazu, denn die Nummern der Schiffe beginnen mit jedem Gefecht
                // von vorn, und was die Oberfläche sich merkt, überdauert das Gefecht.
                .id_salt(("module", refs.feed.epoch, index))
                // Genau so hoch, wie Raum ist (oder die Liste lang): Das Fenster selbst
                // merkt sich nur seine erste Höhe und gäbe sonst nie mehr her.
                .max_height(list_height)
                .min_scrolled_height(list_height)
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
        })
        .map(|shown| shown.response.rect)
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
        .frame(panel_frame(ctx))
        .default_pos([ctx.content_rect().center().x - 260.0, 150.0])
        .show(ctx, |ui| {
            ui.set_min_width(500.0);
            ui.label(
                RichText::new(outcome_text(refs.feed, battle, &outcome))
                    .strong()
                    .color(ACCENT),
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
                // Dieselbe Zählung wie auf den Lagetafeln; auch hier ergibt jede Zeile die
                // Anfangszahl.
                let tally = Tally::of(battle, side);
                egui::Grid::new(("bilanz", side))
                    .striped(true)
                    .num_columns(6)
                    .show(ui, |ui| {
                        column_titles(
                            ui,
                            &[
                                ("Klasse", ""),
                                ("anfangs", ""),
                                ("verloren", "Zerstört."),
                                ("entkommen", "Hat das Feld verlassen."),
                                ("treibt", "Treibt ohne Energie auf dem Feld."),
                                ("im Feld", "Hält das Feld; Jäger an Bord zählen mit."),
                            ],
                        );
                        for class in ShipClass::ALL {
                            let index = class.index();
                            if result.initial[index] == 0 {
                                continue;
                            }
                            ui.label(class.plural());
                            ui.label(result.initial[index].to_string());
                            count(ui, tally.lost[index], Some(WARNING));
                            count(ui, tally.escaped[index], None);
                            count(ui, tally.adrift[index], None);
                            ui.label(tally.under_way(index).to_string());
                            ui.end_row();
                        }
                    });
                ui.add_space(6.0);
            }
            ui.horizontal(|ui| {
                if refs.feed.info.can_restart
                    && ui
                        .button(RichText::new("Neu starten").strong())
                        .on_hover_text("Dasselbe Gefecht noch einmal von vorn")
                        .clicked()
                {
                    actions.push(HudAction::Request(BattleViewRequest::Restart));
                }
                if ui.button(RichText::new("Zurück").strong()).clicked() {
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

/// Markierung eines Wracks im Bildschirmraum: ein kleines Kreuz.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WreckMarker {
    pub center: egui::Pos2,
    /// Halbe Kantenlänge des Kreuzes in Pixeln.
    pub half: f32,
    pub alpha: f32,
    pub side: u16,
}

/// Berechnet die Markierungen der Wracks, die im Bild liegen und zu klein erscheinen, um
/// als Rumpf erkannt zu werden. So bleibt aus der Ferne zu lesen, wo ein Gefecht seine
/// Opfer gefordert hat; aus der Nähe sprechen Wrack und Trümmer für sich.
pub fn collect_wreck_markers(
    battle: &Battle,
    frame: &ViewFrame,
    projector: &Projector,
) -> Vec<WreckMarker> {
    // Zwischen zwei Takten liegt die Anzeige noch ein Stück vor dem letzten Takt.
    let back = if battle.finished {
        0.0
    } else {
        (1.0 - frame.alpha) * frame.tick_seconds
    };
    let pixel = projector.pixel();
    battle
        .wrecks
        .iter()
        .filter_map(|wreck| {
            let position = super::to_world(wreck.pos - wreck.vel * back);
            let (screen, depth) = projector.project(position)?;
            let margin = 20.0;
            if screen.x < -margin
                || screen.y < -margin
                || screen.x > projector.size.x + margin
                || screen.y > projector.size.y + margin
            {
                return None;
            }
            let alpha = marker_alpha(2.0 * wreck.radius / (pixel * depth));
            (alpha > 0.0).then(|| WreckMarker {
                center: egui::pos2(screen.x, screen.y),
                half: marker_outline(wreck.class).1 * 0.55,
                alpha,
                side: wreck.side,
            })
        })
        .collect()
}

/// Zeichnet Markierungen, Auswahlklammern und Namen auf die Hintergrundebene, also unter
/// alle Tafeln.
fn markers(ctx: &egui::Context, refs: &HudRefs, battle: &Battle) {
    let painter = ctx.layer_painter(egui::LayerId::background());
    let found = collect_markers(battle, refs.frame, &refs.frame.projector);
    let with_alpha = |color: Color32, alpha: f32| {
        Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), (alpha * 255.0) as u8)
    };
    // Wracks zuerst, unter den Markierungen der Schiffe: gedämpfte Parteifarbe.
    for wreck in collect_wreck_markers(battle, refs.frame, &refs.frame.projector) {
        let side = color32(refs.feed.side_color(wreck.side as usize));
        let dim = |channel: u8| (channel as f32 * 0.55 + 38.0) as u8;
        let stroke = Stroke::new(
            1.3,
            with_alpha(
                Color32::from_rgb(dim(side.r()), dim(side.g()), dim(side.b())),
                wreck.alpha * 0.85,
            ),
        );
        let (a, b) = (
            egui::vec2(wreck.half, wreck.half),
            egui::vec2(wreck.half, -wreck.half),
        );
        painter.line_segment([wreck.center - a, wreck.center + a], stroke);
        painter.line_segment([wreck.center - b, wreck.center + b], stroke);
    }
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
    mut cover: ResMut<UiCover>,
    mut layout: ResMut<HudLayout>,
) -> Result {
    let _span = timings.span(Part::Hud);
    let ctx = contexts.ctx_mut()?;
    let mut root = UiCover::root(ctx, "gefecht");
    let mut actions = Vec::new();
    let refs = HudRefs {
        feed: &feed,
        data: &data,
        frame: &frame,
        speed: &speed,
        selected: selected.0,
        options: &options,
    };
    if let Some(drawn) = battle_hud(&mut root, &refs, &mut actions) {
        layout.set_if_neq(drawn);
    }
    if ui_state.help_open {
        panels::help_window(root.ctx(), &mut ui_state.help_open, true);
    }
    // Was die Kopfleiste belegt, gehört nicht der Szene.
    cover.record(&root);
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
    use crate::ui::testing::{Painted, Screen, overlap};

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
            epoch: Some(feed.epoch),
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
            fly_speed: 1.0,
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
        let camera = super::super::camera::overview_transform(
            2400.0,
            Vec2::new(1600.0, 900.0),
            &HudLayout::default(),
        );
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
                frame(|ui| {
                    battle_hud(ui, &refs, &mut actions);
                });
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
        frame(|ui| {
            battle_hud(ui, &refs, &mut Vec::new());
        });
    }

    #[test]
    fn hud_draws_the_summary_after_the_battle() {
        let data = data();
        let (mut battle, mut feed) = stepped(&data, MIXED, 0);
        while !battle.finished {
            battle.step(&data);
            feed.refresh(&battle);
        }
        let camera = super::super::camera::overview_transform(
            2400.0,
            Vec2::new(1600.0, 900.0),
            &HudLayout::default(),
        );
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
        frame(|ui| {
            battle_hud(ui, &refs, &mut actions);
        });
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
        frame(|ui| {
            battle_hud(ui, &refs, &mut actions);
        });
        assert!(actions.is_empty());
        // Die Bilanz nennt den Ausgang und zählt je Partei, was der Kern gezählt hat.
        let outcome = matchup::outcome_of(&battle);
        let text = outcome_text(&feed, &battle, &outcome);
        match outcome.winner() {
            Some(winner) => assert!(text.contains(feed.side_name(winner))),
            None => assert!(text.contains("Keine Seite") || text.contains("Zeitgrenze")),
        }
        for (side, result) in outcome.sides.iter().enumerate() {
            let tally = summarize(&battle, &data, side).tally;
            // Verloren ist, was der Kern als verloren führt.
            assert_eq!(tally.lost, result.lost);
            for class in 0..6 {
                // Jede Zeile geht auf: Was verloren, entkommen, treibend oder noch im Feld
                // ist, ergibt zusammen, was anfangs da war (Bordjäger eingerechnet).
                assert_eq!(
                    tally.total(class),
                    result.initial[class],
                    "Partei {side}, Klasse {class}: {tally:?}"
                );
                // Im Feld steht, was nach dem Kern das Feld hält, dazu die Jäger an Bord;
                // entkommen ist, was er als entkommen führt, dazu die Jäger an Bord
                // entkommener Träger.
                assert!(tally.under_way(class) >= result.holding[class]);
                assert!(tally.escaped[class] >= result.escaped[class]);
            }
            // Nach dem Ende zieht sich niemand mehr zurück.
            assert_eq!(tally.withdrawing, [0; 6]);
            assert!(result.lost.iter().sum::<u32>() + result.escaped.iter().sum::<u32>() > 0);
        }
    }

    /// Die Spalten jeder Klasse ergeben zusammen ihre Anfangszahl, für jede Partei.
    fn assert_rows_add_up(battle: &Battle, when: &str) {
        for (side, combatant) in battle.combatants.iter().enumerate() {
            let tally = Tally::of(battle, side);
            for class in ShipClass::ALL {
                let index = class.index();
                assert_eq!(
                    tally.total(index),
                    combatant.initial[index],
                    "{when}, Takt {}, Partei {side}, {}: {tally:?}",
                    battle.tick,
                    class.plural()
                );
            }
        }
    }

    #[test]
    fn every_ship_stands_in_exactly_one_column_at_every_moment() {
        use super::super::practice::PRESETS;

        let data = data();
        for preset in &PRESETS {
            // Einmal bis zum Ende ausgefochten, einmal mit befohlenem Rückzug der ersten
            // Partei mitten im Gefecht.
            for retreat_at in [None, Some(250)] {
                let (mut battle, _) = stepped(&data, preset.fleets, 0);
                assert_rows_add_up(&battle, preset.name);
                // Schiffe der ersten Partei, die sich zurückziehen und noch auf dem Feld
                // sind: Höchststand über das Gefecht.
                let mut most_withdrawing = 0;
                while !battle.finished {
                    if Some(battle.tick) == retreat_at {
                        battle.order_retreat(0);
                    }
                    battle.step(&data);
                    assert_rows_add_up(&battle, preset.name);
                    let tally = Tally::of(&battle, 0);
                    most_withdrawing = most_withdrawing.max(tally.withdrawing.iter().sum());
                }
                if retreat_at.is_some() {
                    // Der befohlene Rückzug war in der Tabelle zu sehen, solange er lief,
                    // und am Ende ist entkommen, wer nicht verloren ging.
                    assert!(most_withdrawing > 0, "{}", preset.name);
                    let tally = Tally::of(&battle, 0);
                    assert_eq!(tally.fighting, [0; 6], "{}", preset.name);
                    assert_eq!(tally.withdrawing, [0; 6], "{}", preset.name);
                    assert!(tally.escaped.iter().sum::<u32>() > 0, "{}", preset.name);
                }
            }
        }
    }

    /// Die Zahlen einer Zeile der Lagetafel einer Partei, wie sie gezeichnet wurden.
    fn painted_row(painted: &Painted, panel: egui::Rect, label: &str) -> Vec<u32> {
        let (_, title) = painted
            .texts
            .iter()
            .find(|(text, rect)| text == label && panel.contains_rect(*rect))
            .unwrap_or_else(|| panic!("Zeile „{label}“ fehlt"));
        let mut cells: Vec<(f32, u32)> = painted
            .texts
            .iter()
            .filter(|(_, rect)| {
                panel.contains_rect(*rect)
                    && rect.left() > title.right()
                    && (rect.center().y - title.center().y).abs() < 4.0
            })
            .filter_map(|(text, rect)| Some((rect.left(), text.parse().ok()?)))
            .collect();
        cells.sort_by(|a, b| a.0.total_cmp(&b.0));
        cells.into_iter().map(|(_, value)| value).collect()
    }

    #[test]
    fn the_side_panel_shows_withdrawing_ships_and_every_row_adds_up() {
        // Die erste Partei zieht sich zurück; ihre Schiffe sind noch auf dem Feld.
        let data = data();
        let (mut battle, mut feed) = stepped(&data, MIXED, 200);
        battle.order_retreat(0);
        for _ in 0..20 {
            battle.step(&data);
        }
        feed.refresh(&battle);
        let scene = Scene::of(data, battle, feed);
        let mut screen = Screen::new(1600.0, 900.0);
        let painted = scene.settle(&mut screen, None);
        for side in 0..2 {
            let panel = window_rect(&screen, &format!("partei_{side}"));
            for title in ["kämpft", "Rückzug", "treibt", "verloren", "entkommen"] {
                assert!(
                    painted
                        .texts
                        .iter()
                        .any(|(text, rect)| text == title && panel.contains_rect(*rect)),
                    "Spalte „{title}“ fehlt auf der Tafel der Partei {side}"
                );
            }
            let combatant = &scene.battle.combatants[side];
            let tally = Tally::of(&scene.battle, side);
            for class in ShipClass::ALL {
                let index = class.index();
                if combatant.initial[index] == 0 {
                    continue;
                }
                let row = painted_row(&painted, panel, class.plural());
                assert_eq!(
                    row,
                    [
                        tally.fighting[index],
                        tally.withdrawing[index],
                        tally.adrift[index],
                        tally.lost[index],
                        tally.escaped[index]
                    ],
                    "Partei {side}, {}",
                    class.plural()
                );
                assert_eq!(row.iter().sum::<u32>(), combatant.initial[index]);
            }
        }
        // Die Tabelle passt in ihre Tafel, auch mit der längsten Klassenbezeichnung und
        // vierstelligen Zahlen in jeder Spalte: Nichts ragt über den Rand.
        for side in 0..2 {
            let panel = window_rect(&screen, &format!("partei_{side}"));
            assert!(panel.width() <= SIDE_PANEL_WIDTH + 16.0, "{panel:?}");
            let title = painted
                .texts
                .iter()
                .find(|(text, rect)| text == "entkommen" && panel.contains_rect(*rect))
                .map(|(_, rect)| *rect)
                .expect("Spalte „entkommen“");
            // Platz für vier Ziffern unter der letzten Überschrift.
            assert!(
                title.left() + 30.0 <= panel.right() - 6.0,
                "{title:?} in {panel:?}"
            );
        }
        // Wer sich zurückzieht, steht jetzt in seiner Spalte und nicht mehr nirgends.
        let withdrawing = Tally::of(&scene.battle, 0).withdrawing;
        assert!(withdrawing.iter().sum::<u32>() >= 5, "{withdrawing:?}");
        // Die Tafeln stehen nebeneinander, auch im schmalen Fenster.
        let mut narrow = Screen::new(1024.0, 600.0);
        scene.settle(&mut narrow, None);
        let (left, right) = (
            window_rect(&narrow, "partei_0"),
            window_rect(&narrow, "partei_1"),
        );
        assert!(left.right() < right.left(), "{left:?} und {right:?}");
        assert!(left.left() >= 0.0 && right.right() <= 1024.0);
    }

    /// Was die Tafeln für einen Zustand der Ansicht brauchen, an einem Stück.
    struct Scene {
        data: GameData,
        battle: Battle,
        feed: BattleFeed,
        frame: ViewFrame,
        speed: SimSpeed,
        options: ViewOptions,
    }

    impl Scene {
        fn new(ticks: u32) -> Self {
            let data = data();
            let (battle, feed) = stepped(&data, MIXED, ticks);
            Self::of(data, battle, feed)
        }

        fn finished() -> Self {
            let data = data();
            let (mut battle, mut feed) = stepped(&data, MIXED, 0);
            while !battle.finished {
                battle.step(&data);
                feed.refresh(&battle);
            }
            Self::of(data, battle, feed)
        }

        fn of(data: GameData, battle: Battle, feed: BattleFeed) -> Self {
            let camera = super::super::camera::overview_transform(
                2400.0,
                Vec2::new(1600.0, 900.0),
                &HudLayout::default(),
            );
            let frame = view_frame(&feed, &camera);
            Self {
                data,
                battle,
                feed,
                frame,
                speed: SimSpeed::default(),
                options: ViewOptions::default(),
            }
        }

        /// Das Gefecht wird als neues angesagt, wie beim nächsten Gefecht der Übung oder
        /// nach „Neu starten“.
        fn begin_again(&mut self) {
            self.feed
                .begin(&self.battle, super::super::feed::testing::info(2));
            self.frame.epoch = Some(self.feed.epoch);
        }

        fn refs(&self, selected: Option<u32>) -> HudRefs<'_> {
            HudRefs {
                feed: &self.feed,
                data: &self.data,
                frame: &self.frame,
                speed: &self.speed,
                selected,
                options: &self.options,
            }
        }

        /// Erstes Schiff einer Klasse und Partei.
        fn ship(&self, class: ShipClass, side: u16) -> u32 {
            self.battle
                .ships
                .iter()
                .position(|ship| ship.class == class && ship.side == side)
                .expect("Schiff der Klasse") as u32
        }

        /// Zeichnet die Tafeln, bis sie ihren Platz gefunden haben.
        fn settle(&self, screen: &mut Screen, selected: Option<u32>) -> Painted {
            let refs = self.refs(selected);
            screen.settle(|ui| {
                battle_hud(ui, &refs, &mut Vec::new());
            })
        }

        /// Klickt auf eine Stelle und gibt zurück, was die Tafeln dabei auslösen.
        fn click(
            &self,
            screen: &mut Screen,
            selected: Option<u32>,
            at: egui::Pos2,
        ) -> Vec<HudAction> {
            let refs = self.refs(selected);
            let mut actions = Vec::new();
            screen.click(at, |ui| {
                battle_hud(ui, &refs, &mut actions);
            });
            actions
        }

        /// Klickt auf den Knopf mit dieser Aufschrift.
        fn press(&self, screen: &mut Screen, selected: Option<u32>, label: &str) -> Vec<HudAction> {
            let at = self
                .settle(screen, selected)
                .text(label)
                .unwrap_or_else(|| panic!("„{label}“ ist nicht zu sehen"))
                .center();
            self.click(screen, selected, at)
        }
    }

    fn window_rect(screen: &Screen, title: &str) -> egui::Rect {
        screen
            .ctx
            .memory(|memory| memory.area_rect(egui::Id::new(Some(title))))
            .unwrap_or_else(|| panic!("Tafel „{title}“ fehlt"))
    }

    #[test]
    fn every_button_reports_its_action() {
        let scene = Scene::new(200);
        let mut screen = Screen::new(1600.0, 900.0);
        let ship = scene.ship(ShipClass::Cruiser, 0);
        let selected = Some(ship);
        let combatant = &scene.battle.combatants[0];
        assert_eq!(
            (combatant.formation, combatant.doctrine),
            (Formation::Line, Doctrine::Balanced)
        );

        // Kopfleiste.
        for (label, expected) in [
            ("▶ Weiter", HudAction::TogglePause),
            ("½×", HudAction::SetSpeed(0)),
            ("25×", HudAction::SetSpeed(5)),
            ("Namen", HudAction::ToggleLabels),
            ("Taktik", HudAction::ToggleOverlay),
            ("Gefecht", HudAction::Camera(CameraRequest::FocusFight)),
            ("Übersicht", HudAction::Camera(CameraRequest::Overview)),
            ("Hilfe", HudAction::ToggleHelp),
            ("Verlassen", HudAction::Request(BattleViewRequest::Leave)),
            // Befehle an die eigene Partei.
            (
                "Keil",
                HudAction::Order(BattleOrder::SetOrders {
                    side: 0,
                    formation: Formation::Wedge,
                    doctrine: Doctrine::Balanced,
                }),
            ),
            (
                "defensiv",
                HudAction::Order(BattleOrder::SetOrders {
                    side: 0,
                    formation: Formation::Line,
                    doctrine: Doctrine::Defensive,
                }),
            ),
            (
                "Rückzug befehlen",
                HudAction::Order(BattleOrder::Retreat { side: 0 }),
            ),
            // Tafel des gewählten Schiffs.
            (
                "Heranholen",
                HudAction::Camera(CameraRequest::FocusShip(ship)),
            ),
            ("Verfolgen", HudAction::ToggleFollow),
            ("×", HudAction::Select(None)),
        ] {
            assert_eq!(
                scene.press(&mut screen, selected, label),
                vec![expected],
                "Knopf „{label}“"
            );
        }
        // Was schon gilt, löst nichts aus; ein Klick neben die Knöpfe auch nicht.
        assert!(scene.press(&mut screen, selected, "Linie").is_empty());
        assert!(scene.press(&mut screen, selected, "ausgewogen").is_empty());
        assert!(scene.press(&mut screen, selected, "Kampfwert").is_empty());
        // Das Ziel des Schiffs ist ein Verweis: Ein Klick wählt es.
        let target = scene.battle.ships[ship as usize].target.expect("ein Ziel");
        let name = &scene.data.designs[scene.battle.ships[target as usize].design as usize].name;
        let link = format!(
            "{name} ({})",
            scene.battle.ships[target as usize].class.label()
        );
        assert_eq!(
            scene.press(&mut screen, selected, &link),
            vec![HudAction::Select(Some(target))]
        );
        // Die Befehle stehen nur auf der Tafel der eigenen Partei.
        let painted = scene.settle(&mut screen, selected);
        assert_eq!(painted.all("Rückzug befehlen").len(), 1);
        assert_eq!(painted.all("Keil").len(), 1);
    }

    #[test]
    fn the_summary_offers_restart_and_orders_are_locked_after_the_end() {
        let scene = Scene::finished();
        let mut screen = Screen::new(1600.0, 900.0);
        for (label, expected) in [
            (
                "Neu starten",
                HudAction::Request(BattleViewRequest::Restart),
            ),
            ("Zurück", HudAction::Request(BattleViewRequest::Leave)),
            ("Feld ansehen", HudAction::ToggleSummary),
            ("Bilanz", HudAction::ToggleSummary),
        ] {
            assert_eq!(
                scene.press(&mut screen, None, label),
                vec![expected],
                "Knopf „{label}“"
            );
        }
        // Nach dem Ende nimmt die Partei keine Befehle mehr an.
        for label in ["Keil", "defensiv", "Rückzug befehlen"] {
            assert!(
                scene.press(&mut screen, None, label).is_empty(),
                "„{label}“ ist gesperrt"
            );
        }
    }

    #[test]
    fn the_inspector_lists_modules_down_to_the_window_edge() {
        let scene = Scene::new(0);
        let bastion = scene.ship(ShipClass::Battleship, 0);
        let modules = scene.data.designs[scene.battle.ships[bastion as usize].design as usize]
            .modules
            .len();
        assert!(modules > 20, "das Schlachtschiff hat viele Module");
        // Vor dem ersten Schuss ist jedes Modul intakt: Jede sichtbare Zeile der Liste
        // trägt genau dieses Wort.
        let rows = |painted: &Painted| painted.all("intakt").len();

        // Im üblichen Fenster passt nicht alles; die Liste nutzt den Raum bis unter die
        // Lagetafel, und ein Rollbalken zeigt, dass sie weitergeht.
        let mut screen = Screen::new(1600.0, 900.0);
        let painted = scene.settle(&mut screen, Some(bastion));
        let shown = rows(&painted);
        assert!(
            shown >= 14 && shown < modules,
            "{shown} von {modules} Modulzeilen bei 900 Pixeln Höhe"
        );
        let tafel = window_rect(&screen, "schiff");
        let above = window_rect(&screen, "partei_1");
        assert!(tafel.top() >= above.bottom(), "die Tafeln überdecken sich");
        assert!(
            tafel.top() < above.bottom() + 40.0,
            "zwischen den Tafeln bleibt Raum ungenutzt: {} bis {}",
            above.bottom(),
            tafel.top()
        );
        let handle = |painted: &Painted, tafel: egui::Rect| {
            painted
                .rects
                .iter()
                .filter(|(rect, color)| {
                    tafel.contains_rect(*rect)
                        && rect.right() > tafel.right() - 16.0
                        && rect.width() <= 10.0
                        && rect.height() >= 24.0
                        && color.a() > 100
                })
                .count()
        };
        assert!(
            handle(&painted, tafel) >= 1,
            "kein sichtbarer Rollbalken, obwohl Module verdeckt sind"
        );
        // Die Überschrift nennt die Zahl der Module, auch der verdeckten.
        assert!(painted.text(&modules.to_string()).is_some());
        assert!(painted.text("alle intakt").is_some());

        // In einem hohen Fenster stehen alle Module da, ohne Rollbalken.
        let mut tall = Screen::new(1600.0, 1200.0);
        let painted = scene.settle(&mut tall, Some(bastion));
        assert_eq!(rows(&painted), modules);
        assert_eq!(handle(&painted, window_rect(&tall, "schiff")), 0);

        // In einem niedrigen Fenster schrumpft die Liste, bleibt aber benutzbar.
        let mut low = Screen::new(1600.0, 620.0);
        let painted = scene.settle(&mut low, Some(bastion));
        assert!((2..shown).contains(&rows(&painted)));
        assert!(window_rect(&low, "schiff").bottom() <= 620.0);
    }

    #[test]
    fn a_newly_selected_ship_starts_at_the_top_of_its_module_list() {
        let scene = Scene::new(0);
        let (own, other) = (
            scene.ship(ShipClass::Battleship, 0),
            scene.ship(ShipClass::Battleship, 1),
        );
        let mut screen = Screen::new(1600.0, 900.0);
        let painted = scene.settle(&mut screen, Some(own));
        // Die Liste beginnt mit den Antrieben.
        let drives = painted.all("Fusionsantrieb").len();
        assert!(drives >= 2);
        // Nach unten blättern, bis die Antriebe verschwunden sind.
        let list = painted.all("intakt")[4].center();
        let refs = scene.refs(Some(own));
        let wheel = egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Line,
            delta: egui::vec2(0.0, -6.0),
            phase: egui::TouchPhase::Move,
            modifiers: egui::Modifiers::default(),
        };
        screen.run(vec![egui::Event::PointerMoved(list)], |ui| {
            battle_hud(ui, &refs, &mut Vec::new());
        });
        screen.run(vec![wheel], |ui| {
            battle_hud(ui, &refs, &mut Vec::new());
        });
        // Die Liste rollt weich aus; das dauert einige Durchläufe.
        for _ in 0..12 {
            scene.settle(&mut screen, Some(own));
        }
        let scrolled = scene.settle(&mut screen, Some(own));
        assert_eq!(scrolled.all("Fusionsantrieb").len(), 0, "die Liste rollt");
        // Ein anderes Schiff derselben Klasse: Seine Liste beginnt wieder oben.
        let painted = scene.settle(&mut screen, Some(other));
        assert_eq!(painted.all("Fusionsantrieb").len(), drives);
        // Zurück beim ersten Schiff steht dessen Liste, wo man sie verließ.
        let painted = scene.settle(&mut screen, Some(own));
        assert_eq!(painted.all("Fusionsantrieb").len(), 0);

        // Das nächste Gefecht, im selben Programm: Seine Schiffe tragen wieder dieselben
        // Nummern, haben mit denen des alten aber nichts zu tun. Auch ihre Listen beginnen
        // oben, die des Schiffs mit der Nummer des eben gerollten eingeschlossen.
        let mut scene = scene;
        scene.begin_again();
        for ship in [own, other] {
            let painted = scene.settle(&mut screen, Some(ship));
            assert_eq!(
                painted.all("Fusionsantrieb").len(),
                drives,
                "die Liste von Schiff {ship} ist im neuen Gefecht noch gerollt"
            );
        }
    }

    #[test]
    fn failed_modules_are_announced_above_the_list() {
        let mut scene = Scene::new(0);
        let bastion = scene.ship(ShipClass::Battleship, 0) as usize;
        // Die letzten Module der Liste (weit unter dem sichtbaren Teil) fallen aus oder
        // werden beschädigt.
        let count = scene.battle.ships[bastion].modules.len();
        let mut battle = scene.battle.clone();
        battle.ships[bastion].modules[count - 1].hp = 0.0;
        battle.ships[bastion].modules[count - 2].hp = 0.0;
        battle.ships[bastion].modules[count - 3].hp *= 0.5;
        scene.feed.refresh(&battle);
        scene.battle = battle;
        assert_eq!(
            module_damage(&scene.data, &scene.battle.ships[bastion]),
            (2, 1)
        );
        let mut screen = Screen::new(1600.0, 900.0);
        let painted = scene.settle(&mut screen, Some(bastion as u32));
        assert!(painted.text("2 ausgefallen").is_some());
        assert!(painted.text("1 beschädigt").is_some());
        assert!(painted.text("alle intakt").is_none());
        // Die betroffenen Zeilen selbst liegen unter dem sichtbaren Teil der Liste.
        assert!(painted.all("ausgefallen").is_empty());
    }

    #[test]
    fn the_top_bar_never_overlaps_itself() {
        let scene = Scene::new(0);
        for (width, rows) in [(1600.0, 1), (1280.0, 1), (1024.0, 2), (800.0, 2)] {
            let mut screen = Screen::new(width, 720.0);
            let painted = scene.settle(&mut screen, None);
            let panel = window_rect(&screen, "partei_0");
            let bar: Vec<(String, egui::Rect)> = painted
                .texts
                .iter()
                .filter(|(_, rect)| rect.bottom() <= panel.top())
                .cloned()
                .collect();
            assert!(bar.len() >= 15, "{} Texte in der Leiste", bar.len());
            assert_eq!(overlap(&bar), None, "Kopfleiste bei {width} Pixeln Breite");
            for (text, rect) in &bar {
                assert!(
                    rect.left() >= 0.0 && rect.right() <= width,
                    "„{text}“ ragt bei {width} Pixeln Breite aus dem Fenster"
                );
            }
            // Im breiten Fenster bleibt die Leiste einzeilig, im schmalen bekommt sie eine
            // zweite Zeile; die Tafeln hängen in beiden Fällen darunter.
            let bottom = bar
                .iter()
                .map(|(_, rect)| rect.bottom())
                .fold(0.0, f32::max);
            assert_eq!(
                bottom > 30.0,
                rows == 2,
                "Leiste endet bei {bottom} ({width} Pixel breit)"
            );
            assert!(panel.top() >= bottom);
        }
    }

    #[test]
    fn panels_over_the_battle_are_opaque() {
        let scene = Scene::finished();
        let mut screen = Screen::new(1600.0, 900.0);
        // Neue Fenster blenden sich über einige Durchläufe ein; gemessen wird danach.
        for _ in 0..6 {
            scene.settle(&mut screen, Some(0));
        }
        let painted = scene.settle(&mut screen, Some(0));
        // Hintergründe der Kopfleiste, der Lagetafeln, der Schiffstafel und der Bilanz:
        // Durch keine scheint die Szene dahinter.
        let backgrounds: Vec<_> = painted
            .rects
            .iter()
            .filter(|(rect, _)| rect.width() >= 200.0 && rect.height() >= 20.0)
            .collect();
        assert!(backgrounds.len() >= 5, "{} Hintergründe", backgrounds.len());
        for (rect, color) in backgrounds {
            assert_eq!(color.a(), 255, "Hintergrund {rect:?} ist durchsichtig");
        }
        let mut open = true;
        for _ in 0..6 {
            screen.settle(|ui| panels::help_window(ui.ctx(), &mut open, true));
        }
        let help = screen.settle(|ui| panels::help_window(ui.ctx(), &mut open, true));
        assert!(
            help.rects
                .iter()
                .filter(|(rect, _)| rect.width() >= 200.0 && rect.height() >= 100.0)
                .all(|(_, color)| color.a() == 255)
        );
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
            // Es kämpft, was der Kern als kämpfend führt; die Jäger im Hangar eines
            // kämpfenden Trägers zählen dazu.
            let mut fighting = battle.active_by_class(side);
            fighting[ShipClass::Fighter.index()] += summary.hangar;
            assert_eq!(summary.tally.fighting, fighting);
        }
        // Der Träger der ersten Partei hat seine Jäger noch an Bord.
        assert_eq!(summarize(&battle, &data, 0).hangar, 24);
        assert_eq!(summarize(&battle, &data, 1).hangar, 0);
        assert_rows_add_up(&battle, "vor dem ersten Takt");

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
        let camera = super::super::camera::overview_transform(
            2400.0,
            Vec2::new(1600.0, 900.0),
            &HudLayout::default(),
        );
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
    fn wrecks_are_marked_from_afar() {
        let scene = Scene::finished();
        let battle = &scene.battle;
        assert!(
            battle.wrecks.len() > 5,
            "das Gefecht hat Wracks hinterlassen"
        );
        let projector = &scene.frame.projector;
        // Aus der Übersicht ist jedes Wrack im Bild klein und trägt ein Kreuz, dort, wo es
        // liegt.
        let marks = collect_wreck_markers(battle, &scene.frame, projector);
        let on_screen: Vec<_> = battle
            .wrecks
            .iter()
            .filter_map(|wreck| projector.project(to_world(wreck.pos)))
            .map(|(at, _)| at)
            .filter(|at| at.x > 0.0 && at.x < 1600.0 && at.y > 0.0 && at.y < 900.0)
            .collect();
        assert!(on_screen.len() > 5);
        assert!(marks.len() >= on_screen.len());
        for at in &on_screen {
            let mark = marks
                .iter()
                .find(|mark| {
                    (mark.center.x - at.x).abs() < 0.5 && (mark.center.y - at.y).abs() < 0.5
                })
                .expect("Kreuz am Ort des Wracks");
            assert!(mark.alpha > 0.8 && mark.half >= 1.5 && mark.half < 6.0);
        }
        // Aus der Nähe verschwindet das Kreuz: Das Wrack selbst ist zu erkennen.
        let wreck = &battle.wrecks[0];
        let at = to_world(wreck.pos);
        let near = Transform::from_translation(
            at + Vec3::new(0.0, wreck.radius * 2.0, wreck.radius * 4.0),
        )
        .looking_at(at, Vec3::Y);
        let close = view_frame(&scene.feed, &near);
        let marks = collect_wreck_markers(battle, &close, &close.projector);
        assert!(
            marks
                .iter()
                .all(|mark| mark.center.distance(egui::pos2(800.0, 450.0)) > 2.0),
            "das nahe Wrack trägt noch ein Kreuz"
        );
        // Die Hilfe erklärt das Zeichen.
        let mut open = true;
        let mut screen = Screen::new(1600.0, 900.0);
        let help = screen.settle(|ui| panels::help_window(ui.ctx(), &mut open, true));
        assert!(help.text("Kreuz = Wrack").is_some());
        // Die Tafeln zeichnen mit Wracks, ohne etwas auszulösen.
        let mut actions = Vec::new();
        let refs = scene.refs(None);
        screen.settle(|ui| {
            battle_hud(ui, &refs, &mut actions);
        });
        assert!(actions.is_empty());
    }

    #[test]
    fn battles_with_three_sides_and_unnamed_sides_draw() {
        use super::super::feed::testing::info;
        use sim::battle::matchup::Side;

        let data = data();
        let sides: Vec<Side> = [
            "speer:2,waechter:2",
            "bastion:1,dolch:3",
            "horst:1,waechter:2",
        ]
        .iter()
        .map(|fleet| Side::parse(&data, fleet).unwrap())
        .collect();
        let mut battle = matchup::build(&data, 3, &sides);
        // Einmal mit drei benannten Parteien, einmal ohne jede Angabe zu den Parteien:
        // Die Tafeln fallen dann auf Ersatznamen und Grau zurück.
        for named in [3, 0] {
            let mut feed = BattleFeed::default();
            feed.begin(&battle, info(named));
            for _ in 0..300 {
                battle.step(&data);
                feed.refresh(&battle);
            }
            let camera = super::super::camera::overview_transform(
                2400.0,
                Vec2::new(1600.0, 900.0),
                &HudLayout::default(),
            );
            let frame_data = view_frame(&feed, &camera);
            let speed = SimSpeed::default();
            let options = ViewOptions::default();
            for selected in [None, Some(0), Some(battle.ships.len() as u32 - 1)] {
                let refs = HudRefs {
                    feed: &feed,
                    data: &data,
                    frame: &frame_data,
                    speed: &speed,
                    selected,
                    options: &options,
                };
                let mut actions = Vec::new();
                frame(|ui| {
                    battle_hud(ui, &refs, &mut actions);
                });
                assert!(actions.is_empty());
            }
            assert_eq!(
                collect_markers(&battle, &frame_data, &frame_data.projector).len(),
                {
                    battle
                        .ships
                        .iter()
                        .filter(|ship| ship.status.in_play())
                        .count()
                }
            );
        }
        assert_eq!(BattleFeed::default().side_name(0), "Unbekannt");
    }

    #[test]
    fn help_draws_for_the_battle_view() {
        let mut open = true;
        frame(|ui| panels::help_window(ui.ctx(), &mut open, true));
        assert!(open);
        // Jede Zeile der Hilfe hat eine Taste und eine Erklärung.
        for (keys, what) in panels::BATTLE_KEYS {
            assert!(!keys.is_empty() && !what.is_empty());
        }
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
