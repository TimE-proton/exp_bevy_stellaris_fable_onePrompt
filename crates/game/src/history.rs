//! Preisverläufe für die Diagramme. Reiner Anzeigezustand: Er gehört nicht zum
//! Spielstand und beeinflusst die Simulation nicht.

use crate::session::SessionEpoch;
use crate::state::AppState;
use bevy::prelude::*;
use sim::clock::SimClock;
use sim::economy::Economy;
use sim::ids::{GoodId, SystemId};
use std::collections::VecDeque;

/// Abstand der Stichproben in Tagen.
pub const SAMPLE_EVERY: u64 = 5;
/// Zahl der gehaltenen Stichproben (rund sechs Jahre).
pub const MAX_SAMPLES: usize = 432;

#[derive(Resource, Default)]
pub struct PriceHistory {
    goods: usize,
    pub days: VecDeque<u64>,
    /// Je Stichprobe alle Preise, geordnet nach System und Gut.
    samples: VecDeque<Vec<f32>>,
    epoch: Option<SessionEpoch>,
}

impl PriceHistory {
    pub fn record(&mut self, day: u64, economy: &Economy) {
        self.goods = economy
            .markets
            .first()
            .map_or(0, |market| market.price.len());
        if self.days.len() >= MAX_SAMPLES {
            self.days.pop_front();
            self.samples.pop_front();
        }
        self.days.push_back(day);
        self.samples.push_back(
            economy
                .markets
                .iter()
                .flat_map(|market| market.price.iter().map(|price| *price as f32))
                .collect(),
        );
    }

    /// Preisverlauf eines Guts in einem System als (Tag, Preis).
    pub fn series(&self, system: SystemId, good: GoodId) -> Vec<[f64; 2]> {
        let index = system.index() * self.goods + good.index();
        self.days
            .iter()
            .zip(&self.samples)
            .filter_map(|(day, sample)| sample.get(index).map(|price| [*day as f64, *price as f64]))
            .collect()
    }

    fn clear(&mut self) {
        self.days.clear();
        self.samples.clear();
    }
}

pub struct HistoryPlugin;

impl Plugin for HistoryPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PriceHistory>()
            .add_systems(Update, sample.run_if(in_state(AppState::InGame)));
    }
}

fn sample(
    epoch: Res<SessionEpoch>,
    clock: Res<SimClock>,
    economy: Res<Economy>,
    mut history: ResMut<PriceHistory>,
) {
    if history.epoch != Some(*epoch) {
        history.clear();
        history.epoch = Some(*epoch);
    }
    let day = clock.day();
    let due = history
        .days
        .back()
        .is_none_or(|last| day >= last + SAMPLE_EVERY || day < *last);
    if due {
        if history.days.back().is_some_and(|last| day < *last) {
            // Ein älterer Spielstand wurde geladen.
            history.clear();
        }
        history.record(day, &economy);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim::state::{SimConfig, SimState};

    #[test]
    fn history_is_bounded_and_addressable() {
        let data = sim::GameData::load_workspace().unwrap();
        let state = SimState::generate(
            &data,
            SimConfig {
                seed: 2,
                player: None,
            },
        );
        let mut history = PriceHistory::default();
        for step in 0..(MAX_SAMPLES as u64 + 20) {
            history.record(step * SAMPLE_EVERY, &state.economy);
        }
        assert_eq!(history.days.len(), MAX_SAMPLES);
        let good = data.good_id("nahrung").unwrap();
        let series = history.series(SystemId(0), good);
        assert_eq!(series.len(), MAX_SAMPLES);
        assert!((series[0][1] - data.good(good).base_price).abs() < 1e-3);
    }
}
