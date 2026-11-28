//! Fenster „Diplomatie“: die Beziehungen der eigenen Gemeinschaft zu allen anderen und
//! alles, was sie diplomatisch tun kann.
//!
//! Was das Fenster zeigt, kommt aus dem Lesemodell des Kerns
//! ([`sim::diplomacy::view::relation_view`]): öffentliche Tatsachen und das eigene Bild,
//! nie die verborgene Haltung der anderen. Jede Handlung ist ein Befehl
//! (`Propose`, `AnswerProposal`, `CancelTreaty`, `SetEmbargo`, `SetTransit`,
//! `DeclareWarWithAim`, `ReleaseWard`), derselbe, den die KI gibt. Was der Kern ablehnen
//! würde, ist gesperrt; der Schwebetext nennt den Grund in den Worten des Kerns.

use super::format::{decimal, grouped, percent, population};
use super::panels::{ACCENT, MUTED, SimRefs, UiAction, color32, detail, truce_text, window_room};
use bevy_egui::egui::{self, Color32, RichText};
use sim::clock::{TICKS_PER_DAY, date_of_tick};
use sim::command::SimCommand;
use sim::diplomacy::proposals::{Deal, PeaceTerms, ProposalStatus, check_deal};
use sim::diplomacy::relations::{TreatyKind, WarAim};
use sim::diplomacy::sight::Ground;
use sim::diplomacy::view::{ProposalView, RelationView, RungState, inbox, relation_view};
use sim::ids::{PlanetId, PolityId};

pub const TITLE: &str = "Diplomatie";
pub const PROPOSE: &str = "Vorschlagen";
pub const CANCEL: &str = "Kündigen";
pub const ACCEPT: &str = "Annehmen";
pub const DECLINE: &str = "Ablehnen";
pub const WITHDRAW: &str = "Zurückziehen";
pub const DECLARE: &str = "Krieg erklären";
pub const OFFER_PEACE: &str = "Frieden anbieten";
pub const EMBARGO_ON: &str = "Embargo verhängen";
pub const EMBARGO_OFF: &str = "Embargo aufheben";
pub const TRANSIT_ON: &str = "Durchfahrt gewähren";
pub const TRANSIT_OFF: &str = "Durchfahrt entziehen";
pub const ASK_PROTECTION: &str = "Um Schutz bitten";
pub const OFFER_PROTECTION: &str = "Schutz anbieten";
pub const MERGE_WE_LEAD: &str = "Vereinigung unter unserer Führung";
pub const MERGE_THEY_LEAD: &str = "Vereinigung unter ihrer Führung";
pub const RELEASE: &str = "In die Eigenständigkeit entlassen";

pub const WAR_COLOR: Color32 = Color32::from_rgb(235, 110, 95);
pub const TRUCE_COLOR: Color32 = Color32::from_rgb(240, 190, 80);
pub const TREATY_COLOR: Color32 = Color32::from_rgb(120, 200, 130);
const TEXT: Color32 = Color32::from_rgb(225, 230, 240);

/// Breite der Liste links in den Fenstern „Diplomatie“ und „Analyse“.
pub const LIST_WIDTH: f32 = 178.0;
/// Breite der Einzelheiten rechts.
pub const DETAIL_WIDTH: f32 = 560.0;

/// Wer im Friedensangebot nachgibt.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Yielder {
    /// Frieden ohne Bedingungen.
    #[default]
    Nobody,
    /// Die eigene Gemeinschaft gibt nach.
    Us,
    /// Die andere gibt nach: Das Angebot ist eine Forderung.
    Them,
}

/// Was der Spieler im Fenster gerade zusammenstellt (kein Spielzustand).
#[derive(Debug, Clone, Default)]
pub struct DiplomacyUi {
    pub open: bool,
    pub analysis_open: bool,
    /// Gemeinschaft, deren Beziehung das Fenster „Diplomatie“ zeigt.
    pub chosen: Option<PolityId>,
    /// Gemeinschaft, deren Dossier das Fenster „Analyse“ zeigt.
    pub analysis_chosen: Option<PolityId>,
    /// Kriegsziel für eine Erklärung.
    pub aim: WarAim,
    /// Entwurf eines Friedens: gegen wen, wer nachgibt, was er gibt.
    pub peace_with: Option<PolityId>,
    pub yielder: Yielder,
    pub cede: Vec<PlanetId>,
    pub tribute_years: u8,
    pub subjugate: bool,
}

impl DiplomacyUi {
    /// Vergisst Wahl und Entwürfe der Partie, die endet; welche Fenster offen sind, bleibt.
    pub fn forget_game(&mut self) {
        *self = Self {
            open: self.open,
            analysis_open: self.analysis_open,
            ..Self::default()
        };
    }

    /// Öffnet das Fenster bei dieser Gemeinschaft.
    pub fn show(&mut self, polity: PolityId) {
        self.open = true;
        self.chosen = Some(polity);
    }

    fn reset_peace(&mut self, with: Option<PolityId>) {
        self.peace_with = with;
        self.yielder = Yielder::Nobody;
        self.cede.clear();
        self.tribute_years = 0;
        self.subjugate = false;
    }

    /// Die Bedingungen des Entwurfs.
    pub fn terms(&self, me: PolityId, other: PolityId) -> PeaceTerms {
        let yielder = match self.yielder {
            Yielder::Nobody => return PeaceTerms::white(),
            Yielder::Us => me,
            Yielder::Them => other,
        };
        PeaceTerms {
            yielder: Some(yielder),
            cede: self.cede.clone(),
            tribute_years: self.tribute_years,
            subjugate: self.subjugate,
        }
    }
}

/// Blick des Kerns auf die laufende Partie, wie ihn das Lesemodell braucht.
pub fn ground<'a>(sim: &SimRefs<'a>) -> Ground<'a> {
    Ground {
        data: sim.data,
        galaxy: sim.galaxy,
        polities: sim.polities,
        diplomacy: sim.diplomacy,
        fleets: sim.fleets,
        research: sim.research,
        finance: sim.finance,
        seed: sim.config.seed,
        day: sim.clock.day(),
    }
}

/// Datum eines Spieltags.
pub fn date(day: u64) -> String {
    date_of_tick(day * TICKS_PER_DAY).to_string()
}

/// Erster Buchstabe groß: Die Gründe des Kerns beginnen klein, weil sie in Sätzen stehen.
pub fn sentence(reason: &str) -> String {
    let mut chars = reason.chars();
    match chars.next() {
        Some(first) => {
            let text: String = first.to_uppercase().chain(chars).collect();
            let text = super::format::german_numbers(&text);
            if text.ends_with('.') {
                text
            } else {
                format!("{text}.")
            }
        }
        None => String::new(),
    }
}

/// Kurzer Stand einer Beziehung, wie ihn die Liste der Gemeinschaften zeigt.
#[derive(Debug, Clone, PartialEq)]
pub struct Status {
    pub label: String,
    pub color: Color32,
    pub hover: String,
}

/// Stand der Beziehung von `me` zu `other`: Krieg, Waffenruhe, Unterstellung, Verträge,
/// Embargo oder Frieden.
pub fn status(sim: &SimRefs, me: PolityId, other: PolityId) -> Status {
    let diplomacy = sim.diplomacy;
    let day = sim.clock.day();
    let name = &sim.polities.get(other).name;
    if diplomacy.at_war(me, other) {
        let aim = diplomacy
            .war_between(me, other)
            .map_or(WarAim::Limited, |war| war.aim);
        return Status {
            label: "Krieg".into(),
            color: WAR_COLOR,
            hover: format!("Krieg mit {name} (Kriegsziel {}).", aim.label()),
        };
    }
    if diplomacy.overlord_of(other) == Some(me) {
        return Status {
            label: "Schützling".into(),
            color: TREATY_COLOR,
            hover: format!("{name} untersteht uns."),
        };
    }
    if diplomacy.overlord_of(me) == Some(other) {
        return Status {
            label: "Oberhaupt".into(),
            color: TRUCE_COLOR,
            hover: format!("Wir unterstehen {name}."),
        };
    }
    if let Some(lord) = diplomacy.overlord_of(other) {
        return Status {
            label: "untersteht".into(),
            color: MUTED,
            hover: format!(
                "{name} untersteht {}: Über Krieg und Frieden entscheidet das Oberhaupt.",
                sim.polities.get(lord).name
            ),
        };
    }
    if let Some(until) = diplomacy.truce_until(me, other, day) {
        return Status {
            label: "Waffenruhe".into(),
            color: TRUCE_COLOR,
            hover: truce_text(until - day),
        };
    }
    if diplomacy.embargoed(me, other) {
        return Status {
            label: "Embargo".into(),
            color: TRUCE_COLOR,
            hover: format!("Zwischen uns und {name} gilt ein Embargo."),
        };
    }
    let kinds: Vec<&str> = TreatyKind::ALL
        .into_iter()
        .filter(|kind| diplomacy.has_treaty(me, other, *kind))
        .map(TreatyKind::label)
        .collect();
    if !kinds.is_empty() {
        return Status {
            label: if kinds.len() == 1 {
                "Vertrag".into()
            } else {
                "Verträge".into()
            },
            color: TREATY_COLOR,
            hover: format!("Verträge mit {name}: {}.", kinds.join(", ")),
        };
    }
    Status {
        label: "Frieden".into(),
        color: MUTED,
        hover: format!("Frieden mit {name}, ohne Verträge."),
    }
}

/// Ein Knopf mit eigener Beschriftung für Vorleseprogramme und Tests (`name`), gesperrt
/// mit Grund, wenn `allowed` einen nennt.
pub fn action_button(
    ui: &mut egui::Ui,
    text: impl Into<egui::WidgetText>,
    name: &str,
    allowed: &Result<(), String>,
    hover: &str,
) -> bool {
    let response = ui.add_enabled(allowed.is_ok(), egui::Button::new(text));
    let response = match allowed {
        Ok(()) if !hover.is_empty() => response.on_hover_text(hover),
        Ok(()) => response,
        Err(reason) => response.on_disabled_hover_text(reason.as_str()),
    };
    let enabled = allowed.is_ok();
    let name = name.to_string();
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, &name));
    response.clicked()
}

/// Liegt dieser Befehl schon vorgemerkt in der Warteschlange?
fn queued(sim: &SimRefs, me: PolityId, wanted: impl Fn(&SimCommand) -> bool) -> bool {
    sim.pending
        .iter()
        .any(|queued| queued.polity == me && wanted(&queued.command))
}

const QUEUED: &str = "Vorgemerkt: gilt, sobald die Zeit weiterläuft.";

/// Würde der Kern diesen Vorschlag annehmen? Prüft die Voraussetzungen, einen schon
/// offenen Vorschlag gleicher Art, die Ruhezeit nach einer Ablehnung und was schon
/// vorgemerkt ist.
pub fn may_propose(sim: &SimRefs, me: PolityId, to: PolityId, deal: &Deal) -> Result<(), String> {
    check_deal(
        sim.data,
        sim.galaxy,
        sim.polities,
        sim.diplomacy,
        sim.finance,
        me,
        to,
        deal,
    )
    .map_err(|reason| sentence(&reason))?;
    let code = deal.code();
    if sim
        .diplomacy
        .proposals_between(me, to)
        .any(|open| open.deal.code() == code)
    {
        return Err("Zwischen uns ist ein solcher Vorschlag schon offen.".into());
    }
    let day = sim.clock.day();
    if let Some(until) = sim
        .diplomacy
        .blocked
        .get(&(me, to, code))
        .copied()
        .filter(|until| *until > day)
    {
        return Err(format!(
            "Nach der letzten Ablehnung ruht dieser Vorschlag noch {} Tage.",
            until - day
        ));
    }
    if queued(sim, me, |command| {
        matches!(command, SimCommand::Propose { to: queued_to, deal: queued }
            if *queued_to == to && queued.code() == code)
    }) {
        return Err(QUEUED.into());
    }
    Ok(())
}

/// Kann `me` `other` jetzt den Krieg erklären?
pub fn may_declare(sim: &SimRefs, view: &RelationView) -> Result<(), String> {
    let me = view.viewer;
    if !view.both_free {
        return Err(lord_reason(sim, view));
    }
    if view.war.is_some() {
        return Err("Es herrscht bereits Krieg.".into());
    }
    if let Some(until) = view.truce_until {
        return Err(truce_text(until.saturating_sub(sim.clock.day())));
    }
    if queued(sim, me, |command| {
        matches!(command,
            SimCommand::DeclareWar { target } | SimCommand::DeclareWarWithAim { target, .. }
            if *target == view.other)
    }) {
        return Err(QUEUED.into());
    }
    Ok(())
}

/// Warum zwischen den beiden nichts geht: Eine von beiden untersteht einem Oberhaupt.
fn lord_reason(sim: &SimRefs, view: &RelationView) -> String {
    let (who, lord) = match sim.diplomacy.overlord_of(view.viewer) {
        Some(lord) => (view.viewer, lord),
        None => (view.other, view.other_overlord.unwrap_or(view.other)),
    };
    format!(
        "{} untersteht {}: Über Krieg, Frieden und Verträge entscheidet das Oberhaupt.",
        sim.polities.get(who).name,
        sim.polities.get(lord).name
    )
}

pub fn diplomacy_window(
    ctx: &egui::Context,
    area: egui::Rect,
    sim: &SimRefs,
    state: &mut DiplomacyUi,
    actions: &mut Vec<UiAction>,
) {
    let mut open = state.open;
    let room = window_room(area);
    egui::Window::new(TITLE)
        .open(&mut open)
        .collapsible(true)
        .default_pos([area.left() + 440.0, area.top() + 8.0])
        // So groß wie der Inhalt; die Einzelheiten rollen, wo der Platz bis zum unteren
        // Rand nicht reicht.
        .auto_sized()
        .constrain_to(area)
        .show(ctx, |ui| {
            let Some(me) = sim.config.player else {
                ui.label("Beobachter führen keine Diplomatie.");
                return;
            };
            // Feste Breite: Trennstrich und umbrechende Zeilen nähmen sonst jede Breite.
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
            if state.chosen.is_none_or(|chosen| !others.contains(&chosen)) {
                state.chosen = others.first().copied();
            }
            let Some(other) = state.chosen else {
                return;
            };
            if state.peace_with != Some(other) || !sim.diplomacy.at_war(me, other) {
                state.reset_peace(Some(other));
            }
            let ground = ground(sim);
            mailbox(ui, sim, &ground, me, actions);
            ui.separator();
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(LIST_WIDTH);
                    egui::ScrollArea::vertical()
                        .id_salt("diplomatie_liste")
                        .max_height((room - 150.0).max(120.0))
                        .show(ui, |ui| {
                            for id in &others {
                                list_entry(ui, sim, me, *id, &mut state.chosen);
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
                        .id_salt("diplomatie_einzelheiten")
                        .max_height((room - 150.0).max(120.0))
                        .show(ui, |ui| {
                            ui.set_width(DETAIL_WIDTH - 16.0);
                            detail_view(ui, sim, &view, state, actions);
                        });
                });
            });
        });
    state.open = open;
}

/// Eine Zeile der Liste links: Name in der Farbe der Gemeinschaft, darunter ihr Stand.
fn list_entry(
    ui: &mut egui::Ui,
    sim: &SimRefs,
    me: PolityId,
    id: PolityId,
    chosen: &mut Option<PolityId>,
) {
    let polity = sim.polities.get(id);
    let status = status(sim, me, id);
    let selected = *chosen == Some(id);
    let text = RichText::new(&polity.name)
        .color(color32(polity.color))
        .strong();
    if ui
        .selectable_label(selected, text)
        .on_hover_text(&status.hover)
        .clicked()
    {
        *chosen = Some(id);
    }
    ui.horizontal(|ui| {
        ui.add_space(8.0);
        ui.label(detail(&status.label).color(status.color));
        let tension = sim.diplomacy.tension(me, id);
        ui.label(detail(format!("· Spannung {}", tension.round() as i32)).color(MUTED));
    });
    ui.add_space(2.0);
}

/// Eingang und Ausgang: offene Vorschläge an uns und von uns, und was zuletzt erledigt
/// wurde.
fn mailbox(
    ui: &mut egui::Ui,
    sim: &SimRefs,
    ground: &Ground,
    me: PolityId,
    actions: &mut Vec<UiAction>,
) {
    let (open, done) = inbox(ground, me);
    let day = sim.clock.day();
    let incoming: Vec<&ProposalView> = open.iter().filter(|view| view.incoming).collect();
    ui.horizontal(|ui| {
        ui.label(RichText::new("Eingang").color(ACCENT).strong());
        if incoming.is_empty() {
            ui.label(RichText::new("keine offenen Vorschläge an uns").color(MUTED));
        }
    });
    for view in &incoming {
        ui.horizontal_wrapped(|ui| {
            let from = sim.polities.get(view.from);
            ui.label(RichText::new(&from.name).color(color32(from.color)));
            ui.label(RichText::new(&view.label).color(TEXT));
            ui.label(
                detail(format!(
                    "· verfällt in {} Tagen",
                    view.expires_day.saturating_sub(day)
                ))
                .color(MUTED),
            );
            proposal_answers(ui, sim, me, view, actions);
        });
    }
    let outgoing: Vec<&ProposalView> = open.iter().filter(|view| !view.incoming).collect();
    if !outgoing.is_empty() {
        ui.label(RichText::new("Ausgang").color(ACCENT).strong());
        for view in outgoing {
            ui.horizontal_wrapped(|ui| {
                let to = sim.polities.get(view.to);
                ui.label(RichText::new(format!("an {}", to.name)).color(color32(to.color)));
                ui.label(RichText::new(&view.label).color(TEXT));
                ui.label(
                    detail(format!(
                        "· Antwort ausstehend, verfällt in {} Tagen",
                        view.expires_day.saturating_sub(day)
                    ))
                    .color(MUTED),
                );
                withdraw_button(ui, sim, me, view, actions);
            });
        }
    }
    // Die letzten Ausgänge, mit dem Grund einer Ablehnung.
    let recent: Vec<_> = done
        .iter()
        .filter(|(_, _, when)| day.saturating_sub(*when) <= 360)
        .take(3)
        .collect();
    for (view, status, when) in recent {
        let other = if view.incoming { view.from } else { view.to };
        let name = &sim.polities.get(other).name;
        let direction = if view.incoming {
            format!("von {name}")
        } else {
            format!("an {name}")
        };
        let reason = match status {
            ProposalStatus::Declined(Some(reason)) => format!(" (Grund: {})", reason.label()),
            _ => String::new(),
        };
        ui.label(
            detail(format!(
                "{} · {} {direction}: {}{reason}",
                date(*when),
                view.label,
                status.label()
            ))
            .color(MUTED),
        );
    }
}

fn proposal_answers(
    ui: &mut egui::Ui,
    sim: &SimRefs,
    me: PolityId,
    view: &ProposalView,
    actions: &mut Vec<UiAction>,
) {
    let answered = queued(
        sim,
        me,
        |command| matches!(command, SimCommand::AnswerProposal { proposal, .. } if *proposal == view.id),
    );
    let allowed = if answered {
        Err(QUEUED.to_string())
    } else {
        Ok(())
    };
    if action_button(
        ui,
        RichText::new(ACCEPT).color(TREATY_COLOR),
        &format!("{ACCEPT}: Nr. {}", view.id),
        &allowed,
        "Der Vorschlag gilt mit dem nächsten Takt. Passen seine Voraussetzungen dann nicht \
         mehr, ist er hinfällig.",
    ) {
        actions.push(UiAction::Command(SimCommand::AnswerProposal {
            proposal: view.id,
            accept: true,
        }));
    }
    if action_button(
        ui,
        DECLINE,
        &format!("{DECLINE}: Nr. {}", view.id),
        &allowed,
        "Danach ruht derselbe Vorschlag eine Weile.",
    ) {
        actions.push(UiAction::Command(SimCommand::AnswerProposal {
            proposal: view.id,
            accept: false,
        }));
    }
}

fn withdraw_button(
    ui: &mut egui::Ui,
    sim: &SimRefs,
    me: PolityId,
    view: &ProposalView,
    actions: &mut Vec<UiAction>,
) {
    let allowed = if queued(
        sim,
        me,
        |command| matches!(command, SimCommand::WithdrawProposal { proposal } if *proposal == view.id),
    ) {
        Err(QUEUED.to_string())
    } else {
        Ok(())
    };
    if action_button(
        ui,
        WITHDRAW,
        &format!("{WITHDRAW}: Nr. {}", view.id),
        &allowed,
        "",
    ) {
        actions.push(UiAction::Command(SimCommand::WithdrawProposal {
            proposal: view.id,
        }));
    }
}

/// Abschnittsüberschrift im Fenster.
fn heading(ui: &mut egui::Ui, text: &str) {
    ui.add_space(6.0);
    ui.label(RichText::new(text).color(ACCENT).strong());
}

/// Ein Balken von 0 bis 1 in einer Farbe, mit Text daneben.
pub fn bar(ui: &mut egui::Ui, width: f32, share: f32, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 10.0), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 2.0, Color32::from_rgb(38, 44, 58));
    let mut filled = rect;
    filled.set_width(width * share.clamp(0.0, 1.0));
    painter.rect_filled(filled, 2.0, color);
}

/// Bilanz eines Krieges als Balken von −100 bis 100 mit der Mitte als Gleichstand.
fn score_bar(ui: &mut egui::Ui, width: f32, score: f32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 10.0), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 2.0, Color32::from_rgb(38, 44, 58));
    let center = rect.center().x;
    let end = center + 0.5 * width * (score / 100.0).clamp(-1.0, 1.0);
    let fill = egui::Rect::from_min_max(
        egui::pos2(center.min(end), rect.top()),
        egui::pos2(center.max(end), rect.bottom()),
    );
    painter.rect_filled(
        fill,
        2.0,
        if score >= 0.0 {
            TREATY_COLOR
        } else {
            WAR_COLOR
        },
    );
    painter.line_segment(
        [
            egui::pos2(center, rect.top() - 2.0),
            egui::pos2(center, rect.bottom() + 2.0),
        ],
        egui::Stroke::new(1.0, TEXT),
    );
}

fn detail_view(
    ui: &mut egui::Ui,
    sim: &SimRefs,
    view: &RelationView,
    state: &mut DiplomacyUi,
    actions: &mut Vec<UiAction>,
) {
    let other = sim.polities.get(view.other);
    let status = status(sim, view.viewer, view.other);
    ui.horizontal(|ui| {
        ui.heading(RichText::new(&other.name).color(color32(other.color)));
        ui.label(RichText::new(&status.label).color(status.color).strong())
            .on_hover_text(&status.hover);
    });
    ui.label(detail(&other.motto).italics().color(MUTED));
    standing_line(ui, sim, view);
    tension(ui, view);
    match &view.war {
        Some(_) => war_section(ui, sim, view, state, actions),
        None => declare_section(ui, sim, view, state, actions),
    }
    proposals_with(ui, sim, view, actions);
    ladder(ui, sim, view, actions);
    borders(ui, sim, view, actions);
    ties(ui, view);
    protection(ui, sim, view, actions);
}

/// Wer wem untersteht.
fn standing_line(ui: &mut egui::Ui, sim: &SimRefs, view: &RelationView) {
    let name = &view.other_name;
    let text = if view.other_is_our_ward {
        format!("{name} ist unser Schützling: Es teilt unsere Kriege und Verträge.")
    } else if view.we_are_their_ward {
        format!("Wir unterstehen {name}: Über Krieg und Frieden entscheidet das Oberhaupt.")
    } else if let Some(lord) = view.other_overlord {
        format!(
            "{name} untersteht {}: Verhandelt wird mit dem Oberhaupt.",
            sim.polities.get(lord).name
        )
    } else {
        return;
    };
    ui.label(RichText::new(text).color(TRUCE_COLOR));
}

fn tension(ui: &mut egui::Ui, view: &RelationView) {
    heading(ui, "Spannung");
    ui.horizontal(|ui| {
        let share = view.tension / 100.0;
        let color = if view.tension >= 60.0 {
            WAR_COLOR
        } else if view.tension >= 30.0 {
            TRUCE_COLOR
        } else {
            TREATY_COLOR
        };
        bar(ui, 180.0, share, color);
        ui.label(format!("{} von 100", view.tension.round() as i32))
            .on_hover_text(
                "Monatlich neu gemessen aus Krieg, Embargo, Vertragsbruch, Eroberungen, \
                 kultureller Ferne, Verträgen und Verflechtung. Hohe Spannung macht Krieg \
                 wahrscheinlicher und Annahmen seltener.",
            );
    });
    if !view.tension_reasons.is_empty() {
        ui.horizontal_wrapped(|ui| {
            for (reason, points) in &view.tension_reasons {
                let sign = if *points >= 0.0 { "+" } else { "−" };
                ui.label(
                    detail(format!("{reason} {sign}{}", points.abs().round() as i32)).color(
                        if *points > 0.0 {
                            TRUCE_COLOR
                        } else {
                            TREATY_COLOR
                        },
                    ),
                );
                ui.label(detail("·").color(MUTED));
            }
        });
    }
    if view.our_grievance > 0.01 || view.their_grievance > 0.01 {
        ui.label(
            detail(format!(
                "Groll: unserer {} · ihrer {}",
                percent(view.our_grievance),
                percent(view.their_grievance)
            ))
            .color(MUTED),
        )
        .on_hover_text("Groll entsteht durch Vertragsbruch und verblasst mit den Jahren.");
    }
}

/// Krieg erklären, mit Wahl des Kriegsziels.
fn declare_section(
    ui: &mut egui::Ui,
    sim: &SimRefs,
    view: &RelationView,
    state: &mut DiplomacyUi,
    actions: &mut Vec<UiAction>,
) {
    heading(ui, "Krieg");
    let allowed = may_declare(sim, view);
    ui.horizontal(|ui| {
        ui.label("Kriegsziel:");
        for aim in [WarAim::Limited, WarAim::Subjugation] {
            ui.selectable_value(&mut state.aim, aim, aim.label())
                .on_hover_text(match aim {
                    WarAim::Limited => {
                        "Begrenzt: Im Frieden lassen sich Tribut und Abtretungen fordern, \
                         keine Unterwerfung."
                    }
                    WarAim::Subjugation => {
                        "Unterwerfung: Mit genug Vorsprung oder der eroberten Hauptwelt lässt \
                         sich fordern, dass der Gegner Vasall wird. Das Ziel ist öffentlich \
                         und sagt anderen viel über uns."
                    }
                });
        }
        let breached: Vec<&str> = view
            .ladder
            .iter()
            .filter(|rung| {
                rung.kind.forbids_war() && matches!(rung.state, RungState::Active { .. })
            })
            .map(|rung| rung.label)
            .collect();
        let mut hover = String::from(
            "Erst im Krieg wirken Blockade, Kaperfahrt und Belagerung. Alle Verträge mit \
             dieser Gemeinschaft enden.",
        );
        let text = if breached.is_empty() {
            RichText::new(DECLARE)
        } else {
            hover = format!(
                "Vertragsbruch: Es gilt {}. Alle Verträge enden, {} grollt uns, unsere Kurse \
                 fallen, und alle sehen den Bruch.",
                breached
                    .iter()
                    .map(|label| format!("„{label}“"))
                    .collect::<Vec<_>>()
                    .join(" und "),
                view.other_name
            );
            RichText::new(format!("{DECLARE} (Vertragsbruch)")).color(WAR_COLOR)
        };
        if action_button(ui, text, DECLARE, &allowed, &hover) {
            actions.push(UiAction::Command(SimCommand::DeclareWarWithAim {
                target: view.other,
                aim: state.aim,
            }));
        }
    });
}

/// Der laufende Krieg: Ziel, Bilanz, zulässige Bedingungen und das Friedensangebot.
fn war_section(
    ui: &mut egui::Ui,
    sim: &SimRefs,
    view: &RelationView,
    state: &mut DiplomacyUi,
    actions: &mut Vec<UiAction>,
) {
    let Some(war) = &view.war else {
        return;
    };
    let me = view.viewer;
    heading(ui, "Krieg");
    let attacker = war.attacker.map_or("unbekannt".to_string(), |id| {
        if id == me {
            "wir".into()
        } else {
            sim.polities.get(id).name.clone()
        }
    });
    ui.label(format!(
        "Seit {} ({} Monate) · Angreifer: {attacker} · Kriegsziel: {}",
        date(war.since_day),
        war.months,
        war.aim.label()
    ));
    ui.horizontal(|ui| {
        ui.label("Bilanz");
        score_bar(ui, 200.0, war.score);
        let score = war.score.round() as i32;
        ui.label(
            RichText::new(if score > 0 {
                format!("+{score} für uns")
            } else if score < 0 {
                format!("{} für {}", -score, view.other_name)
            } else {
                "ausgeglichen".into()
            })
            .strong(),
        )
        .on_hover_text(
            "Von −100 bis 100 aus unserer Sicht: genommene Bevölkerung, verlorener \
             Flottenwert und der Unterschied der Kriegsmüdigkeit.",
        );
    });
    egui::Grid::new("kriegsbilanz")
        .num_columns(3)
        .striped(true)
        .show(ui, |ui| {
            ui.label("");
            ui.label(RichText::new("wir").color(MUTED));
            ui.label(RichText::new(&view.other_name).color(MUTED));
            ui.end_row();
            ui.label("Genommene Bevölkerung");
            ui.label(population(war.taken_by_us));
            ui.label(population(war.taken_by_them));
            ui.end_row();
            ui.label("Verlorener Flottenwert");
            ui.label(format!("{} Cr", grouped(war.lost_by_us)));
            ui.label(format!("{} Cr", grouped(war.lost_by_them)));
            ui.end_row();
            ui.label("Kriegsmüdigkeit");
            ui.label(decimal(war.our_weariness, 0));
            ui.label(decimal(war.their_weariness, 0));
            ui.end_row();
        });
    ui.label(
        detail(format!(
            "In {} Monaten endet der Krieg von selbst: Dann diktiert der Vorsprung den Frieden.",
            war.months_to_diktat
        ))
        .color(MUTED),
    );
    let band = if war.may_demand_subjugation {
        format!(
            "Unser Vorsprung erlaubt zu fordern: Tribut, Abtretungen bis {} ihrer Bevölkerung \
             und die Unterwerfung.",
            percent(war.max_cede_share as f32)
        )
    } else if war.may_demand_concessions {
        format!(
            "Unser Vorsprung erlaubt zu fordern: Tribut und Abtretungen bis {} ihrer \
             Bevölkerung.",
            percent(war.max_cede_share as f32)
        )
    } else {
        "Unser Vorsprung reicht für keine Forderung; anbieten dürfen wir jederzeit alles.".into()
    };
    ui.label(detail(band).color(TEXT));
    peace_offer(ui, sim, view, state, actions);
}

fn peace_offer(
    ui: &mut egui::Ui,
    sim: &SimRefs,
    view: &RelationView,
    state: &mut DiplomacyUi,
    actions: &mut Vec<UiAction>,
) {
    let (me, other) = (view.viewer, view.other);
    let levers = &sim.data.diplomacy.levers;
    ui.add_space(4.0);
    ui.label(RichText::new("Friedensangebot").strong());
    ui.horizontal(|ui| {
        let before = state.yielder;
        ui.selectable_value(&mut state.yielder, Yielder::Nobody, "ohne Bedingungen");
        ui.selectable_value(&mut state.yielder, Yielder::Us, "wir geben nach");
        ui.selectable_value(
            &mut state.yielder,
            Yielder::Them,
            format!("{} gibt nach", view.other_name),
        );
        if state.yielder != before {
            state.cede.clear();
            state.tribute_years = 0;
            state.subjugate = false;
        }
    });
    let yielder = match state.yielder {
        Yielder::Nobody => None,
        Yielder::Us => Some(me),
        Yielder::Them => Some(other),
    };
    if let Some(yielder) = yielder {
        ui.horizontal(|ui| {
            ui.label("Tribut");
            ui.add(
                egui::Slider::new(&mut state.tribute_years, 0..=levers.terms_years)
                    .suffix(" Jahre"),
            )
            .on_hover_text(format!(
                "Der Nachgebende zahlt so lange monatlich {} seiner Einnahmen.",
                percent(levers.reparations as f32)
            ));
            ui.checkbox(&mut state.subjugate, "wird Vasall");
        });
        let capital = sim.polities.get(yielder).capital;
        let worlds: Vec<_> = sim
            .galaxy
            .planets_of(yielder)
            .filter(|planet| Some(planet.id) != capital)
            .collect();
        let total = sim.galaxy.population_of(yielder).max(1e-9);
        ui.label(
            detail(format!(
                "Abtreten (höchstens {}, nie die Hauptwelt):",
                levers.max_cede
            ))
            .color(MUTED),
        );
        if worlds.is_empty() {
            ui.label(detail("keine Welt außer der Hauptwelt").color(MUTED));
        }
        ui.horizontal_wrapped(|ui| {
            for planet in worlds {
                let mut chosen = state.cede.contains(&planet.id);
                let full = state.cede.len() >= levers.max_cede && !chosen;
                let label = format!(
                    "{} ({})",
                    planet.name,
                    percent((planet.population / total) as f32)
                );
                let response = ui
                    .add_enabled(!full, egui::Checkbox::new(&mut chosen, label))
                    .on_disabled_hover_text(format!(
                        "Ein Frieden tritt höchstens {} Welten ab.",
                        levers.max_cede
                    ));
                if response.changed() {
                    if chosen {
                        state.cede.push(planet.id);
                    } else {
                        state.cede.retain(|id| *id != planet.id);
                    }
                }
            }
        });
    }
    let terms = state.terms(me, other);
    let deal = Deal::Peace(terms.clone());
    let allowed = may_propose(sim, me, other, &deal);
    ui.horizontal(|ui| {
        if action_button(
            ui,
            RichText::new(OFFER_PEACE).color(ACCENT),
            OFFER_PEACE,
            &allowed,
            &format!(
                "{}. {} antwortet in einigen Tagen.",
                terms.label(sim.polities),
                view.other_name
            ),
        ) {
            actions.push(UiAction::Command(SimCommand::Propose {
                to: other,
                deal: deal.clone(),
            }));
        }
        if let Err(reason) = &allowed {
            ui.label(detail(reason).color(MUTED));
        }
    });
}

/// Offene Vorschläge zwischen uns und der gewählten Gemeinschaft.
fn proposals_with(
    ui: &mut egui::Ui,
    sim: &SimRefs,
    view: &RelationView,
    actions: &mut Vec<UiAction>,
) {
    if view.proposals.is_empty() {
        return;
    }
    heading(ui, "Offene Vorschläge");
    let day = sim.clock.day();
    for proposal in &view.proposals {
        ui.horizontal_wrapped(|ui| {
            ui.label(if proposal.incoming {
                format!("{} an uns:", view.other_name)
            } else {
                "Wir an sie:".to_string()
            });
            ui.label(RichText::new(&proposal.label).color(TEXT));
            ui.label(
                detail(format!(
                    "· seit {} · verfällt in {} Tagen",
                    date(proposal.made_day),
                    proposal.expires_day.saturating_sub(day)
                ))
                .color(MUTED),
            );
            if proposal.incoming {
                proposal_answers(ui, sim, view.viewer, proposal, actions);
            } else {
                withdraw_button(ui, sim, view.viewer, proposal, actions);
            }
        });
    }
}

/// Die Stufenleiter der Verträge mit Vorschlagen und Kündigen.
fn ladder(ui: &mut egui::Ui, sim: &SimRefs, view: &RelationView, actions: &mut Vec<UiAction>) {
    heading(ui, "Verträge");
    let me = view.viewer;
    let notice = sim.data.diplomacy.levers.notice_days;
    let mut last_reason: Option<String> = None;
    egui::Grid::new("vertragsleiter")
        .num_columns(3)
        .striped(true)
        .min_col_width(80.0)
        .show(ui, |ui| {
            for rung in &view.ladder {
                ui.label(RichText::new(rung.label).strong())
                    .on_hover_text(treaty_meaning(rung.kind));
                match &rung.state {
                    RungState::Active {
                        since_day,
                        ends_day,
                    } => {
                        last_reason = None;
                        match ends_day {
                            Some(end) => ui.label(
                                RichText::new(format!(
                                    "seit {}, gekündigt zum {}",
                                    date(*since_day),
                                    date(*end)
                                ))
                                .color(TRUCE_COLOR),
                            ),
                            None => ui.label(
                                RichText::new(format!("besteht seit {}", date(*since_day)))
                                    .color(TREATY_COLOR),
                            ),
                        };
                        let allowed = if ends_day.is_some() {
                            Err("Der Vertrag ist bereits gekündigt.".to_string())
                        } else if queued(sim, me, |command| {
                            matches!(command, SimCommand::CancelTreaty { with, kind }
                                if *with == view.other && *kind == rung.kind)
                        }) {
                            Err(QUEUED.into())
                        } else {
                            Ok(())
                        };
                        if action_button(
                            ui,
                            CANCEL,
                            &format!("{CANCEL}: {}", rung.label),
                            &allowed,
                            &format!(
                                "Der Vertrag und alle, die auf ihm aufbauen, enden nach {notice} \
                                 Tagen."
                            ),
                        ) {
                            actions.push(UiAction::Command(SimCommand::CancelTreaty {
                                with: view.other,
                                kind: rung.kind,
                            }));
                        }
                    }
                    RungState::Available | RungState::Locked(_) => {
                        let deal = Deal::Treaty(rung.kind);
                        let allowed = may_propose(sim, me, view.other, &deal);
                        match &allowed {
                            Ok(()) => ui.label(RichText::new("möglich").color(TEXT)),
                            // Derselbe Grund wie in der Zeile darüber steht nur einmal.
                            Err(reason) if last_reason.as_ref() == Some(reason) => ui
                                .label(detail("ebenso").color(MUTED))
                                .on_hover_text(reason.as_str()),
                            Err(reason) => ui.label(detail(reason).color(MUTED)),
                        };
                        last_reason = allowed.clone().err();
                        if action_button(
                            ui,
                            PROPOSE,
                            &format!("{PROPOSE}: {}", rung.label),
                            &allowed,
                            &format!(
                                "{} entscheidet nach einigen Tagen und nennt bei einer \
                                 Ablehnung einen Grund.",
                                view.other_name
                            ),
                        ) {
                            actions.push(UiAction::Command(SimCommand::Propose {
                                to: view.other,
                                deal,
                            }));
                        }
                    }
                }
                ui.end_row();
            }
        });
}

fn treaty_meaning(kind: TreatyKind) -> &'static str {
    match kind {
        TreatyKind::Trade => "Handel: Grundlage aller weiteren Verträge; kein Embargo möglich.",
        TreatyKind::NonAggression => {
            "Nichtangriff: Ein Krieg trotz dieses Vertrags ist ein Vertragsbruch."
        }
        TreatyKind::Research => "Forschung: mehr Forschungspunkte für beide.",
        TreatyKind::Investment => {
            "Investition: Schutz der Anteile im Gebiet des anderen; Enteignung ist ein \
             Vertragsbruch."
        }
        TreatyKind::Defence => {
            "Verteidigung: Durchfahrt, Nachschub und Geleit beim Partner; Krieg gegen ihn ist \
             ein Vertragsbruch."
        }
        TreatyKind::Union => {
            "Union: die letzte Stufe vor der Vereinigung; Beteiligungen zählen wie eigene."
        }
    }
}

/// Embargo und Durchfahrt, je Seite.
fn borders(ui: &mut egui::Ui, sim: &SimRefs, view: &RelationView, actions: &mut Vec<UiAction>) {
    heading(ui, "Handel und Grenzen");
    let me = view.viewer;
    let name = &view.other_name;
    let pair_ok = if view.both_free {
        Ok(())
    } else {
        Err(lord_reason(sim, view))
    };
    ui.horizontal_wrapped(|ui| {
        ui.label(match (view.embargo_by_us, view.embargo_by_them) {
            (false, false) => "Kein Embargo.".to_string(),
            (true, false) => format!("Wir halten ein Embargo gegen {name}."),
            (false, true) => format!("{name} hält ein Embargo gegen uns."),
            (true, true) => "Embargo auf beiden Seiten.".to_string(),
        });
        let on = !view.embargo_by_us;
        let allowed = pair_ok.clone().and_then(|()| {
            let trade = view.ladder.iter().any(|rung| {
                matches!(rung.kind, TreatyKind::Trade | TreatyKind::Union)
                    && matches!(rung.state, RungState::Active { .. })
            });
            if on && trade {
                Err("Unter einem Handelsvertrag gibt es kein Embargo; erst kündigen.".into())
            } else if queued(sim, me, |command| {
                matches!(command, SimCommand::SetEmbargo { target, .. } if *target == view.other)
            }) {
                Err(QUEUED.into())
            } else {
                Ok(())
            }
        });
        let label = if on { EMBARGO_ON } else { EMBARGO_OFF };
        if action_button(
            ui,
            label,
            label,
            &allowed,
            "Ein Embargo schließt Häfen und Wege für die Frachter beider Seiten und verbietet \
             Anteilskäufe. Solange es gilt, wird kein Vertrag geschlossen.",
        ) {
            actions.push(UiAction::Command(SimCommand::SetEmbargo {
                target: view.other,
                on,
            }));
        }
    });
    ui.horizontal_wrapped(|ui| {
        let ours = if view.transit_by_us {
            format!("Wir gewähren {name} Durchfahrt")
        } else {
            format!("Wir gewähren {name} keine Durchfahrt")
        };
        let theirs = if view.transit_by_them {
            "sie uns schon."
        } else {
            "sie uns nicht."
        };
        ui.label(format!("{ours}, {theirs}"));
        let open = !view.transit_by_us;
        let allowed = pair_ok.clone().and_then(|()| {
            if open && view.war.is_some() {
                Err("Einem Kriegsgegner wird keine Durchfahrt gewährt.".into())
            } else if queued(sim, me, |command| {
                matches!(command, SimCommand::SetTransit { target, .. } if *target == view.other)
            }) {
                Err(QUEUED.into())
            } else {
                Ok(())
            }
        });
        let label = if open { TRANSIT_ON } else { TRANSIT_OFF };
        if action_button(
            ui,
            label,
            label,
            &allowed,
            "Durchfahrt erlaubt den Flotten der anderen, unser Gebiet zu durchqueren.",
        ) {
            actions.push(UiAction::Command(SimCommand::SetTransit {
                target: view.other,
                open,
            }));
        }
    });
    if let Some((payer, share, until)) = view.tribute {
        let who = if payer == me {
            "Wir zahlen".to_string()
        } else {
            format!("{name} zahlt uns")
        };
        ui.label(
            RichText::new(format!(
                "{who} Tribut: {} der Einnahmen bis {}.",
                percent(share as f32),
                date(until)
            ))
            .color(TRUCE_COLOR),
        );
    }
}

/// Kulturelle Nähe, Beteiligungen und Verflechtung, mit dem Fortschritt zur Vereinigung.
fn ties(ui: &mut egui::Ui, view: &RelationView) {
    heading(ui, "Nähe und Verflechtung");
    egui::Grid::new("verflechtung")
        .num_columns(2)
        .striped(true)
        .show(ui, |ui| {
            ui.label("Kulturelle Nähe").on_hover_text(
                "Wie ähnlich die gelebten Kulturen sind (Offenheit, Gemeinsinn, \
                 Wehrhaftigkeit).",
            );
            ui.horizontal(|ui| {
                bar(ui, 120.0, view.affinity, TREATY_COLOR);
                ui.label(percent(view.affinity));
            });
            ui.end_row();
            ui.label("Unsere Beteiligung an ihren Betrieben");
            ui.label(percent(view.our_stake));
            ui.end_row();
            ui.label("Ihre Beteiligung an unseren Betrieben");
            ui.label(percent(view.their_stake));
            ui.end_row();
            ui.label("Unsere Handelsabhängigkeit von ihnen")
                .on_hover_text("Anteil unseres Handels, der über sie läuft.");
            ui.label(percent(view.our_dependence));
            ui.end_row();
            ui.label("Verflechtung").on_hover_text(format!(
                "Beteiligungen und Handel zusammen. Für eine Vereinigung braucht es \
                 mindestens {}.",
                percent(view.merge_tie)
            ));
            ui.horizontal(|ui| {
                bar(
                    ui,
                    120.0,
                    view.tie / view.merge_tie.max(1e-6),
                    if view.tie >= view.merge_tie {
                        TREATY_COLOR
                    } else {
                        MUTED
                    },
                );
                ui.label(format!(
                    "{} (Schwelle {})",
                    percent(view.tie),
                    percent(view.merge_tie)
                ));
            });
            ui.end_row();
        });
    let union = view.ladder.iter().any(|rung| {
        rung.kind == TreatyKind::Union && matches!(rung.state, RungState::Active { .. })
    });
    if union {
        ui.label(
            detail(format!(
                "Vereinigung: Bedingungen gehalten {} von {} Monaten (Union, Verflechtung ab {}, \
                 kulturelle Nähe ab {}).",
                view.merge_months,
                view.merge_months_needed,
                percent(view.merge_tie),
                percent(view.merge_affinity)
            ))
            .color(if view.merge_possible {
                TREATY_COLOR
            } else {
                MUTED
            }),
        );
    }
}

/// Schutz, Vereinigung und das Entlassen eines Schützlings.
fn protection(ui: &mut egui::Ui, sim: &SimRefs, view: &RelationView, actions: &mut Vec<UiAction>) {
    heading(ui, "Schutz und Vereinigung");
    let (me, other) = (view.viewer, view.other);
    if view.other_is_our_ward {
        let allowed = if queued(
            sim,
            me,
            |command| matches!(command, SimCommand::ReleaseWard { ward } if *ward == other),
        ) {
            Err(QUEUED.to_string())
        } else {
            Ok(())
        };
        if action_button(
            ui,
            RELEASE,
            RELEASE,
            &allowed,
            "Der Schützling wird wieder frei; seine Welten gelten wieder als eigene.",
        ) {
            actions.push(UiAction::Command(SimCommand::ReleaseWard { ward: other }));
        }
        return;
    }
    let deals = [
        (
            ASK_PROTECTION,
            Deal::Protectorate { ward: me },
            "Wir unterstellen uns ihnen als Schützling: Sie führen unsere Kriege und \
             Verträge, wir zahlen einen kleinen Tribut. Unsere Welten gelten als beigetreten.",
        ),
        (
            OFFER_PROTECTION,
            Deal::Protectorate { ward: other },
            "Sie unterstellen sich uns als Schützling.",
        ),
        (
            MERGE_WE_LEAD,
            Deal::Merge { lead: me },
            "Sie gehen mit allen Welten in uns auf.",
        ),
        (
            MERGE_THEY_LEAD,
            Deal::Merge { lead: other },
            "Wir gehen mit allen Welten in ihnen auf; die Partie endet für uns als \
             Vereinigung.",
        ),
    ];
    ui.horizontal_wrapped(|ui| {
        for (label, deal, hover) in deals {
            let allowed = may_propose(sim, me, other, &deal);
            if action_button(ui, label, label, &allowed, hover) {
                actions.push(UiAction::Command(SimCommand::Propose { to: other, deal }));
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::TestGame;
    use crate::time_control::SimSpeed;
    use crate::ui::UiState;
    use bevy::prelude::Vec2;
    use sim::command::{CommandQueue, CommandSource};
    use sim::diplomacy::Diplomacy;
    use sim::polity::Polities;
    use sim::state::SimConfig;

    fn run(game: &mut TestGame, frames: usize) {
        *game.world_mut().resource_mut::<SimSpeed>() = SimSpeed {
            paused: false,
            level: 5,
        };
        game.frames(frames);
        game.world_mut().resource_mut::<SimSpeed>().paused = true;
        game.frames(2);
    }

    fn command(game: &mut TestGame, polity: PolityId, command: SimCommand) {
        game.world_mut().resource_mut::<CommandQueue>().push(
            polity,
            CommandSource::Player,
            command,
        );
    }

    /// Eine Partie mit offenem Fenster „Diplomatie“ bei der ersten anderen Gemeinschaft.
    fn with_window() -> (TestGame, PolityId, PolityId) {
        let mut game = TestGame::start("--seed 3");
        game.frames(4);
        game.click_on(TITLE);
        game.frames(3);
        assert!(game.window_rect(TITLE).is_some());
        let me = game.resource::<SimConfig>().player.expect("Spieler");
        let other = game
            .resource::<Polities>()
            .alive()
            .find(|polity| polity.id != me)
            .expect("eine andere Gemeinschaft")
            .id;
        game.world_mut().resource_mut::<UiState>().diplomacy.chosen = Some(other);
        game.frames(3);
        (game, me, other)
    }

    #[test]
    fn both_windows_open_from_the_bar_and_the_keys_and_fit_both_sizes() {
        let (mut game, _, _) = with_window();
        game.click_on(super::super::analysis::TITLE);
        game.frames(3);
        for size in [Vec2::new(1280.0, 720.0), Vec2::new(1600.0, 900.0)] {
            game.resize(size);
            game.frames(4);
            for title in [TITLE, super::super::analysis::TITLE] {
                let rect = game.window_rect(title).expect("Fenster");
                assert!(
                    rect.left() >= 0.0
                        && rect.top() >= 0.0
                        && rect.right() <= size.x + 0.5
                        && rect.bottom() <= size.y + 0.5,
                    "{title}: {rect:?} bei {size:?}"
                );
            }
        }
        // F2 und F3 schließen und öffnen sie wieder.
        game.key(bevy::prelude::KeyCode::F2);
        game.key(bevy::prelude::KeyCode::F3);
        game.frames(20);
        assert!(game.window_rect(TITLE).is_none());
        assert!(game.window_rect(super::super::analysis::TITLE).is_none());
        game.key(bevy::prelude::KeyCode::F2);
        game.frames(3);
        assert!(game.window_rect(TITLE).is_some());
    }

    #[test]
    fn a_treaty_proposed_in_the_window_reaches_the_core_and_higher_rungs_stay_locked() {
        let (mut game, me, other) = with_window();
        // Ohne Handel gibt es keinen Nichtangriff: gesperrt, mit dem Grund des Kerns.
        assert!(game.disabled_button("Vorschlagen: Nichtangriff").is_some());
        assert!(game.text("„Nichtangriff“ setzt „Handel“ voraus.").is_some());
        game.click_on("Vorschlagen: Handel");
        game.frames(2);
        // Bei stehender Zeit ist der Vorschlag vorgemerkt und wird nicht noch einmal
        // angeboten.
        assert!(game.disabled_button("Vorschlagen: Handel").is_some());
        // Mit dem nächsten Takt ist er offen, ehe die andere Seite antwortet.
        let mut proposal = None;
        for _ in 0..40 {
            run(&mut game, 1);
            proposal = game
                .resource::<Diplomacy>()
                .proposals
                .iter()
                .find(|open| open.from == me && open.to == other)
                .cloned();
            if proposal.is_some() {
                break;
            }
        }
        let proposal = proposal.expect("der Vorschlag ist offen");
        assert_eq!(proposal.deal, Deal::Treaty(TreatyKind::Trade));
        assert!(
            game.button(&format!("{WITHDRAW}: Nr. {}", proposal.id))
                .is_some()
        );
        assert!(game.disabled_button("Vorschlagen: Handel").is_some());
        game.click_on(&format!("{WITHDRAW}: Nr. {}", proposal.id));
        run(&mut game, 4);
        let diplomacy = game.resource::<Diplomacy>();
        assert!(
            diplomacy
                .proposals
                .iter()
                .all(|open| open.id != proposal.id)
        );
        assert!(
            diplomacy
                .history
                .iter()
                .any(|done| done.proposal.id == proposal.id)
        );
    }

    #[test]
    fn an_incoming_proposal_is_accepted_and_the_treaty_can_be_cancelled() {
        let (mut game, me, other) = with_window();
        command(
            &mut game,
            other,
            SimCommand::Propose {
                to: me,
                deal: Deal::Treaty(TreatyKind::Trade),
            },
        );
        run(&mut game, 3);
        let id = game
            .resource::<Diplomacy>()
            .proposals
            .iter()
            .find(|open| open.from == other && open.to == me)
            .expect("Vorschlag an uns")
            .id;
        // Die Kopfleiste zählt ihn, das Fenster bietet Annehmen und Ablehnen.
        assert!(game.button(&format!("{TITLE} (1)")).is_some());
        assert!(game.button(&format!("{DECLINE}: Nr. {id}")).is_some());
        game.click_on(&format!("{ACCEPT}: Nr. {id}"));
        run(&mut game, 3);
        assert!(
            game.resource::<Diplomacy>()
                .has_treaty(me, other, TreatyKind::Trade)
        );
        // Unter Handel kein Embargo; Kündigen und die nächste Stufe sind offen.
        assert!(game.disabled_button(EMBARGO_ON).is_some());
        assert!(game.button("Vorschlagen: Nichtangriff").is_some());
        game.click_on("Kündigen: Handel");
        run(&mut game, 3);
        let ends = game
            .resource::<Diplomacy>()
            .relation(me, other)
            .and_then(|relation| relation.treaty(TreatyKind::Trade))
            .and_then(|treaty| treaty.ends_day);
        assert!(ends.is_some(), "der Vertrag ist gekündigt");
        assert!(game.disabled_button("Kündigen: Handel").is_some());
    }

    #[test]
    fn war_is_declared_with_an_aim_and_peace_is_offered_within_the_band() {
        let (mut game, me, other) = with_window();
        game.world_mut().resource_mut::<UiState>().diplomacy.aim = WarAim::Subjugation;
        game.frames(2);
        game.click_on(DECLARE);
        run(&mut game, 3);
        let aim = game
            .resource::<Diplomacy>()
            .war_between(me, other)
            .expect("Krieg")
            .aim;
        assert_eq!(aim, WarAim::Subjugation);
        // Im Krieg keine Verträge und keine Durchfahrt; ein Frieden ohne Bedingungen geht.
        assert!(game.disabled_button("Vorschlagen: Handel").is_some());
        assert!(game.disabled_button(TRANSIT_ON).is_some());
        assert!(game.button(OFFER_PEACE).is_some());
        // Ohne Vorsprung lässt sich nichts fordern.
        {
            let mut ui = game.world_mut().resource_mut::<UiState>();
            ui.diplomacy.yielder = Yielder::Them;
            ui.diplomacy.tribute_years = 3;
        }
        game.frames(3);
        assert!(game.disabled_button(OFFER_PEACE).is_some());
        // Nachgeben dürfen wir immer.
        game.world_mut().resource_mut::<UiState>().diplomacy.yielder = Yielder::Us;
        game.frames(1);
        game.world_mut()
            .resource_mut::<UiState>()
            .diplomacy
            .tribute_years = 2;
        game.frames(3);
        game.click_on(OFFER_PEACE);
        run(&mut game, 3);
        let offered = game
            .resource::<Diplomacy>()
            .proposals
            .iter()
            .chain(
                game.resource::<Diplomacy>()
                    .history
                    .iter()
                    .map(|done| &done.proposal),
            )
            .find(|proposal| proposal.from == me && proposal.to == other)
            .map(|proposal| proposal.deal.clone());
        assert_eq!(
            offered,
            Some(Deal::Peace(PeaceTerms {
                yielder: Some(me),
                cede: Vec::new(),
                tribute_years: 2,
                subjugate: false,
            }))
        );
    }

    #[test]
    fn the_communities_list_names_the_relation_and_opens_the_window() {
        let (mut game, me, other) = with_window();
        game.world_mut().resource_mut::<UiState>().diplomacy.open = false;
        game.frames(20);
        let name = game.resource::<Polities>().get(other).name.clone();
        assert!(game.window_rect(TITLE).is_none());
        game.click_on(&format!("{TITLE}: {name}"));
        game.frames(3);
        assert!(game.window_rect(TITLE).is_some());
        assert_eq!(game.resource::<UiState>().diplomacy.chosen, Some(other));
        let _ = me;
    }

    #[test]
    fn the_analysis_shows_an_estimate_and_never_a_true_mind() {
        let (mut game, me, other) = with_window();
        game.click_on(super::super::analysis::TITLE);
        game.world_mut()
            .resource_mut::<UiState>()
            .diplomacy
            .analysis_chosen = Some(other);
        game.frames(4);
        assert!(game.text(super::super::analysis::DISCLAIMER).is_some());
        // Zu Beginn weiß niemand etwas: alle vier gleich, mit breiter Spanne.
        let dossier =
            sim::diplomacy::view::DossierView::of(&game.resource::<Diplomacy>().dossier(me, other));
        for guess in &dossier.guesses {
            let text = super::super::analysis::guess_text(guess.belief, guess.lo, guess.hi);
            assert!(game.text(&text).is_some(), "keine Zeile „{text}“");
        }
        assert!(game.text("Vermutete Haltung").is_some());
        assert!(game.text("Unser öffentliches Bild").is_some());
    }

    /// Liegt der Punkt im Fenster mit diesem Titel?
    fn inside(game: &mut TestGame, title: &str, at: Vec2) -> bool {
        game.window_rect(title)
            .is_some_and(|rect| rect.contains(bevy_egui::egui::pos2(at.x, at.y)))
    }

    #[test]
    fn in_a_running_game_both_windows_fit_and_every_action_is_in_reach() {
        use crate::testing::diplomacy_scene;
        let cast = diplomacy_scene::cast();
        let path = diplomacy_scene::save_file("diplomatie_szene");
        let mut game = TestGame::start(&format!("--load {}", path.display()));
        game.frames(6);
        let _ = std::fs::remove_file(&path);
        let analysis = super::super::analysis::TITLE;
        // Ein Vorschlag wartet: Die Kopfleiste zählt ihn.
        game.click_on(&format!("{TITLE} (1)"));
        game.click_on(analysis);
        game.frames(3);
        for size in [Vec2::new(1280.0, 720.0), Vec2::new(1600.0, 900.0)] {
            game.resize(size);
            for (other, wanted) in [
                (cast.enemy, vec![OFFER_PEACE, TRANSIT_ON]),
                (
                    cast.partner,
                    vec!["Kündigen: Forschung", "Vorschlagen: Investition"],
                ),
                (cast.embargoed, vec![EMBARGO_OFF]),
            ] {
                {
                    let mut ui = game.world_mut().resource_mut::<UiState>();
                    ui.diplomacy.chosen = Some(other);
                    ui.diplomacy.analysis_chosen = Some(other);
                }
                game.frames(4);
                for title in [TITLE, analysis] {
                    let rect = game.window_rect(title).expect("Fenster");
                    assert!(
                        rect.left() >= 0.0
                            && rect.top() >= 0.0
                            && rect.right() <= size.x + 0.5
                            && rect.bottom() <= size.y + 0.5,
                        "{title}: {rect:?} bei {size:?}"
                    );
                }
                // Das Fenster „Analyse“ liegt über dem anderen; für die Knöpfe zählt
                // „Diplomatie“ allein.
                game.world_mut()
                    .resource_mut::<UiState>()
                    .diplomacy
                    .analysis_open = false;
                game.frames(20);
                // Was unter den sichtbaren Teil reicht, ist mit dem Mausrad erreichbar.
                for label in wanted {
                    let mut turns = 0;
                    loop {
                        let at = game
                            .button(label)
                            .or_else(|| game.disabled_button(label))
                            .unwrap_or_else(|| panic!("kein Knopf „{label}“"));
                        if inside(&mut game, TITLE, at) {
                            break;
                        }
                        assert!(turns < 12, "„{label}“ bei {at} bleibt außerhalb");
                        let rect = game.window_rect(TITLE).expect("Fenster");
                        let detail = Vec2::new(rect.right() - 120.0, rect.center().y);
                        game.scroll(detail, -3.0);
                        turns += 1;
                    }
                }
                game.world_mut()
                    .resource_mut::<UiState>()
                    .diplomacy
                    .analysis_open = true;
                game.frames(3);
            }
        }
        // Der Vorschlag der dritten steht im Eingang und lässt sich annehmen.
        let id = game
            .resource::<Diplomacy>()
            .proposals
            .iter()
            .find(|open| open.from == cast.trader)
            .expect("Vorschlag")
            .id;
        game.world_mut()
            .resource_mut::<UiState>()
            .diplomacy
            .analysis_open = false;
        game.frames(20);
        game.click_on(&format!("{ACCEPT}: Nr. {id}"));
        run(&mut game, 3);
        let me = game.resource::<SimConfig>().player.unwrap();
        assert!(game.resource::<Diplomacy>().has_treaty(
            me,
            cast.trader,
            TreatyKind::NonAggression
        ));
    }
}
