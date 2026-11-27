//! Eroberung: Aus dem Urteil einer Belagerung wird der Besitzwechsel eines Planeten.
//!
//! Die Eroberung ändert den Besitzer der Welt, nie den Besitz Privater an ihren Betrieben.
//! Ein Vorbesitz des Eroberers mildert die Besatzung, Kriegsschäden belasten sie.

use super::day::{SHOCK_FLOOR, local_issuers, prize, tidy, weary};
use super::{Conquest, DefensePlan, Orbit, Outcome, PlanetDefense, Posture, Sieges, close};
use crate::chronicle::ChronicleKind;
use crate::clock::TICKS_PER_DAY;
use crate::data::SiegeBalance;
use crate::diplomacy::Diplomacy;
use crate::economy::{Economy, Money, pay, to_credits};
use crate::endgame::Path;
use crate::finance::{Finance, Region, Stage, ownership_of_region};
use crate::galaxy::Galaxy;
use crate::ids::{HolderId, PlanetId, PolityId};
use crate::polity::{Polities, Succession, TransferTerms};

/// Schreibt, was die Belagerung über eine Welt weiß, in die Spiegelwerte ihrer Region:
/// Besatzung, wie weit die Garnison sie auffängt, und Kriegsschäden, soweit der heutige
/// Besitzer sie verursacht hat. Daraus rechnet der Zusammenhalt.
pub fn mirror_region(
    balance: &SiegeBalance,
    defense: &PlanetDefense,
    owner: Option<PolityId>,
    population: f64,
    region: &mut Region,
) {
    let needed = balance.hold_k * population.max(0.0).sqrt();
    region.occupation = defense.occupation.clamp(0.0, 1.0);
    region.garrison_cover = if needed > 0.0 {
        (defense.regulars / needed).clamp(0.0, 1.0)
    } else {
        1.0
    };
    region.war_damage = if owner.is_some() && defense.scar_by == owner {
        defense.scar.clamp(0.0, 1.0)
    } else {
        0.0
    };
}

/// Was eine Plünderung eingebracht hat, in Tausendstel Credits.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Loot {
    total: Money,
    /// Der Teil davon aus den Kassen der Firmen: Er schmälert, was ihren Eignern gehört.
    firms: Money,
}

/// Plündert eine eroberte Welt: Der Anteil `sack_share` der Kassen ihrer Haushalte, ihrer
/// Firmen und ihrer Elite geht an die Staatskasse des Eroberers. Geld wird nur verschoben.
fn sack(
    balance: &SiegeBalance,
    economy: &mut Economy,
    finance: &mut Finance,
    planet: PlanetId,
    to: PolityId,
) -> Loot {
    let share = |cash: Money| (cash.max(0) as f64 * balance.sack_share.clamp(0.0, 1.0)) as Money;
    let Economy {
        planets,
        treasuries,
        ..
    } = economy;
    let (Some(local), Some(treasury)) = (
        planets.get_mut(planet.index()),
        treasuries.get_mut(to.index()),
    ) else {
        return Loot::default();
    };
    let mut loot = Loot::default();
    let taken = share(local.households);
    loot.total += pay(&mut local.households, treasury, taken);
    for firm in &mut local.firms {
        let taken = share(firm.cash);
        loot.firms += pay(&mut firm.cash, treasury, taken);
    }
    loot.total += loot.firms;
    if let Some(elite) = finance.elites.get_mut(planet.index()) {
        let taken = share(*elite);
        loot.total += pay(elite, treasury, taken);
    }
    loot
}

/// Vollzieht die Eroberung einer Welt über den gemeinsamen Pfad des Besitzwechsels
/// ([`Succession::transfer_planet`]): War sie die Hauptwelt, zieht die Hauptwelt der
/// Verlierer um; war sie ihre letzte Welt, endet ihre Gemeinschaft, und der Eroberer
/// tritt ihr Erbe unter Zwang an.
fn conquer(succession: &mut Succession, sieges: &mut Sieges, conquest: &Conquest, tick: u64) {
    let data = succession.data;
    let balance = &data.balance.siege;
    let day = tick / TICKS_PER_DAY;
    let (planet, to) = (conquest.planet, conquest.to);
    // Vor dem Wechsel gelesen: Vorbesitz des Eroberers und Dritter, die Betriebe der Welt,
    // die Beute.
    let (foreign, issuers, prize_kept) = {
        let Succession {
            galaxy,
            polities,
            economy,
            finance,
            logistics,
            ..
        } = &*succession;
        let capitals: Vec<Option<PlanetId>> = polities.list.iter().map(|p| p.capital).collect();
        (
            ownership_of_region(finance, galaxy, logistics, &capitals, planet).foreign,
            local_issuers(finance, galaxy, logistics, polities, planet),
            prize(data, galaxy, economy, logistics, planet),
        )
    };
    let stake = foreign.get(&to).copied().unwrap_or(0.0).clamp(0.0, 1.0);
    let defense = sieges.defense(planet);
    let liberated = defense.occupied_from == Some(to);
    let (lowest, highest) = balance.conquest_cohesion;
    let mut cohesion = (balance.conquest_base + balance.stake_pts * stake
        - balance.scar_pts * defense.scar)
        .clamp(lowest, highest);
    if liberated {
        cohesion = cohesion.max(balance.liberation);
    }
    let terms = TransferTerms {
        state_shares_to: HolderId::State(to),
        state_share: 1.0,
        cohesion,
        stage: Stage::Restless,
        shock: balance.conquest_shock,
        elite_flight: balance.elite_flight,
        path: Path::Forced,
    };
    let Ok(transfer) = succession.transfer_planet(planet, to, terms, tick) else {
        // Der Eroberer besteht nicht mehr: Das Urteil verfällt, die Belagerung endet im
        // nächsten Takt von selbst.
        return;
    };
    let old = transfer.from;
    if liberated {
        // Eine zurückgeholte Welt ist wieder eine eigene.
        succession.endgame.tag(planet, Path::Own, 0.0, 0.0);
    }
    let Succession {
        galaxy,
        polities,
        economy,
        finance,
        fleets,
        battles,
        research,
        diplomacy,
        chronicle,
        ..
    } = succession;
    let (galaxy, polities, diplomacy): (&Galaxy, &Polities, &Diplomacy) =
        (galaxy, polities, diplomacy);
    for id in &issuers {
        if let Some(issuer) = finance.issuers.get_mut(id) {
            issuer.shock = issuer.shock.max(SHOCK_FLOOR);
        }
    }
    // Die Schonfrist junger Kolonien schützt auch die Ruinen einer eroberten Welt davor,
    // sofort abgewickelt zu werden.
    economy.planets[planet.index()].founded_day = day;
    // Plünderung, wenn befohlen: nicht nach einer Übergabe, nicht bei einer Befreiung.
    let sacked = conquest.sack && conquest.outcome != Outcome::Capitulated && !liberated;
    let loot = if sacked {
        sack(balance, economy, finance, planet, to)
    } else {
        Loot::default()
    };
    if sacked {
        for id in &issuers {
            if let Some(issuer) = finance.issuers.get_mut(id) {
                issuer.shock = (issuer.shock * balance.sack_shock).max(SHOCK_FLOOR);
            }
        }
        // Dritte, deren Betriebe geplündert wurden, merken es sich.
        for (polity, share) in &foreign {
            let credits = to_credits(loot.firms) * share.clamp(0.0, 1.0);
            if *polity != to && credits > 0.0 {
                *sieges.grievances.entry((to, *polity)).or_default() += credits;
            }
        }
        sieges.stats.sacked += 1;
        sieges.stats.looted += to_credits(loot.total);
    }

    // Der Datensatz endet; die Truppe am Boden wird zur Garnison der Welt.
    let Some(siege) = close(sieges, finance, planet, conquest.outcome, day) else {
        return;
    };
    sieges.assign.retain(|_, target| *target != planet);
    let defense = sieges.defense_mut(planet);
    // Vorräte in Verbandstagen: der Rest des Depots und der Nachschub der Truppe.
    let stores = defense.depot * (defense.militia + defense.regulars) + siege.army_supply;
    defense.regulars = siege.army;
    // Die Miliz der Verlierer löst sich auf; die neue wächst mit dem Zusammenhalt nach.
    defense.militia = 0.0;
    defense.depot = if siege.army > 0.0 {
        (stores / siege.army).min(balance.depot_days_cap as f64)
    } else {
        0.0
    };
    defense.posture = Posture::Forward;
    defense.hold_fire = false;
    if sacked {
        // Die Plünderung zählt zu den Kriegsschäden, die der Eroberer verantwortet.
        defense.scar = (defense.scar + balance.sack_scar).min(1.0);
        defense.scar_by = Some(to);
    }
    if liberated {
        defense.occupation = 0.0;
        defense.occupied_from = None;
        defense.occupied_by = None;
    } else {
        let mercy = if conquest.outcome == Outcome::Capitulated {
            balance.capitulation_mercy
        } else {
            1.0
        };
        defense.occupation = (1.0 - stake).clamp(balance.occupation_min, 1.0) * mercy;
        defense.occupied_from = Some(old);
        defense.occupied_by = Some(to);
    }
    // Anlagen und Vorräte gehen über, wie sie stehen; der Plan nennt genau das.
    defense.plan = DefensePlan {
        stations: defense.stations.ceil() as u8,
        guns: defense.guns.ceil() as u8,
        shield: defense.shield,
        bunkers: defense.bunkers,
        regulars: defense.regulars as f32,
        depot_days: defense.depot.floor() as u16,
    };
    let population = galaxy.planet(planet).population;
    mirror_region(
        balance,
        defense,
        Some(to),
        population,
        &mut finance.regions[planet.index()],
    );
    let mut orbit = Orbit::new(fleets, research);
    if transfer.eliminated {
        // Die Gemeinschaft der Verlierer hat geendet: Was die Belagerung von ihr noch
        // führt (eigene Belagerungen, Zuordnungen und Truppen aufgelöster Flotten),
        // endet im selben Zug.
        sieges.forget(old);
        tidy(
            data, galaxy, polities, diplomacy, finance, sieges, &orbit, chronicle, tick,
        );
        sieges.stats.ended += 1;
    }
    // Wem ein System gehört, kann sich geändert haben: Blockaden neu bestimmen.
    orbit.refresh_blockades(galaxy, diplomacy, battles, sieges);
    weary(finance, to, balance.wear_planet.0);
    if !transfer.eliminated {
        weary(finance, old, balance.wear_planet.1);
    }
    sieges.stats.fallen += 1;
    sieges.stats.prize_start += siege.prize_start;
    sieges.stats.prize_kept += prize_kept;
    let target = galaxy.planet(planet);
    let text = if liberated {
        format!(
            "{} ist {} und kehrt zu {} zurück.",
            target.name,
            conquest.outcome.label(),
            polities.get(to).name
        )
    } else {
        format!(
            "{} ist {} und wird von {} besetzt.",
            target.name,
            conquest.outcome.label(),
            polities.get(to).name
        )
    };
    chronicle.push(
        tick,
        ChronicleKind::War,
        Some(to),
        Some(target.system),
        text,
    );
    if sacked {
        chronicle.push(
            tick,
            ChronicleKind::War,
            Some(to),
            Some(target.system),
            format!(
                "{} plündert {}: {:.0} Credits aus den Kassen der Welt gehen an den Staat.",
                polities.get(to).name,
                target.name,
                to_credits(loot.total)
            ),
        );
    }
}

/// Tagesabschluss: vollzieht die fälligen Besitzwechsel in der Reihenfolge ihrer Urteile.
/// Ein Eintrag gilt nur, solange die Belagerung besteht, aus der er stammt.
pub fn resolve_conquests(succession: &mut Succession, sieges: &mut Sieges, tick: u64) {
    if sieges.pending.is_empty() {
        return;
    }
    for conquest in std::mem::take(&mut sieges.pending) {
        // Eine aufgehobene Belagerung erobert nichts.
        if conquest.outcome == Outcome::Lifted {
            continue;
        }
        let Some(defender) = sieges
            .siege(conquest.planet)
            .filter(|siege| siege.attacker == conquest.to)
            .map(|siege| siege.defender)
        else {
            continue;
        };
        if succession.galaxy.planet(conquest.planet).owner != Some(defender) {
            continue;
        }
        conquer(succession, sieges, &conquest, tick);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::{GameData, Sector};
    use crate::endgame::Path;
    use crate::ids::FleetId;
    use crate::ids::IssuerId;
    use crate::siege::orbit::testing::{FOE, ME, Scene};
    use crate::siege::{Embarked, occupation_force};

    /// Lässt die Belagerung laufen, bis der Planet den Besitzer wechselt, und gibt die
    /// Tage zurück.
    fn until_it_falls(scene: &mut Scene, to: PolityId) -> u32 {
        for days in 1..=scene.data.balance.siege.max_siege_days {
            scene.day();
            if scene.state.galaxy.planet(scene.planet).owner == Some(to) {
                return days;
            }
        }
        panic!("die Welt fiel nicht in der Frist");
    }

    fn embark(scene: &mut Scene, fleet: FleetId, troops: f64) {
        scene.state.sieges.embarked.insert(
            fleet,
            Embarked {
                troops,
                supply: 0.0,
            },
        );
    }

    /// Lässt den Belagerer vorab den Anteil `share` an jedem Betrieb der Welt halten: Von
    /// jedem Halter geht dieser Teil seiner Anteile an den Staat des Belagerers.
    fn give_stake(scene: &mut Scene, share: f64) {
        let state = &mut scene.state;
        let capitals: Vec<_> = state.polities.list.iter().map(|p| p.capital).collect();
        let local: Vec<(IssuerId, Vec<HolderId>)> = crate::finance::issuers_of_region(
            &state.finance,
            &state.galaxy,
            &state.logistics,
            &capitals,
            scene.planet,
        )
        .map(|(id, issuer)| (id, issuer.holdings.keys().copied().collect()))
        .collect();
        for (id, holders) in local {
            for holder in holders {
                crate::finance::shares::move_holdings(
                    &mut state.finance,
                    &[id],
                    holder,
                    HolderId::State(ME),
                    share,
                );
            }
        }
    }

    #[test]
    fn a_prior_stake_thins_the_militia_and_eases_the_conquest() {
        let balance = Scene::colony().data.balance.siege.clone();
        // Zwillinge: dieselbe Kolonie, einmal gehören dem Belagerer vorab 60 % ihrer
        // Betriebe.
        let run = |stake: f64| {
            let mut scene = Scene::colony();
            if stake > 0.0 {
                give_stake(&mut scene, stake);
            }
            let population = scene.state.galaxy.planet(scene.planet).population;
            let needed = occupation_force(population, &balance);
            embark(&mut scene, FleetId(0), needed + 2.0);
            scene.day();
            let siege = scene.siege();
            let (held, militia) = (siege.stake(), siege.garrison_start);
            until_it_falls(&mut scene, ME);
            let region = scene.state.finance.region(scene.planet);
            let occupation = scene.state.sieges.defense(scene.planet).occupation;
            (held, militia, region.cohesion, occupation)
        };
        let (none, militia, cohesion, occupation) = run(0.0);
        let (held, thinned, eased, lighter) = run(0.6);
        assert_eq!(none, 0.0);
        assert!((held - 0.6).abs() < 0.02, "Vorbesitz {held}");
        // Wer an den Betrieben beteiligt ist, dem stellt sich weniger Miliz entgegen.
        assert!(militia > 1.0);
        assert!(
            (thinned / militia - (1.0 - held)).abs() < 1e-6,
            "{thinned} zu {militia}"
        );
        // Die eroberte Welt hält besser zusammen, und die Besatzung lastet leichter.
        assert!(
            eased >= cohesion + 15.0,
            "Zusammenhalt {eased} gegen {cohesion}"
        );
        assert!((lighter / occupation - (1.0 - held)).abs() < 1e-6);
    }

    #[test]
    fn a_starved_colony_changes_its_owner_but_not_its_private_owners() {
        let mut scene = Scene::colony();
        let balance = scene.data.balance.siege.clone();
        let planet = scene.planet;
        let firm = IssuerId::Firm {
            planet,
            sector: Sector::Mining,
        };
        let firms: Vec<IssuerId> = scene
            .state
            .finance
            .issuers
            .keys()
            .filter(|id| matches!(id, IssuerId::Firm { planet: seat, .. } if *seat == planet))
            .copied()
            .collect();
        assert!(!firms.is_empty(), "die Kolonie hat Betriebe an der Börse");
        let firm = if firms.contains(&firm) {
            firm
        } else {
            firms[0]
        };
        let before = scene.state.finance.issuers[&firm].clone();
        let state_shares = before.held_by(HolderId::State(FOE));
        assert!(state_shares > 0);
        let home = scene.state.polities.get(FOE).capital.unwrap();
        let population = scene.state.galaxy.planet(planet).population;
        let needed = occupation_force(population, &balance);
        embark(&mut scene, FleetId(0), needed + 2.0);
        let money = scene.money();
        let weariness = scene.state.finance.polity(FOE).war_weariness;

        let days = until_it_falls(&mut scene, ME);
        assert!(days > 30, "ohne Vorräte hält die Miliz noch Wochen: {days}");
        let state = &scene.state;
        let (galaxy, finance, sieges) = (&state.galaxy, &state.finance, &state.sieges);
        // Der Datensatz ist geschlossen und verbucht, nichts steht mehr aus.
        assert!(sieges.active.is_empty() && sieges.pending.is_empty());
        assert!(sieges.assign.is_empty());
        assert_eq!((sieges.stats.starved, sieges.stats.conquests()), (1, 1));
        assert_eq!(sieges.stats.days, days as u64 - 1);
        assert!(sieges.stats.prize_start > 0.0);
        // Kein Schuss fiel: Die Beute ist heil geblieben.
        assert!(sieges.stats.prize_kept > 0.95 * sieges.stats.prize_start);
        assert_eq!(sieges.stats.lost_attackers, 0.0);

        // Die Region gehört dem Eroberer: unruhig, steuerpflichtig, mit dem Zusammenhalt
        // einer eroberten Welt ohne Vorbesitz und ohne Schäden.
        let region = finance.region(planet);
        assert_eq!(region.owner_seen, Some(ME));
        assert_eq!(region.since_day, scene.day);
        assert_eq!(region.stage, Stage::Restless);
        assert_eq!(region.cohesion, balance.conquest_base);
        assert!(!region.besieged);
        let local = state.economy.planet(planet);
        assert!(!local.tax_exempt);
        assert_eq!(local.founded_day, scene.day);

        // Die Anteile des alten Staates gehören dem neuen, private Halter behalten ihre.
        let issuer = &finance.issuers[&firm];
        assert_eq!(issuer.held_by(HolderId::State(FOE)), 0);
        assert_eq!(issuer.held_by(HolderId::State(ME)), state_shares);
        assert_eq!(issuer.shares, before.shares);
        for holder in [
            HolderId::Elite(home),
            HolderId::Elite(planet),
            HolderId::Public(planet),
        ] {
            assert_eq!(issuer.held_by(holder), before.held_by(holder), "{holder:?}");
        }
        assert!((issuer.shock - before.shock * balance.conquest_shock).abs() < 1e-12);
        // Die Elite der alten Hauptwelt ist jetzt fremder Besitz auf der eroberten Welt.
        let capitals: Vec<_> = state.polities.list.iter().map(|p| p.capital).collect();
        let ownership = ownership_of_region(finance, galaxy, &state.logistics, &capitals, planet);
        assert!(ownership.foreign.get(&FOE).copied().unwrap_or(0.0) > 0.0);

        // Besatzung: voll, weil der Eroberer nichts besaß; die Truppe wird zur Garnison.
        let defense = sieges.defense(planet);
        assert_eq!(defense.occupied_from, Some(FOE));
        assert_eq!(defense.occupation, 1.0);
        assert!((defense.regulars - needed).abs() < 1e-9);
        assert_eq!(defense.militia, 0.0);
        assert_eq!(defense.plan.regulars, defense.regulars as f32);
        assert_eq!(region.occupation, 1.0);
        assert_eq!(region.war_damage, 0.0);
        let cover = balance.occupy_k / balance.hold_k;
        assert!((region.garrison_cover - cover).abs() < 1e-9);
        // Der Rest der Truppen blieb an Bord.
        assert!((sieges.embarked[&FleetId(0)].troops - 2.0).abs() < 1e-9);

        // Die Eroberung hebt die Kriegsmüdigkeit des Verlierers, kein Geld entsteht.
        let wear = finance.polity(FOE).war_weariness - weariness;
        assert!(wear >= balance.wear_planet.1, "{wear}");
        assert_eq!(scene.money(), money);
        let told = state
            .chronicle
            .entries
            .iter()
            .any(|entry| entry.text.contains("ausgehungert") && entry.text.contains("besetzt"));
        assert!(told, "die Chronik nennt die Eroberung");
        assert_eq!(sieges.find_anomaly(galaxy.planets.len()), None);
    }

    #[test]
    fn taking_a_world_back_is_a_liberation() {
        let mut scene = Scene::colony();
        let balance = scene.data.balance.siege.clone();
        let planet = scene.planet;
        embark(&mut scene, FleetId(0), 5.0);
        until_it_falls(&mut scene, ME);
        assert_eq!(scene.state.sieges.defense(planet).occupied_from, Some(FOE));
        // Der Eroberer zieht ab; die Gemeinschaft 1 kommt mit Truppen zurück.
        let home = scene.state.polities.get(ME).capital.unwrap();
        scene.place(0, scene.state.galaxy.planet(home).system);
        scene.place(1, scene.system);
        scene.state.sieges.assign.insert(FleetId(1), planet);
        embark(&mut scene, FleetId(1), 5.0);
        until_it_falls(&mut scene, FOE);
        let defense = scene.state.sieges.defense(planet);
        assert_eq!(defense.occupation, 0.0);
        assert_eq!(defense.occupied_from, None);
        let region = scene.state.finance.region(planet);
        assert!(region.cohesion >= balance.liberation);
        assert_eq!(region.occupation, 0.0);
        assert_eq!(region.stage, Stage::Restless);
        // Der Staat hat seine Anteile an den Betrieben zurück.
        let returned = scene
            .state
            .finance
            .issuers
            .iter()
            .filter(|(id, _)| matches!(id, IssuerId::Firm { planet: seat, .. } if *seat == planet))
            .all(|(_, issuer)| {
                issuer.held_by(HolderId::State(ME)) == 0 && issuer.held_by(HolderId::State(FOE)) > 0
            });
        assert!(returned);
        assert_eq!(scene.state.sieges.stats.conquests(), 2);
        let told = scene
            .state
            .chronicle
            .entries
            .iter()
            .any(|entry| entry.text.contains("kehrt zu"));
        assert!(told, "die Chronik nennt die Rückkehr");
    }

    /// Befiehlt der laufenden Belagerung die Plünderung nach dem Fall.
    fn order_sack(scene: &mut Scene) {
        let planet = scene.planet;
        let orders = crate::siege::SiegeOrders {
            sack: true,
            ..Default::default()
        };
        crate::siege::set_siege_orders(
            &scene.state.galaxy,
            &mut scene.state.sieges,
            ME,
            planet,
            orders,
        )
        .unwrap();
    }

    /// Befiehlt die Plünderung, sobald die Belagerung besteht.
    fn order_sack_when_open(scene: &mut Scene) {
        scene.day();
        order_sack(scene);
    }

    #[test]
    fn a_sacked_world_pays_the_conqueror_and_third_parties_remember() {
        let mut scene = Scene::colony();
        let balance = scene.data.balance.siege.clone();
        let planet = scene.planet;
        let third = PolityId(2);
        // Eine dritte Gemeinschaft hält die Anteile der Elite der Kolonie, und die Elite
        // hat einen Teil des Geldes der Haushalte in ihrer Kasse.
        let firms = local_issuers(
            &scene.state.finance,
            &scene.state.galaxy,
            &scene.state.logistics,
            &scene.state.polities,
            planet,
        );
        for id in &firms {
            let issuer = scene.state.finance.issuers.get_mut(id).unwrap();
            if let Some(shares) = issuer.holdings.remove(&HolderId::Elite(planet)) {
                issuer.holdings.insert(HolderId::State(third), shares);
            }
        }
        let households = scene.state.economy.planet(planet).households;
        assert!(households > 1000);
        scene.state.economy.planets[planet.index()].households -= households / 4;
        scene.state.finance.elites[planet.index()] += households / 4;
        embark(&mut scene, FleetId(0), 5.0);
        scene.day();
        let stake = scene.siege().stakes.get(&third).copied().unwrap_or(0.0);
        assert!(stake > 0.05 && stake < 0.5, "Anteil der Dritten: {stake}");
        order_sack(&mut scene);

        let money = scene.money();
        let treasury = scene.state.economy.treasury(ME);
        let local = scene.state.economy.planet(planet).clone();
        let elite = scene.state.finance.elites[planet.index()];
        let firm_cash: Money = local.firms.iter().map(|firm| firm.cash).sum();
        assert!(local.households > 0 && elite > 0 && firm_cash > 0);
        let shock = scene.state.finance.issuers[&firms[0]].shock;
        until_it_falls(&mut scene, ME);

        // Der Anteil sack_share jeder Kasse der Welt liegt jetzt in der Staatskasse des
        // Eroberers: auf das Tausendstel, und kein Geld ist entstanden oder vergangen.
        let part = |cash: Money| (cash as f64 * balance.sack_share) as Money;
        let state = &scene.state;
        let now = state.economy.planet(planet);
        assert_eq!(now.households, local.households - part(local.households));
        let mut from_firms = 0;
        for (firm, before) in now.firms.iter().zip(&local.firms) {
            assert_eq!(firm.cash, before.cash - part(before.cash));
            from_firms += part(before.cash);
        }
        assert_eq!(state.finance.elites[planet.index()], elite - part(elite));
        let loot = part(local.households) + from_firms + part(elite);
        assert!(loot > 0 && from_firms > 0);
        assert_eq!(state.economy.treasury(ME), treasury + loot);
        assert_eq!(scene.money(), money);
        let stats = &state.sieges.stats;
        assert_eq!(stats.sacked, 1);
        assert!((stats.looted - to_credits(loot)).abs() < 1e-9);
        // Die Plünderung zählt als Kriegsschaden des Eroberers und drückt die Kurse über
        // den Schreck der Eroberung hinaus.
        let defense = state.sieges.defense(planet);
        assert_eq!(defense.scar, balance.sack_scar);
        assert_eq!(defense.scar_by, Some(ME));
        let region = state.finance.region(planet);
        assert_eq!(region.war_damage, balance.sack_scar);
        // Der Zusammenhalt am Tag der Eroberung rechnet mit den Schäden davor; die der
        // Plünderung lasten danach als Kriegsschäden auf ihm.
        assert_eq!(region.cohesion, balance.conquest_base);
        for id in &firms {
            let now = state.finance.issuers[id].shock;
            let expected = shock * balance.conquest_shock * balance.sack_shock;
            assert!((now - expected).abs() < 1e-12);
        }
        // Die Dritte merkt sich, was aus den Kassen ihrer Betriebe genommen wurde. Kein
        // Schuss fiel: Sonst steht nichts in ihrem Vermerk.
        let grievance = state.sieges.grievances[&(ME, third)];
        assert!((grievance - to_credits(from_firms) * stake).abs() < 1e-9);
        assert_eq!(state.sieges.grievances.len(), 1);
        let told = state
            .chronicle
            .entries
            .iter()
            .any(|entry| entry.text.contains("plündert"));
        assert!(told, "die Chronik nennt die Plünderung");
        assert_eq!(state.sieges.find_anomaly(state.galaxy.planets.len()), None);
    }

    #[test]
    fn neither_a_handed_over_nor_a_liberated_world_is_sacked() {
        // Übergabe: Auch ein Urteil, das die Plünderung nennt, plündert nicht.
        let mut scene = Scene::colony();
        let planet = scene.planet;
        embark(&mut scene, FleetId(0), 5.0);
        scene.day();
        let money = scene.money();
        let treasury = scene.state.economy.treasury(ME);
        let needed = occupation_force(
            scene.state.galaxy.planet(planet).population,
            &scene.data.balance.siege,
        );
        scene.state.sieges.active.get_mut(&planet).unwrap().army = needed;
        scene.state.sieges.pending.push(Conquest {
            planet,
            to: ME,
            outcome: Outcome::Capitulated,
            sack: true,
        });
        scene.day();
        assert_eq!(scene.state.galaxy.planet(planet).owner, Some(ME));
        assert_eq!(scene.state.economy.treasury(ME), treasury);
        assert_eq!(scene.money(), money);
        assert_eq!(scene.state.sieges.stats.sacked, 0);
        assert_eq!(scene.state.sieges.defense(planet).scar, 0.0);

        // Befreiung: Wer eine eigene Welt zurückholt, plündert sie nicht, auch auf Befehl.
        let mut scene = Scene::colony();
        let planet = scene.planet;
        embark(&mut scene, FleetId(0), 5.0);
        until_it_falls(&mut scene, ME);
        let home = scene.state.polities.get(ME).capital.unwrap();
        scene.place(0, scene.state.galaxy.planet(home).system);
        scene.place(1, scene.system);
        scene.state.sieges.assign.insert(FleetId(1), planet);
        embark(&mut scene, FleetId(1), 5.0);
        scene.day();
        let orders = crate::siege::SiegeOrders {
            sack: true,
            ..Default::default()
        };
        crate::siege::set_siege_orders(
            &scene.state.galaxy,
            &mut scene.state.sieges,
            FOE,
            planet,
            orders,
        )
        .unwrap();
        let treasury = scene.state.economy.treasury(FOE);
        until_it_falls(&mut scene, FOE);
        assert_eq!(scene.state.economy.treasury(FOE), treasury);
        assert_eq!(scene.state.sieges.stats.sacked, 0);
        assert_eq!(scene.state.sieges.defense(planet).scar, 0.0);
        assert_eq!(scene.state.sieges.stats.conquests(), 2);
    }

    /// Die Hauptwelt der Gemeinschaft 1 ohne Vorräte und Reguläre, die Besatzungstruppe
    /// des Belagerers an Bord.
    fn weaken_the_seat(scene: &mut Scene) {
        let defense = scene.state.sieges.defense_mut(scene.planet);
        defense.depot = 0.0;
        defense.regulars = 0.0;
        embark(scene, FleetId(0), 17.0);
    }

    #[test]
    fn the_fall_of_a_capital_moves_the_seat_to_the_largest_remaining_world() {
        // Die Gemeinschaft 1 hält neben ihrer Hauptwelt eine Kolonie; belagert wird die
        // Hauptwelt.
        let mut scene = Scene::colony();
        let colony = scene.planet;
        let seat = scene.state.polities.get(FOE).capital.unwrap();
        let system = scene.state.galaxy.planet(seat).system;
        scene.state.sieges.assign.insert(FleetId(0), seat);
        scene.place(0, system);
        scene.planet = seat;
        scene.system = system;
        weaken_the_seat(&mut scene);
        let balance = scene.data.balance.siege.clone();
        // Die Kolonie ist autonom: Als Hauptwelt darf sie es nicht bleiben.
        scene.state.finance.regions[colony.index()].stage = Stage::Autonomous;
        scene.state.economy.planets[colony.index()].tax_exempt = true;
        // Was die Elite der Hauptwelt im verbleibenden Gebiet hält: an den Betrieben der
        // Kolonie und am Handelshaus.
        let (old_elite, new_elite) = (HolderId::Elite(seat), HolderId::Elite(colony));
        let remaining = |state: &crate::state::SimState| -> Vec<(IssuerId, u64, u64)> {
            state
                .finance
                .issuers
                .iter()
                .filter(|(id, _)| match id {
                    IssuerId::Firm { planet, .. } => *planet == colony,
                    IssuerId::House(house) => state.logistics.houses[house.index()].polity == FOE,
                    IssuerId::Kontor(_) => false,
                })
                .map(|(id, issuer)| (*id, issuer.held_by(old_elite), issuer.held_by(new_elite)))
                .collect()
        };
        let before = remaining(&scene.state);
        assert!(before.iter().any(|(_, held, _)| *held > 0));
        assert!(
            before
                .iter()
                .any(|(id, held, _)| matches!(id, IssuerId::House(_)) && *held > 0),
            "die Elite der Hauptwelt hält Anteile am Handelshaus"
        );
        let money = scene.money();

        until_it_falls(&mut scene, ME);
        let state = &scene.state;
        // Hauptwelt und Heimathafen des Handelshauses sind umgezogen; die Gemeinschaft lebt.
        let loser = state.polities.get(FOE);
        assert!(loser.alive);
        assert_eq!(loser.capital, Some(colony));
        let home = state.galaxy.planet(colony).system;
        assert!(
            state
                .logistics
                .houses
                .iter()
                .filter(|house| house.polity == FOE)
                .all(|house| house.home == home)
        );
        // Die neue Hauptwelt ist nicht autonom und zahlt Steuern.
        assert_ne!(state.finance.region(colony).stage, Stage::Autonomous);
        assert!(!state.economy.planet(colony).tax_exempt);
        // Die Elite der neuen Hauptwelt gewinnt genau den Anteil elite_flight jedes Postens.
        for ((id, held, had), (_, left, now)) in before.iter().zip(remaining(state)) {
            let moved = (*held as f64 * balance.elite_flight) as u64;
            assert_eq!(now, had + moved, "{id:?}");
            assert_eq!(left, held - moved, "{id:?}");
        }
        // Was der alten Elite bleibt, ist fortan Besitz aus dem Reich des Eroberers.
        let capitals: Vec<_> = state.polities.list.iter().map(|p| p.capital).collect();
        let ownership = ownership_of_region(
            &state.finance,
            &state.galaxy,
            &state.logistics,
            &capitals,
            colony,
        );
        assert!(ownership.foreign.get(&ME).copied().unwrap_or(0.0) > 0.0);
        // Die eroberte Hauptwelt ist besetzt und vermerkt; der Krieg geht weiter.
        assert_eq!(state.galaxy.planet(seat).owner, Some(ME));
        assert_eq!(state.endgame.path_of(seat), Path::Forced);
        assert!(state.diplomacy.at_war(ME, FOE));
        assert_eq!(
            (state.sieges.stats.fallen, state.sieges.stats.ended),
            (1, 0)
        );
        assert_eq!(scene.money(), money);
        assert_eq!(state.sieges.find_anomaly(state.galaxy.planets.len()), None);
    }

    #[test]
    fn the_fall_of_its_last_world_ends_a_community() {
        let mut scene = Scene::new();
        let planet = scene.planet;
        weaken_the_seat(&mut scene);
        order_sack_when_open(&mut scene);
        // Die Verlierer belagern selbst: Auch das endet mit ihnen.
        let elsewhere = scene.state.polities.get(ME).capital.unwrap();
        scene.state.sieges.assign.insert(FleetId(1), elsewhere);
        embark(&mut scene, FleetId(1), 2.0);
        scene.state.sieges.budget.insert(FOE, 5);
        let treasury = scene.state.economy.treasury(FOE);
        assert!(treasury > 0);
        let money = scene.money();
        let mut days = 0;
        while scene.state.galaxy.planet(planet).owner != Some(ME) {
            let before = scene.state.economy.treasury(ME);
            scene.day();
            days += 1;
            // Kein Tag schafft oder vernichtet Geld, auch nicht der der Plünderung.
            assert_eq!(scene.money(), money, "Tag {days}");
            assert!(days < 400, "die Hauptwelt fiel nicht");
            if scene.state.galaxy.planet(planet).owner == Some(ME) {
                // Staatskasse der Verlierer und Plünderung gehen an den Eroberer.
                assert!(scene.state.economy.treasury(ME) >= before + treasury);
            }
        }
        let state = &scene.state;
        let ended = state.polities.get(FOE);
        assert!(!ended.alive);
        assert_eq!(ended.capital, None);
        assert_eq!(state.economy.treasury(FOE), 0);
        // Der Sieger steht nicht mehr im Krieg; Anleihen der Verlierer schuldet er.
        assert!(!state.diplomacy.is_at_war(ME));
        assert!(state.finance.bonds.iter().all(|bond| bond.issuer != FOE));
        // Keine Flotte der Verlierer besteht mehr, und die Belagerung führt nichts mehr
        // von ihnen.
        assert_eq!(state.fleets.of(FOE).count(), 0);
        let sieges = &state.sieges;
        assert!(sieges.active.is_empty() && sieges.pending.is_empty());
        assert!(sieges.assign.is_empty());
        assert!(!sieges.embarked.contains_key(&FleetId(1)));
        assert!(!sieges.budget.contains_key(&FOE));
        assert_eq!((sieges.stats.fallen, sieges.stats.ended), (1, 1));
        assert_eq!(sieges.stats.sacked, 1);
        assert_eq!(state.endgame.path_of(planet), Path::Forced);
        let told = state
            .chronicle
            .entries
            .iter()
            .any(|entry| entry.text.contains("endet") || entry.text.contains("aufgelöst"));
        assert!(told, "die Chronik nennt das Ende der Gemeinschaft");
        assert_eq!(sieges.find_anomaly(state.galaxy.planets.len()), None);
        assert_eq!(scene.fleet_anomaly(), None);
    }

    #[test]
    fn a_verdict_without_its_siege_is_dropped() {
        let mut scene = Scene::colony();
        let planet = scene.planet;
        // Ein Urteil ohne Belagerung, eines für einen anderen als den Belagerer, und eine
        // Aufhebung: Nichts davon erobert etwas.
        scene.day();
        assert!(scene.state.sieges.siege(planet).is_some());
        let entry = |to, outcome| Conquest {
            planet,
            to,
            outcome,
            sack: false,
        };
        scene.state.sieges.pending = vec![
            entry(PolityId(2), Outcome::Starved),
            entry(ME, Outcome::Lifted),
        ];
        scene.day();
        assert!(scene.state.sieges.pending.is_empty());
        assert_eq!(scene.state.galaxy.planet(planet).owner, Some(FOE));
        assert!(scene.state.sieges.siege(planet).is_some());
    }

    #[test]
    fn mirrors_follow_occupation_garrison_and_who_did_the_damage() {
        let balance = GameData::load_workspace().unwrap().balance.siege;
        let mut region = Region::default();
        let mut defense = PlanetDefense {
            occupation: 0.8,
            regulars: 0.5 * balance.hold_k * 10.0,
            scar: 0.3,
            scar_by: Some(ME),
            ..Default::default()
        };
        mirror_region(&balance, &defense, Some(ME), 100.0, &mut region);
        assert_eq!(region.occupation, 0.8);
        assert!((region.garrison_cover - 0.5).abs() < 1e-12);
        assert_eq!(region.war_damage, 0.3);
        // Schäden, die ein anderer verursacht hat, lastet die Welt ihrem Besitzer nicht an.
        mirror_region(&balance, &defense, Some(FOE), 100.0, &mut region);
        assert_eq!(region.war_damage, 0.0);
        // Mehr Truppen als nötig decken nicht mehr als alles.
        defense.regulars = 1000.0;
        mirror_region(&balance, &defense, Some(FOE), 100.0, &mut region);
        assert_eq!(region.garrison_cover, 1.0);
    }
}
