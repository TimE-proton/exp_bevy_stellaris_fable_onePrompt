//! Börse, Besitzverteilung, Zusammenhalt und Staatsfinanzen.

use super::UiState;
use super::format::{decimal, percent};
use super::panels::{ACCENT, MUTED, SimRefs, UiAction, color32};
use bevy_egui::egui::{self, Color32, RichText};
use sim::command::SimCommand;
use sim::economy::to_credits;
use sim::finance::shares::{ExpropriationScope, seat_of_issuer};
use sim::finance::{
    Stage, holder_name, issuer_name, ownership_of_region, polity_of_holder, polity_of_issuer,
};
use sim::ids::{HolderId, IssuerId, PlanetId, PolityId};

const GOOD: Color32 = Color32::from_rgb(120, 200, 130);
const WARN: Color32 = Color32::from_rgb(240, 190, 80);
const BAD: Color32 = Color32::from_rgb(235, 110, 95);

pub fn stage_color(stage: Stage) -> Color32 {
    match stage {
        Stage::Loyal => GOOD,
        Stage::Restless => WARN,
        Stage::Autonomous => BAD,
    }
}

fn capitals(sim: &SimRefs) -> Vec<Option<PlanetId>> {
    sim.polities
        .list
        .iter()
        .map(|polity| polity.capital)
        .collect()
}

fn points_color(points: f64) -> Color32 {
    if points > 0.5 {
        GOOD
    } else if points < -0.5 {
        BAD
    } else {
        MUTED
    }
}

/// Ein waagerechter Balken aus farbigen Abschnitten (Anteile 0..1).
fn share_bar(ui: &mut egui::Ui, parts: &[(Color32, f64)]) {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width().min(380.0), 12.0),
        egui::Sense::hover(),
    );
    let mut left = rect.left();
    for (color, share) in parts {
        let width = rect.width() * (*share as f32).clamp(0.0, 1.0);
        let part = egui::Rect::from_min_max(
            egui::pos2(left, rect.top()),
            egui::pos2(left + width, rect.bottom()),
        );
        ui.painter().rect_filled(part, 0.0, *color);
        left += width;
    }
}

/// Reiter „Besitz": Wem die Betriebe einer Welt gehören und was daraus für den
/// Zusammenhalt folgt.
pub fn ownership_tab(
    ui: &mut egui::Ui,
    sim: &SimRefs,
    planet_id: PlanetId,
    actions: &mut Vec<UiAction>,
) {
    let planet = sim.galaxy.planet(planet_id);
    let Some(owner) = planet.owner else {
        ui.label("Unbesiedelt.");
        return;
    };
    let capitals = capitals(sim);
    let region = sim.finance.region(planet_id);
    let balance = &sim.data.balance.finance.cohesion;

    ui.label(RichText::new(&planet.name).strong());
    ui.horizontal(|ui| {
        ui.label(RichText::new("Zusammenhalt").color(MUTED));
        ui.add(
            egui::ProgressBar::new((region.cohesion / 100.0) as f32)
                .desired_width(170.0)
                .fill(stage_color(region.stage))
                .text(format!(
                    "{} · {}",
                    decimal(region.cohesion, 0),
                    region.stage.label()
                )),
        );
        ui.label(
            RichText::new(format!("strebt zu {}", decimal(region.factors.target, 0))).color(MUTED),
        )
        .on_hover_text("Der Zusammenhalt nähert sich diesem Wert Monat für Monat an.");
    });
    match region.stage {
        Stage::Loyal => {}
        Stage::Restless => {
            ui.label(RichText::new(format!(
                "Unruhig. Fällt der Zusammenhalt {} Monate lang unter {}, wird die Region autonom.",
                balance.autonomy_months, balance.autonomy_below
            )).color(WARN));
        }
        Stage::Autonomous => {
            ui.label(RichText::new(
                "Autonom: Die Region führt keine Steuern mehr ab. Bleibt es so, spaltet sie sich ab.",
            ).color(BAD));
        }
    }

    ui.add_space(4.0);
    egui::Grid::new("zusammenhalt")
        .striped(true)
        .num_columns(2)
        .show(ui, |ui| {
            ui.label(RichText::new("Grundwert").color(MUTED));
            ui.label(decimal(balance.base, 0));
            ui.end_row();
            for (label, points) in region.factors.contributions(balance) {
                if points.abs() < 0.5 {
                    continue;
                }
                ui.label(label);
                ui.label(RichText::new(format!("{points:+.0}")).color(points_color(points)));
                ui.end_row();
            }
        });

    ui.separator();
    ui.label(RichText::new("Wem die Betriebe gehören").color(MUTED));
    let ownership =
        ownership_of_region(sim.finance, sim.galaxy, sim.logistics, &capitals, planet_id);
    if ownership.value <= 0.0 {
        ui.label("Noch keine Betriebe mit Anteilen.");
        return;
    }
    let others: f64 = ownership.regions.values().sum();
    let foreign: f64 = ownership.foreign.values().sum();
    let local_color = Color32::from_rgb(110, 170, 230);
    let center_color = color32(sim.polities.get(owner).color);
    let other_color = Color32::from_rgb(150, 150, 160);
    share_bar(
        ui,
        &[
            (local_color, ownership.local),
            (center_color, ownership.center),
            (other_color, others),
            (BAD, foreign),
        ],
    );
    egui::Grid::new("besitzgruppen")
        .num_columns(2)
        .show(ui, |ui| {
            let mut row = |color: Color32, label: String, share: f64| {
                ui.label(RichText::new(label).color(color));
                ui.label(percent(share as f32));
                ui.end_row();
            };
            row(
                local_color,
                "Einheimische (Elite und Fonds)".into(),
                ownership.local,
            );
            row(
                center_color,
                "Zentrum (Staat und Hauptwelt)".into(),
                ownership.center,
            );
            if others > 0.001 {
                row(other_color, "andere Regionen des Reichs".into(), others);
            }
            for (polity, share) in &ownership.foreign {
                row(
                    BAD,
                    format!("fremd: {}", sim.polities.get(*polity).name),
                    *share,
                );
            }
        });
    ui.label(
        RichText::new(format!(
            "Marktwert {} Credits · Streuung {}",
            decimal(ownership.value, 0),
            percent(ownership.dispersion() as f32)
        ))
        .color(MUTED)
        .small(),
    );

    ui.add_space(6.0);
    ui.label(RichText::new("Betriebe vor Ort").color(MUTED));
    let player = sim.config.player.filter(|player| *player == owner);
    egui::Grid::new("betriebe").striped(true).num_columns(5).show(ui, |ui| {
        for title in ["Betrieb", "Kurs", "Staat", "lokal", ""] {
            ui.label(RichText::new(title).color(MUTED).small());
        }
        ui.end_row();
        for (id, issuer) in &sim.finance.issuers {
            if seat_of_issuer(sim.galaxy, sim.logistics, &capitals, *id) != Some(planet_id)
                || matches!(id, IssuerId::House(_))
            {
                continue;
            }
            if ui.link(issuer_name(sim.galaxy, sim.logistics, *id)).on_hover_text("An der Börse zeigen").clicked() {
                actions.push(UiAction::ShowIssuer(*id));
            }
            ui.label(decimal(issuer.price, 2));
            let state_share = issuer.share_of(HolderId::State(owner));
            ui.label(percent(state_share as f32));
            let local = issuer.share_of(HolderId::Public(planet_id)) + issuer.share_of(HolderId::Elite(planet_id));
            ui.label(percent(local as f32));
            match player {
                Some(_) if state_share > 0.0 => {
                    if ui
                        .small_button("5 % verteilen")
                        .on_hover_text("Der Staat überträgt 5 % der Anteile ohne Gegenleistung an den Fonds dieser Region.")
                        .clicked()
                    {
                        actions.push(UiAction::Command(SimCommand::DistributeShares {
                            issuer: *id,
                            shares: (issuer.shares / 20).max(1),
                            to: HolderId::Public(planet_id),
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

/// Die Börse: alle Emittenten, ihr Kurs und ihre Halter, mit den Werkzeugen des Staates.
pub fn exchange_window(
    ctx: &egui::Context,
    area: egui::Rect,
    sim: &SimRefs,
    state: &mut UiState,
    actions: &mut Vec<UiAction>,
) {
    let mut open = state.exchange_open;
    egui::Window::new("Börse").open(&mut open).constrain_to(area).default_pos([450.0, 60.0]).default_width(760.0).show(ctx, |ui| {
        let capitals = capitals(sim);
        let player = sim.config.player;
        ui.horizontal(|ui| {
            ui.checkbox(&mut state.exchange_all, "alle Gemeinschaften");
            ui.label(RichText::new("Kurse folgen Gewinnen; Risiko und Ereignisse drücken sie.").color(MUTED));
        });
        egui::ScrollArea::vertical().id_salt("kursliste").max_height(250.0).auto_shrink([false, true]).show(ui, |ui| {
            egui::Grid::new("kurse").striped(true).num_columns(7).show(ui, |ui| {
                for title in ["Emittent", "Gebiet", "Kurs", "Div.-Rendite", "Risiko", "Marktwert", "Staat"] {
                    ui.label(RichText::new(title).color(MUTED).small());
                }
                ui.end_row();
                for (id, issuer) in &sim.finance.issuers {
                    let Some(polity) = polity_of_issuer(sim.galaxy, sim.logistics, *id) else {
                        continue;
                    };
                    if !state.exchange_all && Some(polity) != player {
                        continue;
                    }
                    let selected = state.issuer == Some(*id);
                    if ui.selectable_label(selected, issuer_name(sim.galaxy, sim.logistics, *id)).clicked() {
                        state.issuer = Some(*id);
                    }
                    let home = sim.polities.get(polity);
                    ui.label(RichText::new(&home.short).color(color32(home.color)));
                    ui.label(decimal(issuer.price, 2));
                    ui.label(percent(issuer.dividend_yield() as f32));
                    ui.label(RichText::new(percent(issuer.risk as f32)).color(if issuer.risk > 0.2 { BAD } else { MUTED }));
                    ui.label(decimal(issuer.market_value(), 0));
                    match player {
                        Some(player) => ui.label(percent(issuer.share_of(HolderId::State(player)) as f32)),
                        None => ui.label(""),
                    };
                    ui.end_row();
                }
            });
        });
        ui.separator();
        let Some(id) = state.issuer else {
            ui.label(RichText::new("Emittenten wählen, um Halter und Werkzeuge zu sehen.").color(MUTED));
            return;
        };
        let Some(issuer) = sim.finance.issuer(id) else {
            state.issuer = None;
            return;
        };
        let name = issuer_name(sim.galaxy, sim.logistics, id);
        let seat = seat_of_issuer(sim.galaxy, sim.logistics, &capitals, id);
        ui.horizontal(|ui| {
            ui.label(RichText::new(&name).strong());
            ui.label(format!("{} Anteile zu {}", issuer.shares, decimal(issuer.price, 2)));
            if issuer.shock < 0.97 {
                ui.label(RichText::new(format!("Stimmung {}", percent(issuer.shock as f32))).color(BAD))
                    .on_hover_text("Ein Ereignis hat den Kurs gedrückt; die Wirkung klingt ab.");
            }
        });
        ui.columns(2, |columns| {
            let ui = &mut columns[0];
            ui.label(RichText::new("Halter").color(MUTED));
            egui::ScrollArea::vertical().id_salt("halter").max_height(180.0).show(ui, |ui| {
                egui::Grid::new("halterliste").striped(true).num_columns(3).show(ui, |ui| {
                    let mut holders: Vec<_> = issuer.holdings.iter().collect();
                    holders.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
                    for (holder, shares) in holders {
                        let color = match polity_of_holder(sim.galaxy, *holder) {
                            Some(polity) => color32(sim.polities.get(polity).color),
                            None => MUTED,
                        };
                        ui.label(RichText::new(holder_name(sim.galaxy, sim.polities, *holder)).color(color));
                        ui.label(percent(*shares as f32 / issuer.shares.max(1) as f32));
                        let offered = issuer.offered.get(holder).copied().unwrap_or(0);
                        ui.label(if offered > 0 { format!("bietet {offered} an") } else { String::new() });
                        ui.end_row();
                    }
                });
            });

            let ui = &mut columns[1];
            let Some(player) = player else {
                ui.label("Beobachter handeln nicht.");
                return;
            };
            let me = HolderId::State(player);
            let held = issuer.held_by(me);
            let treasury = to_credits(sim.economy.treasury(player));
            ui.label(RichText::new(format!(
                "Staat hält {} ({}) · Kasse {}",
                held,
                percent(issuer.share_of(me) as f32),
                decimal(treasury, 0)
            )).color(MUTED));
            ui.add(egui::Slider::new(&mut state.trade_percent, 1.0..=25.0).suffix(" % der Anteile"));
            let lot = ((issuer.shares as f32 * state.trade_percent / 100.0) as u64).max(1);
            ui.horizontal_wrapped(|ui| {
                if ui.button("Kaufen").on_hover_text(format!(
                    "Bis zu {lot} Anteile am Markt kaufen. Der Kurs steigt mit der Menge; verkauft wird nur, was andere abgeben."
                )).clicked() {
                    actions.push(UiAction::Command(SimCommand::BuyShares { issuer: id, shares: lot, max_spend: treasury }));
                }
                if ui.add_enabled(held > 0, egui::Button::new("Anbieten")).on_hover_text(
                    "Eigene Anteile zum Verkauf stellen. Käufer bedienen sich in den folgenden Monaten.",
                ).clicked() {
                    actions.push(UiAction::Command(SimCommand::OfferShares { issuer: id, shares: lot.min(held) }));
                }
                if let Some(seat) = seat
                    && ui.add_enabled(held > 0, egui::Button::new("An die Region verteilen")).on_hover_text(format!(
                        "Anteile ohne Gegenleistung an den Fonds von {} übertragen. Bindet die Region an ihre Betriebe.",
                        sim.galaxy.planet(seat).name
                    )).clicked()
                {
                        actions.push(UiAction::Command(SimCommand::DistributeShares {
                            issuer: id,
                            shares: lot.min(held),
                            to: HolderId::Public(seat),
                        }));
                }
            });
            ui.add_space(4.0);
            ui.add(egui::Slider::new(&mut state.premium_percent, 5.0..=100.0).suffix(" % Aufschlag"));
            if ui.button("Übernahmeangebot").on_hover_text(
                "Allen anderen Haltern den Kurs plus Aufschlag bieten. Fonds verkaufen ab 10 %, eigene Eliten ab 25 %, fremde ab 40 %; fremde Staaten nie.",
            ).clicked() {
                actions.push(UiAction::Command(SimCommand::TakeoverBid { issuer: id, premium: state.premium_percent as f64 / 100.0 }));
            }
            if polity_of_issuer(sim.galaxy, sim.logistics, id) == Some(player) {
                ui.add_space(4.0);
                ui.label(RichText::new("Enteignung").color(BAD));
                ui.horizontal(|ui| {
                    for (label, scope) in [("Fremde", ExpropriationScope::Foreign), ("Alle", ExpropriationScope::Everyone)] {
                        if ui.button(label).on_hover_text(
                            "Anteile ohne Entschädigung einziehen. Wirkt sofort, aber alle Kurse im Reich brechen ein, und das Misstrauen drückt Zusammenhalt und Kreditwürdigkeit über Jahre.",
                        ).clicked() {
                            actions.push(UiAction::Command(SimCommand::Expropriate { issuer: id, scope }));
                        }
                    }
                });
            }
        });
    });
    state.exchange_open = open;
}

/// Staatsfinanzen: Kasse, Schulden, Steuern, Anleihen, und wer bei wem beteiligt ist.
pub fn treasury_window(
    ctx: &egui::Context,
    area: egui::Rect,
    sim: &SimRefs,
    state: &mut UiState,
    actions: &mut Vec<UiAction>,
) {
    let mut open = state.treasury_open;
    // Das Fenster rollt, wo der Haushalt mit allem anderen nicht mehr auf den Schirm passt.
    // Die Höhe gilt in jedem Frame: Wird das Spielfenster kleiner, bleibt das Fenster unter
    // der Kopfleiste.
    let room = super::panels::window_room(area);
    egui::Window::new("Staatsfinanzen").open(&mut open).default_pos([430.0, 56.0]).default_width(600.0).default_height((room - 60.0).max(160.0)).max_height(room).constrain_to(area).vscroll(true).show(ctx, |ui| {
        let Some(player) = sim.config.player else {
            ui.label("Beobachter führen keinen Staat.");
            return;
        };
        let finance = sim.finance.polity(player);
        let treasury = to_credits(sim.economy.treasury(player));
        let debt = to_credits(sim.finance.debt_of(player));
        let tax = sim.economy.tax_rates[player.index()];
        egui::Grid::new("staat").num_columns(2).show(ui, |ui| {
            let mut row = |label: &str, value: String| {
                ui.label(RichText::new(label).color(MUTED));
                ui.label(value);
                ui.end_row();
            };
            row("Staatskasse", format!("{} Credits", decimal(treasury, 0)));
            // Laufende Einnahmen, wie der Haushalt sie führt; die Kasse selbst wächst kaum
            // noch, seit jeder Monat seine Ansprüche bedient.
            row("Einnahmen des Vormonats", format!("{} Credits", decimal(to_credits(sim.economy.state_income_of(player)), 0)));
            row("Schulden", format!("{} Credits", decimal(debt, 0)));
            row("Zins für neue Anleihen", percent(finance.bond_yield as f32));
            row("Kriegsmüdigkeit", decimal(finance.war_weariness, 0));
            if finance.expropriation_risk > 0.01 {
                row("Misstrauen nach Enteignung", percent(finance.expropriation_risk as f32));
            }
            if finance.defaults > 0 {
                row("Zahlungsausfälle", finance.defaults.to_string());
            }
        });
        ui.separator();
        super::budget::budget_section(ui, sim, player);
        ui.separator();

        ui.label(RichText::new("Steuern").color(ACCENT));
        if (state.tax_percent - (tax * 100.0) as f32).abs() > 20.0 || state.tax_percent == 0.0 {
            state.tax_percent = (tax * 100.0) as f32;
        }
        ui.horizontal(|ui| {
            ui.add(egui::Slider::new(&mut state.tax_percent, 0.0..=50.0).suffix(" % Einkommensteuer"));
            if ui.button("Festsetzen").on_hover_text(format!(
                "Heute {}. Jeder Punkt über {} kostet Zusammenhalt in allen Regionen.",
                percent(tax as f32),
                percent(sim.data.balance.finance.cohesion.tax_neutral as f32)
            )).clicked() {
                actions.push(UiAction::Command(SimCommand::SetTaxRate { rate: state.tax_percent as f64 / 100.0 }));
            }
        });

        ui.add_space(4.0);
        ui.label(RichText::new("Rücklage").color(ACCENT));
        ui.horizontal(|ui| {
            ui.add(egui::DragValue::new(&mut state.reserve_target).speed(500.0).range(0.0..=5_000_000.0).suffix(" Credits"));
            if ui.button("Festsetzen").on_hover_text(format!(
                "Heute {}. Was die Kasse darüber hinaus enthält, gibt der Staat laufend an die Haushalte zurück.",
                decimal(to_credits(sim.economy.treasury_targets[player.index()]), 0)
            )).clicked() {
                actions.push(UiAction::Command(SimCommand::SetTreasuryTarget { amount: state.reserve_target as f64 }));
            }
        });

        ui.add_space(4.0);
        ui.label(RichText::new("Staatsanleihen").color(ACCENT));
        ui.horizontal(|ui| {
            ui.add(egui::DragValue::new(&mut state.bond_amount).speed(500.0).range(1_000.0..=2_000_000.0).suffix(" Credits"));
            if ui.button("Ausgeben").on_hover_text(format!(
                "Laufzeit {} Jahre, Zins {} pro Jahr. Gezeichnet wird von eigenen Eliten und Fonds, dann von fremden Eliten. Wer die Zinsen nicht zahlen kann, fällt aus.",
                sim.data.balance.finance.bond_years,
                percent(finance.bond_yield as f32)
            )).clicked() {
                actions.push(UiAction::Command(SimCommand::IssueBonds { amount: state.bond_amount as f64 }));
            }
        });
        let mine: Vec<_> = sim.finance.bonds.iter().filter(|bond| bond.issuer == player).collect();
        if !mine.is_empty() {
            egui::ScrollArea::vertical().id_salt("anleihen").max_height(110.0).show(ui, |ui| {
                egui::Grid::new("anleihen").striped(true).num_columns(4).show(ui, |ui| {
                    for title in ["Gläubiger", "Betrag", "Zins", "fällig"] {
                        ui.label(RichText::new(title).color(MUTED).small());
                    }
                    ui.end_row();
                    for bond in mine {
                        ui.label(holder_name(sim.galaxy, sim.polities, bond.holder));
                        ui.label(decimal(to_credits(bond.principal), 0));
                        ui.label(percent(bond.rate as f32));
                        ui.label(sim::clock::Date::from_day(bond.matures_on).year.to_string());
                        ui.end_row();
                    }
                });
            });
        }

        ui.separator();
        ui.label(RichText::new("Verflechtung mit anderen Gemeinschaften").color(ACCENT)).on_hover_text(
            "Anteil am Marktwert aller Betriebe einer Gemeinschaft, den Halter der anderen besitzen. Beteiligung schafft Einfluss und Abhängigkeit.",
        );
        let stakes = cross_holdings(sim);
        egui::Grid::new("verflechtung").striped(true).num_columns(3).show(ui, |ui| {
            for title in ["", "wir halten dort", "sie halten bei uns"] {
                ui.label(RichText::new(title).color(MUTED).small());
            }
            ui.end_row();
            for other in sim.polities.alive().filter(|polity| polity.id != player) {
                ui.label(RichText::new(&other.name).color(color32(other.color)));
                ui.label(percent(stakes.share(player, other.id) as f32));
                let theirs = stakes.share(other.id, player);
                ui.label(RichText::new(percent(theirs as f32)).color(if theirs > 0.25 { BAD } else { Color32::from_rgb(210, 215, 225) }));
                ui.end_row();
            }
        });
    });
    state.treasury_open = open;
}

/// Gegenseitige Beteiligungen zwischen Gemeinschaften.
pub struct CrossHoldings {
    polities: usize,
    /// `held[halter * n + gebiet]`: Wert der Anteile.
    held: Vec<f64>,
    /// Marktwert aller Betriebe je Gebiet.
    total: Vec<f64>,
}

impl CrossHoldings {
    /// Anteil am Marktwert der Betriebe von `territory`, den Halter von `holder` besitzen.
    pub fn share(&self, holder: PolityId, territory: PolityId) -> f64 {
        let total = self.total[territory.index()];
        if total <= 0.0 {
            return 0.0;
        }
        self.held[holder.index() * self.polities + territory.index()] / total
    }
}

pub fn cross_holdings(sim: &SimRefs) -> CrossHoldings {
    let polities = sim.polities.list.len();
    let mut result = CrossHoldings {
        polities,
        held: vec![0.0; polities * polities],
        total: vec![0.0; polities],
    };
    for (id, issuer) in &sim.finance.issuers {
        let Some(territory) = polity_of_issuer(sim.galaxy, sim.logistics, *id) else {
            continue;
        };
        for (holder, shares) in &issuer.holdings {
            let value = *shares as f64 * issuer.price;
            result.total[territory.index()] += value;
            if let Some(polity) = polity_of_holder(sim.galaxy, *holder) {
                result.held[polity.index() * polities + territory.index()] += value;
            }
        }
    }
    result
}
