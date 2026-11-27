//! Der Staatshaushalt im Fenster „Staatsfinanzen“: der letzte abgeschlossene Monat und die
//! Summe der letzten zwölf, Zeile für Zeile so, wie der Kern sie führt. Gerechnet wird
//! hier nichts außer der Differenz „offen“ für die Anzeige.

use super::format::{grouped, percent};
use super::panels::{ACCENT, MUTED, SimRefs, detail};
use bevy_egui::egui::{self, Color32, RichText};
use sim::clock::Date;
use sim::economy::{BudgetMonth, Claim, Money, to_credits};
use sim::ids::PolityId;

/// Kopf der ersten Spalte: der Tag, mit dem der gezeigte Haushaltsmonat begann.
pub const PERIOD: &str = "Haushaltsmonat ab";
pub const HEADING: &str = "Haushalt";
pub const REVENUE: &str = "Einnahmen";
pub const REMAINDER: &str = "Rest der Einnahmen";
pub const ARREARS: &str = "Soldrückstand der Flotte";

const BAD: Color32 = Color32::from_rgb(235, 110, 95);

/// Eine Zeile der Haushaltstafel: verlangt, bezahlt und offen, für den letzten Monat und
/// für die Summe der Monate. Leere Zellen bleiben leer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BudgetRow {
    pub label: String,
    pub cells: [String; 6],
    /// Hat der Anspruch im Monat oder über die Monate nicht alles bekommen?
    pub short: [bool; 2],
    /// Summenzeilen sind hervorgehoben.
    pub strong: bool,
}

fn credits(money: Money) -> String {
    grouped(to_credits(money))
}

/// Die Beschriftung eines Anspruchs mit seinem Rang.
pub fn claim_label(claim: Claim) -> String {
    let rank = Claim::ALL
        .iter()
        .position(|other| *other == claim)
        .unwrap_or(0)
        + 1;
    format!("{rank}. {}", claim.label())
}

/// Die Zeilen der Tafel aus dem letzten Monat und der Summe der Monate.
pub fn budget_rows(month: &BudgetMonth, year: &BudgetMonth) -> Vec<BudgetRow> {
    let sum_row = |label: &str, a: Money, b: Money| BudgetRow {
        label: label.into(),
        cells: [
            String::new(),
            credits(a),
            String::new(),
            String::new(),
            credits(b),
            String::new(),
        ],
        short: [a < 0, b < 0],
        strong: true,
    };
    let claim_row = |claim: Claim| {
        let mut cells: [String; 6] = Default::default();
        let mut short = [false; 2];
        for (index, source) in [month, year].into_iter().enumerate() {
            let line = source.line(claim);
            let open = (line.wanted - line.paid).max(0);
            cells[index * 3] = credits(line.wanted);
            cells[index * 3 + 1] = credits(line.paid);
            // Offen ist nur, was vor Rücklage und Ausschüttung verlangt und nicht bezahlt
            // wurde; so zählt auch der Kern (`BudgetMonth::unpaid`).
            if claim.is_spending() && open > 0 {
                cells[index * 3 + 2] = credits(open);
                short[index] = true;
            }
        }
        BudgetRow {
            label: claim_label(claim),
            cells,
            short,
            strong: false,
        }
    };
    let mut rows = vec![sum_row(REVENUE, month.revenue, year.revenue)];
    rows.extend(
        Claim::ALL
            .into_iter()
            .filter(|claim| claim.is_spending())
            .map(claim_row),
    );
    rows.push(sum_row(REMAINDER, month.remainder(), year.remainder()));
    rows.extend(
        Claim::ALL
            .into_iter()
            .filter(|claim| !claim.is_spending())
            .map(claim_row),
    );
    rows
}

/// Namen der Ansprüche, auf die `wanted` zutrifft, in Rangfolge.
fn claim_names(wanted: impl Fn(Claim) -> bool) -> String {
    let names: Vec<&str> = Claim::ALL
        .into_iter()
        .filter(|claim| wanted(*claim))
        .map(Claim::label)
        .collect();
    names.join(", ")
}

/// Was die Rangfolge bedeutet und was eine Kürzung heißt.
pub fn explanation(sim: &SimRefs) -> String {
    format!(
        "Am Monatsersten bedient die Staatskasse ihre Ansprüche in dieser Reihenfolge aus den \
         Einnahmen des abgeschlossenen Monats. Die Rücklage angreifen dürfen: {}. Die \
         Forschung bekommt nur, was danach von den Einnahmen bleibt oder schon über der \
         Rücklage lag. Reicht das Geld nicht, wird von hinten gekürzt: Die Forschung \
         erarbeitet weniger Punkte; Werften, Nachschub und Instandsetzung warten; bleibt \
         der Flotte Sold aus, wächst ihr Rückstand und drückt die Moral der Besatzungen. \
         Vom Rest füllt der Staat die Rücklage bis zu ihrem Ziel; von dem, was dann noch \
         darüber liegt, gehen {} als Ausschüttung an die Haushalte.",
        claim_names(|claim| claim.is_spending() && claim.may_use_reserve()),
        percent(sim.data.balance.economy.handout_share as f32)
    )
}

/// Über welchen Zeitraum die Tafel berichtet: Ein Haushaltsmonat beginnt mit einem
/// Monatsabschluss und endet mit dem nächsten (`BudgetMonth::day`).
pub fn period_text(began: u64, closed: u64) -> String {
    format!(
        "Die Tafel zeigt den letzten abgeschlossenen Haushaltsmonat, vom {} bis zum Abschluss \
         am {}. Bezahlt wurde er aus den Einnahmen des Monats davor (Zeile „Einnahmen“). Die \
         „Einnahmen des Vormonats“ oben sind jünger: Aus ihnen wird der laufende \
         Haushaltsmonat bezahlt, der am {} begann.",
        Date::from_day(began),
        Date::from_day(closed),
        Date::from_day(closed)
    )
}

/// Der Abschnitt „Haushalt“ des Fensters.
pub fn budget_section(ui: &mut egui::Ui, sim: &SimRefs, player: PolityId) {
    ui.label(RichText::new(HEADING).color(ACCENT));
    let budget = sim.economy.budget(player);
    let Some((budget, month)) = budget.and_then(|budget| Some((budget, budget.last()?))) else {
        ui.label(
            detail("Noch ist kein Monat abgeschlossen; der erste Haushalt steht am Monatsersten.")
                .color(MUTED),
        );
        return;
    };
    let year = budget.year();
    let months = budget.history.len();
    let head = |text: String| RichText::new(text).color(MUTED).small();
    egui::Grid::new("haushalt")
        .striped(true)
        .num_columns(7)
        .show(ui, |ui| {
            ui.label(head(format!("{PERIOD} {}", Date::from_day(month.day))));
            for title in ["verlangt", "bezahlt", "offen"] {
                ui.label(head(title.into()));
            }
            let sum = if months == 1 {
                "seit einem Monat".to_string()
            } else {
                format!("Summe aus {months} Monaten")
            };
            ui.label(head(sum));
            for title in ["bezahlt", "offen"] {
                ui.label(head(title.into()));
            }
            ui.end_row();
            for row in budget_rows(month, &year) {
                let label = RichText::new(&row.label);
                ui.label(if row.strong { label.strong() } else { label });
                for (index, cell) in row.cells.iter().enumerate() {
                    let short = row.short[index / 3];
                    // Die Spalte „offen“ und ein Defizit im Rest sind gezeichnet.
                    let marked = short && (index % 3 == 2 || row.strong);
                    let text = RichText::new(cell).monospace();
                    ui.label(if marked { text.color(BAD) } else { text });
                }
                ui.end_row();
            }
        });
    // Welchen Zeitraum die Tafel meint und aus welchen Einnahmen er bezahlt wurde: Die
    // Zeile „Einnahmen des Vormonats“ darüber gehört schon zum laufenden Haushaltsmonat.
    ui.label(detail(period_text(month.day, budget.current.day)).color(MUTED));
    ui.label(
        detail(format!(
            "Beträge in Credits. Offen ist, was verlangt, aber nicht ausgegeben wurde: weil \
             das Geld fehlte oder der Markt die Ware nicht hergab. Staatskasse nach dem \
             Abschluss {} · Ziel der Rücklage {} · übrige Bewegungen des Monats {} (Anleihen, \
             Notenbank, Anteile, Kolonien)",
            credits(month.treasury),
            credits(month.reserve_target),
            credits(month.other)
        ))
        .color(MUTED),
    );
    if month.unfunded > 0 {
        ui.label(
            detail(format!(
                "Dem letzten Abschluss fehlten {} Credits für die Ansprüche vor der Rücklage.",
                credits(month.unfunded)
            ))
            .color(BAD),
        );
    }
    let arrears = sim.fleets.arrears_of(player);
    ui.label(
        RichText::new(format!("{ARREARS}: {}", percent(arrears))).color(if arrears > 0.005 {
            BAD
        } else {
            MUTED
        }),
    )
    .on_hover_text(
        "Der Anteil des Anspruchs der Flotte (Sold und Ersatzteile), den der Haushalt zuletzt \
         nicht decken konnte, geglättet über die Monate. Er senkt die Moral der Besatzungen \
         im Gefecht.",
    );
    egui::CollapsingHeader::new("Rangfolge und Kürzungen")
        .default_open(true)
        .show(ui, |ui| {
            ui.label(detail(explanation(sim)));
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::TestGame;
    use crate::time_control::SimSpeed;
    use sim::clock::SimClock;
    use sim::economy::{Economy, Line};
    use sim::state::SimConfig;

    #[test]
    fn rows_follow_the_rank_order_and_mark_what_was_not_paid() {
        let mut month = BudgetMonth {
            revenue: 10_000_000,
            ..Default::default()
        };
        let mut set = |claim, wanted, paid| {
            month.lines.insert(claim, Line { wanted, paid });
        };
        set(Claim::Civil, 5_000_000, 5_000_000);
        set(Claim::Fleet, 4_000_000, 3_000_000);
        set(Claim::Defense, 800_000, 500_000);
        set(Claim::Research, 3_000_000, 0);
        set(Claim::Reserve, 2_000_000, 0);
        let rows = budget_rows(&month, &month);
        let labels: Vec<&str> = rows.iter().map(|row| row.label.as_str()).collect();
        assert_eq!(
            labels,
            [
                "Einnahmen",
                "1. Schuldendienst",
                "2. Grundleistungen",
                "3. Flotte",
                "4. Verteidigung",
                "5. Beschaffung",
                "6. Kontore",
                "7. Forschung",
                "Rest der Einnahmen",
                "8. Rücklage",
                "9. Ausschüttung"
            ]
        );
        assert_eq!(rows[0].cells[1], "10.000");
        let fleet = &rows[3];
        assert_eq!(fleet.cells[..3], ["4.000", "3.000", "1.000"]);
        assert_eq!(fleet.short, [true, true]);
        assert_eq!(rows[2].cells[2], "", "voll bezahlt: nichts offen");
        // Die Verteidigung steht im Rang gleich nach der Flotte.
        assert_eq!(rows[4].cells[..3], ["800", "500", "300"]);
        assert_eq!(rows[7].cells[2], "3.000");
        // Der Rest ist, was der Kern dafür nennt.
        assert_eq!(rows[8].cells[1], "1.500");
        // Was der Rücklage fehlt, ist kein unbezahlter Anspruch.
        assert_eq!(rows[9].cells[..3], ["2.000", "0", ""]);
        assert_eq!(rows[9].short, [false, false]);
    }

    /// Alle Texte, die in der Zeile des Textes `label` rechts von ihm stehen, von links
    /// nach rechts.
    fn row_of(game: &mut TestGame, label: &str) -> Vec<String> {
        let labels = game.labels();
        let (_, anchor) = labels
            .iter()
            .find(|(text, _)| text == label)
            .unwrap_or_else(|| panic!("„{label}“ steht nicht im Fenster"));
        let mut cells: Vec<(f32, String)> = labels
            .iter()
            .filter(|(_, rect)| {
                (rect.center().y - anchor.center().y).abs() < 4.0 && rect.left() > anchor.right()
            })
            .map(|(text, rect)| (rect.left(), text.clone()))
            .collect();
        cells.sort_by(|a, b| a.0.total_cmp(&b.0));
        cells.into_iter().map(|(_, text)| text).collect()
    }

    /// Tag des Abschlusses, mit dem der laufende Haushaltsmonat des Spielers begann.
    fn budget_day(game: &TestGame) -> u64 {
        let me = game.resource::<SimConfig>().player.expect("Spieler");
        game.resource::<Economy>()
            .budget(me)
            .expect("Haushalt")
            .current
            .day
    }

    #[test]
    fn the_budget_panel_shows_the_figures_of_the_core() {
        let mut game = TestGame::start("--seed 3");
        game.frames(4);
        game.click_on("Staat");
        game.frames(3);
        assert!(game.text(HEADING).is_some());
        // Vor dem ersten Abschluss gibt es keine Tafel.
        assert_eq!(game.text("3. Flotte"), None);
        // Gut drei Monate spielen.
        *game.world_mut().resource_mut::<SimSpeed>() = SimSpeed {
            paused: false,
            level: 5,
        };
        for _ in 0..4000 {
            if game.resource::<SimClock>().day() >= 100 {
                break;
            }
            game.frames(1);
        }
        game.world_mut().resource_mut::<SimSpeed>().paused = true;
        game.frames(3);
        let me = game.resource::<SimConfig>().player.expect("Spieler");
        let budget = game.resource::<Economy>().budget(me).expect("Haushalt");
        let month = budget.last().expect("ein abgeschlossener Monat").clone();
        let year = budget.year();
        assert!(budget.history.len() >= 3);
        assert!(month.revenue > 0 && year.revenue > month.revenue);
        assert!(month.line(Claim::Fleet).paid > 0, "die Flotte kostet etwas");
        for row in budget_rows(&month, &year) {
            let wanted = row.cells.to_vec();
            assert_eq!(row_of(&mut game, &row.label), wanted, "Zeile {}", row.label);
        }
        // Die Zeile der Flotte nennt die Zahlen des Kerns, nicht irgendwelche.
        let fleet = month.line(Claim::Fleet);
        let shown = row_of(&mut game, "3. Flotte");
        assert_eq!(shown[0], grouped(to_credits(fleet.wanted)));
        assert_eq!(shown[1], grouped(to_credits(fleet.paid)));
        assert_eq!(shown[4], grouped(to_credits(year.line(Claim::Fleet).paid)));
        assert!(game.text(&format!("{ARREARS}: 0 %")).is_some());
        // Die Tafel nennt den Zeitraum, über den sie berichtet: vom Beginn des gezeigten
        // Haushaltsmonats bis zum Abschluss, mit dem der laufende begann.
        let (began, closed) = (month.day, budget_day(&game));
        assert!(began < closed && closed <= game.resource::<SimClock>().day());
        assert!(
            game.text(&format!("{PERIOD} {}", Date::from_day(began)))
                .is_some()
        );
        assert!(game.text(&period_text(began, closed)).is_some());
    }
}
