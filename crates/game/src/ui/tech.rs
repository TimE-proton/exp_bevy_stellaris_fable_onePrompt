//! Forschung: der eigene Technologiebaum mit Ziel und Etat, die Wirkung der eigenen
//! Technik und die Einschätzung der anderen Gemeinschaften.
//!
//! Vom eigenen Stand zeigt das Fenster alles. Von fremden Gemeinschaften zeigt es nur,
//! was der Kern als Schätzung anbietet ([`Research::estimate`], [`Research::intel`]):
//! Spannen, nie den wahren Stand.

use super::format::{grouped, percent};
use super::panels::{ACCENT, MUTED, SimRefs, UiAction, color32, detail};
use super::{TechTab, UiState};
use bevy_egui::egui::{self, Color32, RichText};
use sim::command::SimCommand;
use sim::data::{GameData, TechBranch, TechEffect};
use sim::economy::to_credits;
use sim::ids::PolityId;
use sim::polity::Polity;
use sim::tech::{PolityResearch, Research, TechEffects, TierEstimate, top_tier};

const GOOD: Color32 = Color32::from_rgb(120, 200, 130);
const BAD: Color32 = Color32::from_rgb(235, 110, 95);
const WARN: Color32 = Color32::from_rgb(240, 190, 80);
const TEXT: Color32 = Color32::from_rgb(225, 230, 240);

/// Beschriftungen des Fensters. Das README nennt sie; ein Test hält beide beieinander.
pub const TITLE: &str = "Forschung";
pub const TAB_OWN: &str = "Eigene Forschung";
pub const TAB_OTHERS: &str = "Einschätzung der anderen";
pub const RESEARCH: &str = "Erforschen";
pub const SET_BUDGET: &str = "Etat festsetzen";
pub const AUTOMATIC: &str = "Automatik einschalten";

/// Breite einer Technologie im Baum.
const CARD_WIDTH: f32 = 156.0;
/// Breite des ganzen Baums: fünf Zweige und die Spalte mit der Stufe. Fließtext darüber
/// und darunter bricht an dieser Breite um, statt das Fenster zu weiten.
const TREE_WIDTH: f32 = 5.0 * (CARD_WIDTH + 14.0) + 40.0;

/// Stand einer Technologie aus Sicht der eigenen Gemeinschaft.
#[derive(Clone, Debug, PartialEq)]
pub enum TechState {
    /// Erforscht.
    Known,
    /// Das Ziel, in das die Forschungspunkte gerade fließen. `done` ist der erarbeitete
    /// Anteil der Kosten, `months_left` die Zeit bis zum Abschluss beim Tempo des letzten
    /// Monats (`None`, wenn zuletzt keine Punkte zusammenkamen).
    Researching { done: f32, months_left: Option<u32> },
    /// Erforschbar: Alle Voraussetzungen sind erfüllt. `done` ist frühere Arbeit daran.
    Available { done: f32 },
    /// Gesperrt: Diese Voraussetzungen fehlen (Indizes in `techs.ron`).
    Locked { missing: Vec<u16> },
}

/// Erarbeiteter Anteil an den Kosten einer Technologie (0..1).
fn share_done(data: &GameData, entry: &PolityResearch, tech: u16) -> f32 {
    let cost = entry.cost(data, tech);
    let progress = entry.progress.get(tech as usize).copied().unwrap_or(0.0);
    if cost > 0.0 {
        (progress / cost).clamp(0.0, 1.0) as f32
    } else {
        0.0
    }
}

/// Monate bis zum Abschluss, wenn jeder Monat so viele Punkte bringt wie der letzte.
fn months_left(data: &GameData, entry: &PolityResearch, tech: u16) -> Option<u32> {
    let rate = entry.points_last_month;
    if rate <= 0.0 {
        return None;
    }
    // Punkte, die nach dem letzten Abschluss übrig blieben, fließen im nächsten Monat mit ein.
    let open = (entry.remaining(data, tech) - entry.carry).max(0.0);
    Some(((open / rate).ceil() as u32).max(1))
}

pub fn tech_state(data: &GameData, research: &Research, polity: PolityId, tech: u16) -> TechState {
    if research.knows(polity, tech) {
        return TechState::Known;
    }
    let missing: Vec<u16> = data.techs[tech as usize]
        .requires_ids
        .iter()
        .copied()
        .filter(|required| !research.knows(polity, *required))
        .collect();
    if !missing.is_empty() {
        return TechState::Locked { missing };
    }
    let Some(entry) = research.get(polity) else {
        return TechState::Available { done: 0.0 };
    };
    let done = share_done(data, entry, tech);
    if entry.target == Some(tech) {
        TechState::Researching {
            done,
            months_left: months_left(data, entry, tech),
        }
    } else {
        TechState::Available { done }
    }
}

/// Dauer in Jahren und Monaten: „3 Jahre 4 Monate“.
pub fn duration(months: u32) -> String {
    let years = match months / 12 {
        0 => None,
        1 => Some("1 Jahr".to_string()),
        years => Some(format!("{years} Jahre")),
    };
    let rest = match months % 12 {
        0 => None,
        1 => Some("1 Monat".to_string()),
        rest => Some(format!("{rest} Monate")),
    };
    match (years, rest) {
        (Some(years), Some(rest)) => format!("{years} {rest}"),
        (Some(only), None) | (None, Some(only)) => only,
        (None, None) => "weniger als 1 Monat".into(),
    }
}

/// Wirkung in Worten, wie die Spieldaten sie nennen: „+4 % Ausstoß der Werke“.
pub fn effect_text(effect: TechEffect) -> String {
    format!("+{} {}", percent(effect.amount() as f32), effect.label())
}

/// Schiffsentwürfe, die diese Technologie voraussetzen (selbst oder über eines ihrer
/// Module), mit ihrer Klasse: „Speer II (Kreuzer)“.
pub fn designs_requiring(data: &GameData, tech: u16) -> Vec<String> {
    data.designs
        .iter()
        .filter(|design| design.stats.techs.contains(&tech))
        .map(|design| format!("{} ({})", design.name, design.stats.class.label()))
        .collect()
}

/// Die gesammelte Wirkung einer Gemeinschaft als Aufschläge je Art.
pub fn effect_totals(effects: &TechEffects) -> [TechEffect; 12] {
    [
        TechEffect::Yield(effects.yield_factor - 1.0),
        TechEffect::FreighterSpeed(effects.freighter_speed - 1.0),
        TechEffect::FleetSpeed(effects.fleet_speed - 1.0),
        TechEffect::ShipSpeed(effects.ship_speed - 1.0),
        TechEffect::WeaponDamage(effects.weapon_damage - 1.0),
        TechEffect::WeaponRange(effects.weapon_range - 1.0),
        TechEffect::Shields(effects.shields - 1.0),
        TechEffect::Armor(effects.armor - 1.0),
        TechEffect::Hull(effects.hull - 1.0),
        TechEffect::GroundAttack(effects.ground_attack - 1.0),
        TechEffect::GroundDefense(effects.ground_defense - 1.0),
        TechEffect::Research(effects.research - 1.0),
    ]
}

/// Was eine Spanne über die Stufe einer anderen Gemeinschaft sagt.
pub fn estimate_meaning(range: TierEstimate) -> String {
    if range.is_exact() {
        format!("Genau bekannt: Stufe {}.", range.lo)
    } else if range.lo == 0 {
        format!("Nichts bestätigt. Möglich ist jede Stufe bis {}.", range.hi)
    } else {
        format!(
            "Bestätigt: mindestens Stufe {}. Möglich: bis Stufe {}.",
            range.lo, range.hi
        )
    }
}

/// Mischt zwei Farben deckend (`share` = Anteil der ersten).
fn blend(first: Color32, second: Color32, share: f32) -> Color32 {
    let mix = |a: u8, b: u8| (a as f32 * share + b as f32 * (1.0 - share)).round() as u8;
    Color32::from_rgb(
        mix(first.r(), second.r()),
        mix(first.g(), second.g()),
        mix(first.b(), second.b()),
    )
}

pub fn tech_window(
    ctx: &egui::Context,
    sim: &SimRefs,
    state: &mut UiState,
    actions: &mut Vec<UiAction>,
) {
    let mut open = state.tech_open;
    let screen = ctx.content_rect();
    egui::Window::new(TITLE)
        .open(&mut open)
        .resizable(false)
        .default_pos([300.0, 56.0])
        // Platz für den ganzen Baum unter der Kopfleiste; das Fenster selbst ist so groß
        // wie sein Inhalt.
        .default_size([TREE_WIDTH + 24.0, (screen.height() - 70.0).max(200.0)])
        .show(ctx, |ui| {
            let Some(player) = sim.config.player else {
                ui.label("Beobachter forschen nicht.");
                return;
            };
            ui.horizontal(|ui| {
                ui.selectable_value(&mut state.tech_tab, TechTab::Own, TAB_OWN);
                ui.selectable_value(&mut state.tech_tab, TechTab::Others, TAB_OTHERS);
            });
            ui.separator();
            // In kleinen Fenstern rollt der Inhalt, statt über den Rand zu ragen.
            egui::ScrollArea::both()
                .id_salt("forschung")
                .max_width((screen.width() - 40.0).max(240.0))
                .max_height((screen.height() - 150.0).max(200.0))
                .show(ui, |ui| match state.tech_tab {
                    TechTab::Own => own_research(ui, sim, player, state, actions),
                    TechTab::Others => estimates(ui, sim, player),
                });
        });
    state.tech_open = open;
}

/// Reiter „Eigene Forschung“: Etat, Ziel, Baum und die Summe der Wirkungen.
fn own_research(
    ui: &mut egui::Ui,
    sim: &SimRefs,
    player: PolityId,
    state: &mut UiState,
    actions: &mut Vec<UiAction>,
) {
    let Some(entry) = sim.research.get(player) else {
        ui.label("Die Forschung beginnt mit dem ersten Monat.");
        return;
    };
    budget(ui, sim, player, entry, state, actions);
    target(ui, sim, player, entry, actions);
    ui.add_space(4.0);
    tree(ui, sim, player, actions);
    ui.add_space(4.0);
    ui.label(RichText::new("Wirkung der eigenen Technik").color(ACCENT));
    let totals: Vec<String> = effect_totals(&sim.research.effects(player))
        .into_iter()
        .filter(|effect| effect.amount() > 0.0005)
        .map(effect_text)
        .collect();
    if totals.is_empty() {
        ui.label(RichText::new("Noch keine Technologie erforscht.").color(MUTED));
    } else {
        ui.scope(|ui| {
            ui.set_max_width(TREE_WIDTH);
            ui.horizontal_wrapped(|ui| {
                for total in totals {
                    ui.label(total);
                    ui.label(RichText::new("·").color(MUTED));
                }
                ui.label(
                    RichText::new(format!(
                        "{} von {} Technologien erforscht",
                        sim.research.known_count(player),
                        sim.data.techs.len()
                    ))
                    .color(MUTED),
                );
            });
        });
    }
}

/// Der Etat: Anteil der laufenden Staatseinnahmen, den der Spieler einstellt.
fn budget(
    ui: &mut egui::Ui,
    sim: &SimRefs,
    player: PolityId,
    entry: &PolityResearch,
    state: &mut UiState,
    actions: &mut Vec<UiAction>,
) {
    let max_share = sim.data.balance.research.max_budget_share;
    let current = (entry.budget_share * 100.0) as f32;
    let mut wanted = state.research_percent.unwrap_or(current);
    ui.horizontal(|ui| {
        ui.label(RichText::new("Etat").color(ACCENT)).on_hover_text(
            "Anteil der laufenden Staatseinnahmen des Vormonats (Lohnsteuer, Zölle, \
             Dividenden): das Gehalt der Forschenden. Gezahlt wird nur aus dem, was die \
             Staatskasse über ihrer Rücklage hält; Sold und Einkäufe der Flotte gehen vor.",
        );
        let slider = egui::Slider::new(&mut wanted, 0.0..=(max_share * 100.0) as f32)
            .step_by(1.0)
            .fixed_decimals(0)
            .suffix(" % der laufenden Staatseinnahmen");
        if ui.add(slider).changed() {
            state.research_percent = Some(wanted);
        }
        let pending = state
            .research_percent
            .filter(|wanted| (wanted - current).abs() >= 0.5);
        let button = ui
            .add_enabled(pending.is_some(), egui::Button::new(SET_BUDGET))
            .on_hover_text("Gilt ab dem nächsten Monatsersten.")
            .on_disabled_hover_text(format!(
                "Es gilt der eingestellte Etat von {}.",
                percent(entry.budget_share as f32)
            ));
        if button.clicked()
            && let Some(wanted) = pending
        {
            let share = (wanted as f64 / 100.0).clamp(0.0, max_share);
            actions.push(UiAction::Command(SimCommand::SetResearchBudget { share }));
        }
    });
    // Zeigt der Regler wieder den geltenden Etat, folgt er ihm auch wieder.
    if state
        .research_percent
        .is_some_and(|wanted| (wanted - current).abs() < 0.5)
    {
        state.research_percent = None;
    }
    let income = to_credits(sim.economy.state_income_of(player));
    let spent = to_credits(entry.spent_last_month);
    ui.label(
        detail(format!(
            "Es gilt {} · Staatseinnahmen des Vormonats {} Credits · zuletzt {} Credits für \
             {} Forschungspunkte im Monat",
            percent(entry.budget_share as f32),
            grouped(income),
            grouped(spent),
            grouped(entry.points_last_month)
        ))
        .color(MUTED),
    );
    if entry.budget_share > 0.0 && entry.target.is_some() && income > 0.0 && spent <= 0.0 {
        ui.label(
            detail(
                "Im letzten Monat floss kein Geld in die Forschung: Es gab kein Ziel, oder die \
                 Staatskasse lag nicht über ihrer Rücklage.",
            )
            .color(WARN),
        );
    }
}

/// Das Ziel, in das die Forschungspunkte fließen, und wer es wählt.
fn target(
    ui: &mut egui::Ui,
    sim: &SimRefs,
    player: PolityId,
    entry: &PolityResearch,
    actions: &mut Vec<UiAction>,
) {
    let data = sim.data;
    ui.horizontal(|ui| {
        ui.label(RichText::new("Ziel").color(ACCENT));
        match entry.target {
            Some(tech) => {
                let def = &data.techs[tech as usize];
                ui.label(RichText::new(&def.name).strong());
                ui.label(
                    RichText::new(format!("({}, Stufe {})", def.branch.label(), def.tier))
                        .color(MUTED),
                );
                let done = share_done(data, entry, tech);
                ui.add(
                    egui::ProgressBar::new(done)
                        .desired_width(150.0)
                        .fill(Color32::from_rgb(150, 105, 40))
                        .text(percent(done)),
                );
                match months_left(data, entry, tech) {
                    Some(months) => ui.label(format!("noch etwa {}", duration(months))),
                    None => ui.label(
                        RichText::new("Dauer offen: zuletzt kamen keine Punkte zusammen")
                            .color(WARN),
                    ),
                };
            }
            None if sim.research.available(data, player).is_empty() => {
                ui.label("Der ganze Baum ist erforscht.");
            }
            None if entry.auto => {
                ui.label("Die Automatik wählt am nächsten Monatsersten.");
            }
            None => {
                ui.label(
                    RichText::new(
                        "Keines: Die Forschung ruht, der Etat bleibt in der Staatskasse.",
                    )
                    .color(BAD),
                );
            }
        }
    });
    ui.horizontal(|ui| {
        if entry.auto {
            ui.label(
                detail(
                    "Die Automatik wählt das Ziel und prüft es jeden Monat. „Erforschen“ an \
                     einer Technologie legt es selbst fest.",
                )
                .color(MUTED),
            );
        } else {
            ui.label(
                detail("Das Ziel ist selbst gewählt. Ist es erforscht, ruht die Forschung.")
                    .color(MUTED),
            );
            if ui
                .small_button(AUTOMATIC)
                .on_hover_text(
                    "Die Wahl des Ziels geht an die Automatik zurück; sie wählt sofort neu.",
                )
                .clicked()
            {
                actions.push(UiAction::Command(SimCommand::SetResearchTarget {
                    tech: None,
                }));
            }
        }
    });
}

/// Der Baum: je Zweig eine Spalte, je Stufe eine Zeile.
fn tree(ui: &mut egui::Ui, sim: &SimRefs, player: PolityId, actions: &mut Vec<UiAction>) {
    let data = sim.data;
    let mut tiers: Vec<u32> = data.techs.iter().map(|tech| tech.tier).collect();
    tiers.sort_unstable();
    tiers.dedup();
    egui::Grid::new("technologiebaum")
        .num_columns(6)
        .min_col_width(0.0)
        .spacing([6.0, 6.0])
        .show(ui, |ui| {
            ui.label(detail("Stufe").color(MUTED));
            for branch in TechBranch::ALL {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(branch.label()).strong());
                    ui.label(
                        detail(format!("Stufe {}", sim.research.tier(data, player, branch)))
                            .color(MUTED),
                    );
                });
            }
            ui.end_row();
            for tier in tiers {
                ui.label(RichText::new(tier.to_string()).color(MUTED));
                for branch in TechBranch::ALL {
                    ui.vertical(|ui| {
                        let here = data
                            .techs
                            .iter()
                            .enumerate()
                            .filter(|(_, tech)| tech.branch == branch && tech.tier == tier);
                        for (index, _) in here {
                            tech_card(ui, sim, player, index as u16, actions);
                        }
                    });
                }
                ui.end_row();
            }
        });
}

/// Eine Technologie im Baum: Name, Stand, Kosten und Wirkungen.
fn tech_card(
    ui: &mut egui::Ui,
    sim: &SimRefs,
    player: PolityId,
    tech: u16,
    actions: &mut Vec<UiAction>,
) {
    let data = sim.data;
    let def = &data.techs[tech as usize];
    let state = tech_state(data, sim.research, player, tech);
    let (fill, title) = match &state {
        TechState::Known => (Color32::from_rgb(24, 44, 34), GOOD),
        TechState::Researching { .. } => (Color32::from_rgb(54, 43, 22), ACCENT),
        TechState::Available { .. } => (Color32::from_rgb(34, 41, 60), TEXT),
        TechState::Locked { .. } => (Color32::from_rgb(23, 26, 36), MUTED),
    };
    egui::Frame::new()
        .fill(fill)
        .corner_radius(3.0)
        .inner_margin(4.0)
        .show(ui, |ui| {
            ui.set_width(CARD_WIDTH);
            ui.spacing_mut().item_spacing.y = 1.0;
            ui.label(RichText::new(&def.name).strong().color(title));
            match &state {
                TechState::Known => {
                    ui.label(detail("erforscht").color(GOOD));
                }
                TechState::Researching { done, months_left } => {
                    ui.add(
                        egui::ProgressBar::new(*done)
                            .desired_width(CARD_WIDTH)
                            .desired_height(7.0)
                            .fill(ACCENT),
                    );
                    let time = match months_left {
                        Some(months) => format!("noch etwa {}", duration(*months)),
                        None => "Dauer offen".into(),
                    };
                    ui.label(
                        detail(format!("in Arbeit: {} · {time}", percent(*done))).color(ACCENT),
                    );
                }
                TechState::Available { done } => {
                    ui.horizontal(|ui| {
                        let button = ui.small_button(RESEARCH).on_hover_text(
                            "Als Forschungsziel setzen. Was am bisherigen Ziel erarbeitet \
                             ist, bleibt erhalten.",
                        );
                        // Für Vorleseprogramme trägt der Knopf den Namen der Technologie:
                        // Sonst hießen alle gleich.
                        button.widget_info(|| {
                            egui::WidgetInfo::labeled(
                                egui::WidgetType::Button,
                                true,
                                format!("{RESEARCH}: {}", def.name),
                            )
                        });
                        if button.clicked() {
                            actions.push(UiAction::Command(SimCommand::SetResearchTarget {
                                tech: Some(tech),
                            }));
                        }
                        if *done > 0.005 {
                            ui.label(detail(format!("begonnen: {}", percent(*done))).color(MUTED));
                        }
                    });
                }
                TechState::Locked { missing } => {
                    let names: Vec<&str> = missing
                        .iter()
                        .map(|required| data.techs[*required as usize].name.as_str())
                        .collect();
                    // Gesperrt: Die Karte nennt, was noch fehlt.
                    ui.label(detail(format!("fehlt: {}", names.join(", "))).color(MUTED));
                }
            }
            let cost = sim.research.cost(data, player, tech);
            let known = state == TechState::Known;
            if !known && cost < def.cost - 0.5 {
                ui.label(detail(format!(
                    "Kosten {} statt {}",
                    grouped(cost),
                    grouped(def.cost)
                )))
                .on_hover_text(
                    "Nachlass durch Wissenstransfer: Gemeinschaften in der Nähe beherrschen \
                     diese Stufe erkennbar.",
                );
            } else {
                ui.label(
                    detail(format!("Kosten {}", grouped(def.cost))).color(if known {
                        MUTED
                    } else {
                        TEXT
                    }),
                );
            }
            for effect in &def.effects {
                ui.label(detail(effect_text(*effect)));
            }
            let designs = designs_requiring(data, tech);
            if !designs.is_empty() {
                ui.label(detail(format!("Voraussetzung für: {}", designs.join(", "))));
            }
        });
}

/// Reiter „Einschätzung der anderen“: je Gemeinschaft und Zweig die Spanne der Stufen, die
/// der Kern als Schätzung führt, und der eigene Einblick.
fn estimates(ui: &mut egui::Ui, sim: &SimRefs, player: PolityId) {
    ui.scope(|ui| {
        ui.set_max_width(TREE_WIDTH);
        ui.label(
            "Den genauen Stand ihrer Forschung kennt nur jede Gemeinschaft selbst. Von den \
             anderen halten wir je Zweig eine Spanne von Stufen für möglich. Sie wird enger, \
             je mehr Einblick wir haben: durch Besitz an ihren Betrieben und durch Gefechte \
             gegen sie. Was Gefechte an Einblick bringen, verblasst ohne neue wieder.",
        );
    });
    ui.add_space(4.0);
    egui::Grid::new("einschaetzung")
        .striped(true)
        .num_columns(7)
        .spacing([16.0, 6.0])
        .show(ui, |ui| {
            for title in ["Gemeinschaft", "Einblick"] {
                ui.label(detail(title).color(MUTED));
            }
            for branch in TechBranch::ALL {
                ui.label(detail(branch.label()).color(MUTED));
            }
            ui.end_row();
            // Zum Vergleich der eigene Stand; den kennt die Gemeinschaft genau.
            estimate_row(ui, sim, player, sim.polities.get(player));
            for other in sim.polities.alive().filter(|other| other.id != player) {
                estimate_row(ui, sim, player, other);
            }
        });
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        let sample = Color32::from_rgb(200, 205, 215);
        for (kind, label) in [
            (TierBox::Confirmed, "bestätigt: mindestens diese Stufe"),
            (TierBox::Possible, "möglich"),
            (TierBox::Excluded, "ausgeschlossen"),
        ] {
            let (rect, _) =
                ui.allocate_exact_size(egui::vec2(BOX_WIDTH, BOX_HEIGHT), egui::Sense::hover());
            paint_box(ui, rect, kind, sample);
            ui.label(detail(label).color(MUTED));
            ui.add_space(8.0);
        }
    });
}

fn estimate_row(ui: &mut egui::Ui, sim: &SimRefs, viewer: PolityId, target: &Polity) {
    let own = viewer == target.id;
    let color = color32(target.color);
    if own {
        ui.label(
            RichText::new(format!("{} (wir)", target.name))
                .color(color)
                .strong(),
        );
        ui.label(detail("eigener Stand").color(MUTED));
    } else {
        ui.label(RichText::new(&target.name).color(color));
        ui.label(percent(sim.research.intel(viewer, target.id)))
            .on_hover_text(
                "Unser Einblick in die Forschung dieser Gemeinschaft. Bei vollem Einblick \
                 sind die Spannen genau.",
            );
    }
    // Was wir sehen, ist die Schätzung des Kerns, nie der Stand der anderen selbst.
    let estimate = sim.research.estimate(sim.data, viewer, target.id);
    for branch in TechBranch::ALL {
        let range = estimate[branch.index()];
        ui.horizontal(|ui| {
            let meaning = estimate_meaning(range);
            tier_strip(ui, range, top_tier(sim.data, branch), color).on_hover_text(&meaning);
            let text = RichText::new(range.to_string());
            ui.label(if range.is_exact() {
                text.color(GOOD)
            } else {
                text
            })
            .on_hover_text(meaning);
        });
    }
    ui.end_row();
}

const BOX_WIDTH: f32 = 9.0;
const BOX_HEIGHT: f32 = 12.0;

/// Was ein Kästchen der Stufenleiste über eine Stufe sagt.
#[derive(Clone, Copy)]
enum TierBox {
    /// Sicher erkannt: bis zur unteren Grenze der Spanne.
    Confirmed,
    /// In der Spanne, aber nicht bestätigt.
    Possible,
    /// Über der Spanne.
    Excluded,
}

fn paint_box(ui: &egui::Ui, rect: egui::Rect, kind: TierBox, color: Color32) {
    let background = ui.visuals().window_fill;
    match kind {
        TierBox::Confirmed => {
            ui.painter().rect_filled(rect, 1.0, color);
        }
        TierBox::Possible => {
            ui.painter()
                .rect_filled(rect, 1.0, blend(color, background, 0.3));
            ui.painter().rect_stroke(
                rect,
                1.0,
                egui::Stroke::new(1.0, blend(color, background, 0.5)),
                egui::StrokeKind::Inside,
            );
        }
        TierBox::Excluded => {
            ui.painter().rect_stroke(
                rect,
                1.0,
                egui::Stroke::new(1.0, blend(MUTED, background, 0.35)),
                egui::StrokeKind::Inside,
            );
        }
    }
}

/// Leiste mit einem Kästchen je Stufe des Zweigs: voll bis zur bestätigten Stufe, hell,
/// soweit die Spanne reicht, leer darüber.
fn tier_strip(ui: &mut egui::Ui, range: TierEstimate, top: u8, color: Color32) -> egui::Response {
    let gap = 2.0;
    let width = (top as f32 * (BOX_WIDTH + gap) - gap).max(1.0);
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(width, BOX_HEIGHT), egui::Sense::hover());
    for tier in 1..=top {
        let left = rect.left() + (tier - 1) as f32 * (BOX_WIDTH + gap);
        let cell = egui::Rect::from_min_size(
            egui::pos2(left, rect.top()),
            egui::vec2(BOX_WIDTH, BOX_HEIGHT),
        );
        let kind = if tier <= range.lo {
            TierBox::Confirmed
        } else if tier <= range.hi {
            TierBox::Possible
        } else {
            TierBox::Excluded
        };
        paint_box(ui, cell, kind, color);
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::TestGame;
    use crate::time_control::SimSpeed;
    use crate::ui::UiState;
    use bevy::prelude::*;
    use sim::command::{CommandLog, CommandOutcome};
    use sim::state::SimConfig;

    fn data() -> GameData {
        GameData::load_workspace().unwrap()
    }

    #[test]
    fn durations_read_as_years_and_months() {
        assert_eq!(duration(1), "1 Monat");
        assert_eq!(duration(11), "11 Monate");
        assert_eq!(duration(12), "1 Jahr");
        assert_eq!(duration(13), "1 Jahr 1 Monat");
        assert_eq!(duration(40), "3 Jahre 4 Monate");
        assert_eq!(duration(240), "20 Jahre");
    }

    #[test]
    fn effects_are_named_as_in_the_game_data() {
        let data = data();
        assert_eq!(
            effect_text(TechEffect::Yield(0.04)),
            "+4 % Ausstoß der Werke"
        );
        // Die Summe je Art entspricht dem, was der Kern aus den erforschten Technologien
        // sammelt: Mit dem ganzen Baum ist jede Art so hoch wie die Summe ihrer Aufschläge
        // in den Daten.
        let all = TechEffects::of(&data, &vec![true; data.techs.len()]);
        for total in effect_totals(&all) {
            let expected: f64 = data
                .techs
                .iter()
                .flat_map(|tech| &tech.effects)
                .filter(|effect| std::mem::discriminant(*effect) == std::mem::discriminant(&total))
                .map(|effect| effect.amount())
                .sum();
            assert!(expected > 0.0, "{}", total.label());
            assert!(
                (total.amount() - expected).abs() < 1e-9,
                "{}: {} statt {expected}",
                total.label(),
                total.amount()
            );
        }
        // Ohne Technologie wirkt nichts.
        assert!(
            effect_totals(&TechEffects::default())
                .iter()
                .all(|effect| effect.amount().abs() < 1e-12)
        );
        // Was eine Technologie an Entwürfen ermöglicht, steht ebenfalls in den Daten.
        let tech = |key: &str| data.tech_index(key).unwrap();
        assert_eq!(
            designs_requiring(&data, tech("waffen_2")),
            ["Speer II (Kreuzer)"]
        );
        assert!(designs_requiring(&data, tech("wirtschaft_1")).is_empty());
        // Jeder Entwurf mit Voraussetzungen ist bei jeder davon genannt.
        for design in &data.designs {
            for required in &design.stats.techs {
                assert!(
                    designs_requiring(&data, *required)
                        .iter()
                        .any(|name| name.starts_with(&design.name)),
                    "{}",
                    design.key
                );
            }
        }
    }

    #[test]
    fn every_technology_has_exactly_the_state_the_core_gives_it() {
        let data = data();
        let me = PolityId(0);
        let mut research = Research::default();
        research.sync(&data, 2);
        let tech = |key: &str| data.tech_index(key).unwrap();
        research.grant(&data, me, tech("waffen_2"));
        // Ein gesetztes Ziel mit angefangener Arbeit und bekanntem Tempo.
        let wanted = tech("waffen_3");
        research.polities[0].target = Some(wanted);
        research.polities[0].progress[wanted as usize] = 3000.0;
        research.polities[0].points_last_month = 500.0;
        research.polities[0].carry = 400.0;

        assert_eq!(
            tech_state(&data, &research, me, tech("waffen_1")),
            TechState::Known
        );
        // 12.000 Punkte, 3.000 erarbeitet, 400 warten: 8.600 offen bei 500 im Monat.
        assert_eq!(
            tech_state(&data, &research, me, wanted),
            TechState::Researching {
                done: 0.25,
                months_left: Some(18)
            }
        );
        assert_eq!(
            tech_state(&data, &research, me, tech("wirtschaft_1")),
            TechState::Available { done: 0.0 }
        );
        assert_eq!(
            tech_state(&data, &research, me, tech("waffen_5")),
            TechState::Locked {
                missing: vec![tech("waffen_4")]
            }
        );
        // Ohne Punkte im letzten Monat gibt es keine Dauer.
        research.polities[0].points_last_month = 0.0;
        assert_eq!(
            tech_state(&data, &research, me, wanted),
            TechState::Researching {
                done: 0.25,
                months_left: None
            }
        );
        // Erforschbar ist genau, was auch der Kern als erforschbar führt.
        let available = research.available(&data, me);
        for index in 0..data.techs.len() as u16 {
            let state = tech_state(&data, &research, me, index);
            let open = matches!(
                state,
                TechState::Available { .. } | TechState::Researching { .. }
            );
            assert_eq!(
                open,
                available.contains(&index),
                "{}",
                data.techs[index as usize].key
            );
            assert_eq!(state == TechState::Known, research.knows(me, index));
        }
    }

    /// Eine laufende Partie ohne Fenster, das Forschungsfenster über seinen Knopf geöffnet.
    fn campaign_with_window() -> TestGame {
        let mut game = TestGame::start("--seed 3");
        game.frames(4);
        assert!(game.window_rect("Forschung").is_none());
        game.click_on("Forschung");
        game.frames(3);
        assert!(game.resource::<UiState>().tech_open);
        assert!(game.window_rect("Forschung").is_some());
        game
    }

    fn player(game: &TestGame) -> PolityId {
        game.resource::<SimConfig>().player.expect("Spieler")
    }

    fn run(game: &mut TestGame, frames: usize) {
        *game.world_mut().resource_mut::<SimSpeed>() = SimSpeed {
            paused: false,
            level: 5,
        };
        game.frames(frames);
        game.world_mut().resource_mut::<SimSpeed>().paused = true;
        game.frames(2);
    }

    #[test]
    fn the_window_opens_and_closes_with_its_button_and_fits_both_window_sizes() {
        let mut game = campaign_with_window();
        for size in [Vec2::new(1280.0, 720.0), Vec2::new(1600.0, 900.0)] {
            game.resize(size);
            game.frames(4);
            let rect = game.window_rect("Forschung").expect("Fenster");
            assert!(
                rect.left() >= 0.0
                    && rect.top() >= 0.0
                    && rect.right() <= size.x
                    && rect.bottom() <= size.y,
                "das Fenster {rect:?} ragt bei {size} über den Rand"
            );
            // Beide Reiter lassen sich in dieser Größe bedienen.
            game.click_on("Einschätzung der anderen");
            assert_eq!(
                game.resource::<UiState>().tech_tab,
                TechTab::Others,
                "{size}"
            );
            let rect = game.window_rect("Forschung").expect("Fenster");
            assert!(rect.right() <= size.x && rect.bottom() <= size.y);
            game.click_on("Eigene Forschung");
            assert_eq!(game.resource::<UiState>().tech_tab, TechTab::Own);
        }
        // Derselbe Knopf schließt es wieder; das Fenster blendet sich einige Frames lang aus.
        game.click_on("Forschung");
        game.frames(10);
        assert!(!game.resource::<UiState>().tech_open);
        assert!(game.window_rect("Forschung").is_none());
    }

    #[test]
    fn clicks_on_the_window_do_not_reach_the_map() {
        use crate::map::Selection;

        let mut game = campaign_with_window();
        let home = game.resource::<Selection>().system;
        assert!(home.is_some());
        let rect = game.window_rect(TITLE).expect("Fenster");
        // Zwei Stellen des Fensters über der freien Karte, in der Titelleiste und neben
        // den Reitern: Ohne das Fenster höbe ein Klick dort die Auswahl auf (siehe die
        // Gegenprobe unten).
        let on_window = [
            Vec2::new(rect.center().x, rect.top() + 12.0),
            Vec2::new(rect.right() - 60.0, rect.top() + 50.0),
        ];
        let scale = |game: &mut TestGame| {
            let world = game.world_mut();
            let projection = world
                .query_filtered::<&Projection, With<crate::map::MapCamera>>()
                .single(world)
                .unwrap();
            match projection {
                Projection::Orthographic(ortho) => ortho.scale,
                _ => panic!("die Karte wird ohne Perspektive gezeigt"),
            }
        };
        let zoom = scale(&mut game);
        for at in on_window {
            game.click(at);
            assert_eq!(game.resource::<Selection>().system, home, "{at}");
            // Auch wenn Zeigerbewegung und Tastendruck im selben Frame eintreffen.
            game.move_pointer(Vec2::new(860.0, 800.0));
            game.frames(2);
            game.click_at_once(at);
            assert_eq!(game.resource::<Selection>().system, home, "{at}");
            // Das Mausrad über dem Fenster gehört ihm und nicht der Karte.
            game.scroll(at, 3.0);
            assert_eq!(scale(&mut game), zoom, "{at}");
        }

        // Gegenprobe ohne das Fenster: Dieselben Stellen gehören dann der Karte.
        game.click_on(TITLE);
        game.frames(10);
        assert!(game.window_rect(TITLE).is_none());
        for at in on_window {
            game.world_mut().resource_mut::<Selection>().system = home;
            game.frames(2);
            game.click(at);
            assert_ne!(
                game.resource::<Selection>().system,
                home,
                "die Stelle {at} liegt nicht über der freien Karte"
            );
        }
        game.scroll(on_window[0], 3.0);
        assert!(scale(&mut game) < zoom, "das Mausrad zoomt die Karte");
    }

    #[test]
    fn choosing_a_target_sends_the_command_and_the_simulation_researches_it() {
        let mut game = campaign_with_window();
        let me = player(&game);
        let data = game.resource::<GameData>().clone();
        // Die Automatik hat im ersten Monat gewählt; der Spieler nimmt etwas anderes.
        run(&mut game, 6);
        let automatic = game.resource::<Research>().get(me).unwrap().target;
        assert!(automatic.is_some() && game.resource::<Research>().get(me).unwrap().auto);
        let wanted = game
            .resource::<Research>()
            .available(&data, me)
            .into_iter()
            .find(|tech| Some(*tech) != automatic)
            .expect("eine zweite erforschbare Technologie");
        let name = data.techs[wanted as usize].name.clone();
        let entries = game.resource::<CommandLog>().entries.len();
        game.click_on(&format!("Erforschen: {name}"));
        // Mit dem nächsten Takt wendet der Kern den Befehl an.
        run(&mut game, 3);
        let log = game.resource::<CommandLog>();
        let sent: Vec<_> = log.entries[entries..].iter().collect();
        assert_eq!(sent.len(), 1, "genau ein Befehl: {sent:?}");
        assert_eq!(
            sent[0].command,
            SimCommand::SetResearchTarget { tech: Some(wanted) }
        );
        assert_eq!(sent[0].polity, me);
        assert_eq!(sent[0].outcome, CommandOutcome::Applied);
        let entry = game.resource::<Research>().get(me).unwrap().clone();
        assert_eq!(entry.target, Some(wanted));
        assert!(!entry.auto);
        // Das Fenster nennt das Ziel, und der Knopf dafür ist fort.
        assert!(game.text(&name).is_some());
        assert_eq!(game.button(&format!("Erforschen: {name}")), None);

        // Die Partie läuft: Die Punkte der nächsten Monate fließen in dieses Ziel.
        run(&mut game, 60 * 5);
        let entry = game.resource::<Research>().get(me).unwrap().clone();
        assert_eq!(entry.target, Some(wanted));
        let earned = entry.progress[wanted as usize];
        assert!(
            earned > 0.0 && entry.points_last_month > 0.0,
            "nach fünf Monaten ist nichts erarbeitet: {entry:?}"
        );
        assert!(earned <= entry.points_total + 1e-6);

        // Zeitraffer bis kurz vor den Abschluss: Mit dem nächsten Monat ist das Ziel
        // erforscht. Ein selbst gewähltes Ziel hat keinen Nachfolger; die Forschung ruht,
        // und das Fenster bietet die Automatik an.
        {
            let cost = entry.cost(&data, wanted);
            let mut research = game.world_mut().resource_mut::<Research>();
            research.polities[me.index()].progress[wanted as usize] = cost - 1.0;
        }
        run(&mut game, 60 * 2);
        let entry = game.resource::<Research>().get(me).unwrap();
        assert!(entry.known[wanted as usize]);
        assert_eq!(entry.target, None);
        assert!(!entry.auto);
        game.click_on("Automatik einschalten");
        run(&mut game, 3);
        let entry = game.resource::<Research>().get(me).unwrap();
        assert!(entry.auto && entry.target.is_some());
    }

    #[test]
    fn the_budget_slider_sets_the_share_through_a_command() {
        let mut game = campaign_with_window();
        let me = player(&game);
        let before = game.resource::<Research>().get(me).unwrap().budget_share;
        // Ohne Änderung am Regler gibt es nichts festzusetzen.
        assert_eq!(game.button("Etat festsetzen"), None);
        // Der Spieler stellt einen anderen Etat ein und setzt ihn fest.
        let wanted: f32 = if before > 0.15 { 10.0 } else { 25.0 };
        game.world_mut().resource_mut::<UiState>().research_percent = Some(wanted);
        game.frames(2);
        game.click_on("Etat festsetzen");
        run(&mut game, 3);
        let share = wanted as f64 / 100.0;
        let log = game.resource::<CommandLog>();
        let last = log.entries.last().expect("ein Befehl");
        assert_eq!(last.command, SimCommand::SetResearchBudget { share });
        assert_eq!(last.polity, me);
        assert_eq!(last.outcome, CommandOutcome::Applied);
        let entry = game.resource::<Research>().get(me).unwrap();
        assert!((entry.budget_share - share).abs() < 1e-9);
        // Der Regler folgt wieder dem geltenden Etat.
        assert_eq!(game.resource::<UiState>().research_percent, None);
        assert_eq!(game.button("Etat festsetzen"), None);
    }

    #[test]
    fn what_the_window_shows_of_others_is_the_estimate_and_never_the_exact_state() {
        let mut game = campaign_with_window();
        let me = player(&game);
        let data = game.resource::<GameData>().clone();
        // Ein Jahr Frieden: Die anderen forschen, unser Einblick bleibt gering.
        run(&mut game, 60 * 12);
        game.click_on("Einschätzung der anderen");
        game.frames(3);
        let window = game.window_rect("Forschung").expect("Fenster");
        let labels: Vec<(String, egui::Rect)> = game
            .labels()
            .into_iter()
            .filter(|(_, rect)| window.contains_rect(*rect))
            .collect();
        let place = |text: &str| {
            labels
                .iter()
                .find(|(label, _)| label == text)
                .map(|(_, rect)| *rect)
                .unwrap_or_else(|| panic!("im Fenster steht kein „{text}“: {labels:?}"))
        };
        let research = game.resource::<Research>();
        let polities = game.resource::<sim::polity::Polities>();
        let (mut ranges, mut wider) = (0, 0);
        for other in polities.alive().filter(|other| other.id != me) {
            let row = place(&other.name);
            let estimate = research.estimate(&data, me, other.id);
            let insight = research.intel(me, other.id);
            // Der Einblick steht in der Zeile.
            assert!(
                labels.iter().any(|(label, rect)| *label == percent(insight)
                    && (rect.center().y - row.center().y).abs() < 6.0),
                "{}: Einblick {insight}",
                other.name
            );
            for branch in TechBranch::ALL {
                let column = place(branch.label());
                let shown: Vec<&String> = labels
                    .iter()
                    .filter(|(_, rect)| {
                        (rect.center().y - row.center().y).abs() < 6.0
                            && rect.left() >= column.left() - 2.0
                            && rect.left() < column.left() + 100.0
                    })
                    .map(|(label, _)| label)
                    .collect();
                let range = estimate[branch.index()];
                // Genau die Schätzung des Kerns, in seiner Schreibweise.
                assert_eq!(
                    shown,
                    vec![&range.to_string()],
                    "{} / {}",
                    other.name,
                    branch.label()
                );
                // Sie enthält die Wahrheit, verrät sie aber nicht: Bei geringem Einblick
                // ist sie eine Spanne aus mehreren Stufen.
                let truth = research.tier(&data, other.id, branch);
                assert!(range.contains(truth));
                ranges += 1;
                if insight < 0.5 {
                    assert!(
                        range.width() >= 2 && !range.is_exact(),
                        "{} / {}: {range} bei Einblick {insight}",
                        other.name,
                        branch.label()
                    );
                    assert_ne!(range.to_string(), truth.to_string());
                    wider += 1;
                }
            }
        }
        assert!(ranges >= 20, "{ranges} Spannen geprüft");
        assert!(wider >= 20, "nur {wider} Spannen bei geringem Einblick");
        // Die eigene Zeile ist genau.
        let own = place(&format!("{} (wir)", polities.get(me).name));
        for branch in TechBranch::ALL {
            let column = place(branch.label());
            let tier = research.tier(&data, me, branch).to_string();
            assert!(
                labels.iter().any(|(label, rect)| *label == tier
                    && (rect.center().y - own.center().y).abs() < 6.0
                    && rect.left() >= column.left() - 2.0
                    && rect.left() < column.left() + 100.0),
                "eigene Stufe {tier} in {}",
                branch.label()
            );
        }
    }
}
