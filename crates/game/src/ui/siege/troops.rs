//! Eigene Flotten im Fenster „Belagerung“: eine Belagerung beginnen, reguläre Verbände
//! einschiffen und an Land setzen. Dieselben Zeilen stehen gekürzt im Fenster „Flotten“.

use super::{
    BEGIN, DISEMBARK, EMBARK, NO_COMMANDS, SHOW_SIEGE, SiegeTab, SiegeUi, WARN, days_text, gated,
    polity_name, units,
};
use crate::ui::fleets::{FleetState, WAITING, is_waiting, location, waiting};
use crate::ui::panels::{ACCENT, MUTED, SimRefs, UiAction, detail as small};
use bevy_egui::egui::{self, RichText};
use sim::command::SimCommand;
use sim::fleet::Fleet;
use sim::ids::{FleetId, PlanetId, PolityId};

pub const TROOPS_AND_SIEGE: &str = "Truppen und Belagerung …";
pub const WAITS_FOR_ORBIT: &str = "wartet auf die Orbitalhoheit";
pub const CANCEL_TASK: &str = "Auftrag aufheben";

/// Schritt der Regler für Verbände.
pub const STEP: f32 = 0.5;

/// Oberes Ende eines Reglers für `ready` Verbände; nie unter einem Schritt, damit der
/// Regler auch dann einen Bereich hat, wenn nichts bereitsteht.
pub fn slider_max(ready: f64) -> f32 {
    (ready as f32).max(STEP)
}

/// Rastet die Stellung eines Reglers auf halbe Verbände, am oberen Ende auf alles, was
/// bereitsteht. So fragt das Ziehen den Kern nur für wenige verschiedene Stärken.
pub fn snap(value: f32, most: f32) -> f32 {
    if value >= most - STEP * 0.5 {
        most
    } else {
        ((value / STEP).round() * STEP).clamp(STEP, most)
    }
}

/// Die Stärke für den Befehl: die Stellung des Reglers, höchstens was bereitsteht (der
/// Regler rechnet in einfacher Genauigkeit und läge sonst um ein Haar darüber).
pub fn commanded(strength: f32, ready: f64) -> f64 {
    let wanted = strength as f64;
    if ready > 0.0 {
        wanted.min(ready)
    } else {
        wanted
    }
}

/// Truppen an Bord einer Flotte: Verbände und Nachschub in Verbandstagen.
pub fn aboard(sim: &SimRefs, fleet: FleetId) -> (f64, f64) {
    sim.sieges
        .embarked
        .get(&fleet)
        .map_or((0.0, 0.0), |force| (force.troops, force.supply))
}

/// „3,0 Verbände an Bord, Nachschub für 30 Tage“ oder „keine Truppen an Bord“.
pub fn aboard_text(sim: &SimRefs, fleet: FleetId) -> String {
    let (troops, supply) = aboard(sim, fleet);
    if troops <= 0.0 {
        "keine Truppen an Bord".to_string()
    } else {
        format!(
            "{} Verbände an Bord, Nachschub für {}",
            units(troops),
            days_text(supply / troops)
        )
    }
}

/// Reguläre einer eigenen Welt, die an Bord gehen können: Wer im Untergrund einer
/// belagerten Welt sitzt, kommt nicht hinaus. Nur für die Anzeige und das Ende des
/// Reglers; ob eine Einschiffung geht, entscheidet der Probelauf des Kerns.
pub fn ready_regulars(sim: &SimRefs, planet: PlanetId) -> f64 {
    let deep = sim.sieges.siege(planet).map_or(0.0, |siege| siege.deep);
    (sim.sieges.defense(planet).regulars - deep).max(0.0)
}

/// Wartet für diese Flotte schon ein Befehl, der Truppen an oder von Bord bringt? Dann
/// gibt es keinen zweiten: Beide gälten im selben Takt, gegen denselben Stand an Bord.
pub fn move_waiting(sim: &SimRefs, fleet: FleetId) -> bool {
    waiting(sim).any(|command| match command {
        SimCommand::LandTroops { fleet: named, .. }
        | SimCommand::EmbarkTroops { fleet: named, .. } => *named == fleet,
        _ => false,
    })
}

/// Eine Einschiffung, für die die Tafel nach dem Nachschub fragt.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OfferKey {
    pub fleet: FleetId,
    pub planet: PlanetId,
    pub strength: f64,
    pub supply_days: u16,
}

impl OfferKey {
    pub fn command(self) -> SimCommand {
        SimCommand::EmbarkTroops {
            fleet: self.fleet,
            planet: self.planet,
            strength: self.strength,
            supply_days: self.supply_days,
        }
    }
}

/// Nachschub in Verbandstagen, den diese Einschiffung heute bekäme: Der Kern führt sie
/// auf Kopien von Wirtschaft, Belagerungen und Flotten aus, die danach verworfen werden.
/// `None`, wenn er sie ablehnte. Nichts für jeden Frame ([`super::Cache::offer`]).
pub fn supply_offer(sim: &SimRefs, key: OfferKey) -> Option<f64> {
    let player = sim.config.player?;
    let mut economy = sim.economy.clone();
    let mut sieges = sim.sieges.clone();
    let mut fleets = sim.fleets.clone();
    let orbit = sim::siege::Orbit::new(&mut fleets, sim.research);
    let before = sieges
        .embarked
        .get(&key.fleet)
        .map_or(0.0, |force| force.supply);
    sim::siege::orders::embark_troops(
        sim.data,
        sim.galaxy,
        sim.diplomacy,
        &mut economy,
        &mut sieges,
        &orbit,
        player,
        key.fleet,
        key.planet,
        key.strength,
        key.supply_days,
    )
    .ok()?;
    let after = sieges
        .embarked
        .get(&key.fleet)
        .map_or(0.0, |force| force.supply);
    Some((after - before).max(0.0))
}

/// Woran es liegt, wenn der Markt weniger Nachschub hergibt als verlangt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SupplyLimit {
    /// Am Markt des Systems liegen nicht genug Tagesrationen.
    Market,
    /// Der Etat für Verteidigung und Beschaffung (oder die Kasse) reicht nicht.
    Budget,
}

/// Wie viele Verbandstage der Etat bezahlte und wie viele der Markt vorrätig hat, mit
/// den öffentlichen Zahlen des Kerns nachgerechnet. Nur für die Begründung in der
/// Anzeige: Wie viel wirklich an Bord käme, sagt allein der Probelauf
/// ([`supply_offer`]); ein Test hält beide beieinander.
pub fn supply_limits(sim: &SimRefs, player: PolityId, system: sim::ids::SystemId) -> (f64, f64) {
    let bundle = &sim.data.balance.siege.day_bundle_ids;
    let spread = sim.data.balance.economy.spread;
    let Some(market) = sim.economy.markets.get(system.index()) else {
        return (0.0, 0.0);
    };
    let cost: f64 = bundle
        .iter()
        .map(|(good, amount)| amount * market.ask(*good, spread))
        .sum();
    let allowance = sim::siege::orders::armament_allowance(sim.economy, sim.sieges, player);
    let affordable = if cost > 0.0 {
        sim::economy::to_credits(allowance) / cost
    } else {
        f64::INFINITY
    };
    let available = bundle
        .iter()
        .map(|(good, amount)| market.available(*good) / amount)
        .fold(f64::INFINITY, f64::min);
    (affordable.max(0.0), available.max(0.0))
}

/// Was knapper ist, Etat oder Markt.
pub fn supply_limit(sim: &SimRefs, player: PolityId, system: sim::ids::SystemId) -> SupplyLimit {
    let (affordable, available) = supply_limits(sim, player, system);
    if affordable < available {
        SupplyLimit::Budget
    } else {
        SupplyLimit::Market
    }
}

/// Was die Tafel vor dem Einschiffen über den Nachschub sagt: `Ok`, wenn der verlangte
/// zu haben ist, sonst der Satz, wie viel es wird und woran es liegt.
pub fn offer_text(
    strength: f64,
    wanted_days: u16,
    offer: f64,
    limit: SupplyLimit,
    system: &str,
) -> Result<String, String> {
    let days = if strength > 0.0 {
        offer / strength
    } else {
        0.0
    };
    if days + 0.5 >= wanted_days as f64 {
        return Ok(format!(
            "Nachschub für {} ist heute zu haben.",
            days_text(wanted_days as f64)
        ));
    }
    let why = match limit {
        SupplyLimit::Market => format!(
            "Der Markt von {system} gibt je Kauf nur einen Teil seines Bestands her; mehr \
             Tagesrationen sind heute nicht zu haben."
        ),
        SupplyLimit::Budget => {
            "Der Etat für Verteidigung und Beschaffung reicht nicht weiter.".to_string()
        }
    };
    Err(format!(
        "Heute gäbe es nur Nachschub für {} statt für {}. {why}",
        days_text(days.floor()),
        days_text(wanted_days as f64)
    ))
}

/// Eine Einschiffung, die der Spieler befohlen hat: Die Tafel sieht nach, was an Bord kam.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EmbarkAsked {
    pub key: OfferKey,
    /// Stand an Bord beim Befehl: Verbände und Nachschub in Verbandstagen.
    pub before: (f64, f64),
    pub tick: u64,
}

/// Hinweis an einer Flotte, deren Truppen mit weniger Nachschub an Bord kamen als verlangt.
#[derive(Debug, Clone, PartialEq)]
pub struct EmbarkNote {
    pub fleet: FleetId,
    /// Verbände an Bord, als der Hinweis entstand; ändert sich das, ist er überholt.
    pub troops: f64,
    pub text: String,
}

/// Der Hinweis nach einer Einschiffung, bei der Nachschub fehlt; `None`, wenn (fast)
/// alles Verlangte an Bord kam.
pub fn short_supply_text(strength: f64, got_days: f64, wanted_days: u16) -> Option<String> {
    if wanted_days == 0 || got_days + 0.5 >= wanted_days as f64 {
        return None;
    }
    Some(format!(
        "Eingeschifft: {} Verbände, aber nur mit Nachschub für {} statt für {}. Markt und \
         Etat gaben nicht mehr her.",
        units(strength),
        days_text(got_days.max(0.0).floor()),
        days_text(wanted_days as f64)
    ))
}

impl SiegeUi {
    /// Einmal je Frame: Ist eine befohlene Einschiffung ausgeführt, vergleicht die Tafel
    /// den Nachschub an Bord mit dem verlangten. Kam weniger, gibt sie den Hinweis zurück
    /// (für die Einblendung) und hält ihn an der Flotte fest.
    pub fn follow_embark(&mut self, sim: &SimRefs) -> Option<String> {
        if let Some(note) = &self.embark_note
            && (aboard(sim, note.fleet).0 - note.troops).abs() > 1e-6
        {
            self.embark_note = None;
        }
        let asked = self.embark_asked?;
        let (troops, supply) = aboard(sim, asked.key.fleet);
        if troops >= asked.before.0 + asked.key.strength - 1e-6 {
            self.embark_asked = None;
            let got_days = (supply - asked.before.1) / asked.key.strength;
            let text = short_supply_text(asked.key.strength, got_days, asked.key.supply_days)?;
            self.embark_note = Some(EmbarkNote {
                fleet: asked.key.fleet,
                troops,
                text: text.clone(),
            });
            return Some(text);
        }
        // Abgelehnt oder überholt: Der Befehl wartet nicht mehr, und nichts kam an Bord.
        if !is_waiting(sim, &asked.key.command()) && sim.clock.tick > asked.tick + 1 {
            self.embark_asked = None;
        }
        None
    }
}

/// Warum die Belagerung einer Flotte, die den Auftrag hat, noch nicht begonnen hat. Die
/// Orbitalhoheit selbst stellt der Kern fest (`sim::siege::orbit::control`); der Grund
/// wird aus dem Zustand der Flotte und den Flotten im System benannt.
pub fn waiting_reason(sim: &SimRefs, fleet: &Fleet, planet: PlanetId) -> String {
    let system = sim.galaxy.planet(planet).system;
    if sim::siege::orbit::control(
        sim.fleets,
        sim.diplomacy,
        &sim.sieges.assign,
        planet,
        system,
        fleet.owner,
    ) {
        return "Die Flotte hält den Orbit: Die Belagerung beginnt mit dem nächsten Tag.".into();
    }
    if !fleet.is_docked() || fleet.system != system {
        return "Die Flotte liegt nicht über der Welt.".into();
    }
    if fleet.in_battle {
        return "Die Flotte steht im Gefecht; solange ist der Orbit umstritten.".into();
    }
    if fleet.is_regrouping() {
        return "Die Flotte sammelt sich nach einem Abzug und hält solange keinen Orbit.".into();
    }
    if !fleet.has_fuel() {
        return "Die Flotte hat keinen Treibstoff mehr: Ohne ihn hält sie keine Position im \
                Orbit. Sie muss erst versorgt werden."
            .into();
    }
    let rival = sim
        .fleets
        .docked_in(system)
        .find(|other| sim.diplomacy.at_war(other.owner, fleet.owner) && !other.is_regrouping());
    match rival {
        Some(rival) => format!(
            "Der Orbit ist umstritten: {} ({}) liegt im System.",
            rival.name,
            sim.polities.get(rival.owner).name
        ),
        None => "Der Orbit ist noch nicht entschieden.".into(),
    }
}

/// Was die Flotte gerade belagert oder belagern soll.
pub fn target_text(sim: &SimRefs, fleet: FleetId) -> Option<String> {
    let planet = *sim.sieges.assign.get(&fleet)?;
    let name = &sim.galaxy.planet(planet).name;
    Some(if sim.sieges.siege(planet).is_some() {
        format!("belagert {name}")
    } else {
        // Kurz: Die Liste soll das Fenster nicht verbreitern. Warum es noch nicht so weit
        // ist, steht bei der gewählten Flotte.
        format!("soll {name} belagern")
    })
}

/// Je fremder Welt im System der Flotte der Knopf „Belagern“, gesperrt mit dem Grund des
/// Kerns; an der Welt, die sie schon belagert, der Weg zu deren Einzelheiten.
fn begin_buttons(
    ui: &mut egui::Ui,
    sim: &SimRefs,
    player: PolityId,
    fleet: &Fleet,
    state: &mut SiegeUi,
    actions: &mut Vec<UiAction>,
) -> bool {
    let assigned = sim.sieges.assign.get(&fleet.id).copied();
    let mut any = false;
    for planet_id in &sim.galaxy.system(fleet.system).planets {
        let planet = sim.galaxy.planet(*planet_id);
        let Some(owner) = planet.owner.filter(|owner| *owner != player) else {
            continue;
        };
        any = true;
        if assigned == Some(*planet_id) {
            ui.label(RichText::new(&planet.name).color(ACCENT));
            if sim.sieges.siege(*planet_id).is_some() {
                if ui
                    .button(SHOW_SIEGE)
                    .on_hover_text("Phase, Lage, Prognose und Befehle dieser Belagerung.")
                    .clicked()
                {
                    state.show(*planet_id);
                }
            } else {
                // Der Auftrag steht, die Belagerung nicht: warum, und der Weg zurück.
                ui.label(small(WAITS_FOR_ORBIT).color(WARN));
                ui.label(small(waiting_reason(sim, fleet, *planet_id)).color(MUTED));
                let command = SimCommand::LiftSiege { planet: *planet_id };
                let check = state.cache.check(sim, &command);
                if gated(
                    ui,
                    CANCEL_TASK,
                    format!("{CANCEL_TASK}: {}", planet.name),
                    &check,
                    "Nimmt den Auftrag zurück: Die Flotte soll diese Welt nicht mehr belagern.",
                ) {
                    actions.push(UiAction::Command(command));
                }
                if let Err(reason) = &check {
                    ui.label(small(reason).color(MUTED));
                }
            }
            continue;
        }
        let command = SimCommand::BeginSiege {
            fleet: fleet.id,
            planet: *planet_id,
        };
        let check = state.cache.check(sim, &command);
        let label = format!("{BEGIN}: {}", planet.name);
        if gated(
            ui,
            &label,
            label.clone(),
            &check,
            "Die Flotte belagert diese Welt. Die Belagerung beginnt am nächsten Tag, sobald \
             die Flotte den Orbit hält; ein früheres Ziel gibt sie dafür auf.",
        ) {
            actions.push(UiAction::Command(command));
        }
        ui.label(polity_name(sim, owner));
        if let Err(reason) = &check {
            ui.label(small(reason).color(MUTED));
        }
    }
    any
}

/// Einschiffen von einer eigenen Welt und an Land setzen auf ihr, mit den Reglern für
/// Stärke und Nachschub.
fn own_world(
    ui: &mut egui::Ui,
    sim: &SimRefs,
    fleet: &Fleet,
    planet: PlanetId,
    state: &mut SiegeUi,
    actions: &mut Vec<UiAction>,
) {
    let name = &sim.galaxy.planet(planet).name;
    let ready = ready_regulars(sim, planet);
    let (troops, _) = aboard(sim, fleet.id);
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new(name).color(ACCENT));
        ui.label(format!("{} reguläre Verbände bereit", units(ready)));
    });
    ui.horizontal_wrapped(|ui| {
        // Der Regler gilt für alle Welten und Flotten des Fensters: Hier zeigt er, was
        // davon an dieser Welt geht, und ändert die Wahl nur, wenn der Spieler ihn zieht.
        let most = slider_max(ready.max(troops));
        let mut strength = snap(state.strength, most);
        let slider = egui::Slider::new(&mut strength, STEP..=most)
            .custom_formatter(|value, _| crate::ui::format::decimal(value, 1))
            .custom_parser(|text| text.replace(',', ".").parse().ok())
            .suffix(" Verbände");
        if ui.add(slider).changed() {
            state.strength = snap(strength, most);
        }
        let cap = sim.data.balance.siege.supply_days_cap as u32;
        state.supply_days = state.supply_days.min(cap);
        ui.add(
            egui::Slider::new(&mut state.supply_days, 0..=cap)
                .step_by(5.0)
                .suffix(" Tage Nachschub"),
        )
        .on_hover_text(
            "Nachschub für die eingeschifften Verbände, gekauft am Markt des Systems, soweit \
             Ware und Staatskasse reichen.",
        );
    });
    let strength = snap(state.strength, slider_max(ready.max(troops)));
    // Solange für die Flotte schon eine Einschiffung oder Landung vorgemerkt ist, gibt es
    // keine zweite.
    let busy = move_waiting(sim, fleet.id);
    ui.horizontal_wrapped(|ui| {
        let key = OfferKey {
            fleet: fleet.id,
            planet,
            strength: commanded(strength, ready),
            supply_days: state.supply_days as u16,
        };
        let embark = key.command();
        let check = if busy {
            Err(WAITING.to_string())
        } else {
            state.cache.check(sim, &embark)
        };
        if gated(
            ui,
            EMBARK,
            format!("{EMBARK}: {name}"),
            &check,
            "Reguläre Verbände der Welt gehen mit ihrem Nachschub an Bord der Flotte.",
        ) {
            state.embark_asked = Some(EmbarkAsked {
                key,
                before: aboard(sim, fleet.id),
                tick: sim.clock.tick,
            });
            actions.push(UiAction::Command(embark));
        }
        match &check {
            Err(reason) => {
                ui.label(small(reason).color(MUTED));
            }
            // Der Regler nennt einen Wunsch; was Markt und Etat davon heute hergeben,
            // sagt der Probelauf des Kerns, bevor der Spieler klickt.
            Ok(()) if key.supply_days > 0 => {
                if let (Some(offer), Some(player)) =
                    (state.cache.offer(sim, key), sim.config.player)
                {
                    let system = sim.galaxy.system(fleet.system);
                    let limit = supply_limit(sim, player, system.id);
                    match offer_text(key.strength, key.supply_days, offer, limit, &system.name) {
                        Ok(text) => ui.label(small(text).color(MUTED)),
                        Err(text) => ui.label(small(text).color(WARN)),
                    };
                }
            }
            Ok(()) => {}
        }
    });
    if let Some(note) = &state.embark_note
        && note.fleet == fleet.id
    {
        ui.label(small(&note.text).color(WARN));
    }
    ui.horizontal_wrapped(|ui| {
        let land = SimCommand::LandTroops {
            fleet: fleet.id,
            planet,
            strength: commanded(strength, troops),
        };
        let check = if busy {
            Err(WAITING.to_string())
        } else {
            state.cache.check(sim, &land)
        };
        if gated(
            ui,
            DISEMBARK,
            format!("{DISEMBARK}: {name}"),
            &check,
            "Die Verbände verstärken die Garnison der eigenen Welt; ihr Nachschub füllt das \
             Depot.",
        ) {
            actions.push(UiAction::Command(land));
        }
        if let Err(reason) = &check {
            ui.label(small(reason).color(MUTED));
        }
    });
}

/// Die Flotte, mit der gearbeitet wird: die gewählte, solange es sie gibt, sonst die erste
/// eigene.
fn shown_fleet<'a>(
    sim: &'a SimRefs,
    player: PolityId,
    chosen: &mut Option<FleetId>,
) -> Option<&'a Fleet> {
    let found = chosen
        .and_then(|id| sim.fleets.get(id))
        .filter(|fleet| fleet.alive && fleet.owner == player)
        .or_else(|| sim.fleets.of(player).next());
    *chosen = found.map(|fleet| fleet.id);
    found
}

pub fn troops_tab(
    ui: &mut egui::Ui,
    sim: &SimRefs,
    state: &mut SiegeUi,
    actions: &mut Vec<UiAction>,
) {
    let Some(player) = sim.config.player else {
        ui.label(NO_COMMANDS);
        return;
    };
    let shown = shown_fleet(sim, player, &mut state.fleet).map(|fleet| fleet.id);
    egui::Grid::new("belagerung_flotten")
        .striped(true)
        .num_columns(4)
        .show(ui, |ui| {
            for title in ["Flotte", "Ort und Zustand", "Truppen", "Belagerung"] {
                ui.label(RichText::new(title).color(MUTED).small());
            }
            ui.end_row();
            let mut any = false;
            for fleet in sim.fleets.of(player) {
                any = true;
                if ui
                    .selectable_label(shown == Some(fleet.id), &fleet.name)
                    .clicked()
                {
                    state.fleet = Some(fleet.id);
                }
                ui.label(format!(
                    "{} · {}",
                    location(sim, fleet),
                    FleetState::of(fleet, sim.battles).label()
                ));
                ui.label(aboard_text(sim, fleet.id));
                ui.label(target_text(sim, fleet.id).unwrap_or_else(|| "–".into()));
                ui.end_row();
            }
            if !any {
                ui.label(RichText::new("keine Flotte mehr").color(MUTED));
                ui.end_row();
            }
        });
    let Some(fleet) = state.fleet.and_then(|id| sim.fleets.get(id)) else {
        return;
    };
    ui.separator();
    ui.label(RichText::new(&fleet.name).color(ACCENT).strong());
    if !fleet.route.is_empty() {
        ui.label(
            small(
                "Die Flotte ist auf dem Marsch. Belagern, einschiffen und landen kann sie, \
                   wo sie liegt.",
            )
            .color(MUTED),
        );
        return;
    }
    let system = sim.galaxy.system(fleet.system);
    ui.label(RichText::new(format!("Welten in {}", system.name)).color(MUTED));
    ui.horizontal_wrapped(|ui| {
        if !begin_buttons(ui, sim, player, fleet, state, actions) {
            ui.label(small("Keine fremde Welt in diesem System.").color(MUTED));
        }
    });
    for planet in &system.planets {
        if sim.galaxy.planet(*planet).owner == Some(player) {
            ui.add_space(4.0);
            ui.push_id(("eigene_welt", planet.0), |ui| {
                own_world(ui, sim, fleet, *planet, state, actions);
            });
        }
    }
    ui.add_space(4.0);
    ui.label(
        small(
            "Eingeschifft werden reguläre Verbände einer eigenen Welt auf eine Flotte in \
             ihrem System. Über einer belagerten Welt landen sie im Reiter „Belagerungen“ \
             bei der Belagerung selbst.",
        )
        .color(MUTED),
    );
}

/// Die Zeile im Fenster „Flotten“.
pub fn fleet_entry(
    ui: &mut egui::Ui,
    sim: &SimRefs,
    fleet: &Fleet,
    state: &mut SiegeUi,
    actions: &mut Vec<UiAction>,
) {
    let Some(player) = sim.config.player else {
        return;
    };
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new("Belagerung").color(MUTED));
        if let Some(text) = target_text(sim, fleet.id) {
            ui.label(text);
        }
        ui.label(small(aboard_text(sim, fleet.id)).color(MUTED));
        if fleet.route.is_empty() {
            begin_buttons(ui, sim, player, fleet, state, actions);
        }
        if ui
            .button(TROOPS_AND_SIEGE)
            .on_hover_text(
                "Öffnet das Fenster „Belagerung“ bei dieser Flotte: belagern, Truppen \
                 einschiffen und an Land setzen.",
            )
            .clicked()
        {
            state.open = true;
            state.tab = SiegeTab::Troops;
            state.fleet = Some(fleet.id);
        }
    });
}
