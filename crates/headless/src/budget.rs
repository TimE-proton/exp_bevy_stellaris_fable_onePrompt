//! Der Staatshaushalt auf der Kommandozeile: die Zeilen eines Monats und eines Jahres je
//! Gemeinschaft und die Summen über einen ganzen Lauf.

use sim::economy::{BudgetMonth, Claim, Economy, to_credits};
use sim::fleet::Fleets;
use sim::harness::Headless;
use sim::ids::PolityId;

/// Was der Haushalt einer Gemeinschaft über einen Lauf zusammen ergibt.
#[derive(Clone, Debug, Default)]
pub struct BudgetTotals {
    /// Summe aller abgeschlossenen Monate.
    pub sum: BudgetMonth,
    pub months: u32,
    /// Monate, in denen die Ansprüche mehr kosteten, als der Monat einbrachte.
    pub deficit_months: u32,
    /// Monate, in denen der Haushalt einen Anspruch aus Geldmangel kürzen musste.
    pub short_months: u32,
    /// Monate, in denen die Kasse nach dem Abschluss unter der Rücklage lag.
    pub below_reserve: u32,
    /// Höchster Soldrückstand zu einem Jahresende.
    pub max_arrears: f32,
}

impl BudgetTotals {
    /// Nimmt die zwölf zuletzt abgeschlossenen Monate einer Gemeinschaft auf. Am Ende
    /// jedes Jahres aufgerufen, ergibt das die Summe über den ganzen Lauf.
    pub fn add_year(&mut self, sim: &Headless, polity: PolityId) {
        let world = sim.world();
        let Some(budget) = world.resource::<Economy>().budget(polity) else {
            return;
        };
        for month in &budget.history {
            self.months += 1;
            self.sum.revenue += month.revenue;
            self.sum.wage_bill += month.wage_bill;
            self.sum.other += month.other;
            for (claim, line) in &month.lines {
                let sum = self.sum.lines.entry(*claim).or_default();
                sum.wanted += line.wanted;
                sum.paid += line.paid;
            }
            self.deficit_months += u32::from(month.remainder() < 0);
            self.short_months += u32::from(month.unfunded > 0);
            self.below_reserve += u32::from(month.treasury < month.reserve_target);
        }
        self.max_arrears = self
            .max_arrears
            .max(world.resource::<Fleets>().arrears_of(polity));
    }

    /// Fasst mehrere Gemeinschaften zusammen.
    pub fn merge(&mut self, other: &BudgetTotals) {
        self.months += other.months;
        self.sum.revenue += other.sum.revenue;
        self.sum.wage_bill += other.sum.wage_bill;
        self.sum.other += other.sum.other;
        for (claim, line) in &other.sum.lines {
            let sum = self.sum.lines.entry(*claim).or_default();
            sum.wanted += line.wanted;
            sum.paid += line.paid;
        }
        self.deficit_months += other.deficit_months;
        self.short_months += other.short_months;
        self.below_reserve += other.below_reserve;
        self.max_arrears = self.max_arrears.max(other.max_arrears);
    }

    /// Laufende Einnahmen je Monat in Credits.
    pub fn revenue_per_month(&self) -> f64 {
        to_credits(self.sum.revenue) / f64::from(self.months.max(1))
    }

    /// Was ein Anspruch bekommen hat, als Anteil der laufenden Einnahmen.
    pub fn share(&self, claim: Claim) -> f64 {
        self.sum.line(claim).paid as f64 / (self.sum.revenue.max(1)) as f64
    }

    pub fn remainder_share(&self) -> f64 {
        self.sum.remainder() as f64 / (self.sum.revenue.max(1)) as f64
    }

    pub fn other_share(&self) -> f64 {
        self.sum.other as f64 / (self.sum.revenue.max(1)) as f64
    }

    fn months_share(&self, count: u32) -> f64 {
        f64::from(count) / f64::from(self.months.max(1))
    }

    pub fn deficit_share(&self) -> f64 {
        self.months_share(self.deficit_months)
    }

    pub fn short_share(&self) -> f64 {
        self.months_share(self.short_months)
    }

    pub fn below_reserve_share(&self) -> f64 {
        self.months_share(self.below_reserve)
    }
}

/// Kopfzeile der Übersichtstabelle.
pub fn print_table_header(first: &str) {
    println!(
        "{first:>6} {:>9} {:>7} {:>7} {:>7} {:>7} {:>7} {:>7} {:>7} {:>7} {:>7} {:>7} {:>7} {:>7} {:>7}",
        "Einn./Mon",
        "Schuld.",
        "Grundl.",
        "Flotte",
        "Besch.",
        "Kontore",
        "Forsch.",
        "Rest",
        "Aussch.",
        "Sonst.",
        "Defizit",
        "gekürzt",
        "u. Rückl.",
        "Rückst."
    );
}

/// Eine Zeile der Übersichtstabelle: Einnahmen je Monat in Credits, die Ansprüche, der
/// Rest, die Ausschüttung und der Saldo der übrigen Bewegungen in Prozent der laufenden
/// Einnahmen; dann der Anteil der Monate mit Defizit, mit gekürzten Ansprüchen und mit
/// einer Kasse unter der Rücklage, und der höchste Soldrückstand.
pub fn print_table_row(first: &str, totals: &BudgetTotals) {
    let percent = |share: f64| format!("{:.1} %", 100.0 * share);
    println!(
        "{first:>6} {:>9.0} {:>7} {:>7} {:>7} {:>7} {:>7} {:>7} {:>7} {:>7} {:>7} {:>7} {:>7} {:>7} {:>7}",
        totals.revenue_per_month(),
        percent(totals.share(Claim::DebtService)),
        percent(totals.share(Claim::Civil)),
        percent(totals.share(Claim::Fleet)),
        percent(totals.share(Claim::Procurement)),
        percent(totals.share(Claim::Markets)),
        percent(totals.share(Claim::Research)),
        percent(totals.remainder_share()),
        percent(totals.share(Claim::Handout)),
        percent(totals.other_share()),
        percent(totals.deficit_share()),
        percent(totals.short_share()),
        percent(totals.below_reserve_share()),
        percent(f64::from(totals.max_arrears))
    );
}

fn print_month(label: &str, month: &BudgetMonth, months: f64) {
    let per_month = |money| to_credits(money) / months;
    println!(
        "    {label}: Einnahmen {:.0}, Sonstiges {:+.0}, Rest nach den Ansprüchen {:+.0}",
        per_month(month.revenue),
        per_month(month.other),
        per_month(month.remainder())
    );
    for claim in Claim::ALL {
        let line = month.line(claim);
        if line.wanted == 0 && line.paid == 0 {
            continue;
        }
        println!(
            "      {:<16} verlangt {:>8.0}  bezahlt {:>8.0}  ({:.1} % der Einnahmen)",
            claim.label(),
            per_month(line.wanted),
            per_month(line.paid),
            100.0 * line.paid as f64 / month.revenue.max(1) as f64
        );
    }
}

/// Die letzten zwölf abgeschlossenen Monate einer Gemeinschaft, je Monat eine Zeile: je
/// Anspruch verlangt/bezahlt in Credits.
pub fn print_months(sim: &Headless, polity: PolityId) {
    let Some(budget) = sim.world().resource::<Economy>().budget(polity) else {
        return;
    };
    println!(
        "    {:<11} {:>7} {:>7} {:>11} {:>11} {:>11} {:>11} {:>11} {:>11} {:>7} {:>7} {:>7} {:>7}",
        "Monat ab",
        "Einn.",
        "Sonst.",
        "Schulden",
        "Grundl.",
        "Flotte",
        "Beschaff.",
        "Kontore",
        "Forschung",
        "Rest",
        "Aussch.",
        "Kasse",
        "Rückl."
    );
    let credits = |money| to_credits(money).round();
    for month in &budget.history {
        let pair = |claim| {
            let line = month.line(claim);
            format!("{}/{}", credits(line.wanted), credits(line.paid))
        };
        println!(
            "    {:<11} {:>7} {:>7} {:>11} {:>11} {:>11} {:>11} {:>11} {:>11} {:>7} {:>7} {:>7} {:>7}{}",
            sim::clock::Date::from_day(month.day).to_string(),
            credits(month.revenue),
            credits(month.other),
            pair(Claim::DebtService),
            pair(Claim::Civil),
            pair(Claim::Fleet),
            pair(Claim::Procurement),
            pair(Claim::Markets),
            pair(Claim::Research),
            credits(month.remainder()),
            credits(month.handout()),
            credits(month.treasury),
            credits(month.reserve_target),
            if month.unfunded > 0 {
                format!("  gekürzt um {}", credits(month.unfunded))
            } else {
                String::new()
            }
        );
    }
}

/// Der Haushalt einer Gemeinschaft: der letzte abgeschlossene Monat und das Mittel der
/// letzten zwölf, in Credits je Monat.
pub fn print_budget(sim: &Headless, polity: PolityId) {
    let Some(budget) = sim.world().resource::<Economy>().budget(polity) else {
        return;
    };
    let Some(last) = budget.last() else {
        return;
    };
    println!(
        "  Haushalt: Kasse nach dem letzten Abschluss {:.0}, Rücklage {:.0}",
        to_credits(last.treasury),
        to_credits(last.reserve_target)
    );
    print_month("letzter Monat", last, 1.0);
    print_month(
        "Mittel der letzten zwölf Monate",
        &budget.year(),
        budget.history.len().max(1) as f64,
    );
}
