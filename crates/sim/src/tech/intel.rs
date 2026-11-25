//! Einblick in fremde Forschung: woher er kommt und was er zeigt.

use super::{Research, TierEstimate};
use crate::data::ResearchBalance;
use crate::finance::{Finance, polity_of_holder, polity_of_issuer};
use crate::galaxy::Galaxy;
use crate::logistics::Logistics;

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

/// Spanne um die wahre Stufe `truth` (0..=`top`). Ihre Weite hängt allein vom Einblick ab:
/// bei 1 ist sie genau, bei 0 misst sie `2 * spread` Stufen. `roll` (0..1) legt fest, wo
/// die Wahrheit in der Spanne liegt. Bei gleichem `roll` enthält die Spanne zu geringerem
/// Einblick stets die zu höherem: Mehr Einblick widerspricht nie dem früheren Bild.
pub fn estimate_tier(truth: u8, top: u8, intel: f32, spread: u8, roll: f64) -> TierEstimate {
    let truth = truth.min(top);
    let widest = 2 * spread as u32;
    let blur = (1.0 - intel as f64).clamp(0.0, 1.0);
    let width = ((blur * widest as f64).ceil() as u32).min(widest);
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
