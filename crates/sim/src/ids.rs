//! Stabile, fortlaufende Kennungen. Alle Zustandslisten sind über diese Indizes
//! adressiert; nichts hängt an Speicheradressen oder Hash-Reihenfolgen.

use crate::data::Sector;
use serde::{Deserialize, Serialize};
use std::fmt;

macro_rules! id_type {
    ($(#[$meta:meta])* $name:ident, $prefix:literal) => {
        $(#[$meta])*
        #[derive(
            Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default, Serialize, Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(pub u32);

        impl $name {
            #[inline]
            pub const fn index(self) -> usize {
                self.0 as usize
            }
            #[inline]
            pub const fn from_index(index: usize) -> Self {
                Self(index as u32)
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, concat!($prefix, "{}"), self.0)
            }
        }
    };
}

id_type!(
    /// Sternsystem in [`crate::galaxy::Galaxy::systems`].
    SystemId, "S"
);
id_type!(
    /// Sprungroute in [`crate::galaxy::Galaxy::lanes`].
    LaneId, "L"
);
id_type!(
    /// Planet in [`crate::galaxy::Galaxy::planets`].
    PlanetId, "P"
);
id_type!(
    /// Galaktische Gemeinschaft in [`crate::polity::Polities::list`].
    PolityId, "G"
);
id_type!(
    /// Gut aus `goods.ron`, Index in [`crate::data::GameData::goods`].
    GoodId, "W"
);
id_type!(
    /// Flotte in [`crate::fleet::Fleets::list`].
    FleetId, "F"
);
id_type!(
    /// Frachter in [`crate::logistics::Logistics::freighters`].
    FreighterId, "T"
);
id_type!(
    /// Handelshaus in [`crate::logistics::Logistics::houses`].
    HouseId, "H"
);
id_type!(
    /// Schiff einer Flotte ([`crate::fleet::Fleet::ships`]), fortlaufend über die ganze Partie.
    ShipId, "K"
);
id_type!(
    /// Gefecht, fortlaufend über die ganze Partie.
    BattleId, "B"
);

/// Wer Anteile ausgibt: Firmen und Werften je Planet und Zweig, Handelshäuser, und das
/// Kontor eines Systems als Kolonialgesellschaft.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize)]
pub enum IssuerId {
    Firm { planet: PlanetId, sector: Sector },
    House(HouseId),
    Kontor(SystemId),
}

/// Wer Anteile und Anleihen hält: der Staat, die Elite einer Region, der Bevölkerungsfonds
/// einer Region. Zu welcher Gemeinschaft eine Region gehört, folgt dem Besitz des
/// Planeten: Wechselt er, werden seine Halter für das alte Reich zu Fremden.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize)]
pub enum HolderId {
    State(PolityId),
    Elite(PlanetId),
    Public(PlanetId),
}
