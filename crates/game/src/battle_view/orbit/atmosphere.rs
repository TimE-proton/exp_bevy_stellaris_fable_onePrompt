//! Eigenes Material für den Saum der Atmosphäre (Shader `atmosphere.wesl`, im Programm
//! eingebettet). Ein Shader ist nötig, weil der Saum vom Blickwinkel abhängt: Neben der
//! Scheibe fällt der Dunst nach außen ab, über ihr verdichtet er sich zum Rand hin.

use bevy::asset::embedded_asset;
use bevy::prelude::*;
use bevy::render::render_resource::AsBindGroup;
use bevy::shader::ShaderRef;

const SHADER: &str = "embedded://game/battle_view/orbit/atmosphere.wesl";

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct AtmosphereMaterial {
    /// Farbe (linear) und Stärke.
    #[uniform(0)]
    pub haze: Vec4,
    /// Richtung zur Sonne.
    #[uniform(1)]
    pub sun: Vec4,
    /// Mittelpunkt und Halbmesser des Planeten.
    #[uniform(2)]
    pub body: Vec4,
    /// Halbmesser der Hülle und Höhe, auf die der Dunst abfällt.
    #[uniform(3)]
    pub shape: Vec4,
}

impl Material for AtmosphereMaterial {
    fn fragment_shader() -> ShaderRef {
        SHADER.into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Add
    }

    fn enable_prepass() -> bool {
        false
    }

    fn enable_shadows() -> bool {
        false
    }
}

pub(super) fn plugin(app: &mut App) {
    embedded_asset!(app, "atmosphere.wesl");
    app.add_plugins(MaterialPlugin::<AtmosphereMaterial>::default());
}
