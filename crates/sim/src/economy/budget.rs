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

    /// Zahlt Sold oder Gehälter an die, die im Dienst des Staates stehen
    /// ([`Economy::staff`]), und verbucht die Zahlung beim Anspruch. Gezahlt wird nie mehr,
    /// als die Staatskasse hergibt. Gibt den gezahlten Betrag zurück.
    pub fn pay_staff(&mut self, polity: PolityId, claim: Claim, amount: Money) -> Money {
        self.sync_polities();
        let Some(treasury) = self.treasuries.get_mut(polity.index()) else {
            return 0;
        };
        let paid = pay(treasury, &mut self.staff[polity.index()], amount);
        self.book_paid(polity, claim, paid);
        paid
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::economy::{setup_economy, staff_consume, to_credits, to_money};
    use crate::fleet::testing::setup;
    use crate::galaxy::Galaxy;
    use crate::polity::Polities;

    const ME: PolityId = PolityId(0);

    fn start() -> (GameData, Galaxy, Polities, Economy) {
        let (data, galaxy, polities, _) = setup();
        let mut economy = setup_economy(&data, &galaxy, &polities);
        // Kein Kontor braucht Kapital: Der Anspruch „Kontore“ bleibt in diesen Prüfungen leer.
        for market in &mut economy.markets {
            market.cash = to_money(10_000.0);
        }
        (data, galaxy, polities, economy)
    }

    fn households(galaxy: &Galaxy, economy: &Economy) -> Money {
        galaxy
            .planets_of(ME)
            .map(|planet| economy.planet(planet.id).households)
            .sum()
    }

    fn wants(fleet: f64, procurement: f64, research: f64) -> Wants {
        Wants::from([
            (Claim::Fleet, to_money(fleet)),
            (Claim::Procurement, to_money(procurement)),
            (Claim::Research, to_money(research)),
        ])
    }

    /// Schließt für die Gemeinschaft 0 einen Monat ab, der `revenue` eingebracht hat und an
    /// dessen Ende `treasury` in der Kasse liegt (die Einnahmen eingeschlossen).
    fn close(
        data: &GameData,
        galaxy: &Galaxy,
        economy: &mut Economy,
        treasury: Money,
        revenue: f64,
        wants: &Wants,
    ) -> BudgetMonth {
        economy.treasuries[ME.index()] = treasury;
        economy.state_income[ME.index()] = to_money(revenue);
        let day = economy.budgets[ME.index()].history.len() as u64 * 30;
        economy.open_budget_month(day, data.balance.economy.income_tax);
        let injections = settle_budget(galaxy, economy, data, ME, wants);
        assert!(injections.is_empty());
        economy.budget(ME).unwrap().current.clone()
    }

    #[test]
    fn claims_are_served_in_their_order_and_the_last_are_cut_first() {
        let (data, galaxy, _, fresh) = start();
        let balance = &data.balance.economy;
        let reserve = state_reserve(&fresh, &data, ME, galaxy.population_of(ME));
        assert!(reserve > 0);
        assert_eq!(
            Claim::ALL.to_vec(),
            {
                let mut sorted = Claim::ALL.to_vec();
                sorted.sort();
                sorted
            },
            "die Rangfolge ist die Reihenfolge der Varianten"
        );
        let asked = wants(500.0, 300.0, 600.0);
        let civil = to_money(4000.0 * balance.civil_share);

        // Ein guter Monat über einer vollen Rücklage: Jeder Anspruch bekommt, was er
        // verlangt; vom Rest über der Rücklage geht der Anteil der Ausschüttung an die
        // Haushalte, der andere bleibt in der Kasse.
        let mut economy = fresh.clone();
        let before = households(&galaxy, &economy);
        let treasury = reserve + to_money(10_000.0);
        let month = close(&data, &galaxy, &mut economy, treasury, 4000.0, &asked);
        let handout = ((to_money(10_000.0 - 500.0 - 300.0 - 600.0) - civil) as f64
            * balance.handout_share) as Money;
        assert_eq!(
            month.line(Claim::Civil),
            Line {
                wanted: civil,
                paid: civil
            }
        );
        assert_eq!(month.handout(), handout);
        assert_eq!(households(&galaxy, &economy) - before, civil + handout);
        assert_eq!(month.unfunded, 0);
        assert_eq!(economy.allowance(ME, Claim::Fleet), to_money(500.0));
        assert_eq!(economy.allowance(ME, Claim::Research), to_money(600.0));
        // Die Beschaffung kauft den Monat über aus allem, was nicht anderen bewilligt ist.
        let left = treasury - civil - handout;
        assert_eq!(economy.treasury(ME), left);
        assert_eq!(
            economy.allowance(ME, Claim::Procurement),
            left - to_money(500.0 + 600.0)
        );
        assert_eq!(month.treasury, left - to_money(500.0 + 600.0));
        assert_eq!(month.reserve_target, reserve);
        assert_eq!(month.line(Claim::Reserve), Line::default());
        // Der Abschluss verschiebt Geld und schafft keines.
        assert_eq!(
            economy.money_in_accounts(),
            fresh.money_in_accounts() - fresh.treasury(ME) + treasury
        );

        // Die Kasse reicht nicht für alle: Gekürzt wird von hinten. Grundleistungen und
        // Flotte bekommen alles, die Beschaffung den Rest, die Forschung nichts; nichts
        // füllt die Rücklage, nichts wird ausgeschüttet.
        let mut economy = fresh.clone();
        let month = close(
            &data,
            &galaxy,
            &mut economy,
            to_money(2600.0),
            4000.0,
            &asked,
        );
        assert_eq!(month.line(Claim::Civil).paid, civil);
        assert_eq!(economy.allowance(ME, Claim::Fleet), to_money(500.0));
        assert_eq!(economy.allowance(ME, Claim::Research), 0);
        assert_eq!(month.unfunded, to_money(200.0 + 600.0));
        assert_eq!(month.handout(), 0);
        assert_eq!(
            month.line(Claim::Reserve),
            Line {
                wanted: reserve,
                paid: 0
            }
        );

        // Noch weniger: Schon die Grundleistungen werden gekürzt, und die Flotte geht leer
        // aus. Wie sie damit umgeht (Soldrückstand), ist ihre Sache.
        let mut economy = fresh.clone();
        let month = close(
            &data,
            &galaxy,
            &mut economy,
            to_money(1500.0),
            4000.0,
            &asked,
        );
        assert_eq!(
            month.line(Claim::Civil),
            Line {
                wanted: civil,
                paid: to_money(1500.0)
            }
        );
        assert_eq!(economy.allowance(ME, Claim::Fleet), 0);
        assert_eq!(month.unfunded, civil - to_money(1500.0) + to_money(1400.0));
        assert_eq!(economy.treasury(ME), 0);
    }

    #[test]
    fn research_lives_on_the_month_and_never_on_the_reserve() {
        let (data, galaxy, _, fresh) = start();
        let balance = &data.balance.economy;
        let reserve = state_reserve(&fresh, &data, ME, galaxy.population_of(ME));
        let asked = wants(300.0, 0.0, 600.0);

        // Die Rücklage ist voll, der Monat bringt 1000: Nach Grundleistungen und Flotte
        // bleiben davon 200. Mehr bekommt die Forschung nicht, obwohl die Kasse voll ist.
        let mut economy = fresh.clone();
        let treasury = reserve + to_money(1000.0);
        let month = close(&data, &galaxy, &mut economy, treasury, 1000.0, &asked);
        let civil = to_money(1000.0 * balance.civil_share);
        assert_eq!(month.line(Claim::Civil).paid, civil);
        let rest = to_money(1000.0 - 300.0) - civil;
        assert!(rest > 0 && rest < to_money(600.0));
        assert_eq!(economy.allowance(ME, Claim::Research), rest);
        assert_eq!(month.unfunded, to_money(600.0) - rest);
        assert_eq!(month.handout(), 0);

        // Die Flotte dagegen greift die Rücklage an: Ohne jede Einnahme bekommt sie ihren
        // Sold aus ihr, die Forschung nichts.
        let mut economy = fresh.clone();
        close(&data, &galaxy, &mut economy, reserve, 0.0, &asked);
        assert_eq!(economy.allowance(ME, Claim::Fleet), to_money(300.0));
        assert_eq!(economy.allowance(ME, Claim::Research), 0);

        // Was vor dem Monat schon über der Rücklage lag, steht der Forschung offen. Eine
        // angegriffene Rücklage füllt sich erst nach ihr wieder auf: Die Forschung geht
        // der Rücklage vor, die Rücklage der Ausschüttung.
        let mut economy = fresh.clone();
        let treasury = reserve + to_money(1500.0);
        close(&data, &galaxy, &mut economy, treasury, 1000.0, &asked);
        assert_eq!(economy.allowance(ME, Claim::Research), to_money(600.0));
        let mut economy = fresh.clone();
        let short = reserve - to_money(400.0) + to_money(1000.0);
        let month = close(&data, &galaxy, &mut economy, short, 1000.0, &asked);
        assert_eq!(economy.allowance(ME, Claim::Research), rest);
        assert_eq!(
            month.line(Claim::Reserve),
            Line {
                wanted: to_money(400.0),
                paid: 0
            }
        );
        let month = close(
            &data,
            &galaxy,
            &mut economy.clone(),
            short,
            1000.0,
            &wants(0.0, 0.0, 0.0),
        );
        // Ohne Flotte und Forschung füllt derselbe Monat die Rücklage und schüttet vom Rest aus.
        assert_eq!(month.line(Claim::Reserve).paid, to_money(400.0));
        assert!(month.handout() > 0);
    }

    #[test]
    fn the_read_model_keeps_twelve_months_and_sums_them() {
        let (data, galaxy, _, mut economy) = start();
        let reserve = state_reserve(&economy, &data, ME, galaxy.population_of(ME));
        let asked = wants(0.0, 0.0, 0.0);
        let mut revenues = Vec::new();
        let mut handouts = Vec::new();
        for index in 0..15u32 {
            let revenue = 1000.0 + 100.0 * index as f64;
            let treasury = economy.treasury(ME).max(reserve) + to_money(revenue);
            let month = close(&data, &galaxy, &mut economy, treasury, revenue, &asked);
            // Eine Zahlung mitten im Monat steht in der Zeile ihres Anspruchs.
            economy.want(ME, Claim::Procurement, to_money(50.0));
            economy.spend(ME, Claim::Procurement, to_money(40.0));
            revenues.push(to_money(revenue));
            handouts.push(month.handout());
        }
        let budget = economy.budget(ME).unwrap();
        // Abgeschlossen sind alle Monate bis auf den laufenden; aufbewahrt die letzten zwölf.
        assert_eq!(budget.history.len(), BUDGET_HISTORY);
        let last = budget.last().unwrap();
        assert_eq!(last.revenue, revenues[13]);
        assert_eq!(budget.current.revenue, revenues[14]);
        assert_eq!(
            last.line(Claim::Procurement),
            Line {
                wanted: to_money(50.0),
                paid: to_money(40.0)
            }
        );
        let year = budget.year();
        assert_eq!(year.revenue, revenues[2..14].iter().sum::<Money>());
        assert_eq!(year.handout(), handouts[2..14].iter().sum::<Money>());
        assert_eq!(year.line(Claim::Procurement).paid, to_money(12.0 * 40.0));
        assert_eq!(
            year.spending(),
            year.line(Claim::Civil).paid + year.line(Claim::Procurement).paid
        );
        assert_eq!(year.remainder(), year.revenue - year.spending());
        assert_eq!(year.unpaid(), to_money(12.0 * 10.0));
        // Die Grundleistungen folgen dem Mittel der Einnahmen, nicht dem einzelnen Monat.
        let mean = revenues[2..15].iter().sum::<Money>() as f64 / 13.0;
        assert_eq!(
            budget.current.line(Claim::Civil).wanted,
            (mean * data.balance.economy.civil_share).round() as Money
        );
        assert_eq!(economy.find_anomaly(), None);
    }

    #[test]
    fn other_movements_of_the_treasury_show_as_their_own_figure() {
        let (data, galaxy, _, mut economy) = start();
        let reserve = state_reserve(&economy, &data, ME, galaxy.population_of(ME));
        let asked = wants(0.0, 0.0, 0.0);
        close(&data, &galaxy, &mut economy, reserve, 0.0, &asked);
        // Geld der Notenbank kommt hinzu, eine Koloniegründung geht ab: Beides läuft am
        // Haushalt vorbei und steht im nächsten Monat als Saldo.
        economy.treasuries[ME.index()] += to_money(700.0 - 250.0);
        let treasury = economy.treasury(ME) + to_money(1000.0);
        let month = close(&data, &galaxy, &mut economy, treasury, 1000.0, &asked);
        assert_eq!(month.other, to_money(450.0));
    }

    #[test]
    fn a_higher_tax_leaves_the_civil_base_where_it_was() {
        let (data, galaxy, _, fresh) = start();
        let standard = data.balance.economy.income_tax;
        let run = |rate: f64| {
            let mut economy = fresh.clone();
            economy.tax_rates[ME.index()] = rate;
            // Lohnsumme 10 000: Die Lohnsteuer bringt den Satz davon, dazu 500 an Zöllen.
            let wages = 10_000.0;
            economy.wage_bill_running[ME.index()] = to_money(wages);
            let revenue = wages * rate + 500.0;
            let treasury = fresh.treasury(ME) + to_money(revenue);
            let month = close(
                &data,
                &galaxy,
                &mut economy,
                treasury,
                revenue,
                &wants(0.0, 0.0, 0.0),
            );
            (
                month.revenue,
                month.standard_revenue,
                month.line(Claim::Civil).wanted,
            )
        };
        let (usual, usual_standard, usual_civil) = run(standard);
        let (raised, raised_standard, raised_civil) = run(standard + 0.10);
        let (lowered, _, lowered_civil) = run(standard - 0.05);
        assert_eq!(usual, usual_standard);
        assert_eq!(raised - usual, to_money(1000.0));
        assert_eq!(usual - lowered, to_money(500.0));
        // Der Zuwachs aus der höheren Steuer steht für alles andere frei; wer sie senkt,
        // muss anderswo kürzen.
        assert_eq!(raised_standard, usual_standard);
        assert_eq!(raised_civil, usual_civil);
        assert_eq!(lowered_civil, usual_civil);
    }

    #[test]
    fn pay_goes_to_the_staff_who_spend_it_at_the_seat() {
        let (data, galaxy, polities, mut economy) = start();
        let balance = &data.balance.economy;
        let seat = polities.get(ME).seat(&galaxy).unwrap();
        assert_eq!(Some(seat), polities.get(ME).capital);
        let system = galaxy.planet(seat).system;
        let seats: Vec<_> = polities.list.iter().map(|p| p.seat(&galaxy)).collect();
        let before = economy.clone();

        // Sold und Gehälter verlassen die Staatskasse und stehen beim Anspruch; die
        // Haushalte der Welten bekommen davon nichts.
        let paid = economy.pay_staff(ME, Claim::Research, to_money(900.0));
        assert_eq!(paid, to_money(900.0));
        assert_eq!(before.treasury(ME) - economy.treasury(ME), paid);
        assert_eq!(economy.staff_money(ME), paid);
        assert_eq!(
            economy
                .budget(ME)
                .unwrap()
                .current
                .line(Claim::Research)
                .paid,
            paid
        );
        assert_eq!(households(&galaxy, &economy), households(&galaxy, &before));
        // Mehr als die Kasse hergibt, wird nie gezahlt.
        let mut poor = economy.clone();
        poor.treasuries[ME.index()] = to_money(10.0);
        assert_eq!(
            poor.pay_staff(ME, Claim::Fleet, to_money(50.0)),
            to_money(10.0)
        );

        // Am Markt des Sitzes kauft der Staatsdienst ein, was Haushalte kaufen, im
        // Verhältnis des Grundbedarfs und je Tag für einen Teil seines Geldes.
        for need in &balance.needs {
            economy.markets[system.index()].stock[need.good_id.index()] = 500.0;
        }
        let stocked = economy.clone();
        staff_consume(&galaxy, &mut economy, &data, &seats);
        let spent = stocked.staff_money(ME) - economy.staff_money(ME);
        let budget = to_credits(paid) * balance.spend_rate;
        assert!((to_credits(spent) - budget).abs() < 0.01, "{spent}");
        assert_eq!(
            economy.market(system).cash - stocked.market(system).cash,
            spent
        );
        let taken: Vec<f64> = balance
            .needs
            .iter()
            .map(|need| {
                stocked.market(system).stock_of(need.good_id)
                    - economy.market(system).stock_of(need.good_id)
            })
            .collect();
        for (need, amount) in balance.needs.iter().zip(&taken) {
            assert!(*amount > 0.0);
            let ratio = amount / taken[0] - need.per_million / balance.needs[0].per_million;
            assert!(ratio.abs() < 1e-6, "{ratio}");
            assert!(economy.market(system).demand_today[need.good_id.index()] > 0.0);
        }
        // Dieser Verbrauch ist nicht der der Haushalte: Ihr Geld und ihre Deckung bleiben.
        assert_eq!(households(&galaxy, &economy), households(&galaxy, &stocked));
        assert_eq!(
            economy.planet(seat).needs_met,
            stocked.planet(seat).needs_met
        );
        assert_eq!(economy.money_in_accounts(), before.money_in_accounts());

        // Gibt der Markt nichts her, bleibt das Geld in der Kasse des Staatsdienstes.
        for need in &balance.needs {
            economy.markets[system.index()].stock[need.good_id.index()] = 0.0;
        }
        let waiting = economy.staff_money(ME);
        staff_consume(&galaxy, &mut economy, &data, &seats);
        assert_eq!(economy.staff_money(ME), waiting);
        assert_eq!(economy.find_anomaly(), None);
    }
}
