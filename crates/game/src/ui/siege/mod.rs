//! Fenster „Belagerung“: die Belagerungen, die der Spieler sehen darf, mit Phase, Lage,
//! Prognose und Befehlen (`detail`), Flotten und Truppen (`troops`) und der stehenden
//! Verteidigung der eigenen Welten (`defense`).
//!
//! Gelesen wird das Lesemodell des Kerns (`sim::siege::view`); gerechnet wird hier nichts.
//! Ob der Kern einen Befehl annähme, sagt er selbst (`command_block`): Der Befehl läuft
//! dazu auf einer Kopie, deshalb merkt sich die Oberfläche die Antwort, bis sich der Tag,
//! die Auswahl oder die Lage ändert ([`Cache`]).

pub(crate) mod defense;
pub(crate) mod detail;
pub(crate) mod troops;

use super::fleets::{WAITING, is_waiting, sentence};
use super::format::{decimal, percent};
use super::panels::{MUTED, SimRefs, UiAction, color32, detail as small, window_room};
use bevy_egui::egui::{self, Color32, RichText};
use sim::chronicle::{ChronicleEntry, ChronicleKind};
use sim::clock::TICKS_PER_DAY;
use sim::command::SimCommand;
use sim::fleet::Fleets;
use sim::galaxy::Galaxy;
use sim::ids::{FleetId, PlanetId, PolityId, SystemId};
use sim::siege::{
    Course, DefensePlan, Forecast, Limit, Phase, SiegeCard, SiegeRefs, command_block, siege_cards,
};
use std::hash::{Hash, Hasher};

/// Beschriftungen des Fensters. Das README nennt sie; ein Test hält beide beieinander.
pub const TITLE: &str = "Belagerung";
pub const TAB_SIEGES: &str = "Belagerungen";
pub const TAB_TROOPS: &str = "Flotten und Truppen";
pub const RESTING: &str = "ruht, Orbit umstritten";
pub const NONE_IN_SIGHT: &str = "Keine laufende Belagerung in Sicht.";
pub const BEGIN: &str = "Belagern";
pub const LIFT: &str = "Belagerung aufheben";
pub const CAPITULATE: &str = "Welt übergeben";
pub const EMBARK: &str = "Einschiffen";
pub const LAND: &str = "Landen";
pub const DISEMBARK: &str = "An Land setzen";
pub const FORECAST: &str = "Prognose";
pub const SHOW_SIEGE: &str = "Belagerung zeigen";
pub const NO_COMMANDS: &str = "Beobachter geben keine Befehle.";

pub(super) const BAD: Color32 = Color32::from_rgb(235, 110, 95);
pub(super) const WARN: Color32 = Color32::from_rgb(240, 190, 80);
pub(super) const GOOD: Color32 = Color32::from_rgb(120, 200, 140);

/// Reiter des Fensters.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SiegeTab {
    /// Liste der Belagerungen und die gewählte im Einzelnen.
    #[default]
    Sieges,
    /// Eigene Flotten: Belagerung beginnen, Truppen einschiffen und an Land setzen.
    Troops,
}

/// Was der Spieler erst bestätigen muss.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Confirm {
    Lift(PlanetId),
    Capitulate(PlanetId),
}

/// Zustand des Fensters und der Tafeln der Verteidigung (kein Spielzustand).
#[derive(Debug, Clone)]
pub struct SiegeUi {
    pub open: bool,
    pub tab: SiegeTab,
    /// Belagerte Welt, deren Einzelheiten das Fenster zeigt.
    pub chosen: Option<PlanetId>,
    /// Flotte, mit der eingeschifft, gelandet und belagert wird.
    pub fleet: Option<FleetId>,
    /// Stärke in Verbänden und Nachschub in Tagen für Einschiffen und Landen.
    pub strength: f32,
    pub supply_days: u32,
    pub confirm: Option<Confirm>,
    /// Frame, in dem die Rückfrage zuletzt auf dem Schirm stand. Eine Rückfrage, die der
    /// Spieler nicht mehr sieht (Fenster zu, andere Ansicht), verfällt.
    pub confirm_frame: u64,
    /// Einschiffung, deren Nachschub noch nachzusehen ist, und was dabei zu kurz kam.
    pub embark_asked: Option<troops::EmbarkAsked>,
    pub embark_note: Option<troops::EmbarkNote>,
    /// Das Fenster steht gerade über der Orbitansicht: Wege zur Karte entfallen.
    pub in_orbit: bool,
    /// Welt, deren Verteidigungsplan das Fenster „Staatsfinanzen“ zum Ändern zeigt.
    pub plan_planet: Option<PlanetId>,
    /// Der Plan, an dem der Spieler gerade dreht, solange er ihn nicht festgesetzt hat.
    pub plan_edit: Option<(PlanetId, DefensePlan)>,
    pub cache: Cache,
}

impl Default for SiegeUi {
    fn default() -> Self {
        Self {
            open: false,
            tab: SiegeTab::default(),
            chosen: None,
            fleet: None,
            strength: 1.0,
            supply_days: 30,
            confirm: None,
            confirm_frame: 0,
            embark_asked: None,
            embark_note: None,
            in_orbit: false,
            plan_planet: None,
            plan_edit: None,
            cache: Cache::default(),
        }
    }
}

impl SiegeUi {
    /// Vergisst, was nur für die Partie galt, die endet; ob das Fenster offen ist, bleibt.
    pub fn forget_game(&mut self) {
        *self = Self {
            open: self.open,
            ..Self::default()
        };
    }

    /// Öffnet das Fenster bei dieser Belagerung.
    pub fn show(&mut self, planet: PlanetId) {
        self.open = true;
        self.tab = SiegeTab::Sieges;
        self.chosen = Some(planet);
    }
}

/// Die Sicht des Lesemodells auf die laufende Partie.
pub fn siege_refs<'a>(sim: &SimRefs<'a>) -> SiegeRefs<'a> {
    SiegeRefs {
        data: sim.data,
        galaxy: sim.galaxy,
        polities: sim.polities,
        diplomacy: sim.diplomacy,
        economy: sim.economy,
        finance: sim.finance,
        logistics: sim.logistics,
        fleets: sim.fleets,
        research: sim.research,
        sieges: sim.sieges,
    }
}

/// So viele Antworten des Kerns hält der Zwischenspeicher höchstens: genug für alle Knöpfe
/// eines Fensters und einige Stellungen der Regler.
const KEPT_CHECKS: usize = 96;

/// Karten der Belagerungen und Antworten des Kerns auf Probeläufe, gültig für einen Tag
/// und eine Lage. Die Prognosen der Karten und jeder Probelauf rechnen auf Kopien: nichts
/// für jeden Frame.
#[derive(Debug, Clone, Default)]
pub struct Cache {
    stamp: Option<u64>,
    cards: Vec<SiegeCard>,
    checks: Vec<(SimCommand, Option<String>)>,
    /// Was der Kern beim Einschiffen an Nachschub kaufte, je Flotte, Welt und Stellung
    /// der Regler: Verbandstage, oder `None`, wenn er die Einschiffung ablehnte.
    offers: Vec<(troops::OfferKey, Option<f64>)>,
    /// Wie oft der Kern gefragt wurde (Karten und Probeläufe); für Tests.
    pub asked: u32,
}

/// Woran der Zwischenspeicher erkennt, dass er neu fragen muss: der Tag, die wartenden
/// Befehle und alles, was ein Befehl des Spielers zwischen zwei Tagen an Belagerungen,
/// Truppen, Plänen und der Lage der eigenen Flotten ändert.
fn stamp(sim: &SimRefs) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    sim.clock.day().hash(&mut hasher);
    sim.pending.len().hash(&mut hasher);
    sim.config.player.hash(&mut hasher);
    sim.diplomacy.wars.len().hash(&mut hasher);
    for (planet, siege) in &sim.sieges.active {
        planet.hash(&mut hasher);
        (siege.attacker, siege.defender).hash(&mut hasher);
        let orders = siege.orders;
        (orders.fire as u8, orders.assault, orders.focus, orders.sack).hash(&mut hasher);
        siege.army.to_bits().hash(&mut hasher);
        siege.deep.to_bits().hash(&mut hasher);
        let defense = sim.sieges.defense(*planet);
        (defense.posture as u8, defense.hold_fire).hash(&mut hasher);
    }
    sim.sieges.pending.len().hash(&mut hasher);
    for (fleet, planet) in &sim.sieges.assign {
        (fleet, planet).hash(&mut hasher);
    }
    for (fleet, force) in &sim.sieges.embarked {
        fleet.hash(&mut hasher);
        force.troops.to_bits().hash(&mut hasher);
        force.supply.to_bits().hash(&mut hasher);
    }
    if let Some(player) = sim.config.player {
        for fleet in sim.fleets.of(player) {
            (fleet.id, fleet.system, fleet.route.is_empty()).hash(&mut hasher);
            (fleet.in_battle, fleet.regroup_until, fleet.ships.len()).hash(&mut hasher);
        }
        for planet in sim.galaxy.planets_of(player) {
            let defense = sim.sieges.defense(planet.id);
            let plan = &defense.plan;
            planet.id.hash(&mut hasher);
            (plan.stations, plan.guns, plan.shield, plan.bunkers).hash(&mut hasher);
            (plan.regulars.to_bits(), plan.depot_days).hash(&mut hasher);
            (defense.posture as u8, defense.hold_fire).hash(&mut hasher);
            defense.regulars.to_bits().hash(&mut hasher);
        }
    }
    hasher.finish()
}

impl Cache {
    /// Einmal je Frame: verwirft Karten und Antworten, wenn Tag oder Lage sich geändert
    /// haben, und liest die Karten dann neu.
    pub fn refresh(&mut self, sim: &SimRefs) {
        let now = stamp(sim);
        if self.stamp == Some(now) {
            return;
        }
        self.stamp = Some(now);
        self.checks.clear();
        self.offers.clear();
        self.cards = if sim.sieges.active.is_empty() {
            Vec::new()
        } else {
            self.asked += 1;
            siege_cards(&siege_refs(sim), sim.clock.day())
        };
    }

    /// Alle laufenden Belagerungen, wie der Kern sie zuletzt beschrieben hat.
    pub fn cards(&self) -> &[SiegeCard] {
        &self.cards
    }

    /// Nähme der Kern diesen Befehl des Spielers jetzt an? Sonst sein Grund als Satz. Ein
    /// Befehl, der schon auf den nächsten Takt wartet, wird nicht noch einmal angeboten.
    pub fn check(&mut self, sim: &SimRefs, command: &SimCommand) -> Result<(), String> {
        let Some(player) = sim.config.player else {
            return Err(NO_COMMANDS.into());
        };
        if is_waiting(sim, command) {
            return Err(WAITING.into());
        }
        let known = self
            .checks
            .iter()
            .find(|(asked, _)| asked == command)
            .map(|(_, answer)| answer.clone());
        let answer = match known {
            Some(answer) => answer,
            None => {
                self.asked += 1;
                let answer = command_block(&siege_refs(sim), player, command, sim.clock.tick);
                if self.checks.len() >= KEPT_CHECKS {
                    self.checks.clear();
                }
                self.checks.push((command.clone(), answer.clone()));
                answer
            }
        };
        match answer {
            Some(reason) => Err(sentence(reason)),
            None => Ok(()),
        }
    }

    /// Nachschub in Verbandstagen, den diese Einschiffung heute bekäme: Der Kern führt sie
    /// dazu auf einer Kopie aus ([`troops::supply_offer`]), einmal je Tag, Lage und
    /// Stellung der Regler.
    pub fn offer(&mut self, sim: &SimRefs, key: troops::OfferKey) -> Option<f64> {
        if let Some((_, known)) = self.offers.iter().find(|(asked, _)| *asked == key) {
            return *known;
        }
        self.asked += 1;
        let offer = troops::supply_offer(sim, key);
        if self.offers.len() >= KEPT_CHECKS {
            self.offers.clear();
        }
        self.offers.push((key, offer));
        offer
    }
}

/// Darf der Spieler diese Belagerung sehen? Seine eigene immer, als Belagerer wie als
/// Verteidiger; eine fremde nur in einem System, in dem er selbst eine Flotte oder eine
/// Welt hat, wie bei den Gefechten. Beobachter ohne eigene Gemeinschaft sehen jede.
pub fn may_see(
    galaxy: &Galaxy,
    fleets: &Fleets,
    player: Option<PolityId>,
    attacker: PolityId,
    defender: PolityId,
    system: SystemId,
) -> bool {
    let Some(player) = player else {
        return true;
    };
    concerns(Some(player), attacker, defender)
        || fleets.docked_in(system).any(|fleet| fleet.owner == player)
        || galaxy
            .planets_of(player)
            .any(|planet| planet.system == system)
}

/// Geht die Belagerung den Spieler selbst an, als Belagerer oder als Verteidiger?
pub fn concerns(player: Option<PolityId>, attacker: PolityId, defender: PolityId) -> bool {
    player.is_some_and(|player| player == attacker || player == defender)
}

/// Die Rolle des Spielers in einer Belagerung.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Attacker,
    Defender,
    Onlooker,
}

impl Role {
    pub fn of(player: Option<PolityId>, card: &SiegeCard) -> Self {
        match player {
            Some(player) if player == card.attacker => Role::Attacker,
            Some(player) if player == card.defender => Role::Defender,
            _ => Role::Onlooker,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Role::Attacker => "eigene Belagerung",
            Role::Defender => "eigene Welt unter Belagerung",
            Role::Onlooker => "fremde Belagerung",
        }
    }
}

/// Die Belagerungen, die der Spieler sehen darf: die eigenen zuerst.
pub fn visible<'a>(sim: &SimRefs, cards: &'a [SiegeCard]) -> Vec<&'a SiegeCard> {
    let player = sim.config.player;
    let mut seen: Vec<&SiegeCard> = cards
        .iter()
        .filter(|card| {
            may_see(
                sim.galaxy,
                sim.fleets,
                player,
                card.attacker,
                card.defender,
                card.system,
            )
        })
        .collect();
    seen.sort_by_key(|card| !concerns(player, card.attacker, card.defender));
    seen
}

/// Zahl der Belagerungen, die den Spieler selbst angehen; für Beobachter alle.
pub fn concerning(sim: &SimRefs) -> usize {
    let player = sim.config.player;
    sim.sieges
        .active
        .values()
        .filter(|siege| player.is_none() || concerns(player, siege.attacker, siege.defender))
        .count()
}

/// Beschriftung des Knopfs in der Kopfleiste: mit der Zahl, sobald es etwas gibt.
pub fn toggle_label(count: usize) -> String {
    if count == 0 {
        TITLE.to_string()
    } else {
        format!("{TITLE} ({count})")
    }
}

/// „1 Tag“, „12 Tagen“: Tage im Dativ, für „in … Tagen“ und „nach … Tagen“.
fn in_days(days: u32) -> String {
    if days == 1 {
        "1 Tag".into()
    } else {
        format!("{days} Tagen")
    }
}

/// „1 Tag“, „12 Tage“.
pub fn days_text(days: f64) -> String {
    let whole = days.round();
    if (days - whole).abs() < 0.05 {
        if whole == 1.0 {
            "1 Tag".into()
        } else {
            format!("{} Tage", decimal(whole, 0))
        }
    } else {
        format!("{} Tage", decimal(days, 1))
    }
}

/// Verbände mit einer Nachkommastelle.
pub fn units(strength: f64) -> String {
    decimal(strength, 1)
}

/// Woran ein Weg scheitert, als Satz.
pub fn limit_text(limit: Limit) -> &'static str {
    match limit {
        Limit::NoControl => "Der Orbit ist umstritten; ohne Orbitalhoheit geht nichts voran.",
        Limit::Stations => {
            "Die Orbitalstationen halten den Orbit; solange sie stehen, ist die Welt nicht \
             eingeschlossen."
        }
        Limit::Fleet => "Die Flotte hält dem Feuer von Stationen und Geschützen nicht stand.",
        Limit::Fuel => "Der Treibstoff der Flotte geht vorher aus.",
        Limit::Ammunition => "Die Munition der Flotte geht vorher aus.",
        Limit::Shield => "Der Schild hält dem Beschuss stand.",
        Limit::NoTroops => {
            "Es fehlen Truppen: keine an Bord, oder zu wenige, um die Welt zu besetzen."
        }
        Limit::Repelled => "Die Landung würde abgeschlagen.",
        Limit::Time => "Die Frist einer Belagerung läuft vorher ab.",
    }
}

/// Die Aussage der Prognose für einen Weg: fällt die Welt, wann, und wenn nicht, woran es
/// liegt.
pub fn forecast_text(course: Course, forecast: &Forecast) -> String {
    let name = course.label();
    if forecast.falls {
        let how = forecast
            .outcome
            .map(|outcome| format!(" ({})", outcome.label()))
            .unwrap_or_default();
        return format!("{name}: Die Welt fällt in {}{how}.", in_days(forecast.days));
    }
    let why = forecast
        .limited_by
        .map(limit_text)
        .unwrap_or("Der Weg führt nicht zum Fall.");
    if forecast.days == 0 {
        format!("{name}: Die Welt fällt nicht. {why}")
    } else {
        format!(
            "{name}: Die Welt fällt nicht; nach {} ist Schluss. {why}",
            in_days(forecast.days)
        )
    }
}

/// Was der Weg kostet: Verluste beider Seiten am Boden, Rumpf der Flotte, Beute.
pub fn forecast_cost(forecast: &Forecast) -> String {
    format!(
        "eigene Verluste {} Verbände · Verluste der Verteidiger {} · Rumpf der schweren \
         Schiffe {} · Beute {}",
        units(forecast.own_losses),
        units(forecast.enemy_losses),
        loss_text(forecast.hull_loss),
        loss_text(forecast.prize_loss)
    )
}

/// Ein Verlust als Anteil: „−14 %“, und „0 %“ ohne Vorzeichen, wenn nichts verloren geht.
pub fn loss_text(share: f64) -> String {
    let text = percent(share as f32);
    if text == percent(0.0) {
        text
    } else {
        format!("−{text}")
    }
}

/// Was die Prognose einer ruhenden Belagerung sagt. Der Tagesrechner rechnet seine drei
/// Wege mit dem, was im Orbit liegt; hält der Belagerer ihn nicht, gelten sie nicht, und
/// was eine Truppe am Boden ohne ihn noch erreicht, rechnet er nicht voraus. `None`,
/// solange die Belagerung läuft oder schon entschieden ist.
pub fn resting_forecast(card: &SiegeCard) -> Option<String> {
    if card.control || card.handover.is_some() {
        return None;
    }
    let ground = if card.troops.army > 0.0 {
        " Die Truppe am Boden kämpft ohne Feuer und Nachschub aus dem Orbit weiter; wie das \
         ausgeht, rechnet die Prognose nicht voraus."
    } else {
        ""
    };
    Some(format!(
        "{} Die Prognose gilt erst wieder mit der Orbitalhoheit.{ground}",
        limit_text(Limit::NoControl)
    ))
}

/// Fortschritt einer Belagerung für die Anzeige: Anteil 0..1 und ein Satz dazu.
#[derive(Debug, Clone, PartialEq)]
pub struct Progress {
    pub share: f32,
    pub text: String,
    pub hint: &'static str,
}

/// Der Fortschritt, wie der Kern ihn nennt. Im Untergrund ist die Oberfläche genommen und
/// sein Wert damit voll, obwohl die Welt noch hält: Dort zeigt die Anzeige, wie viel der
/// Garnison vom Beginn nicht mehr steht, und nennt, was im Untergrund noch sitzt (beides
/// Zahlen der Karte).
pub fn progress_of(card: &SiegeCard) -> Progress {
    if card.phase != Phase::Underground {
        return Progress {
            share: card.progress as f32,
            text: percent(card.progress as f32),
            hint: "Der Anteil der Garnison vom Beginn, der nicht mehr steht, oder der Anteil \
                   der genommenen Zonen, je nachdem, was weiter ist.",
        };
    }
    let troops = &card.troops;
    let standing = troops.militia + troops.reserve + troops.deep;
    let share = if troops.garrison_start > 0.0 {
        (1.0 - standing / troops.garrison_start).clamp(0.0, 1.0) as f32
    } else {
        0.0
    };
    Progress {
        share,
        text: format!(
            "Oberfläche genommen · Garnison zu {} gebrochen · im Untergrund noch {} Verbände",
            percent(share),
            units(troops.deep)
        ),
        hint: "Alle Zonen der Oberfläche sind genommen. Die Welt fällt erst, wenn auch der \
               Untergrund aufgibt oder ausgehungert ist; gezeigt wird der Anteil der \
               Garnison vom Beginn, der nicht mehr steht.",
    }
}

/// Nennt der Text diesen Namen als ganzes Wort? „Zeloth I“ steht nicht in „Zeloth II“:
/// Vor und nach dem Namen darf kein Buchstabe und keine Ziffer stehen.
pub fn names(text: &str, name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    text.match_indices(name).any(|(at, found)| {
        let before = text[..at].chars().next_back();
        let after = text[at + found.len()..].chars().next();
        !before.is_some_and(char::is_alphanumeric) && !after.is_some_and(char::is_alphanumeric)
    })
}

/// Die Lage in einem Wort: die Übergabe steht aus, die Belagerung ruht, oder sie läuft.
/// Ob eine Welt eingeschlossen ist, sagt allein die Phase.
pub fn state_text(card: &SiegeCard) -> String {
    if let Some(outcome) = card.handover {
        format!("entschieden ({}), Übergabe steht aus", outcome.label())
    } else if !card.control {
        RESTING.to_string()
    } else {
        "läuft".to_string()
    }
}

/// „Phase 3 von 5: Beschuss“.
pub fn phase_text(card: &SiegeCard) -> String {
    format!("Phase {} von 5: {}", card.phase_number, card.phase_label)
}

/// Die laufende Belagerung, von der ein Eintrag der Chronik berichtet, wenn der Spieler
/// sie sehen darf: ein Kriegseintrag aus ihrem System, der ihre Welt nennt und nicht aus
/// der Zeit vor ihrem Beginn stammt.
pub fn siege_of_entry(
    sim: &SimRefs,
    cards: &[SiegeCard],
    entry: &ChronicleEntry,
) -> Option<PlanetId> {
    if entry.kind != ChronicleKind::War {
        return None;
    }
    let system = entry.system?;
    let today = sim.clock.day();
    cards
        .iter()
        .find(|card| {
            card.system == system
                && names(&entry.text, &card.planet_name)
                && entry.tick / TICKS_PER_DAY + card.days + 1 >= today
                && may_see(
                    sim.galaxy,
                    sim.fleets,
                    sim.config.player,
                    card.attacker,
                    card.defender,
                    card.system,
                )
        })
        .map(|card| card.planet)
}

/// Name einer Gemeinschaft in ihrer Farbe.
pub(super) fn polity_name(sim: &SimRefs, polity: PolityId) -> RichText {
    let polity = sim.polities.get(polity);
    RichText::new(&polity.name).color(color32(polity.color))
}

/// Ein Knopf, der gesperrt ist, wenn der Kern den Befehl ablehnen würde; der Grund steht
/// im Schwebetext. `name` ist die Beschriftung für Vorleseprogramme.
pub(super) fn gated(
    ui: &mut egui::Ui,
    text: impl Into<egui::WidgetText>,
    name: String,
    check: &Result<(), String>,
    hover: &str,
) -> bool {
    super::fleets::gated_button(ui, text, Some(name), check, hover)
}

/// Die Liste der sichtbaren Belagerungen; ein Klick auf die Welt wählt sie.
fn siege_list(
    ui: &mut egui::Ui,
    sim: &SimRefs,
    cards: &[&SiegeCard],
    chosen: &mut Option<PlanetId>,
) {
    egui::Grid::new("belagerungen")
        .striped(true)
        .num_columns(7)
        .show(ui, |ui| {
            for title in [
                "Welt",
                "Belagerer",
                "Verteidiger",
                "Phase",
                "Dauer",
                "Fortschritt",
                "Lage",
            ] {
                ui.label(RichText::new(title).color(MUTED).small());
            }
            ui.end_row();
            for card in cards {
                if ui
                    .selectable_label(*chosen == Some(card.planet), &card.planet_name)
                    .on_hover_text("Einzelheiten dieser Belagerung zeigen")
                    .clicked()
                {
                    *chosen = Some(card.planet);
                }
                ui.label(polity_name(sim, card.attacker));
                ui.label(polity_name(sim, card.defender));
                ui.label(format!("{} {}", card.phase_number, card.phase_label));
                ui.label(days_text(card.days as f64));
                ui.label(percent(progress_of(card).share));
                let state = state_text(card);
                let color = if card.handover.is_some() {
                    GOOD
                } else if card.control {
                    MUTED
                } else {
                    WARN
                };
                ui.label(RichText::new(state).color(color));
                ui.end_row();
            }
        });
}

fn sieges_tab(ui: &mut egui::Ui, sim: &SimRefs, state: &mut SiegeUi, actions: &mut Vec<UiAction>) {
    // Die Karten gehören dem Zwischenspeicher; die Befehle unten fragen ihn selbst.
    let cards: Vec<SiegeCard> = visible(sim, state.cache.cards())
        .into_iter()
        .cloned()
        .collect();
    if cards.is_empty() {
        state.chosen = None;
        ui.label(RichText::new(NONE_IN_SIGHT).color(MUTED));
        ui.label(small(
            "Eine Belagerung beginnt, wenn eine Flotte über einer Welt liegt, mit deren \
             Besitzer Krieg herrscht, und den Befehl dazu erhält (Reiter „Flotten und \
             Truppen“ oder das Fenster „Flotten“). Gezeigt werden die eigenen Belagerungen \
             und fremde in Systemen, in denen eine eigene Flotte oder Welt liegt.",
        ));
        return;
    }
    // Gezeigt wird die gewählte, solange es sie gibt, sonst die erste.
    if !cards.iter().any(|card| Some(card.planet) == state.chosen) {
        state.chosen = Some(cards[0].planet);
    }
    let refs: Vec<&SiegeCard> = cards.iter().collect();
    // Die Liste bleibt stehen; viele Belagerungen rollen in ihrem eigenen kleinen Bereich.
    egui::ScrollArea::vertical()
        .id_salt("belagerungen_liste")
        .max_height(132.0)
        .show(ui, |ui| siege_list(ui, sim, &refs, &mut state.chosen));
    ui.separator();
    if let Some(card) = cards.iter().find(|card| Some(card.planet) == state.chosen) {
        scrolled(ui, "belagerung_einzeln", |ui| {
            detail::siege_detail(ui, sim, card, state, actions);
        });
    }
}

/// Der rollende Teil des Fensters unter Reitern und Liste, mit einem Rollbalken, der zu
/// sehen ist, sobald der Inhalt nicht ganz in das Fenster passt.
fn scrolled(ui: &mut egui::Ui, salt: &str, content: impl FnOnce(&mut egui::Ui)) {
    ui.scope(|ui| {
        ui.style_mut().spacing.scroll = egui::style::ScrollStyle::solid();
        egui::ScrollArea::vertical().id_salt(salt).show(ui, content);
    });
}

pub fn siege_window(
    ctx: &egui::Context,
    area: egui::Rect,
    sim: &SimRefs,
    state: &mut SiegeUi,
    actions: &mut Vec<UiAction>,
) {
    let mut open = state.open;
    // Wie die anderen Fenster der Partie bleibt es unter der Kopfleiste, auch wenn das
    // Spielfenster kleiner wird. Reiter und Liste stehen fest; es rollt nur, was darunter
    // nicht auf den Schirm passt.
    let room = window_room(area);
    egui::Window::new(TITLE)
        .open(&mut open)
        .default_pos([340.0, 56.0])
        .default_width(760.0)
        .default_height((room - 60.0).max(160.0))
        .max_height(room)
        .constrain_to(area)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut state.tab, SiegeTab::Sieges, TAB_SIEGES);
                ui.selectable_value(&mut state.tab, SiegeTab::Troops, TAB_TROOPS);
            });
            ui.separator();
            match state.tab {
                SiegeTab::Sieges => sieges_tab(ui, sim, state, actions),
                SiegeTab::Troops => scrolled(ui, "belagerung_truppen", |ui| {
                    troops::troops_tab(ui, sim, state, actions);
                }),
            }
        });
    state.open = open;
}

/// Was das Fenster „Flotten“ zur gewählten Flotte über Belagerung und Truppen zeigt: ihr
/// Ziel, Truppen an Bord, die Welten, die sie belagern könnte, und den Weg ins Fenster
/// „Belagerung“.
pub fn fleet_entry(
    ui: &mut egui::Ui,
    sim: &SimRefs,
    fleet: &sim::fleet::Fleet,
    state: &mut SiegeUi,
    actions: &mut Vec<UiAction>,
) {
    troops::fleet_entry(ui, sim, fleet, state, actions);
}

#[cfg(test)]
pub(crate) mod tests;
