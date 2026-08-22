use bevy::{picking::hover::Hovered, prelude::*, ui::Pressed};

pub(super) fn plugin(app: &mut App) {
    app.register_type::<InteractionPalette>();
    app.add_systems(Update, apply_interaction_palette);
}

/// Palette for widget interactions. Add this to an entity that supports
/// [`Hovered`]/[`Pressed`] states, such as a button, to change its
/// [`BackgroundColor`] based on the current interaction state.
#[derive(Component, Debug, Reflect)]
#[reflect(Component)]
pub struct InteractionPalette {
    pub none: Color,
    pub disabled: Color,
    pub hovered: Color,
    pub pressed: Color,
}

fn apply_interaction_palette(
    mut palette_query: Query<(
        Option<&Hovered>,
        Has<Pressed>,
        &InteractionPalette,
        &mut BackgroundColor,
        Option<&Pickable>,
    )>,
) {
    for (hovered, pressed, palette, mut background, pickable) in &mut palette_query {
        let color = if pickable.is_some_and(|p| !p.should_block_lower) {
            palette.disabled
        } else if pressed {
            palette.pressed
        } else if hovered.is_some_and(|hovered| hovered.0) {
            palette.hovered
        } else {
            palette.none
        };
        background.set_if_neq(BackgroundColor(color));
    }
}
