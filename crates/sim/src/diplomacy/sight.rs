//! Was eine Gemeinschaft von fremder Macht sieht, und das Bild einer Lage.
//!
//! Fremde Flotten und Technik kennt niemand genau. Eine Schätzung ist eine Spanne
//! ([`Estimate`]), deren Breite mit dem Einblick (`Research::intel`, aus Verflechtung und
//! Ereignissen) schrumpft und deren Mitte um die Wahrheit streut, monatlich neu gewürfelt
//! (`Domain::Intel`). Das öffentliche Bild rechnet mit dem Grundeinblick
//! (`research.intel_base`), das eigene mit dem eigenen.
//!
//! [`picture`] baut daraus die Lage einer Gemeinschaft gegenüber einer anderen, wie sie
//! sie sieht ([`Perspective::Own`]) oder wie sie öffentlich erscheint
//! ([`Perspective::Public`]). Vertrauen und Einschätzung der anderen kommen dabei aus
//! einem Dossier, nie aus einer verborgenen Haltung.

use super::occasion::{Archetype, Picture};
use super::relations::side;
use super::{Diplomacy, war};
use crate::clock::DAYS_PER_MONTH;
use crate::data::GameData;
use crate::finance::Finance;
use crate::fleet::Fleets;
use crate::galaxy::Galaxy;
use crate::ids::PolityId;
use crate::polity::Polities;
use crate::rng::{Domain, Rng};
use crate::tech::Research;
use serde::{Deserialize, Serialize};

/// Spanne einer Schätzung.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Estimate {
    pub lo: f32,
    pub hi: f32,
}

impl Estimate {
    pub fn exact(value: f32) -> Self {
        Self {
            lo: value,
            hi: value,
        }
    }

    pub fn mid(self) -> f32 {
        0.5 * (self.lo + self.hi)
    }
}

/// Lesender Zugriff auf alles, woraus Schätzungen und Bilder entstehen.
#[derive(Clone, Copy)]
pub struct Ground<'a> {
    pub data: &'a GameData,
    pub galaxy: &'a Galaxy,
    pub polities: &'a Polities,
    pub diplomacy: &'a Diplomacy,
    pub fleets: &'a Fleets,
    pub research: &'a Research,
    pub finance: &'a Finance,
    pub seed: u64,
    pub day: u64,
}

/// Aus wessen Sicht ein Bild entsteht.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Perspective {
    /// Öffentlich: Grundeinblick, öffentliche Bilder der anderen.
    Public,
    /// Die Gemeinschaft selbst: eigener Einblick, eigene Dossiers, eigene Zahlen genau.
    Own,
}

impl<'a> Ground<'a> {
    /// Blick auf eine Welt mit laufender Partie; `None`, wenn keine läuft.
    pub fn of_world(world: &'a bevy_ecs::world::World) -> Option<Self> {
        Some(Ground {
            data: world.get_resource::<GameData>()?,
            galaxy: world.get_resource::<Galaxy>()?,
            polities: world.get_resource::<Polities>()?,
            diplomacy: world.get_resource::<Diplomacy>()?,
            fleets: world.get_resource::<Fleets>()?,
            research: world.get_resource::<Research>()?,
            finance: world.get_resource::<Finance>()?,
            seed: world.get_resource::<crate::state::SimConfig>()?.seed,
            day: world.get_resource::<crate::clock::SimClock>()?.day(),
        })
    }
}

impl Ground<'_> {
    /// Einblick des Betrachters (`None` = öffentlich) in eine Gemeinschaft, 0..1.
    pub fn intel(&self, observer: Option<PolityId>, subject: PolityId) -> f32 {
        let base = self.data.balance.research.intel_base;
        match observer {
            Some(observer) if observer == subject => 1.0,
            Some(observer) => self
                .research
                .intel
                .get(observer.index())
                .and_then(|row| row.get(subject.index()))
                .copied()
                .unwrap_or(base),
            None => base,
        }
        .clamp(0.0, 1.0)
    }

    fn spread(
        &self,
        observer: Option<PolityId>,
        subject: PolityId,
        truth: f32,
        salt: u64,
    ) -> Estimate {
        let intel = self.intel(observer, subject);
        if intel >= 1.0 {
            return Estimate::exact(truth);
        }
        let width = 0.6 * (1.0 - intel);
        let viewer = observer.map_or(0, |id| id.0 as u64 + 1);
        let month = self.day / DAYS_PER_MONTH;
        let mut rng = Rng::stream(
            self.seed,
            Domain::Intel,
            (viewer << 24) | ((subject.0 as u64) << 4) | salt,
            month,
        );
        let center = truth * (1.0 + 0.5 * width * (2.0 * rng.f64() as f32 - 1.0));
        Estimate {
            lo: (center * (1.0 - width)).max(0.0),
            hi: center * (1.0 + width),
        }
    }

    /// Geschätzter Bauwert der Flotten einer Gemeinschaft (Credits).
    pub fn fleet_estimate(&self, observer: Option<PolityId>, subject: PolityId) -> Estimate {
        let truth = super::survey::fleet_value(self.data, self.fleets, subject) as f32;
        self.spread(observer, subject, truth, 1)
    }

    /// Geschätzte Zahl der erforschten Technologien einer Gemeinschaft.
    pub fn tech_estimate(&self, observer: Option<PolityId>, subject: PolityId) -> Estimate {
        let truth = self.research.known_count(subject) as f32;
        self.spread(observer, subject, truth, 2)
    }

    /// Geschätzter Machtanteil einer Gemeinschaft. Die Wirtschaft ist öffentlich, Flotte
    /// und Technik gehen mit der Mitte ihrer Schätzung ein.
    pub fn power_estimate(&self, observer: Option<PolityId>, subject: PolityId) -> f32 {
        let entry = self.diplomacy.surveyed(subject);
        let factor = |estimate: Estimate, truth: f32| {
            if truth > 0.0 {
                estimate.mid() / truth
            } else {
                1.0
            }
        };
        let fleet = factor(self.fleet_estimate(observer, subject), entry.fleet_value);
        let tech = factor(self.tech_estimate(observer, subject), entry.techs);
        0.5 * entry.econ_share + 0.35 * entry.fleet_share * fleet + 0.15 * entry.tech_share * tech
    }

    fn observer(perspective: Perspective, actor: PolityId) -> Option<PolityId> {
        match perspective {
            Perspective::Public => None,
            Perspective::Own => Some(actor),
        }
    }

    /// Einschätzung der Haltung von `subject` aus Sicht von `actor` (eigenes Dossier) oder
    /// öffentlich.
    pub fn belief(&self, perspective: Perspective, actor: PolityId, subject: PolityId) -> [f32; 4] {
        match perspective {
            Perspective::Public => self.diplomacy.public_face_of(subject).belief(),
            Perspective::Own => self.diplomacy.dossier(actor, subject).belief(),
        }
    }

    /// Macht von `actor`, wie sie in dieser Sicht erscheint.
    fn own_power(&self, perspective: Perspective, actor: PolityId) -> f32 {
        match perspective {
            Perspective::Own => self.diplomacy.surveyed(actor).power,
            Perspective::Public => self.power_estimate(None, actor),
        }
    }

    /// Bedrohung von `a` durch `b` in dieser Sicht, 0..1.
    pub fn threat(
        &self,
        perspective: Perspective,
        actor: PolityId,
        a: PolityId,
        b: PolityId,
    ) -> f32 {
        let observer = Self::observer(perspective, actor);
        let own = if observer == Some(a) {
            self.diplomacy.surveyed(a).power
        } else {
            self.power_estimate(observer, a)
        };
        let other = self.power_estimate(observer, b);
        let ratio = (own.max(1e-4) / other.max(1e-4)).ln();
        let belief = if Some(a) == observer {
            self.diplomacy.dossier(a, b).belief()
        } else {
            self.diplomacy.public_face_of(b).belief()
        };
        let menace = self.diplomacy.tension(a, b) / 100.0
            + belief[Archetype::Expansionist.index()]
            + 0.5 * belief[Archetype::Opportunistic.index()];
        (menace * (0.3 - ratio).clamp(0.0, 1.0)).clamp(0.0, 1.0)
    }

    /// Lage von `actor` gegenüber `other` in der gewählten Sicht.
    pub fn picture(&self, perspective: Perspective, actor: PolityId, other: PolityId) -> Picture {
        let observer = Self::observer(perspective, actor);
        let levers = &self.data.diplomacy.levers;
        let me = self.diplomacy.surveyed(actor);
        let them = self.diplomacy.surveyed(other);
        let share_me = self.own_power(perspective, actor);
        let share_other = self.power_estimate(observer, other);
        let ratio = (share_me.max(1e-4) / share_other.max(1e-4)).ln();
        let own_fleet = match perspective {
            Perspective::Own => me.fleet_value,
            Perspective::Public => self.fleet_estimate(None, actor).mid(),
        };
        let foe = self.fleet_estimate(observer, other);
        let hops = match (
            self.polities.get(actor).capital,
            self.polities.get(other).capital,
        ) {
            (Some(a), Some(b)) => {
                let (a, b) = (self.galaxy.planet(a).system, self.galaxy.planet(b).system);
                self.galaxy
                    .hops_from(a)
                    .get(b.index())
                    .copied()
                    .filter(|hops| *hops < u32::MAX)
                    .unwrap_or(12) as f32
            }
            _ => 12.0,
        };
        let relation = self.diplomacy.relation(actor, other);
        let dep = relation.map_or(0.0, |relation| relation.dep[side(actor, other)]);
        let belief = self.belief(perspective, actor, other);
        let trust: f32 = belief
            .iter()
            .zip(&self.data.diplomacy.archetypes)
            .map(|(belief, profile)| belief * profile.honor)
            .sum();
        let others: Vec<PolityId> = self
            .polities
            .alive()
            .map(|polity| polity.id)
            .filter(|id| *id != actor && *id != other)
            .collect();
        let threat_by_other = self.threat(perspective, actor, actor, other);
        let threat = others
            .iter()
            .map(|k| self.threat(perspective, actor, actor, *k))
            .fold(threat_by_other, f32::max);
        let common = others
            .iter()
            .map(|k| {
                self.threat(perspective, actor, actor, *k).min(self.threat(
                    perspective,
                    actor,
                    other,
                    *k,
                ))
            })
            .fold(0.0, f32::max);
        let want = me.want(levers);
        Picture {
            ratio,
            trend_gap: them.trend - me.trend,
            other_leads: share_other > levers.leader_share,
            minor: share_me < levers.minor_share,
            share_me,
            share_other,
            own_fleet,
            other_fleet: (foe.lo, foe.hi),
            hops,
            dep,
            aff: self.diplomacy.affinity(actor, other),
            trust,
            belief_opp: belief[Archetype::Opportunistic.index()],
            belief_exp: belief[Archetype::Expansionist.index()],
            tension: self.diplomacy.tension(actor, other),
            common,
            threat,
            want,
            rungs: self.diplomacy.rungs(actor, other) as f32,
            truce: self.diplomacy.truce_until(actor, other, self.day).is_some(),
            weariness: war::weariness(self.finance, actor) as f32,
            lead: war::lead_of(self.diplomacy, self.finance, levers, actor, other),
        }
    }
}
