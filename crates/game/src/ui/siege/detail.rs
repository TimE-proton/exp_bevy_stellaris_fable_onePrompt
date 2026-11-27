//! Eine Belagerung im Einzelnen: Phase, Lage, Stärken, Prognose und die Befehle der Seite,
//! die der Spieler führt.

use super::{
    BAD, CAPITULATE, Confirm, FORECAST, GOOD, LAND, LIFT, Role, SiegeUi, WARN, days_text,
    forecast_cost, forecast_text, gated, phase_text, polity_name, progress_of, resting_forecast,
    state_text, units,
};
use crate::battle_view::orbit::WATCH;
use crate::map::Selection;
use crate::ui::fleets::SHOW_ON_MAP;
use crate::ui::format::{decimal, grouped, percent};
use crate::ui::panels::{ACCENT, MUTED, SimRefs, UiAction, detail as small};
use bevy_egui::egui::{self, Color32, RichText};
use sim::command::SimCommand;
use sim::ids::{FleetId, PlanetId, PolityId};
use sim::siege::{Fire, Phase, Posture, SiegeCard, SiegeOrders};

pub const ORDERS_ATTACKER: &str = "Befehle des Belagerers";
pub const ORDERS_DEFENDER: &str = "Befehle der Garnison";
pub const ASSAULT: &str = "Sturm";
pub const SACK: &str = "Nach dem Fall plündern";
pub const HOLD_FIRE: &str = "Geschütze schweigen";
pub const CONFIRM_LIFT: &str = "Ja, aufheben";
pub const CONFIRM_CAPITULATE: &str = "Ja, übergeben";
pub const CANCEL: &str = "Abbrechen";
/// Titel des Fensters mit den Befehlen über der Orbitansicht.
pub const ORDERS_WINDOW: &str = "Befehle";
pub const ASK_LIFT: &str = "Die Belagerung wirklich aufheben?";
pub const ASK_CAPITULATE: &str = "Die Welt wirklich übergeben?";
pub const ASK_LAST_WORLD: &str = "Es ist die letzte eigene Welt: Mit der Übergabe endet die \
     eigene Gemeinschaft, und die Partie läuft als Beobachterpartie weiter. Wirklich übergeben?";

const PHASES: [Phase; 5] = [
    Phase::Approach,
    Phase::Investment,
    Phase::Bombardment,
    Phase::Ground,
    Phase::Underground,
];

/// Die fünf Phasen als Leiste: zurückliegende gedämpft, die laufende hervorgehoben.
fn phase_bar(ui: &mut egui::Ui, card: &SiegeCard) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 3.0;
        for phase in PHASES {
            let number = phase.number();
            let (fill, text) = if number == card.phase_number {
                (
                    if card.control { ACCENT } else { WARN },
                    Color32::from_rgb(20, 20, 24),
                )
            } else if number < card.phase_number {
                (
                    Color32::from_rgb(70, 78, 96),
                    Color32::from_rgb(210, 214, 222),
                )
            } else {
                (Color32::from_rgb(34, 39, 52), MUTED)
            };
            egui::Frame::new()
                .fill(fill)
                .inner_margin(egui::Margin::symmetric(8, 3))
                .corner_radius(3)
                .show(ui, |ui| {
                    ui.label(
                        RichText::new(format!("{number} {}", phase.label()))
                            .color(text)
                            .size(12.0),
                    );
                });
        }
    });
}

/// Die stehenden Befehle, wie sie gelten werden: Was der Spieler bei stehender Zeit schon
/// vorgemerkt hat, zählt, damit ein zweiter Klick den ersten nicht zurücknimmt.
pub fn effective_orders(sim: &SimRefs, card: &SiegeCard) -> SiegeOrders {
    crate::ui::fleets::waiting(sim)
        .filter_map(|command| match command {
            SimCommand::SetSiegeOrders { planet, orders } if *planet == card.planet => {
                Some(*orders)
            }
            _ => None,
        })
        .last()
        .unwrap_or(card.view.siege.orders)
}

/// Haltung und Feuerverbot der Garnison, wie sie gelten werden.
pub fn effective_garrison(sim: &SimRefs, planet: PlanetId) -> (Posture, bool) {
    let defense = sim.sieges.defense(planet);
    crate::ui::fleets::waiting(sim)
        .filter_map(|command| match command {
            SimCommand::SetGarrisonOrders {
                planet: named,
                posture,
                hold_fire,
            } if *named == planet => Some((*posture, *hold_fire)),
            _ => None,
        })
        .last()
        .unwrap_or((defense.posture, defense.hold_fire))
}

fn fire_hint(fire: Fire) -> &'static str {
    match fire {
        Fire::Hold => "Die Flotte bleibt im hohen Orbit und schießt nicht auf die Welt.",
        Fire::Support => "Stützfeuer für die Truppen am Boden.",
        Fire::Precise => "Gezielter Beschuss: viel militärische Wirkung, wenig Begleitschaden.",
        Fire::Area => "Flächenbeschuss: schnell, auf Kosten von Bevölkerung und Kapital.",
    }
}

/// Feuerstufe, Sturm, Schwerpunkt und Plünderung. Jede Änderung geht sofort als Befehl
/// hinaus; lehnte der Kern Befehle für diese Belagerung ab, sind die Felder gesperrt.
fn siege_orders(
    ui: &mut egui::Ui,
    sim: &SimRefs,
    card: &SiegeCard,
    state: &mut SiegeUi,
    actions: &mut Vec<UiAction>,
) {
    let current = effective_orders(sim, card);
    // Ob der Kern Befehle für diese Belagerung überhaupt annimmt, zeigt der Probelauf mit
    // den geltenden; der Schwerpunkt wird nur unter den Zonen gewählt, die es gibt.
    let check = match state.cache.check(
        sim,
        &SimCommand::SetSiegeOrders {
            planet: card.planet,
            orders: card.view.siege.orders,
        },
    ) {
        // Ein schon vorgemerkter gleicher Befehl sperrt die Felder nicht.
        Err(reason) if reason == crate::ui::fleets::WAITING => Ok(()),
        other => other,
    };
    let mut wanted = current;
    ui.add_enabled_ui(check.is_ok(), |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("Feuer").color(MUTED));
            egui::ComboBox::from_id_salt(("feuerstufe", card.planet.0))
                .selected_text(current.fire.label())
                .width(96.0)
                .show_ui(ui, |ui| {
                    for fire in Fire::ALL {
                        ui.selectable_value(&mut wanted.fire, fire, fire.label())
                            .on_hover_text(fire_hint(fire));
                    }
                });
            ui.checkbox(&mut wanted.assault, ASSAULT).on_hover_text(
                "Die Landungstruppe greift an, Zone um Zone. Ohne Sturm hält sie, was sie hat.",
            );
            ui.label(RichText::new("Schwerpunkt").color(MUTED));
            let zones = &card.view.siege.zones;
            let zone_label = |index: u8| {
                zones
                    .get(index as usize)
                    .map_or("–", |zone| zone.kind.label())
            };
            egui::ComboBox::from_id_salt(("schwerpunkt", card.planet.0))
                .selected_text(zone_label(current.focus))
                .width(96.0)
                .show_ui(ui, |ui| {
                    for index in 0..zones.len() as u8 {
                        ui.selectable_value(&mut wanted.focus, index, zone_label(index));
                    }
                })
                .response
                .on_hover_text(
                    "Zone, um die als nächste gekämpft wird, sobald das Umland genommen ist.",
                );
            ui.checkbox(&mut wanted.sack, SACK).on_hover_text(
                "Ein Teil der Kassen der Welt geht an die eigene Staatskasse. Nach einer \
                 Übergabe wird nicht geplündert.",
            );
        });
    });
    if let Err(reason) = &check {
        ui.label(small(reason).color(MUTED));
    }
    if wanted != current && check.is_ok() {
        actions.push(UiAction::Command(SimCommand::SetSiegeOrders {
            planet: card.planet,
            orders: wanted,
        }));
    }
}

/// Eine Welle an Land setzen: Flotte der Belagerung mit Truppen an Bord, Stärke am Regler.
fn landing(
    ui: &mut egui::Ui,
    sim: &SimRefs,
    player: PolityId,
    card: &SiegeCard,
    state: &mut SiegeUi,
    actions: &mut Vec<UiAction>,
) {
    let aboard = |fleet: FleetId| {
        sim.sieges
            .embarked
            .get(&fleet)
            .map_or(0.0, |force| force.troops)
    };
    let own: Vec<FleetId> = card
        .fleets
        .iter()
        .copied()
        .filter(|id| {
            sim.fleets
                .get(*id)
                .is_some_and(|fleet| fleet.owner == player)
        })
        .collect();
    // Die gewählte Flotte, wenn sie hier belagert; sonst die mit den meisten Truppen.
    let fleet = state.fleet.filter(|id| own.contains(id)).or_else(|| {
        own.iter()
            .copied()
            .max_by(|a, b| aboard(*a).total_cmp(&aboard(*b)))
    });
    let Some(fleet) = fleet else {
        ui.label(small("Keine eigene Flotte liegt über der Welt.").color(MUTED));
        return;
    };
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new("Landung").color(MUTED));
        let name = |id: FleetId| sim.fleets.get(id).map_or("–", |fleet| fleet.name.as_str());
        let mut picked = fleet;
        egui::ComboBox::from_id_salt(("landungsflotte", card.planet.0))
            .selected_text(name(fleet))
            .width(150.0)
            .show_ui(ui, |ui| {
                for id in &own {
                    ui.selectable_value(&mut picked, *id, name(*id));
                }
            });
        state.fleet = Some(picked);
        let ready = aboard(picked);
        ui.label(format!("{} Verbände an Bord", units(ready)));
        let most = super::troops::slider_max(ready);
        let mut strength = super::troops::snap(state.strength, most);
        let slider = egui::Slider::new(&mut strength, super::troops::STEP..=most)
            .custom_formatter(|value, _| crate::ui::format::decimal(value, 1))
            .custom_parser(|text| text.replace(',', ".").parse().ok())
            .suffix(" Verbände");
        if ui.add(slider).changed() {
            state.strength = super::troops::snap(strength, most);
        }
        let strength = super::troops::snap(state.strength, most);
        let command = SimCommand::LandTroops {
            fleet: picked,
            planet: card.planet,
            strength: super::troops::commanded(strength, ready),
        };
        // Solange eine Welle dieser Flotte vorgemerkt ist, gibt es keine zweite: Beide
        // gälten im selben Takt, und die zweite fände die Truppen nicht mehr an Bord.
        let check = if super::troops::move_waiting(sim, picked) {
            Err(crate::ui::fleets::WAITING.to_string())
        } else {
            state.cache.check(sim, &command)
        };
        if gated(
            ui,
            LAND,
            format!("{LAND}: {}", card.planet_name),
            &check,
            "Die Verbände landen als Welle der Belagerung. Die Welt muss eingeschlossen \
             sein; aktive Geschütze fangen einen Teil der Welle ab.",
        ) {
            actions.push(UiAction::Command(command));
        }
        if let Err(reason) = &check {
            ui.label(small(reason).color(MUTED));
        }
    });
}

/// Ein Knopf, dessen Befehl erst nach einer Rückfrage hinausgeht.
#[allow(clippy::too_many_arguments)]
fn confirmed(
    ui: &mut egui::Ui,
    sim: &SimRefs,
    state: &mut SiegeUi,
    actions: &mut Vec<UiAction>,
    label: &str,
    yes: &str,
    question: &str,
    hover: &str,
    (confirm, command): (Confirm, SimCommand),
) {
    let check = state.cache.check(sim, &command);
    // Eine Rückfrage, die im vorigen Frame nicht auf dem Schirm stand (Fenster zu, andere
    // Ansicht), ist verfallen: Sie soll nicht Tage später noch auf ein „Ja“ warten.
    let frame = ui.ctx().cumulative_frame_nr();
    if state.confirm.is_some() && frame > state.confirm_frame + 1 {
        state.confirm = None;
    }
    if state.confirm == Some(confirm) {
        state.confirm_frame = frame;
    }
    ui.horizontal_wrapped(|ui| {
        if state.confirm == Some(confirm) && check.is_ok() {
            ui.label(RichText::new(question).color(WARN));
            if ui.button(RichText::new(yes).color(BAD)).clicked() {
                actions.push(UiAction::Command(command));
                state.confirm = None;
            }
            if ui.button(CANCEL).clicked() {
                state.confirm = None;
            }
        } else {
            if state.confirm == Some(confirm) {
                // Der Kern nähme den Befehl nicht mehr an: Die Rückfrage ist hinfällig.
                state.confirm = None;
            }
            if gated(
                ui,
                RichText::new(label).color(BAD),
                label.to_string(),
                &check,
                hover,
            ) {
                state.confirm = Some(confirm);
                state.confirm_frame = frame;
            }
            if let Err(reason) = &check {
                ui.label(small(reason).color(MUTED));
            }
        }
    });
}

fn attacker_orders(
    ui: &mut egui::Ui,
    sim: &SimRefs,
    player: PolityId,
    card: &SiegeCard,
    state: &mut SiegeUi,
    actions: &mut Vec<UiAction>,
) {
    ui.label(RichText::new(ORDERS_ATTACKER).color(ACCENT));
    siege_orders(ui, sim, card, state, actions);
    landing(ui, sim, player, card, state, actions);
    confirmed(
        ui,
        sim,
        state,
        actions,
        LIFT,
        CONFIRM_LIFT,
        ASK_LIFT,
        "Die Flotten geben die Welt frei. Truppen am Boden schiffen sich wieder ein, soweit \
         an Bord Platz ist; dazu braucht der Abzug die Orbitalhoheit.",
        (
            Confirm::Lift(card.planet),
            SimCommand::LiftSiege {
                planet: card.planet,
            },
        ),
    );
}

/// Haltung der Garnison und Feuerverbot; auch für eine Welt, die nicht belagert wird.
pub fn garrison_orders(
    ui: &mut egui::Ui,
    sim: &SimRefs,
    planet: PlanetId,
    state: &mut SiegeUi,
    actions: &mut Vec<UiAction>,
) {
    let (posture, hold_fire) = effective_garrison(sim, planet);
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new("Haltung").color(MUTED));
        for (option, hint) in [
            (
                Posture::Forward,
                "Die Reserve stellt sich der Landung in jeder Zone.",
            ),
            (
                Posture::Deep,
                "Die Regulären sitzen im Untergrund; was dort keinen Platz hat, hält das \
                 Zentrum.",
            ),
        ] {
            let command = SimCommand::SetGarrisonOrders {
                planet,
                posture: option,
                hold_fire,
            };
            let chosen = option == posture;
            let check = if chosen {
                Ok(())
            } else {
                state.cache.check(sim, &command)
            };
            let mut response = ui
                .add_enabled(
                    check.is_ok(),
                    egui::Button::selectable(chosen, option.label()),
                )
                .on_hover_text(hint);
            if let Err(reason) = &check {
                response = response.on_disabled_hover_text(reason);
            }
            if response.clicked() && !chosen {
                actions.push(UiAction::Command(command));
            }
        }
        let command = SimCommand::SetGarrisonOrders {
            planet,
            posture,
            hold_fire: !hold_fire,
        };
        let check = state.cache.check(sim, &command);
        let mut wanted = hold_fire;
        let response = ui
            .add_enabled(check.is_ok(), egui::Checkbox::new(&mut wanted, HOLD_FIRE))
            .on_hover_text(
                "Die Geschütze feuern nicht auf Schiffe im niedrigen Orbit; das spart Vorräte.",
            );
        if let Err(reason) = &check {
            response.on_disabled_hover_text(reason);
        }
        if wanted != hold_fire {
            actions.push(UiAction::Command(command));
        }
    });
}

/// Die Rückfrage vor der Übergabe. Mit der letzten Welt endet die Gemeinschaft: Das sagt
/// die Rückfrage vorher.
pub fn capitulate_question(worlds: usize) -> &'static str {
    if worlds <= 1 {
        ASK_LAST_WORLD
    } else {
        ASK_CAPITULATE
    }
}

fn defender_orders(
    ui: &mut egui::Ui,
    sim: &SimRefs,
    card: &SiegeCard,
    state: &mut SiegeUi,
    actions: &mut Vec<UiAction>,
) {
    ui.label(RichText::new(ORDERS_DEFENDER).color(ACCENT));
    garrison_orders(ui, sim, card.planet, state, actions);
    let worlds = sim.galaxy.planets_of(card.defender).count();
    confirmed(
        ui,
        sim,
        state,
        actions,
        CAPITULATE,
        CONFIRM_CAPITULATE,
        capitulate_question(worlds),
        "Die Welt geht an den Belagerer, sobald er eine Besatzungstruppe am Boden oder im \
         Orbit hat. Nach einer Übergabe wird nicht geplündert.",
        (
            Confirm::Capitulate(card.planet),
            SimCommand::Capitulate {
                planet: card.planet,
            },
        ),
    );
}

/// Was die stehenden Befehle heute nicht bewirken konnten, in den Worten des Kerns.
pub fn blocked_text(card: &SiegeCard) -> Option<String> {
    let reasons: Vec<&str> = card
        .view
        .siege
        .report
        .blocked
        .iter()
        .map(|reason| reason.label())
        .collect();
    (!reasons.is_empty()).then(|| format!("Zuletzt ohne Wirkung: {}.", reasons.join("; ")))
}

/// Die Anlagen der Welt in einer Zeile.
pub fn defense_text(card: &SiegeCard) -> String {
    let defense = &card.defense;
    let guns = if defense.guns <= 0.0 {
        "keine Geschütze".to_string()
    } else {
        format!(
            "Geschütze: {} von {} Stufen aktiv",
            if defense.guns_active {
                decimal(defense.guns, 1)
            } else {
                "0".into()
            },
            decimal(defense.guns, 1)
        )
    };
    let shield = if defense.shield == 0 {
        "kein Schild".to_string()
    } else {
        format!(
            "Schild Stufe {}, Ladung {}",
            defense.shield,
            grouped(defense.charge)
        )
    };
    format!(
        "Stationen {} · {guns} · {shield} · Bunker {}",
        decimal(defense.stations, 1),
        defense.bunkers
    )
}

/// Die Flotten im Orbit mit Feuerkraft und Füllständen; liegt keine dort, gibt es auch
/// keine Füllstände zu nennen.
pub fn orbit_text(names: &[&str], card: &SiegeCard) -> String {
    if names.is_empty() {
        return "keine".to_string();
    }
    format!(
        "{} · Feuerkraft {} · Munition {} · Treibstoff {}",
        names.join(", "),
        grouped(card.firepower),
        percent(card.ammo as f32),
        percent(card.fuel as f32)
    )
}

/// Wie eine Zone steht, und in welcher Farbe: Für den Belagerer ist eine genommene Zone
/// ein Erfolg, für den Verteidiger ein Verlust.
pub fn zone_state(role: Role, taken: bool, contested: bool) -> (&'static str, Color32) {
    if taken {
        let color = match role {
            Role::Attacker => GOOD,
            Role::Defender => BAD,
            Role::Onlooker => Color32::from_rgb(200, 205, 215),
        };
        ("genommen", color)
    } else if contested {
        ("umkämpft", WARN)
    } else {
        ("gehalten", MUTED)
    }
}

/// Die Befehle der Seite, die der Spieler führt.
fn orders(
    ui: &mut egui::Ui,
    sim: &SimRefs,
    card: &SiegeCard,
    state: &mut SiegeUi,
    actions: &mut Vec<UiAction>,
) {
    let player = sim.config.player;
    match (Role::of(player, card), player) {
        (Role::Attacker, Some(player)) => attacker_orders(ui, sim, player, card, state, actions),
        (Role::Defender, Some(_)) => defender_orders(ui, sim, card, state, actions),
        _ => {
            ui.label(small("Befehle gibt hier, wer belagert oder belagert wird.").color(MUTED));
        }
    }
}

/// Die Befehle als eigenes kleines Fenster über der Orbitansicht: Feuer, Sturm, Landung,
/// Aufheben oder die Befehle der Garnison, während die Szene weiterläuft. Beobachter
/// bekommen kein Fenster.
pub fn orders_window(
    ctx: &egui::Context,
    area: egui::Rect,
    sim: &SimRefs,
    card: &SiegeCard,
    state: &mut SiegeUi,
    actions: &mut Vec<UiAction>,
) {
    if Role::of(sim.config.player, card) == Role::Onlooker {
        return;
    }
    egui::Window::new(ORDERS_WINDOW)
        .resizable(false)
        .pivot(egui::Align2::RIGHT_BOTTOM)
        .default_pos([area.right() - 12.0, area.bottom() - 12.0])
        .constrain_to(area)
        .show(ctx, |ui| {
            ui.set_width(400.0);
            orders(ui, sim, card, state, actions);
        });
}

pub fn siege_detail(
    ui: &mut egui::Ui,
    sim: &SimRefs,
    card: &SiegeCard,
    state: &mut SiegeUi,
    actions: &mut Vec<UiAction>,
) {
    let player = sim.config.player;
    let role = Role::of(player, card);
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new(&card.planet_name).color(ACCENT).strong());
        ui.label(format!(
            "{} · {}",
            sim.galaxy.system(card.system).name,
            role.label()
        ));
        if !state.in_orbit
            && ui
                .button(SHOW_ON_MAP)
                .on_hover_text("Wählt die Welt und führt die Karte dorthin.")
                .clicked()
        {
            actions.push(UiAction::Select(Selection {
                system: Some(card.system),
                planet: Some(card.planet),
            }));
            actions.push(UiAction::Focus(card.system));
        }
        if !state.in_orbit
            && ui
                .button(RichText::new(WATCH).color(ACCENT))
                .on_hover_text(
                    "Zeigt die Belagerung in der Orbitansicht: die Welt, die Flotten im \
                     Orbit, Schild, Beschuss, Landung und den Kampf am Boden. Sie läuft mit \
                     der Zeit der Partie weiter; die Befehle stehen dort in einem eigenen \
                     kleinen Fenster.",
                )
                .clicked()
        {
            actions.push(UiAction::WatchSiege(card.planet));
        }
    });
    ui.horizontal_wrapped(|ui| {
        ui.label(polity_name(sim, card.attacker));
        ui.label(RichText::new("belagert").color(MUTED));
        ui.label(polity_name(sim, card.defender));
    });
    phase_bar(ui, card);
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new(phase_text(card)).strong());
        let color = if card.handover.is_some() {
            GOOD
        } else if card.control {
            MUTED
        } else {
            WARN
        };
        ui.label(RichText::new(state_text(card)).color(color));
    });
    if !card.control && card.handover.is_none() {
        ui.label(small(
            "Die Flotten des Belagerers stehen im Gefecht oder halten den Orbit nicht: \
             Niemand zehrt an Vorräten oder Garnison, bis der Orbit entschieden ist.",
        ));
    }
    if let Some(text) = blocked_text(card) {
        ui.label(small(text).color(WARN));
    }

    // Die Befehle stehen vor Zahlen und Prognose: In einem kleinen Spielfenster sind sie
    // so schon beim Öffnen zu sehen.
    ui.add_space(4.0);
    orders(ui, sim, card, state, actions);
    ui.add_space(4.0);

    egui::Grid::new(("belagerung_lage", card.planet.0))
        .num_columns(2)
        .show(ui, |ui| {
            let label = |ui: &mut egui::Ui, text: &str| {
                ui.label(RichText::new(text).color(MUTED));
            };
            label(ui, "Dauer");
            ui.label(days_text(card.days as f64));
            ui.end_row();
            label(ui, "Fortschritt");
            let progress = progress_of(card);
            ui.horizontal(|ui| {
                ui.add(
                    egui::ProgressBar::new(progress.share)
                        .desired_width(160.0)
                        .desired_height(10.0)
                        .fill(ACCENT),
                );
                ui.label(&progress.text).on_hover_text(progress.hint);
            });
            ui.end_row();
            label(ui, "Vorräte der Garnison");
            ui.label(format!(
                "{}{}",
                days_text(card.stores_days),
                if card.blockaded {
                    " · System gesperrt"
                } else {
                    " · System offen"
                }
            ));
            ui.end_row();
            label(ui, "Nachschub der Landungstruppe");
            ui.label(if card.troops.army > 0.0 {
                days_text(card.army_supply_days)
            } else {
                "keine Truppe am Boden".to_string()
            });
            ui.end_row();
            label(ui, "Anlagen der Welt");
            ui.label(defense_text(card));
            ui.end_row();
            label(ui, "Truppen des Belagerers");
            ui.label(format!(
                "{} Verbände am Boden · {} an Bord",
                units(card.troops.army),
                units(card.troops.embarked)
            ));
            ui.end_row();
            label(ui, "Truppen der Verteidiger");
            ui.label(format!(
                "Miliz {} · Reserve {} · Untergrund {} (bei Beginn {})",
                units(card.troops.militia),
                units(card.troops.reserve),
                units(card.troops.deep),
                units(card.troops.garrison_start)
            ))
            .on_hover_text(format!(
                "Eine Übergabe oder ausgehungerte Welt annehmen kann eine Besatzungstruppe \
                 von {} Verbänden.",
                units(card.troops.occupation_force)
            ));
            ui.end_row();
            label(ui, "Flotten im Orbit");
            let names: Vec<&str> = card
                .fleets
                .iter()
                .filter_map(|id| sim.fleets.get(*id))
                .map(|fleet| fleet.name.as_str())
                .collect();
            ui.label(orbit_text(&names, card));
            ui.end_row();
        });

    let siege = &card.view.siege;
    if !siege.zones.is_empty() {
        let front = siege.front();
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("Zonen").color(MUTED));
            for (index, zone) in siege.zones.iter().enumerate() {
                // Welche Zone umkämpft ist, sagt die Lage des Kerns (`front`); hier wird
                // sie nur benannt.
                let (state, color) =
                    zone_state(role, zone.taken, front == Some(index) && siege.army > 0.0);
                ui.label(
                    RichText::new(format!(
                        "{} {} ({state}, Miliz {})",
                        zone.kind.label(),
                        percent(zone.share),
                        units(zone.militia)
                    ))
                    .color(color)
                    .size(12.0),
                );
            }
        });
    }

    ui.add_space(4.0);
    ui.label(RichText::new(FORECAST).color(ACCENT));
    if let Some(resting) = resting_forecast(card) {
        // Ohne Orbitalhoheit gelten die drei Wege nicht: ein Satz statt dreier Zahlen,
        // die der Verlauf widerlegte.
        ui.label(RichText::new(resting).color(WARN));
        return;
    }
    for (course, forecast) in &card.forecasts {
        let color = if forecast.falls {
            GOOD
        } else {
            Color32::from_rgb(200, 205, 215)
        };
        ui.label(RichText::new(forecast_text(*course, forecast)).color(color));
        ui.label(small(forecast_cost(forecast)).color(MUTED));
    }
    ui.label(
        small(
            "Vorausgerechnet mit dem Tagesrechner der Belagerung auf der heutigen Lage: \
             Aushungern ohne Feuer und Landung, Sturm mit allem, was an Bord ist, \
             Flächenbeschuss mit Landung, sobald sie trägt.",
        )
        .color(MUTED),
    );
}
