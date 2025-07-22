//! An example showing a very basic implementation.

use bevy::prelude::*;
use bevy_simple_text_input::{
    TextInput, TextInputPlugin, TextInputPointerAction, TextInputPointerEvent,
    TextInputSubmitEvent, TextInputSystem, TextInputTextColor, TextInputTextFont,
};

const BORDER_COLOR_ACTIVE: Color = Color::srgb(0.75, 0.52, 0.99);
const TEXT_COLOR: Color = Color::srgb(0.9, 0.9, 0.9);
const BACKGROUND_COLOR: Color = Color::srgb(0.15, 0.15, 0.15);

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(TextInputPlugin)
        .add_systems(Startup, setup)
        .add_systems(Update, listener.after(TextInputSystem))
        .add_systems(Update, send_mouse)
        .run();
}

fn setup(mut commands: Commands) {
    commands.spawn(Camera2d);

    commands
        .spawn(Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            flex_direction: FlexDirection::Column,
            ..default()
        })
        .with_children(|parent| {
            parent.spawn(Text::new("line above"));
            parent.spawn((
                Node {
                    width: Val::Px(200.0),
                    border: UiRect::all(Val::Px(5.0)),
                    padding: UiRect::all(Val::Px(5.0)),
                    ..default()
                },
                BorderColor(BORDER_COLOR_ACTIVE),
                BackgroundColor(BACKGROUND_COLOR),
                TextInput,
                TextInputTextFont(TextFont {
                    font_size: 34.,
                    ..default()
                }),
                TextInputTextColor(TextColor(TEXT_COLOR)),
            ));
            parent.spawn(Text::new("line below"));
        });
}

fn listener(mut events: EventReader<TextInputSubmitEvent>) {
    for event in events.read() {
        info!("{:?} submitted: {}", event.entity, event.value);
    }
}

fn send_mouse(
    window: Query<&Window>,
    button: Res<ButtonInput<MouseButton>>,
    mut was_pressed: Local<bool>,
    mut prev_pos: Local<Vec2>,
    mut pointer: EventWriter<TextInputPointerEvent>,
    interaction_check: Query<&Interaction, With<TextInput>>,
) {
    let just_pressed = button.just_pressed(MouseButton::Left);
    let still_pressed = *was_pressed && button.pressed(MouseButton::Left);

    let position = window
        .single()
        .unwrap()
        .cursor_position()
        .unwrap_or_default();

    if just_pressed && interaction_check.single().unwrap() != &Interaction::None {
        pointer.write(TextInputPointerEvent {
            position,
            action: TextInputPointerAction::Press,
        });
    } else if still_pressed && *prev_pos != position {
        pointer.write(TextInputPointerEvent {
            position,
            action: TextInputPointerAction::Drag,
        });
    } else if *was_pressed && !still_pressed {
        pointer.write(TextInputPointerEvent {
            position,
            action: TextInputPointerAction::Release,
        });
    }

    *was_pressed = just_pressed || still_pressed;
    *prev_pos = position;
}
