mod camera;
mod environment;
mod lighting;
mod scene;

use crate::generation::GenerationCoordinator;
use bevy::{ecs::system::SystemParam, prelude::*};

pub(crate) use camera::PreviewViewport;
pub(crate) use lighting::PreviewLighting;

#[derive(Default, Resource)]
pub(crate) struct PreviewDisplayState {
    revision: Option<u64>,
    lighting: Option<PreviewLighting>,
}

#[derive(SystemParam)]
pub(crate) struct PreviewAssets<'w> {
    meshes: ResMut<'w, Assets<Mesh>>,
    images: ResMut<'w, Assets<Image>>,
    materials: ResMut<'w, Assets<StandardMaterial>>,
}

pub(crate) fn setup_preview(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let camera = camera::spawn_cameras(&mut commands);
    scene::spawn_scene(&mut commands, &mut meshes, &mut images, &mut materials);
    environment::spawn_environment(
        &mut commands,
        camera,
        &mut meshes,
        &mut images,
        &mut materials,
    );
    commands.insert_resource(PreviewDisplayState::default());
}

pub(crate) fn update_preview(
    generation: Res<GenerationCoordinator>,
    lighting: Res<PreviewLighting>,
    mut display: ResMut<PreviewDisplayState>,
    scene: Res<scene::PreviewScene>,
    environment: Res<environment::PreviewEnvironment>,
    assets: PreviewAssets,
) {
    let Some(revision) = generation.generated_revision else {
        return;
    };
    let mode = *lighting;
    if display.revision == Some(revision) && display.lighting == Some(mode) {
        return;
    }
    let Some(pack) = generation.resolved.as_ref() else {
        return;
    };
    let PreviewAssets {
        mut meshes,
        mut images,
        mut materials,
    } = assets;
    scene::update_scene(&scene, pack, mode, &mut meshes, &mut images, &mut materials);
    environment::update_environment(&environment, pack, mode, &mut images);
    display.revision = Some(revision);
    display.lighting = Some(mode);
}

pub(crate) use camera::apply_preview_viewport;
