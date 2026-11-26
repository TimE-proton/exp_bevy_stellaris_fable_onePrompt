//! Einblick in fremde Forschung: woher er kommt und was er zeigt.

use super::{Research, TierEstimate, tier_of, top_tier};
use crate::data::{GameData, ResearchBalance, TechBranch};
use crate::finance::{Finance, polity_of_holder, polity_of_issuer};
use crate::galaxy::Galaxy;
use crate::logistics::Logistics;
use crate::rng::{Domain, Rng};

/// Wirtschaftliche Verflechtung: `ties[Halter][Sitz]` ist der Anteil am Marktwert aller
/// Emittenten auf dem Gebiet der Gemeinschaft `Sitz`, den Halter der Gemeinschaft `Halter`
/// besitzen. Wer an den Betrieben der anderen beteiligt ist, sieht in ihre Werkhallen.
pub fn ownership_ties(
    finance: &Finance,
    galaxy: &Galaxy,
    logistics: &Logistics,
    polities: usize,
) -> Vec<Vec<f64>> {
    let mut held = vec![vec![0.0f64; polities]; polities];
    let mut total = vec![0.0f64; polities];
    for (id, issuer) in &finance.issuers {
        let Some(seat) = polity_of_issuer(galaxy, logistics, *id) else {
            continue;
        };
        if seat.index() >= polities {
            continue;
        }
        for (holder, shares) in &issuer.holdings {
            let value = *shares as f64 * issuer.price;
            total[seat.index()] += value;
            if let Some(owner) = polity_of_holder(galaxy, *holder)
                && owner != seat
                && owner.index() < polities
            {
                held[owner.index()][seat.index()] += value;
            }
        }
    }
    for row in &mut held {
        for (seat, value) in row.iter_mut().enumerate() {
            *value = if total[seat] > 0.0 {
                *value / total[seat]
            } else {
                0.0
            };
        }
    }
    held
}

/// Einblick, den ein Besitzanteil `share` an der Wirtschaft der anderen verschafft.
pub fn intel_from_ties(balance: &ResearchBalance, share: f64) -> f32 {
    let reached = (share / balance.intel_ties_share).clamp(0.0, 1.0) as f32;
    if reached >= 1.0 {
        balance.intel_ties_max
    } else {
        balance.intel_base + (balance.intel_ties_max - balance.intel_base) * reached
    }
}

/// Monatlich: Der Einblick folgt der Verflechtung. Was Ereignisse darüber hinaus
/// eingebracht haben, verblasst langsam.
pub fn update_intel(research: &mut Research, ties: &[Vec<f64>], balance: &ResearchBalance) {
    for (viewer, row) in research.intel.iter_mut().enumerate() {
        for (target, intel) in row.iter_mut().enumerate() {
            if viewer == target {
                *intel = 1.0;
                continue;
            }
            let share = ties
                .get(viewer)
                .and_then(|row| row.get(target))
                .copied()
                .unwrap_or(0.0);
            let level = intel_from_ties(balance, share);
            *intel = if *intel > level {
                level + (*intel - level) * (1.0 - balance.intel_decay)
            } else {
                level
            };
        }
    }
}

/// Weite einer Schätzung in Stufen: bei vollem Einblick 0, ohne jeden `2 * spread`.
pub fn estimate_width(intel: f32, spread: u8) -> u8 {
    let widest = 2 * spread as u32;
    // Ein unbrauchbarer Wert gilt als kein Einblick, nie als voller.
    let intel = if intel.is_nan() { 0.0 } else { intel as f64 };
    let blur = (1.0 - intel).clamp(0.0, 1.0);
    ((blur * widest as f64).ceil() as u32).min(widest) as u8
}

/// Einzelne Momentaufnahme: Spanne um die wahre Stufe `truth` (0..=`top`). Ihre Weite
/// hängt allein vom Einblick ab ([`estimate_width`]); `roll` (0..1) legt fest, wo die
/// Wahrheit in der Spanne liegt. Bei gleichem `roll` enthält die Spanne zu geringerem
/// Einblick stets die zu höherem.
///
/// Mehrere Momentaufnahmen mit verschiedenem `roll` verraten zusammen mehr als jede für
/// sich. Das Bild, das eine Gemeinschaft im Spiel von einer anderen hat, ist deshalb
/// gespeicherter Zustand ([`Research::estimate`], [`advance_belief`]) und wird nicht aus
/// dieser Funktion gewürfelt.
pub fn estimate_tier(truth: u8, top: u8, intel: f32, spread: u8, roll: f64) -> TierEstimate {
    let truth = truth.min(top);
    let width = estimate_width(intel, spread) as u32;
    if width == 0 {
        return TierEstimate::exact(truth);
    }
    if width >= top as u32 {
        return TierEstimate { lo: 0, hi: top };
    }
    // Die Wahrheit liegt an zufälliger Stelle im Fenster; an den Rändern des Baums wird
    // es verschoben statt gestutzt, damit seine Weite nichts verrät.
    let below = ((roll.clamp(0.0, 1.0) * (width + 1) as f64) as u32).min(width);
    let lo = (truth as u32).saturating_sub(below).min(top as u32 - width);
    TierEstimate {
        lo: lo as u8,
        hi: (lo + width) as u8,
    }
}

/// Bild ohne Vorgeschichte: die weiteste Spanne der Weite `width`, in der nichts
/// bestätigt ist, was die Weite nicht erzwingt.
pub(super) fn initial_belief(truth: u8, top: u8, width: u8) -> TierEstimate {
    let truth = truth.min(top);
    let lo = truth.saturating_sub(width);
    TierEstimate {
        lo,
        hi: lo.saturating_add(width).min(top),
    }
}

/// Hält die Wahrheit in der Spanne, ohne Zufall: Hat sie die Spanne nach oben verlassen,
/// rückt die Spanne gerade so weit nach. Für Wissen, das außerhalb des Monatsschritts
/// gewährt wird.
pub(super) fn contain(belief: TierEstimate, truth: u8, top: u8) -> TierEstimate {
    let truth = truth.min(top);
    let lo = belief.lo.min(truth);
    let hi = belief.hi.min(top).max(lo);
    if truth > hi {
        TierEstimate {
            lo: lo + (truth - hi),
            hi: truth,
        }
    } else {
        TierEstimate { lo, hi }
    }
}

/// Monatsschritt eines Bildes: `belief` ist die Spanne, die ein Betrachter bisher für die
/// Stufe `truth` (0..=`top`) einer anderen Gemeinschaft hielt, `width` die Weite, die sein
/// Einblick jetzt erlaubt ([`estimate_width`]).
///
/// 1. Hat die Wahrheit die Spanne verlassen, rückt die Spanne gerade so weit nach.
/// 2. Mehr Einblick verengt die Spanne innerhalb der alten: Sie wird oben abgeschnitten,
///    solange die Wahrheit das zulässt (derselbe Einblick zeigt so stets dasselbe Bild,
///    gleich wie oft er kommt und geht); sonst rückt sie an zufälliger Stelle hinauf
///    (`rolls[1]`). Weniger Einblick weitet sie nach oben: Was bestätigt ist, bleibt.
///    Nur am oberen Rand des Baums ist sie deshalb enger, als der Einblick erlaubt.
/// 3. Mit Wahrscheinlichkeit `sighting` bestätigt sich eine weitere Stufe (`rolls[0]`):
///    Die Spanne rückt um eine Stufe hinauf, sofern die Wahrheit über ihrer unteren
///    Grenze liegt und der Baum darüber noch Platz lässt.
///
/// Was der Betrachter daraus schließen kann, steht in der Spanne selbst: Die untere Grenze
/// fällt nie, also hilft kein Mitschreiben. Ein Schritt um eine Stufe kann Fall 1 sein
/// (die Wahrheit liegt jetzt ganz oben) oder Fall 3 (sie liegt irgendwo darunter); im
/// selben Monat geschieht nie beides, also bleibt jede Stufe der Spanne möglich. Ein
/// Fortschritt der anderen innerhalb der Spanne bleibt unbemerkt.
///
/// Eine Ausnahme bleibt, und sie fällt nur mit neuem Einblick an: Rückt die Spanne in
/// Fall 2 hinauf, lag die Wahrheit über dem, was sich oben abschneiden ließ. Wo die
/// engere Spanne das fassen kann, beginnt sie deshalb erst dort; wo nicht (die alte
/// Spanne war weniger als doppelt so weit), zeigt sie mehr Stufen, als wirklich noch in
/// Frage kommen.
pub fn advance_belief(
    belief: TierEstimate,
    truth: u8,
    top: u8,
    width: u8,
    sighting: f64,
    rolls: [f64; 2],
) -> TierEstimate {
    let truth = truth.min(top);
    // Schritt 1 bewegt die Spanne genau dann, wenn die Wahrheit sie verlassen hatte.
    let mut moved = truth > belief.hi;
    let TierEstimate { mut lo, mut hi } = contain(belief, truth, top);

    let current = hi - lo;
    if current > width {
        if truth - lo <= width {
            hi = lo + width;
        } else {
            // Die Wahrheit liegt über `lo + width`: Was darunter liegt, scheidet aus. Passt
            // eine Spanne voller Weite über diese Grenze, beginnt die neue frühestens dort.
            let known_above = lo + width + 1;
            let mut first = truth - width;
            if hi - width >= known_above {
                first = first.max(known_above);
            }
            let last = truth.min(hi - width);
            let choices = (last - first) as u32 + 1;
            let pick = ((rolls[1].clamp(0.0, 1.0) * choices as f64) as u32).min(choices - 1);
            lo = first + pick as u8;
            hi = lo + width;
            moved = true;
        }
    } else if current < width {
        hi = lo.saturating_add(width).min(top);
    }

    if !moved && lo < truth && hi < top && rolls[0] < sighting {
        lo += 1;
        hi += 1;
    }
    TierEstimate { lo, hi }
}

/// Monatlich, nach den Abschlüssen des Monats: Jedes Bild folgt der Wahrheit und dem
/// Einblick ([`advance_belief`]). Die Würfe hängen an Seed, Paar, Zweig und Monat; die
/// Reihenfolge der Gemeinschaften spielt keine Rolle. Teil des Forschungsschritts, nicht
/// für Anzeigen: Die lesen das Bild mit [`Research::estimate`].
pub fn update_beliefs(research: &mut Research, data: &GameData, seed: u64, month: u64) {
    let balance = &data.balance.research;
    let tops = TechBranch::ALL.map(|branch| top_tier(data, branch));
    let tiers: Vec<[u8; 5]> = research
        .polities
        .iter()
        .map(|entry| TechBranch::ALL.map(|branch| tier_of(data, &entry.known, branch)))
        .collect();
    let Research { intel, beliefs, .. } = research;
    for (viewer, row) in beliefs.iter_mut().enumerate() {
        for (target, cell) in row.iter_mut().enumerate() {
            let Some(truth) = tiers.get(target) else {
                continue;
            };
            if viewer == target {
                *cell = truth.map(TierEstimate::exact);
                continue;
            }
            let insight = intel
                .get(viewer)
                .and_then(|row| row.get(target))
                .copied()
                .unwrap_or(0.0);
            let width = estimate_width(insight, balance.estimate_spread);
            for branch in 0..5 {
                let mut rng = Rng::stream(
                    seed,
                    Domain::Intel,
                    (viewer as u64) << 32 | target as u64,
                    month << 8 | branch as u64,
                );
                let rolls = [rng.f64(), rng.f64()];
                cell[branch] = advance_belief(
                    cell[branch],
                    truth[branch],
                    tops[branch],
                    width,
                    balance.sighting_per_month,
                    rolls,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::GameData;

    #[test]
    fn full_insight_is_exact_and_no_insight_is_widest() {
        for truth in 0..=6u8 {
            for roll in [0.0, 0.3, 0.999] {
                assert_eq!(
                    estimate_tier(truth, 6, 1.0, 2, roll),
                    TierEstimate::exact(truth)
                );
                let blind = estimate_tier(truth, 6, 0.0, 2, roll);
                assert_eq!(blind.width(), 4);
                assert!(blind.contains(truth));
            }
        }
        // Ohne Unschärfe ist jede Schätzung genau, gleich wie gering der Einblick.
        assert_eq!(estimate_tier(3, 6, 0.0, 0, 0.5), TierEstimate::exact(3));
        // Ein Baum, der kürzer ist als das Fenster, ist ganz offen.
        assert_eq!(
            estimate_tier(1, 2, 0.0, 2, 0.5),
            TierEstimate { lo: 0, hi: 2 }
        );
    }

    #[test]
    fn estimates_contain_the_truth_and_nest_with_rising_insight() {
        let levels: Vec<f32> = (0..=40).map(|step| step as f32 / 40.0).collect();
        for top in [3u8, 6, 9] {
            for spread in 0..=3u8 {
                for truth in 0..=top {
                    for step in 0..=50 {
                        let roll = step as f64 / 50.0;
                        let mut previous: Option<TierEstimate> = None;
                        for intel in &levels {
                            let estimate = estimate_tier(truth, top, *intel, spread, roll);
                            assert!(
                                estimate.contains(truth) && estimate.hi <= top,
                                "Stufe {truth} von {top}, Einblick {intel}: {estimate:?}"
                            );
                            if let Some(wider) = previous {
                                assert!(
                                    wider.lo <= estimate.lo && estimate.hi <= wider.hi,
                                    "mehr Einblick ({intel}) widerspricht: {wider:?} → {estimate:?}"
                                );
                            }
                            previous = Some(estimate);
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn the_window_keeps_its_width_at_the_edges_of_the_tree() {
        // An den Rändern darf die Weite nichts über die Wahrheit verraten.
        for truth in 0..=6u8 {
            for step in 0..=20 {
                let estimate = estimate_tier(truth, 6, 0.5, 2, step as f64 / 20.0);
                assert_eq!(estimate.width(), 2, "Stufe {truth}: {estimate:?}");
            }
        }
        // Die Wahrheit liegt nicht immer an derselben Stelle der Spanne.
        let offsets: std::collections::BTreeSet<u8> = (0..=20)
            .map(|step| 3 - estimate_tier(3, 6, 0.5, 2, step as f64 / 20.0).lo)
            .collect();
        assert_eq!(offsets.len(), 3);
    }

    #[test]
    fn the_width_follows_insight_alone() {
        // 2 * spread Stufen ohne Einblick, genau bei vollem; dazwischen aufgerundet.
        assert_eq!(estimate_width(0.0, 2), 4);
        assert_eq!(estimate_width(0.15, 2), 4);
        assert_eq!(estimate_width(0.25, 2), 3);
        assert_eq!(estimate_width(0.5, 2), 2);
        assert_eq!(estimate_width(0.8, 2), 1);
        assert_eq!(estimate_width(0.99, 2), 1);
        assert_eq!(estimate_width(1.0, 2), 0);
        assert_eq!(estimate_width(0.3, 0), 0);
        // Unbrauchbare Werte gelten als kein Einblick, nie als voller.
        assert_eq!(estimate_width(f32::NAN, 2), 4);
        assert_eq!(estimate_width(-3.0, 2), 4);
        assert_eq!(estimate_width(7.0, 2), 0);
        let mut last = u8::MAX;
        for step in 0..=100 {
            let width = estimate_width(step as f32 / 100.0, 3);
            assert!(width <= last, "mehr Einblick macht die Spanne nie weiter");
            last = width;
        }
    }

    /// Alle Spannen, die für eine Wahrheit gültig sind.
    fn valid_beliefs(truth: u8, top: u8) -> impl Iterator<Item = TierEstimate> {
        (0..=truth).flat_map(move |lo| (truth..=top).map(move |hi| TierEstimate { lo, hi }))
    }

    const ROLLS: [f64; 5] = [0.0, 0.2, 0.5, 0.8, 0.999];

    #[test]
    fn a_picture_keeps_the_truth_and_never_lowers_what_is_confirmed() {
        for top in [0u8, 1, 3, 6, 9] {
            for truth in 0..=top {
                for before in valid_beliefs(truth, top) {
                    // Die Wahrheit ist seit dem letzten Monat um 0 bis 2 Stufen gestiegen.
                    for now in truth..=(truth + 2).min(top) {
                        for width in 0..=6u8 {
                            for sighting in [0.0, 0.3, 1.0] {
                                for first in ROLLS {
                                    for second in ROLLS {
                                        let after = advance_belief(
                                            before,
                                            now,
                                            top,
                                            width,
                                            sighting,
                                            [first, second],
                                        );
                                        let case = format!(
                                            "{before:?} → {after:?}, Wahrheit {now} von {top}, Weite {width}"
                                        );
                                        assert!(after.contains(now), "{case}");
                                        assert!(after.hi <= top, "{case}");
                                        assert!(after.lo >= before.lo, "{case}");
                                        assert!(after.width() <= width, "{case}");
                                        // Volle Weite, außer am oberen Rand des Baums.
                                        assert!(
                                            after.width() == width || after.hi == top,
                                            "{case}"
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn without_new_insight_the_picture_moves_one_tier_at_most_and_only_upward() {
        for top in [3u8, 6, 9] {
            for width in 0..=4u8 {
                for truth in 0..=top {
                    for before in valid_beliefs(truth, top) {
                        if before.width() != width && before.hi != top {
                            continue;
                        }
                        if before.width() > width {
                            continue;
                        }
                        for now in truth..=(truth + 1).min(top) {
                            for sighting in [0.0, 1.0] {
                                let after =
                                    advance_belief(before, now, top, width, sighting, [0.0, 0.5]);
                                let step = after.lo - before.lo;
                                assert!(step <= 1, "{before:?} → {after:?}");
                                assert_eq!(after.hi - before.hi, step, "{before:?} → {after:?}");
                                // Ohne Sichtung bewegt sich das Bild nur, wenn die Wahrheit
                                // es verlassen hat.
                                if sighting == 0.0 {
                                    assert_eq!(step == 1, now > before.hi);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn progress_inside_the_picture_goes_unnoticed() {
        // Zwei Welten, die sich nur darin unterscheiden, wie weit die andere Gemeinschaft
        // innerhalb der Spanne wirklich ist: Der Betrachter sieht in beiden dasselbe,
        // solange keine Sichtung möglich ist, die es in der anderen nicht wäre.
        let (top, width) = (6u8, 3u8);
        for lo in 0..=(top - width) {
            let before = TierEstimate { lo, hi: lo + width };
            for sighting in [0.0, 1.0] {
                let pictures: Vec<TierEstimate> = (lo + 1..=lo + width)
                    .map(|truth| advance_belief(before, truth, top, width, sighting, [0.0, 0.5]))
                    .collect();
                assert!(
                    pictures.iter().all(|picture| *picture == pictures[0]),
                    "{before:?}: {pictures:?}"
                );
            }
            // Auch ein Aufstieg innerhalb der Spanne ändert nichts.
            let quiet = advance_belief(before, lo + 2, top, width, 0.0, [0.9, 0.9]);
            assert_eq!(quiet, before);
        }
    }

    #[test]
    fn a_step_can_be_progress_or_confirmation_and_looks_the_same() {
        let (top, width) = (6u8, 2u8);
        let before = TierEstimate { lo: 1, hi: 3 };
        // Die andere Gemeinschaft steigt von 3 auf 4: Die Wahrheit verlässt die Spanne.
        let progress = advance_belief(before, 4, top, width, 0.0, [0.9, 0.9]);
        // Sie steht unverändert bei 2 oder 3, und eine weitere Stufe bestätigt sich.
        let confirmed_low = advance_belief(before, 2, top, width, 1.0, [0.0, 0.9]);
        let confirmed_high = advance_belief(before, 3, top, width, 1.0, [0.0, 0.9]);
        assert_eq!(progress, TierEstimate { lo: 2, hi: 4 });
        assert_eq!(confirmed_low, progress);
        assert_eq!(confirmed_high, progress);
        // Nach einem Schritt ist also jede Stufe der neuen Spanne möglich: 2, 3 und 4.
        // Beides zugleich gibt es nicht, sonst verriete ein Doppelschritt die Wahrheit.
        let both = advance_belief(before, 4, top, width, 1.0, [0.0, 0.9]);
        assert_eq!(both, progress);
        // Was genau bekannt ist, wird nicht bestätigt, und am oberen Rand rückt nichts.
        let exact = TierEstimate { lo: 3, hi: 5 };
        assert_eq!(advance_belief(exact, 3, top, width, 1.0, [0.0, 0.0]), exact);
        let edge = TierEstimate { lo: 4, hi: 6 };
        assert_eq!(advance_belief(edge, 6, top, width, 1.0, [0.0, 0.0]), edge);
    }

    #[test]
    fn more_insight_narrows_inside_the_old_picture() {
        for top in [3u8, 6, 9] {
            for truth in 0..=top {
                for before in valid_beliefs(truth, top) {
                    for width in 0..before.width() {
                        for roll in ROLLS {
                            let after = advance_belief(before, truth, top, width, 0.0, [0.9, roll]);
                            assert!(
                                before.lo <= after.lo && after.hi <= before.hi,
                                "mehr Einblick widerspricht: {before:?} → {after:?}"
                            );
                            assert_eq!(after.width(), width);
                            assert!(after.contains(truth));
                            // Solange die Wahrheit es zulässt, wird nur oben abgeschnitten.
                            if truth - before.lo <= width {
                                assert_eq!(after.lo, before.lo);
                            }
                        }
                    }
                }
            }
        }
        // Liegt die Wahrheit über dem, was die engere Spanne von unten fasst, rückt die
        // Spanne hinauf, und zwar nicht immer an dieselbe Stelle.
        let before = TierEstimate { lo: 0, hi: 4 };
        let places: std::collections::BTreeSet<u8> = (0..=20)
            .map(|step| advance_belief(before, 3, 6, 1, 0.0, [0.9, step as f64 / 20.0]).lo)
            .collect();
        assert_eq!(places.into_iter().collect::<Vec<_>>(), vec![2, 3]);
    }

    #[test]
    fn a_narrowed_picture_leaves_every_tier_it_shows_possible() {
        // Für jede alte Spanne und jede engere Weite: Welche Wahrheiten können zu welchem
        // neuen Bild führen? Kommt für ein Bild nicht jede Stufe in Frage, die es zeigt,
        // verrät der Wechsel mehr als das Bild.
        let top = 9u8;
        for old_lo in 0..=3u8 {
            for old_width in 1..=5u8 {
                let before = TierEstimate {
                    lo: old_lo,
                    hi: old_lo + old_width,
                };
                for width in 0..old_width {
                    let mut origins: std::collections::BTreeMap<(u8, u8), Vec<u8>> =
                        std::collections::BTreeMap::new();
                    for truth in before.lo..=before.hi {
                        for step in 0..=40 {
                            let after = advance_belief(
                                before,
                                truth,
                                top,
                                width,
                                0.0,
                                [0.9, step as f64 / 40.0],
                            );
                            let truths = origins.entry((after.lo, after.hi)).or_default();
                            if !truths.contains(&truth) {
                                truths.push(truth);
                            }
                        }
                    }
                    for ((lo, hi), truths) in origins {
                        let shown: Vec<u8> = (lo..=hi).collect();
                        let honest = truths == shown;
                        // Abgeschnitten oder mit genug Platz hinaufgerückt: genau das Bild.
                        if lo == before.lo || old_width > 2 * width {
                            assert!(
                                honest,
                                "{before:?} → {lo}–{hi} bei Weite {width}: möglich {truths:?}"
                            );
                        }
                        // Sonst die genannte Ausnahme: Das Bild zeigt zu viel, nie zu wenig.
                        assert!(!truths.is_empty());
                        assert!(truths.iter().all(|truth| shown.contains(truth)));
                    }
                }
            }
        }
    }

    #[test]
    fn insight_that_comes_and_goes_shows_the_same_picture_again() {
        // Einblick, der um eine Schwelle schwankt, darf das Bild nicht Schritt für Schritt
        // schärfen: Nach dem ersten Mal zeigt derselbe Einblick stets dieselbe Spanne.
        for top in [6u8, 9] {
            for truth in 0..=top {
                for before in valid_beliefs(truth, top) {
                    for (narrow, wide) in [(1u8, 4u8), (2, 3), (0, 2), (3, 4)] {
                        if before.width() != wide && before.hi != top {
                            continue;
                        }
                        let first = advance_belief(before, truth, top, narrow, 0.0, [0.9, 0.7]);
                        let mut picture = first;
                        for cycle in 0..6 {
                            let roll = cycle as f64 / 6.0;
                            let widened =
                                advance_belief(picture, truth, top, wide, 0.0, [0.9, roll]);
                            // Weniger Einblick: Bestätigtes bleibt, nach oben wird es offen.
                            assert_eq!(widened.lo, picture.lo);
                            assert!(widened.hi >= picture.hi);
                            assert!(widened.width() == wide || widened.hi == top);
                            picture = advance_belief(widened, truth, top, narrow, 0.0, [0.9, roll]);
                            assert_eq!(
                                picture, first,
                                "Zyklus {cycle}: {before:?}, Wahrheit {truth}"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn full_insight_is_exact_and_what_was_known_ages() {
        let top = 6u8;
        // Voller Einblick: genau, und jeder Aufstieg wird bemerkt.
        let known = advance_belief(TierEstimate { lo: 0, hi: 4 }, 3, top, 0, 0.0, [0.9, 0.3]);
        assert_eq!(known, TierEstimate::exact(3));
        assert_eq!(
            advance_belief(known, 4, top, 0, 0.0, [0.9, 0.9]),
            TierEstimate::exact(4)
        );
        // Der Einblick lässt nach: Die untere Grenze bleibt, darüber wird es unsicher …
        let faded = advance_belief(known, 3, top, 2, 0.0, [0.9, 0.9]);
        assert_eq!(faded, TierEstimate { lo: 3, hi: 5 });
        // … und Fortschritt innerhalb der Spanne bleibt fortan unbemerkt: Ein Augenblick
        // vollen Einblicks verrät nicht den Rest der Partie.
        assert_eq!(advance_belief(faded, 4, top, 2, 0.0, [0.9, 0.9]), faded);
        assert_eq!(advance_belief(faded, 5, top, 2, 0.0, [0.9, 0.9]), faded);
    }

    #[test]
    fn a_notebook_over_decades_learns_no_more_than_the_picture_shows() {
        // Viele Verläufe über hundert Jahre: Aufstiege und Sichtungen zu zufälligen Zeiten,
        // Einblick fest. Wer jedes Bild mitschreibt, kennt als sichere Grenzen die höchste
        // je gesehene untere und die aktuelle obere; das ist genau die aktuelle Spanne.
        // Und jede Stelle der Spanne kommt als Wahrheit wirklich vor.
        let top = 6u8;
        for width in 1..=4u8 {
            let mut offsets_seen = vec![0u32; width as usize + 1];
            let mut exact_pictures = 0u32;
            for world in 0..400u64 {
                let mut rng = Rng::stream(world, Domain::Intel, width as u64, 0);
                let mut truth = (rng.f64() * 2.0) as u8;
                let mut picture = initial_belief(truth, top, width);
                let mut best_floor = picture.lo;
                for _month in 0..1200 {
                    if truth < top && rng.chance(0.004) {
                        truth += 1;
                    }
                    let rolls = [rng.f64(), rng.f64()];
                    picture = advance_belief(picture, truth, top, width, 0.004, rolls);
                    best_floor = best_floor.max(picture.lo);
                    assert!(picture.contains(truth));
                    assert_eq!(
                        best_floor, picture.lo,
                        "eine frühere untere Grenze lag höher als die jetzige"
                    );
                    if picture.is_exact() {
                        exact_pictures += 1;
                    }
                    if picture.hi < top {
                        offsets_seen[(truth - picture.lo) as usize] += 1;
                    }
                }
            }
            assert_eq!(exact_pictures, 0, "Weite {width}: ein Bild wurde genau");
            assert!(
                offsets_seen.iter().all(|count| *count > 0),
                "Weite {width}: die Wahrheit liegt nie an jeder Stelle der Spanne: {offsets_seen:?}"
            );
        }
    }

    #[test]
    fn ownership_raises_insight_up_to_the_cap() {
        let balance = GameData::load_workspace().unwrap().balance.research;
        assert_eq!(intel_from_ties(&balance, 0.0), balance.intel_base);
        let some = intel_from_ties(&balance, balance.intel_ties_share * 0.5);
        assert!(some > balance.intel_base && some < balance.intel_ties_max);
        assert_eq!(
            intel_from_ties(&balance, balance.intel_ties_share),
            balance.intel_ties_max
        );
        assert_eq!(intel_from_ties(&balance, 1.0), balance.intel_ties_max);
    }

    #[test]
    fn event_insight_fades_back_to_the_level_of_the_ties() {
        let data = GameData::load_workspace().unwrap();
        let balance = &data.balance.research;
        let mut research = Research::default();
        research.sync(&data, 2);
        let (a, b) = (crate::ids::PolityId(0), crate::ids::PolityId(1));
        research.add_intel(a, b, 1.0);
        let none = vec![vec![0.0; 2]; 2];
        let mut last = research.intel(a, b);
        assert_eq!(last, 1.0);
        for _ in 0..12 {
            update_intel(&mut research, &none, balance);
            let now = research.intel(a, b);
            assert!(now < last && now > balance.intel_base, "{last} → {now}");
            last = now;
        }
        // Nach einem Jahr ist noch der größere Teil da, nach Jahrzehnten nichts mehr.
        assert!(last > balance.intel_base + 0.5 * (1.0 - balance.intel_base));
        for _ in 0..600 {
            update_intel(&mut research, &none, balance);
        }
        assert!((research.intel(a, b) - balance.intel_base).abs() < 1e-3);
        // Verflechtung hebt den Einblick sofort und hält ihn.
        let tied = vec![vec![0.0, balance.intel_ties_share], vec![0.0, 0.0]];
        update_intel(&mut research, &tied, balance);
        assert_eq!(research.intel(a, b), balance.intel_ties_max);
        assert!(research.intel(b, a) < balance.intel_ties_max);
        assert_eq!(research.intel(a, a), 1.0);
    }
}
