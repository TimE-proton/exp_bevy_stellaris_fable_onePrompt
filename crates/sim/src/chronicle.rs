//! Chronik: was in der Galaxie geschah. Quelle für Ereignislog und Headless-Ausgabe.

use crate::ids::{PolityId, SystemId};
use bevy_ecs::prelude::Resource;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChronicleKind {
    Info,
    Colony,
    Economy,
    Finance,
    Cohesion,
    Tech,
    War,
    Diplomacy,
    End,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChronicleEntry {
    pub tick: u64,
    pub kind: ChronicleKind,
    pub polity: Option<PolityId>,
    pub system: Option<SystemId>,
    pub text: String,
}

#[derive(Resource, Clone, Debug, Default, Serialize, Deserialize)]
pub struct Chronicle {
    /// Die jüngsten Einträge, älteste zuerst.
    pub entries: VecDeque<ChronicleEntry>,
    /// Zahl aller je geschriebenen Einträge.
    pub total: u64,
}

impl Chronicle {
    /// So viele Einträge bleiben im Zustand; ältere fallen heraus.
    pub const CAPACITY: usize = 400;

    pub fn push(
        &mut self,
        tick: u64,
        kind: ChronicleKind,
        polity: Option<PolityId>,
        system: Option<SystemId>,
        text: impl Into<String>,
    ) {
        if self.entries.len() >= Self::CAPACITY {
            self.entries.pop_front();
        }
        self.entries.push_back(ChronicleEntry {
            tick,
            kind,
            polity,
            system,
            text: text.into(),
        });
        self.total += 1;
    }
}
