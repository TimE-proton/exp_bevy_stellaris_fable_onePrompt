use bevy_app::prelude::*;

/// Belagerung in der Partie. Noch ohne Systeme: Der Zustand steht, der Tagesrechner ist
/// eine reine Funktion. Tagesablauf, Beschaffung und Besitzwechsel hängen sich später in
/// `SimSet::Siege` und `SimSet::Aftermath` ein.
pub struct SiegePlugin;

impl Plugin for SiegePlugin {
    fn build(&self, _app: &mut App) {}
}
