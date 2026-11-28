//! Fenster „Analyse“: was die eigene Gemeinschaft über die anderen zu wissen glaubt.
//!
//! Alles hier ist Einschätzung. Die Haltung einer anderen Gemeinschaft ist verborgen;
//! das Fenster zeigt das eigene Dossier über sie (Schluss aus ihren öffentlichen Taten),
//! die Indizien, auf denen es beruht, und Schätzungen ihrer Flotte und Technik als
//! Spannen. Woher das Lesemodell kommt, steht in [`sim::diplomacy::view`]: Es nimmt keine
//! verborgene Haltung entgegen.

use super::diplomacy::{
    DETAIL_WIDTH, DiplomacyUi, LIST_WIDTH, TREATY_COLOR, TRUCE_COLOR, bar, date, ground, status,
};
use super::format::{grouped, percent};
use super::panels::{ACCENT, MUTED, SimRefs, color32, detail, window_room};
use bevy_egui::egui::{self, Color32, RichText};
use sim::diplomacy::occasion::Archetype;
use sim::diplomacy::sight::Estimate;
use sim::diplomacy::view::{DossierView, RelationView, relation_view};
use sim::ids::PolityId;

pub const TITLE: &str = "Analyse";
/// Steht über jedem Dossier: Was folgt, ist ein Schluss, kein Blick in die Führung.
pub const DISCLAIMER: &str = "Einschätzung aus ihren öffentlichen Taten. Ihre Haltung selbst \
                              ist uns verborgen; so beurteilen auch die anderen uns.";
pub const OVERTURNED: &str = "Das jüngste Verhalten passt nicht mehr zur bisherigen Einschätzung";

const TEXT: Color32 = Color32::from_rgb(225, 230, 240);

/// Farbe einer Grundhaltung in Balken und Indizien.
pub fn archetype_color(archetype: Archetype) -> Color32 {
    match archetype {
        Archetype::Cooperative => Color32::from_rgb(120, 200, 130),
        Archetype::Opportunistic => Color32::from_rgb(240, 190, 80),
        Archetype::Paranoid => Color32::from_rgb(120, 160, 230),
        Archetype::Expansionist => Color32::from_rgb(235, 110, 95),
    }
}

/// „45 % (35–55 %)“: Einschätzung mit Spanne.
pub fn guess_text(belief: f32, lo: f32, hi: f32) -> String {
    let (lo, hi) = ((lo * 100.0).round() as i32, (hi * 100.0).round() as i32);
    format!("{} ({lo}–{hi} %)", percent(belief))
}

/// Wohin ein Indiz weist: „spricht für kooperativ (+0,12)“; ein Indiz ohne Gewicht sagt
/// es so.
pub fn clue_direction(label: &str, shift: f32) -> String {
    if shift < 0.005 {
        "sagt wenig über die Haltung".into()
    } else {
        format!(
            "spricht für {label} (+{})",
            super::format::decimal(shift as f64, 2)
        )
    }
}

/// Spanne einer Schätzung in Worten.
fn range(estimate: Estimate, unit: &str) -> String {
    let (lo, hi) = (estimate.lo as f64, estimate.hi as f64);
    if (hi - lo).abs() < 0.5 {
        format!("{} {unit}", grouped(lo))
    } else {
        format!("{} – {} {unit}", grouped(lo), grouped(hi))
    }
}

pub fn analysis_window(
    ctx: &egui::Context,
    area: egui::Rect,
    sim: &SimRefs,
    state: &mut DiplomacyUi,
) {
    let mut open = state.analysis_open;
    let room = window_room(area);
    egui::Window::new(TITLE)
        .open(&mut open)
        .collapsible(true)
        .default_pos([area.left() + 460.0, area.top() + 24.0])
        // So groß wie der Inhalt; die Einzelheiten rollen, wo der Platz bis zum unteren
        // Rand nicht reicht.
        .auto_sized()
        .constrain_to(area)
        .show(ctx, |ui| {
            let Some(me) = sim.config.player else {
                ui.label("Beobachter führen keine Dossiers.");
                return;
            };
            ui.set_width(LIST_WIDTH + 12.0 + DETAIL_WIDTH);
            let others: Vec<PolityId> = sim
                .polities
                .alive()
                .map(|polity| polity.id)
                .filter(|id| *id != me)
                .collect();
            if others.is_empty() {
                ui.label("Es gibt keine andere Gemeinschaft mehr.");
                return;
            }
            if state
                .analysis_chosen
                .is_none_or(|chosen| !others.contains(&chosen))
            {
                state.analysis_chosen = others.first().copied();
            }
            let Some(other) = state.analysis_chosen else {
                return;
            };
            let ground = ground(sim);
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(LIST_WIDTH);
                    egui::ScrollArea::vertical()
                        .id_salt("analyse_liste")
                        .max_height((room - 20.0).max(120.0))
                        .show(ui, |ui| {
                            for id in &others {
                                list_entry(ui, sim, me, *id, &mut state.analysis_chosen);
                            }
                        });
                });
                // Kein Trennstrich: Er reichte in einem Fenster, das so groß ist wie sein
                // Inhalt, bis an den unteren Rand.
                ui.add_space(12.0);
                ui.vertical(|ui| {
                    ui.set_width(DETAIL_WIDTH);
                    let view = relation_view(&ground, me, other);
                    egui::ScrollArea::vertical()
                        .id_salt("analyse_einzelheiten")
                        .max_height((room - 20.0).max(120.0))
                        .show(ui, |ui| {
                            ui.set_width(DETAIL_WIDTH - 16.0);
                            dossier_view(ui, sim, &view);
                        });
                });
            });
        });
    state.analysis_open = open;
}

/// Eine Zeile der Liste: Name und die führende Vermutung mit ihrer Klarheit.
fn list_entry(
    ui: &mut egui::Ui,
    sim: &SimRefs,
    me: PolityId,
    id: PolityId,
    chosen: &mut Option<PolityId>,
) {
    let polity = sim.polities.get(id);
    let text = RichText::new(&polity.name)
        .color(color32(polity.color))
        .strong();
    let relation = status(sim, me, id);
    if ui
        .selectable_label(*chosen == Some(id), text)
        .on_hover_text(&relation.hover)
        .clicked()
    {
        *chosen = Some(id);
    }
    let dossier = DossierView::of(&sim.diplomacy.dossier(me, id));
    ui.horizontal(|ui| {
        ui.add_space(8.0);
        match dossier.guesses.first() {
            Some(lead) if dossier.weight >= 1.0 => {
                ui.label(
                    detail(format!("eher {}", lead.label)).color(archetype_color(lead.archetype)),
                );
            }
            _ => {
                ui.label(detail("noch unklar").color(MUTED));
            }
        }
    });
    ui.add_space(2.0);
}

/// Die vier Grundhaltungen als Balken: Mitte der Einschätzung und ihre Spanne.
pub fn guesses(ui: &mut egui::Ui, dossier: &DossierView, id: &str) {
    egui::Grid::new(id).num_columns(3).show(ui, |ui| {
        for guess in &dossier.guesses {
            let color = archetype_color(guess.archetype);
            ui.label(RichText::new(guess.label).color(color));
            band_bar(ui, 220.0, guess.belief, guess.lo, guess.hi, color);
            ui.label(guess_text(guess.belief, guess.lo, guess.hi));
            ui.end_row();
        }
    });
}

/// Ein Balken mit getönter Spanne und einer Marke an der Mitte der Einschätzung.
fn band_bar(ui: &mut egui::Ui, width: f32, belief: f32, lo: f32, hi: f32, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 12.0), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 2.0, Color32::from_rgb(38, 44, 58));
    let at = |share: f32| rect.left() + width * share.clamp(0.0, 1.0);
    let band = egui::Rect::from_min_max(
        egui::pos2(at(lo), rect.top() + 2.0),
        egui::pos2(at(hi).max(at(lo) + 1.0), rect.bottom() - 2.0),
    );
    painter.rect_filled(band, 1.0, color.gamma_multiply(0.35));
    let mut filled = rect;
    filled.set_width(width * belief.clamp(0.0, 1.0));
    painter.rect_filled(filled.shrink2(egui::vec2(0.0, 4.0)), 1.0, color);
    let x = at(belief);
    painter.line_segment(
        [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
        egui::Stroke::new(2.0, color),
    );
}

fn heading(ui: &mut egui::Ui, text: &str) {
    ui.add_space(6.0);
    ui.label(RichText::new(text).color(ACCENT).strong());
}

fn dossier_view(ui: &mut egui::Ui, sim: &SimRefs, view: &RelationView) {
    let other = sim.polities.get(view.other);
    ui.horizontal(|ui| {
        ui.heading(RichText::new(&other.name).color(color32(other.color)));
        let relation = status(sim, view.viewer, view.other);
        ui.label(
            RichText::new(&relation.label)
                .color(relation.color)
                .strong(),
        )
        .on_hover_text(&relation.hover);
    });
    ui.label(detail(DISCLAIMER).italics().color(MUTED));
    let dossier = &view.dossier;
    if let Some(day) = dossier.overturned_day {
        ui.label(
            RichText::new(format!("{OVERTURNED} (seit {}).", date(day)))
                .color(TRUCE_COLOR)
                .strong(),
        );
    }
    heading(ui, "Vermutete Haltung");
    guesses(ui, dossier, "dossier_haltung");
    ui.label(
        detail(format!(
            "Stand der Aufklärung: {} · Gewicht der Indizien {} · Vertrauen, das ihr Verhalten \
             rechtfertigt: {}",
            dossier.clarity,
            super::format::decimal(dossier.weight as f64, 1),
            percent(dossier.trust)
        ))
        .color(MUTED),
    )
    .on_hover_text(
        "Die Spanne schrumpft mit jedem Indiz und wächst wieder, wenn lange nichts geschieht. \
         Alte Indizien verblassen.",
    );

    heading(ui, "Indizien");
    if dossier.clues.is_empty() {
        ui.label(
            detail(
                "Noch keine. Indizien sind öffentliche Taten: Kriegserklärungen und ihre Ziele, \
                 Anträge auf Bindung und Antworten auf Vorschläge.",
            )
            .color(MUTED),
        );
    }
    // Je Indiz zwei Zeilen: was geschah, und wohin es weist. So bleibt die Spalte schmal.
    for clue in dossier.clues.iter().take(10) {
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(date(clue.day)).monospace().color(MUTED));
            ui.label(RichText::new(format!("{}: {}", clue.kind, clue.source)).color(TEXT));
        });
        let toward = if clue.toward == view.viewer {
            "gegenüber uns".to_string()
        } else {
            format!("gegenüber {}", sim.polities.get(clue.toward).name)
        };
        let color = if clue.shift >= 0.005 {
            archetype_color(clue.points_to)
        } else {
            MUTED
        };
        ui.horizontal(|ui| {
            ui.add_space(96.0);
            ui.label(
                detail(format!(
                    "{toward} · {}",
                    clue_direction(clue.points_to_label, clue.shift)
                ))
                .color(color),
            );
        });
    }

    heading(ui, "Geschätzte Stärke");
    let intel = view.intel;
    egui::Grid::new("schaetzung")
        .num_columns(3)
        .striped(true)
        .show(ui, |ui| {
            let own_fleet = sim::diplomacy::survey::fleet_value(sim.data, sim.fleets, view.viewer);
            ui.label("Flotte (Bauwert)");
            ui.label(RichText::new(range(view.fleet_estimate, "Cr")).strong());
            ui.label(detail(format!("eigene: {} Cr", grouped(own_fleet))).color(MUTED));
            ui.end_row();
            ui.label("Technik (erforschte Technologien)");
            ui.label(RichText::new(range(view.tech_estimate, "")).strong());
            ui.label(
                detail(format!("eigene: {}", sim.research.known_count(view.viewer))).color(MUTED),
            );
            ui.end_row();
            ui.label("Einblick");
            ui.horizontal(|ui| {
                bar(ui, 100.0, intel, TREATY_COLOR);
                ui.label(percent(intel));
            });
            ui.label(detail("wächst mit Beteiligungen und Gefechten").color(MUTED));
            ui.end_row();
        });

    heading(ui, "Verflechtung");
    ui.label(format!(
        "Kulturelle Nähe {} · unsere Beteiligung an ihnen {} · ihre an uns {} · unsere \
         Handelsabhängigkeit {} · Verflechtung {}",
        percent(view.affinity),
        percent(view.our_stake),
        percent(view.their_stake),
        percent(view.our_dependence),
        percent(view.tie)
    ));

    heading(ui, "Was wir nicht wissen");
    let unseen: Vec<&str> = Archetype::ALL
        .iter()
        .filter(|archetype| {
            !dossier
                .clues
                .iter()
                .any(|clue| clue.points_to == **archetype)
        })
        .map(|archetype| archetype.label())
        .collect();
    ui.label(
        detail(
            "Ihre Haltung selbst, ihre Pläne und wie sie sich in Lagen verhält, die sie noch \
             nicht hatte. Wie sie unter Druck (Not, Bedrohung, Gelegenheit) treibt, zeigt erst \
             ihr nächstes Handeln.",
        )
        .color(TEXT),
    );
    if !unseen.is_empty() {
        ui.label(
            detail(format!(
                "Kein Indiz spricht bisher für: {}.",
                unseen.join(", ")
            ))
            .color(MUTED),
        );
    }

    heading(ui, "Unser öffentliches Bild");
    ui.label(
        detail(
            "So lesen alle anderen uns aus unseren öffentlichen Taten. Was {name} selbst über uns \
             denkt, wissen wir nicht."
                .replace("{name}", &view.other_name),
        )
        .color(MUTED),
    );
    guesses(ui, &view.our_public_face, "oeffentliches_bild");
}
