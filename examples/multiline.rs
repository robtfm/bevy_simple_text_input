//! An example showing a very basic implementation.

use bevy::prelude::*;
use bevy_simple_text_input::{
    TextInput, TextInputPlugin, TextInputSelectionStyle, TextInputSettings, TextInputSubmitEvent,
    TextInputSystem, TextInputTextColor, TextInputTextFont, TextInputValue,
};

const BORDER_COLOR_ACTIVE: Color = Color::srgb(0.75, 0.52, 0.99);
const TEXT_COLOR: Color = Color::srgb(0.9, 0.9, 0.9);
const BACKGROUND_COLOR: Color = Color::srgb(0.15, 0.15, 0.15);
const SELECTION_COLOR: Color = Color::srgb(0.35, 0.35, 1.0);

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(TextInputPlugin)
        .add_systems(Startup, setup)
        .add_systems(Update, listener.after(TextInputSystem))
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
                ..default()
        })
        .with_children(|parent| {
            parent.spawn((
                Node {
                        width: Val::Px(400.0),
                        height: Val::Px(200.0),
                        border: UiRect::all(Val::Px(5.0)),
                        padding: UiRect::all(Val::Px(5.0)),
                        ..default()
                    },
                BorderColor::from(BORDER_COLOR_ACTIVE),
                BackgroundColor::from(BACKGROUND_COLOR),
                TextInput,
                TextInputTextFont(TextFont {
                    font_size: 40.,
                    ..Default::default()
                }),
                TextInputTextColor(TextColor(TEXT_COLOR)),
                TextInputSelectionStyle {
                    color: Some(BACKGROUND_COLOR),
                    background: Some(SELECTION_COLOR),
                },
                TextInputSettings {
                    multiline: true,
                    ..Default::default()
                },
                TextInputValue("one two three four five six seven eight nine ten eleven twelve thirteen fourteen fifteen sixteen seventeen eighteen nineteen twenty".to_owned()),
            ));
        });
}

fn listener(mut events: EventReader<TextInputSubmitEvent>) {
    for event in events.read() {
        info!("{:?} submitted: {}", event.entity, event.value);
    }
}
