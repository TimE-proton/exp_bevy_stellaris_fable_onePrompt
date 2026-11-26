//! Prüfen der Stellschrauben unter `balance.ron: siege`: Schlüssel auflösen, Wertebereiche,
//! und die Schranke, die jede Einschließung enden lässt.

use super::defs::{CohesionBalance, SiegeBalance};
use super::{DataError, GameData};
use crate::ids::GoodId;

const FILE: &str = "balance.ron";

fn invalid(message: String) -> DataError {
    DataError::Invalid {
        file: FILE,
        message,
    }
}

/// Löst Güter und Technologien der Belagerung auf und prüft alle Stellschrauben. Läuft am
/// Ende von [`GameData::resolve_and_validate`]: Güter und Technologien sind dann geladen.
pub(super) fn resolve_siege(data: &mut GameData) -> Result<(), DataError> {
    let good_keys: Vec<&str> = data.goods.iter().map(|good| good.key.as_str()).collect();
    let bundle = |name: &str, list: &[(String, f64)]| -> Result<Vec<(GoodId, f64)>, DataError> {
        if list.is_empty() {
            return Err(invalid(format!("siege.{name} braucht mindestens ein Gut")));
        }
        let mut resolved = Vec::with_capacity(list.len());
        for (key, amount) in list {
            let Some(index) = good_keys.iter().position(|candidate| candidate == key) else {
                return Err(invalid(format!("siege.{name}: unbekanntes Gut `{key}`")));
            };
            if !amount.is_finite() || *amount <= 0.0 {
                return Err(invalid(format!(
                    "siege.{name}: Menge von `{key}` muss positiv sein"
                )));
            }
            resolved.push((GoodId::from_index(index), *amount));
        }
        Ok(resolved)
    };
    let siege = &data.balance.siege;
    let day_bundle = bundle("day_bundle", &siege.day_bundle)?;
    let regular_bundle = bundle("regular_bundle", &siege.regular_bundle)?;
    let station = bundle("station", &siege.station)?;
    let gun = bundle("gun", &siege.gun)?;
    let shield = bundle("shield", &siege.shield)?;
    let bunker = bundle("bunker", &siege.bunker)?;

    let mut gate_ids = [0u16; 4];
    let gates = [
        ("bunker", &siege.gates.bunker),
        ("support", &siege.gates.support),
        ("tunnel", &siege.gates.tunnel),
        ("shield", &siege.gates.shield),
    ];
    for (slot, (name, key)) in gates.into_iter().enumerate() {
        gate_ids[slot] = data.tech_index(key).ok_or_else(|| {
            invalid(format!(
                "siege.gates.{name}: unbekannte Technologie `{key}`"
            ))
        })?;
    }

    check_levers(siege).map_err(invalid)?;
    check_cohesion(&data.balance.finance.cohesion).map_err(invalid)?;

    let siege = &mut data.balance.siege;
    siege.day_bundle_ids = day_bundle;
    siege.regular_bundle_ids = regular_bundle;
    siege.station_ids = station;
    siege.gun_ids = gun;
    siege.shield_ids = shield;
    siege.bunker_ids = bunker;
    siege.gate_ids = gate_ids;
    Ok(())
}

/// Längste Einschließung mit Besatzungstruppe in Tagen: Das größte Depot schwindet auch
/// bei voller Versorgung vom Markt, danach löst sich die Garnison bis zur Aufgabe auf.
pub fn siege_day_bound(siege: &SiegeBalance) -> f64 {
    siege.depot_days_cap as f64 / (1.0 - siege.forage_share)
        + (1.0 / siege.capitulate_frac).ln() / siege.starve_rate
}

/// Die Meldung nennt die Stellschraube.
fn check_levers(siege: &SiegeBalance) -> Result<(), String> {
    // Echte Raten: Null hieße, es geschieht nie etwas, Eins, alles an einem Tag.
    for (name, value) in [
        ("r", siege.r),
        ("zone_fall", siege.zone_fall),
        ("starve_rate", siege.starve_rate),
        ("strand_rate", siege.strand_rate),
        ("capitulate_frac", siege.capitulate_frac),
        ("bombard_cap", siege.bombard_cap),
        ("center_share", siege.center_share),
        ("works_share", siege.works_share),
        ("will_base", siege.will_base),
        ("disrupt_decay", siege.disrupt_decay),
        ("occupation_decay", siege.occupation_decay),
        ("scar_decay", siege.scar_decay),
        ("station_fuel", siege.station_fuel),
        ("buy_share", siege.buy_share),
        ("recruit_per_month", siege.recruit_per_month),
        ("ammo (Stationen)", siege.ammo[0]),
        ("ammo (stützen)", siege.ammo[1]),
        ("ammo (gezielt)", siege.ammo[2]),
        ("ammo (flächig)", siege.ammo[3]),
    ] {
        if !(value.is_finite() && value > 0.0 && value < 1.0) {
            return Err(format!(
                "siege.{name} muss zwischen 0 und 1 liegen (ausschließlich)"
            ));
        }
    }
    // Anteile, die auch ganz fehlen dürfen, aber nie voll sein: Sonst endete das
    // Aushungern nie (forage_share) oder der Wille fiele auf null (will_stake).
    for (name, value) in [
        ("forage_share", siege.forage_share),
        ("will_stake", siege.will_stake),
        ("pop_floor", siege.pop_floor),
        ("landing_loss", siege.landing_loss),
        ("damage_per_loss", siege.damage_per_loss),
        ("sack_share", siege.sack_share),
        ("sack_scar", siege.sack_scar),
    ] {
        if !(value.is_finite() && (0.0..1.0).contains(&value)) {
            return Err(format!(
                "siege.{name} muss in 0..1 liegen und kleiner als 1 sein"
            ));
        }
    }
    // Faktoren und Anteile, die von über 0 bis 1 reichen (1 = ohne Wirkung).
    for (name, value) in [
        ("beach", siege.beach),
        ("unsupplied", siege.unsupplied),
        ("center_shock", siege.center_shock),
        ("army_idle_supply", siege.army_idle_supply),
        ("capitulation_mercy", siege.capitulation_mercy),
        ("sack_shock", siege.sack_shock),
        ("conquest_shock", siege.conquest_shock),
        ("landing_loss_cap", siege.landing_loss_cap),
        ("storm_loss_limit", siege.storm_loss_limit),
        ("budget_share (Frieden)", siege.budget_share.0),
        ("budget_share (Krieg)", siege.budget_share.1),
    ] {
        if !(value.is_finite() && value > 0.0 && value <= 1.0) {
            return Err(format!(
                "siege.{name} muss über 0 und höchstens bei 1 liegen"
            ));
        }
    }
    for (name, value) in [
        ("elite_flight", siege.elite_flight),
        ("intel_spread", siege.intel_spread),
        ("disrupt (umkämpft)", siege.disrupt.0),
        ("disrupt (besetzt)", siege.disrupt.1),
    ] {
        if !(value.is_finite() && (0.0..=1.0).contains(&value)) {
            return Err(format!("siege.{name} muss in 0..1 liegen"));
        }
    }
    for (name, value) in [
        ("station_hp", siege.station_hp),
        ("shield_cap", siege.shield_cap),
        ("shield_regen", siege.shield_regen),
        ("bombard_pop0", siege.bombard_pop0),
        ("support_ref", siege.support_ref),
        ("sortie_ratio", siege.sortie_ratio),
        ("tunnel_front", siege.tunnel_front),
        ("pop_per_strength", siege.pop_per_strength),
        ("hold_k", siege.hold_k),
        ("militia_cohesion (Bezugswert)", siege.militia_cohesion.0),
        ("city_from (eine Stadt)", siege.city_from.0),
    ] {
        if !(value.is_finite() && value > 0.0) {
            return Err(format!("siege.{name} muss positiv sein"));
        }
    }
    for (name, value) in [
        ("station_damage", siege.station_damage),
        ("gun_damage", siege.gun_damage),
        ("gun_depot", siege.gun_depot),
        ("counter_battery", siege.counter_battery),
        ("berth_per_segment", siege.berth_per_segment),
        ("shield_depot", siege.shield_depot),
        ("bombard_k", siege.bombard_k),
        ("under_per_bunker", siege.under_per_bunker),
        ("deep_per_bunker", siege.deep_per_bunker),
        ("bunker_days", siege.bunker_days),
        ("militia_k", siege.militia_k),
        ("regulars_cap", siege.regulars_cap),
        ("occupy_k", siege.occupy_k),
        ("pay", siege.pay),
        ("level_upkeep", siege.level_upkeep),
        ("prize_per_million", siege.prize_per_million),
        ("wear_loss", siege.wear_loss),
        ("wear_fire (Belagerer)", siege.wear_fire.0),
        ("wear_fire (Verteidiger)", siege.wear_fire.1),
        ("wear_portfolio", siege.wear_portfolio),
        ("wear_month (Belagerer)", siege.wear_month.0),
        ("wear_month (Verteidiger)", siege.wear_month.1),
        ("home_start.regulars", siege.home_start.regulars),
        ("precise (militärisch)", siege.precise.0),
        ("precise (Begleitschaden)", siege.precise.1),
        ("area (militärisch)", siege.area.0),
        ("area (Begleitschaden)", siege.area.1),
        ("support_max (ohne Technologie)", siege.support_max.0),
    ] {
        if !(value.is_finite() && value >= 0.0) {
            return Err(format!("siege.{name} darf nicht negativ sein"));
        }
    }
    for (name, value) in [
        ("conquest_base", siege.conquest_base),
        ("stake_pts", siege.stake_pts),
        ("scar_pts", siege.scar_pts),
        ("liberation", siege.liberation),
    ] {
        if !(value.is_finite() && (0.0..=100.0).contains(&value)) {
            return Err(format!("siege.{name} muss in 0..100 liegen"));
        }
    }
    if !siege.wear_planet.0.is_finite() || !siege.wear_planet.1.is_finite() {
        return Err("siege.wear_planet muss aus endlichen Zahlen bestehen".into());
    }

    // Befestigung schwächt den Verteidiger nie.
    if siege
        .fort
        .iter()
        .any(|fort| !(fort.is_finite() && *fort >= 1.0))
    {
        return Err("siege.fort: jeder Befestigungsfaktor muss mindestens 1 sein".into());
    }
    if !(siege.under_base.is_finite() && siege.under_base >= 1.0) {
        return Err("siege.under_base muss mindestens 1 sein".into());
    }
    if !(siege.tunnel_tech.is_finite() && siege.tunnel_tech >= 1.0) {
        return Err("siege.tunnel_tech muss mindestens 1 sein".into());
    }
    if siege
        .gun_weight
        .iter()
        .any(|weight| !(weight.is_finite() && *weight >= 0.0))
        || siege.gun_weight.iter().all(|weight| *weight <= 0.0)
    {
        return Err("siege.gun_weight braucht Gewichte ab 0 und mindestens ein positives".into());
    }
    if !(siege.support_max.1.is_finite() && siege.support_max.1 >= siege.support_max.0) {
        return Err(
            "siege.support_max: der Wert mit Technologie darf nicht unter dem ohne liegen".into(),
        );
    }
    if !(siege.city_from.1.is_finite() && siege.city_from.1 >= siege.city_from.0) {
        return Err(
            "siege.city_from: die zweite Schwelle darf nicht unter der ersten liegen".into(),
        );
    }
    let (_, low, high) = siege.militia_cohesion;
    if !(low.is_finite() && high.is_finite() && low >= 0.0 && high >= low) {
        return Err(
            "siege.militia_cohesion: Schranken ab 0, die obere nicht unter der unteren".into(),
        );
    }
    // Der Rest neben Zentrum und beiden Werken gehört Umland und Städten.
    if siege.center_share + 2.0 * siege.works_share >= 1.0 {
        return Err(
            "siege.center_share und works_share lassen Umland und Städten keinen Anteil".into(),
        );
    }
    // Auch an der Obergrenze der Rate bleibt etwas stehen.
    for (name, (military, collateral)) in [("precise", siege.precise), ("area", siege.area)] {
        if military * siege.bombard_cap >= 1.0 || collateral * siege.bombard_cap >= 1.0 {
            return Err(format!("siege.{name} mal bombard_cap muss unter 1 bleiben"));
        }
    }
    if siege.shield_max.0 > siege.shield_max.1 || siege.bunker_max.0 > siege.bunker_max.1 {
        return Err(
            "siege.shield_max und bunker_max: die Stufe mit Technologie darf nicht unter der \
             ohne liegen"
                .into(),
        );
    }
    if siege.home_start.guns > siege.gun_max {
        return Err("siege.home_start.guns liegt über gun_max".into());
    }
    if siege.depot_days_cap == 0 || siege.home_start.depot_days > siege.depot_days_cap {
        return Err(
            "siege.depot_days_cap muss positiv sein und home_start.depot_days abdecken".into(),
        );
    }

    // Die Schranke, die jede Einschließung mit Besatzungstruppe enden lässt.
    let bound = siege_day_bound(siege);
    if !(bound.is_finite() && bound <= siege.max_siege_days as f64) {
        return Err(format!(
            "siege.max_siege_days ({}) liegt unter der längsten Einschließung: depot_days_cap / \
             (1 − forage_share) + ln(1 / capitulate_frac) / starve_rate = {bound:.0} Tage",
            siege.max_siege_days
        ));
    }
    Ok(())
}

fn check_cohesion(cohesion: &CohesionBalance) -> Result<(), String> {
    for (name, value) in [
        ("occupation", cohesion.occupation),
        ("war_damage", cohesion.war_damage),
    ] {
        if !(value.is_finite() && value >= 0.0) {
            return Err(format!("finance.cohesion.{name} darf nicht negativ sein"));
        }
    }
    if !(cohesion.occupation_hold.is_finite() && (0.0..=1.0).contains(&cohesion.occupation_hold)) {
        return Err("finance.cohesion.occupation_hold muss in 0..1 liegen".into());
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
        resolve_siege(&mut data)
            .expect_err("die Prüfung hätte anschlagen müssen")
            .to_string()
    }

    #[test]
    fn the_shipped_levers_are_sound_and_resolved() {
        let data = data();
        let siege = &data.balance.siege;
        let good = |key: &str| data.good_id(key).unwrap();
        assert_eq!(
            siege
                .day_bundle_ids
                .iter()
                .map(|(id, _)| *id)
                .collect::<Vec<_>>(),
            vec![good("nahrung"), good("munition"), good("treibstoff")]
        );
        assert_eq!(siege.regular_bundle_ids.len(), siege.regular_bundle.len());
        assert_eq!(siege.station_ids[0].0, good("rumpfsegmente"));
        assert_eq!(siege.gun_ids[0].0, good("waffensysteme"));
        assert_eq!(siege.shield_ids[0].0, good("elektronik"));
        assert_eq!(siege.bunker_ids[0].0, good("legierungen"));
        // Die vier Technologien heißen, was die Belagerung von ihnen erwartet.
        let names: Vec<&str> = siege
            .gate_ids
            .iter()
            .map(|id| data.techs[*id as usize].name.as_str())
            .collect();
        assert_eq!(
            names,
            vec![
                "Tiefbunkerbau",
                "Orbitale Feuerleitung",
                "Tunnelkriegführung",
                "Schildgeneratoren"
            ]
        );
        // Die Schranke hält mit den ausgelieferten Werten.
        assert!(siege_day_bound(siege) <= siege.max_siege_days as f64);
    }

    #[test]
    fn unknown_goods_and_technologies_are_reported() {
        let message = error_after(|data| data.balance.siege.day_bundle[0].0 = "luft".into());
        assert!(
            message.contains("balance.ron")
                && message.contains("day_bundle")
                && message.contains("luft"),
            "{message}"
        );
        let message = error_after(|data| data.balance.siege.gun[1].1 = 0.0);
        assert!(
            message.contains("siege.gun") && message.contains("positiv"),
            "{message}"
        );
        let message = error_after(|data| data.balance.siege.bunker.clear());
        assert!(message.contains("siege.bunker"), "{message}");
        let message = error_after(|data| data.balance.siege.gates.tunnel = "boden_9".into());
        assert!(
            message.contains("gates.tunnel") && message.contains("boden_9"),
            "{message}"
        );
    }

    #[test]
    fn the_termination_bound_is_checked_at_load() {
        // Ein größeres Depot ohne längere Frist: Die Schranke reißt.
        let message = error_after(|data| data.balance.siege.depot_days_cap = 400);
        assert!(
            message.contains("max_siege_days") && message.contains("Einschließung"),
            "{message}"
        );
        // Ebenso, wenn die Garnison sich kaum noch auflöst.
        let message = error_after(|data| data.balance.siege.starve_rate = 0.001);
        assert!(message.contains("max_siege_days"), "{message}");
        // Mit längerer Frist ist das größere Depot zulässig.
        let mut data = data();
        data.balance.siege.depot_days_cap = 400;
        data.balance.siege.max_siege_days = 800;
        resolve_siege(&mut data).unwrap();
    }

    #[test]
    fn rates_forts_and_shares_are_checked() {
        let cases: Vec<(&str, Box<dyn Fn(&mut GameData)>)> = vec![
            ("siege.r", Box::new(|data| data.balance.siege.r = 0.0)),
            (
                "starve_rate",
                Box::new(|data| data.balance.siege.starve_rate = 1.0),
            ),
            (
                "strand_rate",
                Box::new(|data| data.balance.siege.strand_rate = -0.1),
            ),
            (
                "capitulate_frac",
                Box::new(|data| data.balance.siege.capitulate_frac = 1.2),
            ),
            (
                "zone_fall",
                Box::new(|data| data.balance.siege.zone_fall = f64::NAN),
            ),
            (
                "forage_share",
                Box::new(|data| data.balance.siege.forage_share = 1.0),
            ),
            (
                "will_stake",
                Box::new(|data| data.balance.siege.will_stake = 1.0),
            ),
            (
                "siege.fort",
                Box::new(|data| data.balance.siege.fort[1] = 0.9),
            ),
            (
                "under_base",
                Box::new(|data| data.balance.siege.under_base = 0.5),
            ),
            (
                "gun_weight",
                Box::new(|data| data.balance.siege.gun_weight = [0.0; 6]),
            ),
            (
                "ammo (flächig)",
                Box::new(|data| data.balance.siege.ammo[3] = 0.0),
            ),
            (
                "support_max",
                Box::new(|data| data.balance.siege.support_max = (0.5, 0.2)),
            ),
            (
                "center_share",
                Box::new(|data| data.balance.siege.works_share = 0.31),
            ),
            (
                "siege.area",
                Box::new(|data| data.balance.siege.area = (30.0, 1.0)),
            ),
            (
                "shield_max",
                Box::new(|data| data.balance.siege.shield_max = (3, 1)),
            ),
            (
                "home_start.guns",
                Box::new(|data| data.balance.siege.home_start.guns = 9),
            ),
            (
                "depot_days_cap",
                Box::new(|data| data.balance.siege.home_start.depot_days = 300),
            ),
            (
                "station_hp",
                Box::new(|data| data.balance.siege.station_hp = 0.0),
            ),
            (
                "militia_k",
                Box::new(|data| data.balance.siege.militia_k = -1.0),
            ),
            (
                "city_from",
                Box::new(|data| data.balance.siege.city_from = (500.0, 100.0)),
            ),
            ("beach", Box::new(|data| data.balance.siege.beach = 0.0)),
            (
                "liberation",
                Box::new(|data| data.balance.siege.liberation = 140.0),
            ),
            (
                "occupation_hold",
                Box::new(|data| data.balance.finance.cohesion.occupation_hold = 1.5),
            ),
            (
                "finance.cohesion.war_damage",
                Box::new(|data| data.balance.finance.cohesion.war_damage = -3.0),
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
