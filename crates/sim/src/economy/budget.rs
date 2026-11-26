//! Der Staatshaushalt: Jede Gemeinschaft führt einen Monatshaushalt mit Ansprüchen in
//! fester Rangfolge.
//!
//! Am Monatsersten steht fest, was der abgeschlossene Monat eingebracht hat. Aus der Kasse
//! werden dann der Reihe nach bedient:
//!
//! 1. Schuldendienst: Zinsen und fällige Anleihen.
//! 2. Grundleistungen: die laufende Verwaltung und Versorgung, ein fester Anteil dessen,
//!    was der Staat zum üblichen Steuersatz im Mittel der letzten zwölf Monate einnimmt;
//!    sie gehen an die Haushalte.
//! 3. Flotte: Sold der Besatzungen und Ersatzteile für die Wartung.
//! 4. Beschaffung: Werften, Nachschub und Instandsetzung. Bezahlt wird, wenn gekauft wird,
//!    den ganzen Monat über; beim Abschluss wird dafür der Voranschlag zurückgelegt.
//! 5. Kontore: Einlagen in Kontore, deren Kasse leer ist. Der Staat erhält dafür Anteile.
//! 6. Forschung.
//! 7. Rücklage: Was übrig ist, füllt sie bis zu ihrem Ziel auf.
//! 8. Ausschüttung: Erst vom Rest darüber geht ein Anteil an die Haushalte.
//!
//! Die Ansprüche 1 bis 5 dürfen die Rücklage angreifen, die Forschung nicht: Sie bekommt nur,
//! was die laufenden Einnahmen des Monats nach den vorrangigen Ansprüchen übrig lassen, und
//! was schon über der Rücklage lag. Reicht das Geld nicht, wird von hinten gekürzt; was das
//! heißt, entscheidet jeder Anspruch selbst (Soldrückstand, langsamere Forschung, wartende
//! Werften).
//!
//! Bewilligungen sind Ausgabeerlaubnisse, keine Konten: Das Geld bleibt bis zur Zahlung in
//! der Staatskasse. Jeder Anspruch fragt mit [`Economy::allowance`], was er noch ausgeben
//! darf, und verbucht mit [`Economy::spend`] oder [`Economy::book_paid`], was er gezahlt hat.

use super::{Economy, Money, market_capital_needs, pay, pay_households, state_reserve};
use crate::data::GameData;
use crate::galaxy::Galaxy;
use crate::ids::{PolityId, SystemId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// So viele abgeschlossene Monate hält der Haushalt vor.
pub const BUDGET_HISTORY: usize = 12;

/// Ein Anspruch an die Staatskasse. Die Reihenfolge der Varianten ist die Rangfolge. Ein
/// neuer Anspruch wird an seiner Stelle eingefügt, meldet beim Monatsabschluss seinen
/// Bedarf ([`settle_budget`]) und gibt über [`Economy::allowance`] und [`Economy::spend`]
/// aus.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Claim {
    /// Zinsen und fällige Anleihen.
    DebtService,
    /// Laufende Verwaltung und Versorgung; geht an die Haushalte.
    Civil,
    /// Sold der Besatzungen und Wartung der Schiffe.
    Fleet,
    /// Werften, Nachschub und Instandsetzung, bezahlt, wenn gekauft wird.
    Procurement,
    /// Einlagen in Kontore, deren Kasse leer ist; der Staat erhält dafür Anteile.
    Markets,
    /// Gehälter der Forschenden.
    Research,
    /// Auffüllen der Rücklage.
    Reserve,
    /// Ausschüttung an die Haushalte.
    Handout,
}

impl Claim {
    /// Alle Ansprüche in ihrer Rangfolge.
    pub const ALL: [Claim; 8] = [
        Claim::DebtService,
        Claim::Civil,
        Claim::Fleet,
        Claim::Procurement,
        Claim::Markets,
        Claim::Research,
        Claim::Reserve,
        Claim::Handout,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Claim::DebtService => "Schuldendienst",
            Claim::Civil => "Grundleistungen",
            Claim::Fleet => "Flotte",
            Claim::Procurement => "Beschaffung",
            Claim::Markets => "Kontore",
            Claim::Research => "Forschung",
            Claim::Reserve => "Rücklage",
            Claim::Handout => "Ausschüttung",
        }
    }

    /// Darf der Anspruch die Rücklage angreifen? Sonst bekommt er nur, was über ihr liegt
    /// oder was die Einnahmen des Monats nach den vorrangigen Ansprüchen übrig lassen.
    pub fn may_use_reserve(self) -> bool {
        self <= Claim::Markets
    }

    /// Zählt der Anspruch zu den Ausgaben, die vom Monat abgehen, bevor Rücklage und
    /// Ausschüttung bedient werden?
    pub fn is_spending(self) -> bool {
        self < Claim::Reserve
    }
}

/// Eine Zeile des Haushalts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Line {
    /// Was der Anspruch verlangt hat.
    pub wanted: Money,
    /// Was er bekommen und ausgegeben hat.
    pub paid: Money,
}

/// Ein Monat im Haushalt einer Gemeinschaft: reine Daten, wie sie auch eine Oberfläche
/// zeigen kann.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BudgetMonth {
    /// Tag des Monatsabschlusses, mit dem dieser Haushaltsmonat begann.
    pub day: u64,
    /// Laufende Einnahmen des eben abgeschlossenen Monats, aus denen dieser bezahlt wird:
    /// Lohnsteuer, Zölle der Kontore, Dividenden auf staatliche Anteile.
    pub revenue: Money,
    /// Besteuerte Lohnsumme desselben Monats.
    pub wage_bill: Money,
    /// Was der Monat zum üblichen Lohnsteuersatz eingebracht hätte: die laufenden
    /// Einnahmen ohne das, was ein höherer Satz mehr und ein niedrigerer weniger bringt.
    /// An ihrem Mittel bemessen sich die Grundleistungen: Wer die Steuer anhebt, gewinnt
    /// den Zuwachs für alles andere; wer sie senkt, muss anderswo kürzen.
    pub standard_revenue: Money,
    /// Saldo aller übrigen Bewegungen der Staatskasse seit dem vorigen Abschluss: Geld der
    /// Notenbank und Anleihen als Zufluss; Koloniegründungen, Anteilskäufe und Einlagen in
    /// Kontore als Abfluss.
    pub other: Money,
    /// Je Anspruch, was er verlangt und was er bekommen hat. Bei der Rücklage: was ihr zum
    /// Ziel fehlte und was davon aufgefüllt wurde.
    pub lines: BTreeMap<Claim, Line>,
    /// Was der Abschluss den Ansprüchen vor der Rücklage nicht bewilligen konnte, weil
    /// das Geld fehlte. (Bleibt ein Anspruch dagegen unter dem Verlangten, weil am Markt
    /// die Ware fehlte, steht das nur in seiner Zeile.)
    pub unfunded: Money,
    /// Staatskasse nach dem Abschluss, ohne das, was Flotte und Forschung bewilligt ist
    /// und noch am selben Tag abgeht: der Bestand, aus dem die Beschaffung den Monat über
    /// kauft.
    pub treasury: Money,
    /// Rücklage, die der Staat halten will.
    pub reserve_target: Money,
}

impl BudgetMonth {
    pub fn line(&self, claim: Claim) -> Line {
        self.lines.get(&claim).copied().unwrap_or_default()
    }

    /// Was die Ansprüche vor Rücklage und Ausschüttung zusammen gekostet haben.
    pub fn spending(&self) -> Money {
        self.lines
            .iter()
            .filter(|(claim, _)| claim.is_spending())
            .map(|(_, line)| line.paid)
            .sum()
    }

    /// Was die Ansprüche verlangt, aber nicht bekommen haben.
    pub fn unpaid(&self) -> Money {
        self.lines
            .iter()
            .filter(|(claim, _)| claim.is_spending())
            .map(|(_, line)| (line.wanted - line.paid).max(0))
            .sum()
    }

    /// Rest der laufenden Einnahmen nach den Ansprüchen. Negativ heißt Defizit: Die
    /// Ansprüche haben mehr gekostet, als der Monat eingebracht hat.
    pub fn remainder(&self) -> Money {
        self.revenue - self.spending()
    }

    /// Ausschüttung an die Haushalte.
    pub fn handout(&self) -> Money {
        self.line(Claim::Handout).paid
    }

    /// Zählt einen weiteren Monat hinzu. Kasse und Rücklagenziel sind Bestände: Es gilt
    /// der Stand des späteren Monats.
    fn add(&mut self, month: &BudgetMonth) {
        self.revenue += month.revenue;
        self.wage_bill += month.wage_bill;
        self.standard_revenue += month.standard_revenue;
        self.other += month.other;
        self.unfunded += month.unfunded;
        for (claim, line) in &month.lines {
            let sum = self.lines.entry(*claim).or_default();
            sum.wanted += line.wanted;
            sum.paid += line.paid;
        }
        self.treasury = month.treasury;
        self.reserve_target = month.reserve_target;
    }
}

/// Der Haushalt einer Gemeinschaft: der laufende Monat und die letzten abgeschlossenen.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Budget {
    /// Der laufende Monat. Beim Abschluss am Monatsersten bezahlt und bewilligt; die
    /// Käufe der Beschaffung laufen bis zum nächsten Abschluss auf.
    pub current: BudgetMonth,
    /// Abgeschlossene Monate, der jüngste zuletzt; höchstens [`BUDGET_HISTORY`].
    pub history: Vec<BudgetMonth>,
    /// Was jeder Anspruch im laufenden Monat noch ausgeben darf.
    pub grants: BTreeMap<Claim, Money>,
    /// Stand der Staatskasse, wie er sich aus dem letzten Abschluss und allem seither
    /// Verbuchten ergibt. Die Abweichung der wirklichen Kasse davon ist der Saldo der
    /// übrigen Bewegungen (`BudgetMonth::other`).
    pub tracked: Money,
    /// Ob schon ein Haushaltsmonat eröffnet wurde.
    pub opened: bool,
}

impl Budget {
    /// Summe der abgeschlossenen Monate, höchstens der letzten zwölf.
    pub fn year(&self) -> BudgetMonth {
        let mut sum = BudgetMonth {
            day: self.history.first().map_or(self.current.day, |m| m.day),
            ..Default::default()
        };
        for month in &self.history {
            sum.add(month);
        }
        sum
    }

    /// Der zuletzt abgeschlossene Monat.
    pub fn last(&self) -> Option<&BudgetMonth> {
        self.history.last()
    }

    /// Mittlere Einnahmen zum üblichen Steuersatz je Monat über die vorliegenden Monate,
    /// den eben abgeschlossenen eingeschlossen.
    fn mean_standard_revenue(&self) -> f64 {
        let months = self.history.len() + 1;
        let sum: Money = self
            .history
            .iter()
            .map(|month| month.standard_revenue)
            .sum::<Money>()
            + self.current.standard_revenue;
        sum as f64 / months as f64
    }
}

impl Economy {
    /// Der Haushalt einer Gemeinschaft.
    pub fn budget(&self, polity: PolityId) -> Option<&Budget> {
        self.budgets.get(polity.index())
    }

    /// Was ein Anspruch jetzt noch ausgeben darf: der Rest seiner Bewilligung, höchstens
    /// die Staatskasse. Solange für die Gemeinschaft noch kein Haushaltsmonat eröffnet ist
    /// (eine eben gegründete, bis zum nächsten Monatsersten), gilt nur die Kasse.
    pub fn allowance(&self, polity: PolityId, claim: Claim) -> Money {
        let treasury = self.treasury(polity).max(0);
        match self.budgets.get(polity.index()) {
            Some(budget) if budget.opened => budget
                .grants
                .get(&claim)
                .copied()
                .unwrap_or(0)
                .clamp(0, treasury),
            _ => treasury,
        }
    }

    /// Meldet, was ein Anspruch verlangt, gleich ob er es bekommt.
    pub fn want(&mut self, polity: PolityId, claim: Claim, amount: Money) {
        if amount <= 0 {
            return;
        }
        if let Some(budget) = self.budgets.get_mut(polity.index()) {
            budget.current.lines.entry(claim).or_default().wanted += amount;
        }
    }

    /// Verbucht eine Zahlung, die die Staatskasse schon verlassen hat.
    pub fn book_paid(&mut self, polity: PolityId, claim: Claim, amount: Money) {
        if amount <= 0 {
            return;
        }
        if let Some(budget) = self.budgets.get_mut(polity.index()) {
            budget.current.lines.entry(claim).or_default().paid += amount;
            budget.tracked -= amount;
            if let Some(grant) = budget.grants.get_mut(&claim) {
                *grant = (*grant - amount).max(0);
            }
        }
    }

    /// Zieht eine Ausgabe von der Staatskasse ab und verbucht sie. Für Käufer, die mit
    /// einem geliehenen Geldbeutel einkaufen: Sie holen sich [`Economy::allowance`], zahlen
    /// daraus und melden hier, was fehlt.
    pub fn spend(&mut self, polity: PolityId, claim: Claim, amount: Money) {
        if amount <= 0 {
            return;
        }
        if let Some(treasury) = self.treasuries.get_mut(polity.index()) {
            *treasury -= amount;
        }
        self.book_paid(polity, claim, amount);
    }

    /// Schließt den Haushaltsmonat jeder Gemeinschaft und eröffnet den nächsten. Gehört an
    /// den Monatsabschluss, sobald die laufenden Einnahmen des Monats feststehen
    /// ([`Economy::close_income_month`]) und bevor Schuldendienst und Ansprüche bezahlt
    /// werden. `standard_tax` ist der übliche Lohnsteuersatz (`economy.income_tax`).
    pub fn open_budget_month(&mut self, day: u64, standard_tax: f64) {
        self.sync_polities();
        for index in 0..self.treasuries.len() {
            let treasury = self.treasuries[index];
            let revenue = self.state_income[index];
            let wage_bill = std::mem::take(&mut self.wage_bill_running[index]);
            let surcharge = (self.tax_rates[index] - standard_tax) * wage_bill as f64;
            let standard_revenue = (revenue - surcharge.round() as Money).max(0);
            let budget = &mut self.budgets[index];
            let closed = std::mem::take(&mut budget.current);
            let other = if budget.opened {
                budget.history.push(closed);
                if budget.history.len() > BUDGET_HISTORY {
                    let surplus = budget.history.len() - BUDGET_HISTORY;
                    budget.history.drain(..surplus);
                }
                treasury - budget.tracked - revenue
            } else {
                // Der erste Monat: Die Anfangsausstattung ist kein Zufluss.
                0
            };
            budget.opened = true;
            budget.current = BudgetMonth {
                day,
                revenue,
                wage_bill,
                standard_revenue,
                other,
                ..Default::default()
            };
            budget.tracked = treasury;
            budget.grants.clear();
        }
    }
}

/// Bedarf der Ansprüche, die ihre Mittel selbst ausgeben, für den beginnenden Monat.
pub type Wants = BTreeMap<Claim, Money>;

/// Monatsabschluss des Haushalts einer Gemeinschaft, nach dem Schuldendienst. In der
/// Rangfolge der Ansprüche: zahlt die Grundleistungen, bewilligt Flotte und Forschung, was
/// sie verlangen und die Kasse hergibt, legt den Voranschlag der Beschaffung zurück, schießt
/// Kontoren ohne Kasse Kapital nach, hält die Rücklage zurück und schüttet vom Rest darüber
/// einen Anteil an die Haushalte aus.
///
/// `wants` nennt den Bedarf der Ansprüche, die selbst ausgeben (Flotte, Beschaffung,
/// Forschung und was später hinzukommt). Für die Beschaffung ist es der Voranschlag des
/// Monats: Er wird vor allem Nachrangigen zurückgelegt. Ausgeben darf sie den ganzen Monat
/// über alles, was nach dem Abschluss in der Kasse liegt und niemandem sonst bewilligt
/// ist, auch die Rücklage; die Einnahmen des laufenden Monats rührt sie nicht an, sie
/// gehören dem nächsten Abschluss.
///
/// Gibt die Einlagen in Kontore zurück (System und Betrag): Die Börse gibt dem Staat dafür
/// neue Anteile aus.
pub fn settle_budget(
    galaxy: &Galaxy,
    economy: &mut Economy,
    data: &GameData,
    polity: PolityId,
    wants: &Wants,
) -> Vec<(SystemId, Money)> {
    let index = polity.index();
    let population = galaxy.population_of(polity);
    let mut injections = Vec::new();
    if index >= economy.budgets.len() || population <= 0.0 {
        return injections;
    }
    let balance = &data.balance.economy;
    let reserve = state_reserve(economy, data, polity, population);
    let revenue = economy.budgets[index].current.revenue;
    let debt = economy.budgets[index].current.line(Claim::DebtService);
    // Bestand vor den Einnahmen des abgeschlossenen Monats: Nur er ist Rücklage. Was der
    // Monat eingebracht hat, steht den Ansprüchen offen, bevor es die Rücklage auffüllt.
    let stock = (economy.treasuries[index] + debt.paid - revenue).max(0);
    let protected = stock.min(reserve);
    let mut unfunded = (debt.wanted - debt.paid).max(0);
    // Was Flotte, Forschung und ihresgleichen bewilligt ist und noch in der Kasse liegt.
    let mut granted: Money = 0;
    // Was der Beschaffung zurückgelegt ist.
    let mut earmarked: Money = 0;

    for claim in Claim::ALL {
        // Was noch niemandem zugesagt ist; ohne Zugriff auf die Rücklage nur, was über
        // ihrem Bestand liegt.
        let free = economy.treasuries[index].max(0) - granted - earmarked;
        let available = if claim.may_use_reserve() {
            free
        } else {
            free - protected
        }
        .max(0);
        match claim {
            // Schon bezahlt: Der Schuldendienst geht dem Abschluss voraus.
            Claim::DebtService => {}
            Claim::Civil => {
                let wanted = (economy.budgets[index].mean_standard_revenue() * balance.civil_share)
                    .round() as Money;
                economy.want(polity, claim, wanted);
                // Beim Verteilen nach Köpfen bleibt ein Rundungsrest in der Kasse; gekürzt
                // ist der Anspruch nur, wenn die Kasse ihn nicht hergab.
                unfunded += (wanted - available).max(0);
                let paid = pay_households(galaxy, economy, polity, wanted.min(available));
                economy.book_paid(polity, claim, paid);
            }
            Claim::Markets => {
                let mut left = available;
                for (system, needed) in market_capital_needs(galaxy, economy, polity) {
                    economy.want(polity, claim, needed);
                    let got = pay(
                        &mut economy.treasuries[index],
                        &mut economy.markets[system.index()].cash,
                        needed.min(left),
                    );
                    left -= got;
                    unfunded += needed - got;
                    if got > 0 {
                        economy.book_paid(polity, claim, got);
                        injections.push((system, got));
                    }
                }
            }
            Claim::Reserve => {
                // Was der Rücklage zum Ziel fehlte, und was der Monat davon aufgefüllt hat.
                let gap = (reserve - protected).max(0);
                let filled = (free.min(reserve) - protected).clamp(0, gap);
                economy.want(polity, claim, gap);
                if filled > 0 {
                    economy.budgets[index]
                        .current
                        .lines
                        .entry(claim)
                        .or_default()
                        .paid += filled;
                }
            }
            Claim::Handout => {
                // Ein Anteil dessen, was über der Rücklage liegt.
                let excess = free - reserve;
                if excess > 0 {
                    let handout = (excess as f64 * balance.handout_share) as Money;
                    let paid = pay_households(galaxy, economy, polity, handout);
                    economy.want(polity, claim, paid);
                    economy.book_paid(polity, claim, paid);
                }
            }
            // Ansprüche, die ihre Mittel selbst ausgeben.
            _ => {
                let Some(wanted) = wants.get(&claim).copied() else {
                    continue;
                };
                let grant = wanted.clamp(0, available);
                unfunded += (wanted - grant).max(0);
                if claim == Claim::Procurement {
                    earmarked += grant;
                } else {
                    granted += grant;
                    economy.budgets[index].grants.insert(claim, grant);
                }
            }
        }
    }

    // Die Beschaffung kauft den Monat über aus allem, was jetzt in der Kasse liegt und
    // nicht den anderen bewilligt ist.
    let purse = (economy.treasuries[index] - granted).max(0);
    let budget = &mut economy.budgets[index];
    budget.grants.insert(Claim::Procurement, purse);
    budget.current.treasury = purse;
    budget.current.reserve_target = reserve;
    budget.current.unfunded = unfunded;
    injections
}
