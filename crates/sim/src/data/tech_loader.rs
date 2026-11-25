//! Prüfen der Forschungsdaten: Technologiebaum, Starttechnologien, Stellschrauben.

use super::ships::BranchWeights;
use super::{DataError, GameData};

/// Höchste Stufe, die ein Zweig haben darf.
const MAX_TECH_TIER: u32 = 20;

fn invalid(file: &'static str, message: String) -> DataError {
    DataError::Invalid { file, message }
}

/// Läuft nach [`super::ship_loader::resolve_ships`]: Die Voraussetzungen der Technologien
/// sind dann schon in Indizes aufgelöst.
pub(super) fn resolve_research(data: &mut GameData) -> Result<(), DataError> {
    // --- Baum ---
    let file = "techs.ron";
    for tech in &data.techs {
        if tech.tier > MAX_TECH_TIER {
            return Err(invalid(
                file,
                format!(
                    "`{}`: Stufe {} liegt über der Obergrenze {MAX_TECH_TIER}",
                    tech.key, tech.tier
                ),
            ));
        }
        if !tech.cost.is_finite() {
            return Err(invalid(
                file,
                format!("`{}`: Kosten sind keine Zahl", tech.key),
            ));
        }
        for effect in &tech.effects {
            let amount = effect.amount();
            if !amount.is_finite() || amount <= 0.0 {
                return Err(invalid(
                    file,
                    format!(
                        "`{}`: Aufschlag `{}` muss positiv sein",
                        tech.key,
                        effect.label()
                    ),
                ));
            }
        }
    }
    // Ein Ring aus Voraussetzungen wäre nie erforschbar: Jede Technologie muss sich in
    // endlich vielen Schritten aus dem Nichts erreichen lassen.
    let mut reachable = vec![false; data.techs.len()];
    loop {
        let mut changed = false;
        for (index, tech) in data.techs.iter().enumerate() {
            if !reachable[index]
                && tech
                    .requires_ids
                    .iter()
                    .all(|required| reachable[*required as usize])
            {
                reachable[index] = true;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    if let Some(stuck) = reachable.iter().position(|reached| !reached) {
        return Err(invalid(
            file,
            format!(
                "`{}` ist nie erforschbar: Die Voraussetzungen bilden einen Ring",
                data.techs[stuck].key
            ),
        ));
    }

    // --- Starttechnologien der Vorlagen ---
    let file = "polities.ron";
    for index in 0..data.polities.len() {
        let mut ids = Vec::new();
        for key in &data.polities[index].start_techs {
            let Some(tech) = data.tech_index(key) else {
                return Err(invalid(
                    file,
                    format!(
                        "`{}`: unbekannte Technologie `{key}`",
                        data.polities[index].key
                    ),
                ));
            };
            if !ids.contains(&tech) {
                ids.push(tech);
            }
        }
        for tech in &ids {
            for required in &data.techs[*tech as usize].requires_ids {
                if !ids.contains(required) {
                    return Err(invalid(
                        file,
                        format!(
                            "`{}`: `{}` setzt `{}` voraus, das unter start_techs fehlt",
                            data.polities[index].key,
                            data.techs[*tech as usize].key,
                            data.techs[*required as usize].key
                        ),
                    ));
                }
            }
        }
        ids.sort_unstable();
        data.polities[index].start_tech_ids = ids;
    }

    // --- Stellschrauben ---
    let file = "balance.ron";
    let research = &data.balance.research;
    let share = |value: f64| value.is_finite() && (0.0..=1.0).contains(&value);
    if !share(research.max_budget_share) || !share(research.budget_share) {
        return Err(invalid(
            file,
            "research: budget_share und max_budget_share müssen in 0..1 liegen".into(),
        ));
    }
    if research.budget_share > research.max_budget_share {
        return Err(invalid(
            file,
            "research: budget_share liegt über max_budget_share".into(),
        ));
    }
    if !research.efficiency.is_finite() || research.efficiency <= 0.0 {
        return Err(invalid(
            file,
            "research: efficiency muss positiv sein".into(),
        ));
    }
    if !share(research.population_exponent) {
        return Err(invalid(
            file,
            "research: population_exponent muss in 0..1 liegen".into(),
        ));
    }
    if !research.spillover_max.is_finite() || !(0.0..=0.9).contains(&research.spillover_max) {
        return Err(invalid(
            file,
            "research: spillover_max muss in 0..0,9 liegen".into(),
        ));
    }
    if !research.spillover_half_hops.is_finite() || research.spillover_half_hops <= 0.0 {
        return Err(invalid(
            file,
            "research: spillover_half_hops muss positiv sein".into(),
        ));
    }
    for (name, weights) in [
        ("auto_peace", &research.auto_peace),
        ("auto_war", &research.auto_war),
    ] {
        check_weights(name, weights).map_err(|message| invalid(file, message))?;
    }
    let level = |value: f32| value.is_finite() && (0.0..=1.0).contains(&value);
    if !level(research.intel_base)
        || !level(research.intel_ties_max)
        || research.intel_ties_max < research.intel_base
    {
        return Err(invalid(
            file,
            "research: intel_base und intel_ties_max müssen in 0..1 liegen, intel_ties_max \
             nicht unter intel_base"
                .into(),
        ));
    }
    if !research.intel_ties_share.is_finite()
        || research.intel_ties_share <= 0.0
        || research.intel_ties_share > 1.0
    {
        return Err(invalid(
            file,
            "research: intel_ties_share muss über 0 und höchstens bei 1 liegen".into(),
        ));
    }
    if !level(research.intel_decay) {
        return Err(invalid(
            file,
            "research: intel_decay muss in 0..1 liegen".into(),
        ));
    }
    if research.estimate_spread as u32 > MAX_TECH_TIER {
        return Err(invalid(
            file,
            format!("research: estimate_spread darf höchstens {MAX_TECH_TIER} sein"),
        ));
    }
    Ok(())
}

fn check_weights(name: &str, weights: &BranchWeights) -> Result<(), String> {
    let values = [
        weights.economy,
        weights.propulsion,
        weights.weapons,
        weights.defense,
        weights.ground,
    ];
    if values
        .iter()
        .any(|value| !value.is_finite() || *value < 0.0)
        || values.iter().all(|value| *value <= 0.0)
    {
        return Err(format!(
            "research: {name} braucht Gewichte ab 0 und mindestens ein positives"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data() -> GameData {
        GameData::load_workspace().unwrap()
    }

    /// Fehlertext, nachdem `change` die geladenen Daten verändert hat.
    fn error_after(change: impl FnOnce(&mut GameData)) -> String {
        let mut data = data();
        change(&mut data);
        resolve_research(&mut data)
            .expect_err("die Prüfung hätte anschlagen müssen")
            .to_string()
    }

    #[test]
    fn the_shipped_tree_is_sound() {
        let mut data = data();
        assert!(resolve_research(&mut data).is_ok());
        // Jede Stufe setzt genau die vorige ihres Zweigs voraus, und jede kostet mehr.
        for tech in &data.techs {
            for required in &tech.requires_ids {
                let earlier = &data.techs[*required as usize];
                assert_eq!(earlier.branch, tech.branch, "{}", tech.key);
                assert_eq!(earlier.tier + 1, tech.tier, "{}", tech.key);
                assert!(earlier.cost < tech.cost, "{}", tech.key);
            }
            assert_eq!(tech.requires_ids.is_empty(), tech.tier == 1, "{}", tech.key);
            assert!(!tech.effects.is_empty(), "{} bewirkt nichts", tech.key);
        }
        // Jede Vorlage bringt Startwissen mit, das zum Baum passt.
        for template in &data.polities {
            assert_eq!(template.start_tech_ids.len(), template.start_techs.len());
            assert!(!template.start_tech_ids.is_empty(), "{}", template.key);
        }
    }

    #[test]
    fn a_ring_of_prerequisites_is_rejected() {
        let message = error_after(|data| {
            let last = data.tech_index("wirtschaft_6").unwrap();
            data.techs[0].requires_ids.push(last);
        });
        assert!(
            message.contains("techs.ron") && message.contains("Ring"),
            "{message}"
        );
    }

    #[test]
    fn effects_must_be_positive() {
        let message = error_after(|data| {
            data.techs[3].effects = vec![crate::data::TechEffect::Shields(-0.1)];
        });
        assert!(message.contains("Schilde"), "{message}");
        let message = error_after(|data| {
            data.techs[3].effects = vec![crate::data::TechEffect::Yield(f64::NAN)];
        });
        assert!(message.contains("Aufschlag"), "{message}");
    }

    #[test]
    fn starting_technologies_must_exist_and_bring_their_prerequisites() {
        let message = error_after(|data| data.polities[0].start_techs = vec!["nirgends".into()]);
        assert!(
            message.contains("polities.ron") && message.contains("nirgends"),
            "{message}"
        );
        let message = error_after(|data| data.polities[0].start_techs = vec!["waffen_2".into()]);
        assert!(message.contains("waffen_1"), "{message}");
        // Mit der Voraussetzung ist es zulässig; doppelte Angaben zählen einmal.
        let mut data = data();
        data.polities[0].start_techs =
            vec!["waffen_2".into(), "waffen_1".into(), "waffen_1".into()];
        resolve_research(&mut data).unwrap();
        assert_eq!(
            data.polities[0].start_tech_ids,
            vec![
                data.tech_index("waffen_1").unwrap(),
                data.tech_index("waffen_2").unwrap()
            ]
        );
    }

    #[test]
    fn research_balance_is_checked() {
        let cases: Vec<(&str, Box<dyn Fn(&mut GameData)>)> = vec![
            (
                "budget_share",
                Box::new(|data| data.balance.research.budget_share = 1.5),
            ),
            (
                "max_budget_share",
                Box::new(|data| data.balance.research.max_budget_share = 0.01),
            ),
            (
                "efficiency",
                Box::new(|data| data.balance.research.efficiency = 0.0),
            ),
            (
                "population_exponent",
                Box::new(|data| data.balance.research.population_exponent = 1.5),
            ),
            (
                "spillover_max",
                Box::new(|data| data.balance.research.spillover_max = 1.0),
            ),
            (
                "spillover_half_hops",
                Box::new(|data| data.balance.research.spillover_half_hops = 0.0),
            ),
            (
                "auto_war",
                Box::new(|data| data.balance.research.auto_war = BranchWeights::default()),
            ),
            (
                "auto_peace",
                Box::new(|data| data.balance.research.auto_peace.ground = -1.0),
            ),
            (
                "intel_ties_max",
                Box::new(|data| data.balance.research.intel_ties_max = 0.05),
            ),
            (
                "intel_ties_share",
                Box::new(|data| data.balance.research.intel_ties_share = 0.0),
            ),
            (
                "intel_decay",
                Box::new(|data| data.balance.research.intel_decay = 2.0),
            ),
            (
                "estimate_spread",
                Box::new(|data| data.balance.research.estimate_spread = 99),
            ),
        ];
        for (name, change) in cases {
            let message = error_after(change);
            assert!(
                message.contains("balance.ron") && message.contains(name),
                "{name}: {message}"
            );
        }
    }
}
