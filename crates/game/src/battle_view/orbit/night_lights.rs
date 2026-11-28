//! Eigenes Material für die Stadtlichter (Shader `night_lights.wesl`, im Programm
//! eingebettet). Ein Shader ist nötig, weil die Lichter nur auf der Nachtseite leuchten
//! sollen: Das Leuchten des Standardmaterials kennt die Sonne nicht und stünde auch am
//! Tag als gelbe Punkte auf dem Land.

use bevy::asset::embedded_asset;
use bevy::prelude::*;
use bevy::render::render_resource::AsBindGroup;
use bevy::shader::ShaderRef;

const SHADER: &str = "embedded://game/battle_view/orbit/night_lights.wesl";

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct NightLightsMaterial {
    /// Wo Städte stehen (sRGB), äquirektangular wie die Oberfläche.
    #[texture(0)]
    #[sampler(1)]
    pub lights: Handle<Image>,
    /// xyz: Richtung zur Sonne, w: Helligkeit der Lichter.
    #[uniform(2)]
    pub sun: Vec4,
}

impl Material for NightLightsMaterial {
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
    embedded_asset!(app, "night_lights.wesl");
    app.add_plugins(MaterialPlugin::<NightLightsMaterial>::default());
}
