//! A Bevy plugin the provides a simple single-line text input widget.
//!
//! # Examples
//!
//! See the [examples](https://github.com/rparrett/bevy_simple_text_input/tree/latest/examples) folder.
//!
//! ```no_run
//! use bevy::prelude::*;
//! use bevy_simple_text_input::{TextInput, TextInputPlugin};
//!
//! fn main() {
//!     App::new()
//!         .add_plugins(DefaultPlugins)
//!         .add_plugins(TextInputPlugin)
//!         .add_systems(Startup, setup)
//!         .run();
//! }
//!
//! fn setup(mut commands: Commands) {
//!     commands.spawn(Camera2d);
//!     commands.spawn((
//!         TextInput,
//!         Node {
//!             padding: UiRect::all(Val::Px(5.0)),
//!             border: UiRect::all(Val::Px(2.0)),
//!             ..default()
//!         },
//!         BorderColor(Color::BLACK)
//!     ));
//! }
//! ```

use bevy::{
    ecs::{event::EventCursor, system::SystemParam},
    input::keyboard::{Key, KeyboardInput},
    prelude::*,
    tasks::IoTaskPool,
    text::{
        ComputedTextBlock, CosmicBuffer, CosmicFontSystem, LineBreak,
        cosmic_text::{Action, Change, Cursor, Edit, Editor, Selection},
    },
    // ui::FocusPolicy,
};
use once_cell::unsync::Lazy;

#[cfg(feature = "clipboard")]
use copypwasmta::{ClipboardContext, ClipboardProvider};

/// A Bevy `Plugin` providing the systems and assets required to make a [`TextInput`] work.
pub struct TextInputPlugin;

/// Label for systems that update text inputs.
#[derive(Debug, PartialEq, Eq, Clone, Hash, SystemSet)]
pub struct TextInputSystem;

impl Plugin for TextInputPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TextInputNavigationBindings>()
            .add_event::<TextInputSubmitEvent>()
            .add_event::<TextInputPointerEvent>()
            .add_observer(create)
            .add_systems(
                Update,
                (
                    blink_cursor,
                    set_positions,
                    set_selection,
                    show_hide_placeholder,
                    update_style,
                    update_placeholder_style,
                    keyboard,
                    pointer,
                    update_value,
                )
                    .chain()
                    .in_set(TextInputSystem),
            )
            .register_type::<TextInputSettings>()
            .register_type::<TextInputTextFont>()
            .register_type::<TextInputTextColor>()
            .register_type::<TextInputSelectionStyle>()
            .register_type::<TextInputInactive>()
            .register_type::<TextInputCursorTimer>()
            .register_type::<TextInputInner>()
            .register_type::<TextInputValue>()
            .register_type::<TextInputPlaceholder>();
    }
}

/// The main "driving component" for the Text Input.
///
/// # Example
///
/// ```rust
/// # use bevy::prelude::*;
/// use bevy_simple_text_input::TextInput;
/// fn setup(mut commands: Commands) {
///     commands.spawn(TextInput);
/// }
/// ```
#[derive(Component, Default)]
#[require(
    TextInputSettings,
    TextInputTextFont,
    TextInputTextColor,
    TextInputSelectionStyle,
    TextInputInactive,
    TextInputCursorTimer,
    TextInputValue,
    TextInputPlaceholder,
    Node,
    Interaction
)]
pub struct TextInput;

/// The Bevy `TextColor` that will be used when creating the text input's inner Bevy `TextBundle`.
#[derive(Component, Default, Reflect)]
pub struct TextInputTextFont(pub TextFont);

/// The Bevy `TextColor` that will be used when creating the text input's inner Bevy `TextBundle`.
#[derive(Component, Default, Reflect)]
pub struct TextInputTextColor(pub TextColor);

/// selection color and background color
#[derive(Component, Default, Reflect)]
pub struct TextInputSelectionStyle {
    /// selected text color
    pub color: Option<Color>,
    /// selected text background color
    pub background: Option<Color>,
}

#[derive(Component)]
struct TextInputSelection;

#[derive(Component)]
struct TextInputContainer;

/// If true, the text input does not respond to keyboard events and the cursor is hidden.
#[derive(Component, Default, Reflect)]
pub struct TextInputInactive(pub bool);

/// A component that manages the cursor's blinking.
#[derive(Component, Reflect)]
pub struct TextInputCursorTimer {
    /// The timer that blinks the cursor on and off, and resets when the user types.
    pub timer: Timer,
    should_reset: bool,
}

impl Default for TextInputCursorTimer {
    fn default() -> Self {
        Self {
            timer: Timer::from_seconds(0.5, TimerMode::Repeating),
            should_reset: false,
        }
    }
}

/// A component containing the text input's settings.
#[derive(Component, Default, Reflect)]
pub struct TextInputSettings {
    /// multiline
    pub multiline: bool,
    /// If true, text is not cleared after pressing enter.
    pub retain_on_submit: bool,
    /// Mask text with the provided character.
    pub mask_character: Option<char>,
}

/// Text navigation actions that can be bound via `TextInputNavigationBindings`.
#[derive(Debug)]
pub enum TextInputAction {
    /// Moves the cursor one char to the left.
    CharLeft,
    /// Moves the cursor one char to the right.
    CharRight,
    /// Moves the cursor to the start of line.
    LineStart,
    /// Moves the cursor to the end of line.
    LineEnd,
    /// move up one line
    LineUp,
    /// move down one line
    LineDown,
    /// document start
    TextStart,
    /// document end
    TextEnd,
    /// Moves the cursor one word to the left.
    WordLeft,
    /// Moves the cursor one word to the right.
    WordRight,
    /// Removes the char left of the cursor.
    DeletePrev,
    /// Removes the char right of the cursor.
    DeleteNext,
    /// Triggers a `TextInputSubmitEvent`, optionally clearing the text input.
    Submit,
    /// add a new line
    NewLine,
    /// select full buffer
    SelectAll,
    /// cut
    #[cfg(feature = "clipboard")]
    Cut,
    /// copy
    #[cfg(feature = "clipboard")]
    Copy,
    /// pasta
    #[cfg(feature = "clipboard")]
    Paste,
    /// undo
    Undo,
    /// redo
    Redo,
}
/// A resource in which key bindings can be specified. Bindings are given as a tuple of (`TextInputAction`, `TextInputBinding`).
///
/// All modifiers must be held when the primary key is pressed to perform the action.
/// The first matching action in the list will be performed, so a binding that is the same as another with additional
/// modifier keys should be earlier in the vector to be applied.
#[derive(Resource)]
pub struct TextInputNavigationBindings(pub Vec<(TextInputAction, TextInputBinding)>);

/// A combination of a key and required modifier keys that might trigger a `TextInputAction`.
pub struct TextInputBinding {
    /// Primary key
    key: KeyCode,
    /// Required modifier keys
    modifiers: Vec<KeyCode>,
}

impl TextInputBinding {
    /// Creates a new `TextInputBinding` from a key and required modifiers.
    pub fn new(key: KeyCode, modifiers: impl Into<Vec<KeyCode>>) -> Self {
        Self {
            key,
            modifiers: modifiers.into(),
        }
    }
}

impl Default for TextInputNavigationBindings {
    fn default() -> Self {
        #[cfg(not(target_os = "macos"))]
        return Self::non_macos_default();

        #[cfg(target_os = "macos")]
        Self::macos_default()
    }
}

impl TextInputNavigationBindings {
    /// default key bindings for all except macos.
    /// usually Default::default is fine, but on wasm you need to specify manually
    pub fn non_macos_default() -> Self {
        use KeyCode::*;
        use TextInputAction::*;
        Self(vec![
            (TextStart, TextInputBinding::new(Home, [ControlLeft])),
            (TextStart, TextInputBinding::new(Home, [ControlRight])),
            (TextEnd, TextInputBinding::new(End, [ControlLeft])),
            (TextEnd, TextInputBinding::new(End, [ControlRight])),
            (LineStart, TextInputBinding::new(Home, [])),
            (LineEnd, TextInputBinding::new(End, [])),
            (WordLeft, TextInputBinding::new(ArrowLeft, [ControlLeft])),
            (WordLeft, TextInputBinding::new(ArrowLeft, [ControlRight])),
            (WordRight, TextInputBinding::new(ArrowRight, [ControlLeft])),
            (WordRight, TextInputBinding::new(ArrowRight, [ControlRight])),
            (CharLeft, TextInputBinding::new(ArrowLeft, [])),
            (CharRight, TextInputBinding::new(ArrowRight, [])),
            (LineUp, TextInputBinding::new(ArrowUp, [])),
            (LineDown, TextInputBinding::new(ArrowDown, [])),
            (DeletePrev, TextInputBinding::new(Backspace, [])),
            (DeletePrev, TextInputBinding::new(NumpadBackspace, [])),
            (DeleteNext, TextInputBinding::new(Delete, [])),
            // newline must be before submit as it is the same but with modifiers
            (NewLine, TextInputBinding::new(Enter, [ShiftLeft])),
            (NewLine, TextInputBinding::new(Enter, [ShiftRight])),
            (Submit, TextInputBinding::new(Enter, [])),
            (Submit, TextInputBinding::new(NumpadEnter, [])),
            (SelectAll, TextInputBinding::new(KeyA, [ControlLeft])),
            (SelectAll, TextInputBinding::new(KeyA, [ControlRight])),
            #[cfg(feature = "clipboard")]
            (
                TextInputAction::Cut,
                TextInputBinding::new(KeyX, [ControlLeft]),
            ),
            #[cfg(feature = "clipboard")]
            (
                TextInputAction::Cut,
                TextInputBinding::new(KeyX, [ControlRight]),
            ),
            #[cfg(feature = "clipboard")]
            (
                TextInputAction::Copy,
                TextInputBinding::new(KeyC, [ControlLeft]),
            ),
            #[cfg(feature = "clipboard")]
            (
                TextInputAction::Copy,
                TextInputBinding::new(KeyC, [ControlRight]),
            ),
            #[cfg(feature = "clipboard")]
            (
                TextInputAction::Paste,
                TextInputBinding::new(KeyV, [ControlLeft]),
            ),
            #[cfg(feature = "clipboard")]
            (
                TextInputAction::Paste,
                TextInputBinding::new(KeyV, [ControlRight]),
            ),
            (
                TextInputAction::Undo,
                TextInputBinding::new(KeyZ, [ControlLeft]),
            ),
            (
                TextInputAction::Undo,
                TextInputBinding::new(KeyZ, [ControlRight]),
            ),
            (
                TextInputAction::Redo,
                TextInputBinding::new(KeyY, [ControlLeft]),
            ),
            (
                TextInputAction::Redo,
                TextInputBinding::new(KeyY, [ControlRight]),
            ),
        ])
    }

    /// default key bindings for macos
    /// usually Default::default is fine, but on wasm you need to specify manually
    pub fn macos_default() -> Self {
        use KeyCode::*;
        use TextInputAction::*;
        Self(vec![
            (TextStart, TextInputBinding::new(ArrowUp, [SuperLeft])),
            (TextStart, TextInputBinding::new(ArrowUp, [SuperRight])),
            (TextStart, TextInputBinding::new(Home, [SuperLeft])),
            (TextStart, TextInputBinding::new(Home, [SuperRight])),
            (TextEnd, TextInputBinding::new(ArrowDown, [SuperLeft])),
            (TextEnd, TextInputBinding::new(ArrowDown, [SuperRight])),
            (TextEnd, TextInputBinding::new(End, [SuperLeft])),
            (TextEnd, TextInputBinding::new(End, [SuperRight])),
            (LineStart, TextInputBinding::new(ArrowLeft, [SuperLeft])),
            (LineStart, TextInputBinding::new(ArrowLeft, [SuperRight])),
            (LineStart, TextInputBinding::new(Home, [])),
            (LineEnd, TextInputBinding::new(ArrowRight, [SuperLeft])),
            (LineEnd, TextInputBinding::new(ArrowRight, [SuperRight])),
            (LineEnd, TextInputBinding::new(End, [])),
            (WordLeft, TextInputBinding::new(ArrowLeft, [AltLeft])),
            (WordLeft, TextInputBinding::new(ArrowLeft, [AltRight])),
            (WordRight, TextInputBinding::new(ArrowRight, [AltLeft])),
            (WordRight, TextInputBinding::new(ArrowRight, [AltRight])),
            (CharLeft, TextInputBinding::new(ArrowLeft, [])),
            (CharRight, TextInputBinding::new(ArrowRight, [])),
            (LineUp, TextInputBinding::new(ArrowUp, [])),
            (LineDown, TextInputBinding::new(ArrowDown, [])),
            (DeletePrev, TextInputBinding::new(Backspace, [])),
            (DeletePrev, TextInputBinding::new(NumpadBackspace, [])),
            (DeleteNext, TextInputBinding::new(Delete, [])),
            // newline must be before submit as it is the same but with modifiers
            (NewLine, TextInputBinding::new(Enter, [ShiftLeft])),
            (NewLine, TextInputBinding::new(Enter, [ShiftRight])),
            (NewLine, TextInputBinding::new(Enter, [AltLeft])),
            (NewLine, TextInputBinding::new(Enter, [AltRight])),
            (Submit, TextInputBinding::new(Enter, [])),
            (Submit, TextInputBinding::new(NumpadEnter, [])),
            (SelectAll, TextInputBinding::new(KeyA, [SuperLeft])),
            (SelectAll, TextInputBinding::new(KeyA, [SuperRight])),
            #[cfg(feature = "clipboard")]
            (
                TextInputAction::Cut,
                TextInputBinding::new(KeyX, [SuperLeft]),
            ),
            #[cfg(feature = "clipboard")]
            (
                TextInputAction::Cut,
                TextInputBinding::new(KeyX, [SuperRight]),
            ),
            #[cfg(feature = "clipboard")]
            (
                TextInputAction::Copy,
                TextInputBinding::new(KeyC, [SuperLeft]),
            ),
            #[cfg(feature = "clipboard")]
            (
                TextInputAction::Copy,
                TextInputBinding::new(KeyC, [SuperRight]),
            ),
            #[cfg(feature = "clipboard")]
            (
                TextInputAction::Paste,
                TextInputBinding::new(KeyV, [SuperLeft]),
            ),
            #[cfg(feature = "clipboard")]
            (
                TextInputAction::Paste,
                TextInputBinding::new(KeyV, [SuperRight]),
            ),
            (
                TextInputAction::Undo,
                TextInputBinding::new(KeyZ, [SuperLeft]),
            ),
            (
                TextInputAction::Undo,
                TextInputBinding::new(KeyZ, [SuperRight]),
            ),
            // Redo on macOS is typically Cmd+Shift+Z
            (
                TextInputAction::Redo,
                TextInputBinding::new(KeyZ, [SuperLeft, ShiftLeft]),
            ),
            (
                TextInputAction::Redo,
                TextInputBinding::new(KeyZ, [SuperRight, ShiftLeft]),
            ),
            (
                TextInputAction::Redo,
                TextInputBinding::new(KeyZ, [SuperLeft, ShiftRight]),
            ),
            (
                TextInputAction::Redo,
                TextInputBinding::new(KeyZ, [SuperRight, ShiftRight]),
            ),
        ])
    }
}

/// A component containing the current value of the text input.
#[derive(Component, Default, Reflect)]
pub struct TextInputValue(pub String);

/// A component containing the placeholder text that is displayed when the text input is empty and not focused.
#[derive(Component, Default, Reflect)]
pub struct TextInputPlaceholder {
    /// The placeholder text.
    pub value: String,
    /// The `TextFont` to use when rendering the placeholder text.
    ///
    /// If `None`, the text input font will be used.
    pub text_font: Option<TextFont>,
    /// The style to use when rendering the placeholder text.
    ///
    /// If `None`, the text input color will be used with alpha value of `0.25`.
    pub text_color: Option<TextColor>,
}

#[derive(Component, Reflect)]
struct TextInputPlaceholderInner;

#[derive(Component, Reflect)]
struct TextInputInner;

#[derive(Component)]
struct CosmicEditor {
    editor: Editor<'static>,
    selection_bounds: Option<(usize, usize)>,
    undo: Vec<Change>,
    redo: Vec<Change>,
}

impl CosmicEditor {
    fn new(text: &str) -> Self {
        let mut editor = Editor::new(CosmicBuffer::default().0);
        editor.insert_string(text, None);
        Self {
            editor,
            selection_bounds: None,
            undo: Vec::default(),
            redo: Vec::default(),
        }
    }

    fn update_selection_bounds(&mut self) {
        self.selection_bounds = self.editor.selection_bounds().map(|(from, to)| {
            let index = |c: Cursor| -> usize {
                self.editor.with_buffer(|b| {
                    let mut lines = b.lines.iter();

                    let prior_sum: usize = lines
                        .by_ref()
                        .take(c.line)
                        .map(|line| line.text().len() + 1)
                        .sum();

                    let line_sum = lines
                        .next()
                        .map(|line| {
                            line.text()
                                .char_indices()
                                .enumerate()
                                .find(|(_, ci)| ci.0 == c.index)
                                .map(|(ix, _)| ix)
                                .unwrap_or(line.text().len())
                        })
                        .unwrap_or(0);

                    prior_sum + line_sum
                })
            };

            (index(from), index(to))
        });
    }
}

#[derive(Component)]
struct TextInputCursorDisplay;

/// An event that is fired when the user presses the enter key.
#[derive(Event)]
pub struct TextInputSubmitEvent {
    /// The text input that triggered the event.
    pub entity: Entity,
    /// The string contained in the text input at the time of the event.
    pub value: String,
}

/// A convenience parameter for dealing with a text input's inner Bevy `Text` entity.
#[derive(SystemParam)]
struct InnerText<'w, 's> {
    inner_query: Query<'w, 's, Entity, With<TextInputInner>>,
    computed_text_query: Query<'w, 's, &'static ComputedTextBlock, With<TextInputInner>>,
    computed_node_query: Query<'w, 's, &'static ComputedNode, With<TextInputInner>>,
    cursor_query: Query<
        'w,
        's,
        (&'static mut Node, &'static mut BackgroundColor),
        With<TextInputCursorDisplay>,
    >,
    children_query: Query<'w, 's, &'static Children>,
}
impl InnerText<'_, '_> {
    fn computed_text(&self, entity: Entity) -> Option<&ComputedTextBlock> {
        self.computed_text_query
            .get(self.inner_entity(entity)?)
            .ok()
    }

    fn computed_node(&self, entity: Entity) -> Option<&ComputedNode> {
        self.computed_node_query
            .get(self.inner_entity(entity)?)
            .ok()
    }

    fn cursor_style(&mut self, entity: Entity) -> Option<(&mut Node, &mut BackgroundColor)> {
        self.cursor_query
            .get_mut(
                self.children_query
                    .iter_descendants(entity)
                    .find(|d| self.cursor_query.get(*d).is_ok())?,
            )
            .ok()
            .map(|(node, bg)| (node.into_inner(), bg.into_inner()))
    }

    fn inner_entity(&self, entity: Entity) -> Option<Entity> {
        self.children_query
            .iter_descendants(entity)
            .find(|descendant_entity| self.inner_query.get(*descendant_entity).is_ok())
    }
}

// get results from a task
#[cfg(feature = "clipboard")]
trait TaskExt {
    type Output;

    fn complete(&mut self) -> Option<Self::Output>;
}

#[cfg(feature = "clipboard")]
impl<T> TaskExt for bevy::tasks::Task<T> {
    type Output = T;

    #[cfg(target_arch = "wasm32")]
    fn complete(&mut self) -> Option<Self::Output> {
        use futures_lite::FutureExt;
        // wasm doesn't have `is_finished``, but polling is cheap as it is just a oneshot receiver
        let mut context = std::task::Context::from_waker(std::task::Waker::noop());
        if let std::task::Poll::Ready(res) = self.poll(&mut context) {
            Some(res)
        } else {
            None
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn complete(&mut self) -> Option<Self::Output> {
        match self.is_finished() {
            true => Some(
                futures_lite::future::block_on(futures_lite::future::poll_once(self))
                    .expect("is_finished but !Some?"),
            ),

            false => None,
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn keyboard(
    key_input: Res<ButtonInput<KeyCode>>,
    input_events: Res<Events<KeyboardInput>>,
    mut input_reader: Local<EventCursor<KeyboardInput>>,
    mut text_input_query: Query<(
        Entity,
        &TextInputSettings,
        &TextInputInactive,
        &mut TextInputValue,
        &mut TextInputCursorTimer,
        &mut CosmicEditor,
    )>,
    mut submit_writer: EventWriter<TextInputSubmitEvent>,
    navigation: Res<TextInputNavigationBindings>,
    inner_text: InnerText,
    mut font_system: ResMut<CosmicFontSystem>,
    #[cfg(feature = "clipboard")] mut clipboard_read: Local<
        Option<(Entity, bevy::tasks::Task<Result<String, String>>)>,
    >,
) {
    #[allow(unused_mut)]
    let mut copy_text: Option<(Entity, Result<String, String>)> = None;

    #[cfg(feature = "clipboard")]
    if let Some((ent, read_task)) = clipboard_read.as_mut() {
        if let Some(result) = read_task.complete() {
            copy_text = Some((*ent, result));
            *clipboard_read = None;
        }
    }

    if copy_text.is_none() && input_reader.clone().read(&input_events).next().is_none() {
        return;
    }

    // collect actions that have all required modifiers held
    let valid_actions = navigation
        .0
        .iter()
        .filter(|(_, TextInputBinding { modifiers, .. })| {
            modifiers.iter().all(|m| key_input.pressed(*m))
        })
        .map(|(action, TextInputBinding { key, .. })| (*key, action));

    let select = key_input.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);

    for (input_entity, settings, inactive, mut text_input, mut cursor_timer, mut editor) in
        &mut text_input_query
    {
        if inactive.0 {
            continue;
        }

        let mut submitted_value = None;
        let mut is_undo_redo = false;

        // use a lazy cell to avoid initializing the editor if not required (copying the buffer is expensive)
        let mut editor = Lazy::new(|| {
            let (max_line, max_index) = editor.editor.with_buffer_mut(|b| {
                b.clone_from(&inner_text.computed_text(input_entity).unwrap().buffer().0);
                (
                    b.lines.len() - 1,
                    b.lines.last().map(|l| l.text().len()).unwrap_or(0),
                )
            });
            // we need to reset the cursor position if it's invalid, else some actions (backspace) will panic
            if editor.editor.cursor_position().is_none() {
                editor.editor.set_cursor(Cursor {
                    line: max_line,
                    index: max_index,
                    affinity: bevy::text::cosmic_text::Affinity::Before,
                });
            }
            editor.editor.start_change();
            editor
        });

        #[cfg(feature = "clipboard")]
        if let Some(copy_result) = copy_text
            .clone()
            .filter(|(copy_ent, _)| input_entity == *copy_ent)
            .map(|(_, result)| result)
        {
            match copy_result {
                Ok(text) => {
                    editor.editor.delete_selection();
                    editor
                        .editor
                        .insert_string(&text.replace("\r\n", "\n"), None);
                }
                Err(err) => warn!("failed to read clipboard: {err}"),
            }
        }

        for input in input_reader.clone().read(&input_events) {
            if !input.state.is_pressed() {
                continue;
            };

            if let Some((_, action)) = valid_actions
                .clone()
                .find(|(key, _)| *key == input.key_code)
            {
                let mut select = select;

                if select && editor.editor.selection() == Selection::None {
                    let cursor = editor.editor.cursor();
                    editor.editor.set_selection(Selection::Normal(cursor));
                }

                use TextInputAction::*;
                use bevy::text::cosmic_text::Motion;
                let mut timer_should_reset = true;

                let editor_action = match action {
                    CharLeft => Some(Action::Motion(Motion::Left)),
                    CharRight => Some(Action::Motion(Motion::Right)),
                    TextStart => Some(Action::Motion(Motion::BufferStart)),
                    TextEnd => Some(Action::Motion(Motion::BufferEnd)),
                    LineStart => Some(Action::Motion(Motion::Home)),
                    LineEnd => Some(Action::Motion(Motion::End)),
                    WordLeft => Some(Action::Motion(Motion::LeftWord)),
                    WordRight => Some(Action::Motion(Motion::RightWord)),
                    LineUp => Some(Action::Motion(Motion::Up)),
                    LineDown => Some(Action::Motion(Motion::Down)),
                    DeletePrev => Some(Action::Backspace),
                    DeleteNext => Some(Action::Delete),
                    Submit => {
                        if settings.retain_on_submit {
                            submitted_value = Some(text_input.0.clone());
                        } else {
                            submitted_value = Some(std::mem::take(&mut text_input.0));
                        };
                        timer_should_reset = false;
                        Some(Action::Motion(Motion::BufferStart))
                    }
                    NewLine => settings.multiline.then_some(Action::Enter),
                    SelectAll => {
                        editor
                            .editor
                            .set_selection(Selection::Normal(Cursor::default()));

                        select = true;

                        Some(Action::Motion(Motion::BufferEnd))
                    }
                    #[cfg(feature = "clipboard")]
                    Cut | Copy => {
                        {
                            if let Some(selection) = editor.editor.copy_selection() {
                                IoTaskPool::get()
                                    .spawn(async move {
                                        let result = match ClipboardContext::new() {
                                            Ok(mut ctx) => ctx
                                                .set_contents(selection)
                                                .await
                                                .map_err(|e| e.to_string()),
                                            Err(e) => Err(e.to_string()),
                                        };

                                        if let Err(e) = result {
                                            warn!("failed to copy to clipboard: {e:?}");
                                        }
                                    })
                                    .detach();
                            }
                        }

                        if let Cut = action {
                            editor.editor.delete_selection();
                        } else {
                            // avoid clearing selection on copy
                            select = true;
                        }

                        None
                    }
                    #[cfg(feature = "clipboard")]
                    Paste => {
                        *clipboard_read = Some((
                            input_entity,
                            IoTaskPool::get().spawn(async {
                                let Ok(mut ctx) = ClipboardContext::new() else {
                                    return Err("can't get clipboard".to_owned());
                                };
                                ctx.get_contents().await.map_err(|e| format!("{e:?}"))
                            }),
                        ));
                        select = true;
                        None
                    }

                    Undo => {
                        if let Some(mut undo) = editor.undo.pop() {
                            undo.reverse();
                            editor.editor.finish_change();
                            editor.editor.apply_change(&undo);
                            editor.editor.start_change();
                            editor.redo.push(undo);
                        }

                        is_undo_redo = true;
                        None
                    }

                    Redo => {
                        if let Some(mut redo) = editor.redo.pop() {
                            redo.reverse();
                            editor.editor.finish_change();
                            editor.editor.apply_change(&redo);
                            editor.editor.start_change();
                            editor.undo.push(redo);
                        }

                        is_undo_redo = true;
                        None
                    }
                };

                if let Some(action) = editor_action {
                    editor.editor.action(&mut font_system, action);
                }

                if !select {
                    editor.editor.set_selection(Selection::None);
                }

                cursor_timer.should_reset |= timer_should_reset;
                continue;
            }

            match input.logical_key {
                Key::Space => {
                    editor.editor.insert_string(" ", None);
                    cursor_timer.should_reset = true;
                }
                Key::Character(ref s) => {
                    editor.editor.insert_string(s, None);
                    cursor_timer.should_reset = true;
                }
                _ => (),
            }
        }

        if let Some(value) = submitted_value {
            submit_writer.write(TextInputSubmitEvent {
                entity: input_entity,
                value,
            });
            editor.redo.clear();
            editor.undo.clear();
        } else if let Ok(mut editor) = Lazy::into_value(editor) {
            if let Some(change) = editor.editor.finish_change() {
                if !change.items.is_empty() && !is_undo_redo {
                    editor.redo.clear();
                    editor.undo.push(change);
                }
            }

            editor.editor.shape_as_needed(&mut font_system, false);
            editor.editor.with_buffer(|b| {
                text_input.0 = b
                    .lines
                    .iter()
                    .map(|line| format!("{}{}", line.text(), line.ending().as_str()))
                    .collect::<Vec<_>>()
                    .join("");
            });

            editor.update_selection_bounds();
        }
    }

    input_reader.clear(&input_events);
}

/// TextPositionFinder
#[derive(SystemParam)]
pub struct TextPositionFinder<'w, 's> {
    block: Query<'w, 's, &'static ComputedTextBlock>,
    reader: TextUiReader<'w, 's>,
}

impl TextPositionFinder<'_, '_> {
    /// TextPositionFinder
    pub fn cursor_hit(&self, entity: Entity, position: Vec2) -> Option<Cursor> {
        let block = self.block.get(entity).ok()?;
        let buffer = block.buffer();
        buffer.hit(position.x, position.y)
    }

    /// TextPositionFinder
    pub fn cursor_entity(&mut self, entity: Entity, position: Vec2) -> Option<(Entity, usize)> {
        let Cursor {
            mut line,
            mut index,
            ..
        } = self.cursor_hit(entity, position)?;
        for (entity, _, text, _, _) in self.reader.iter(entity) {
            let mut parts = text.split('\n');
            let line_breaks = parts.clone().count() - 1;
            if line_breaks < line {
                line -= line_breaks;
                continue;
            }

            let entity_line_offset: usize = parts.by_ref().take(line).map(|text| text.len()).sum();
            line = 0;

            let len = parts.next().unwrap().len();
            if len > index {
                return Some((entity, entity_line_offset + index));
            } else {
                index -= len;
            }

            if parts.next().is_some() {
                panic!();
            }
        }

        None
    }
}

/// TextInputPointerAction
#[derive(Debug, PartialEq)]
pub enum TextInputPointerAction {
    /// TextInputPointerAction
    Press,
    /// TextInputPointerAction
    Drag,
    /// TextInputPointerAction
    Release,
}

/// TextInputPointerEvent
#[derive(Event, Debug)]
pub struct TextInputPointerEvent {
    /// TextInputPointerEvent
    pub position: Vec2,
    /// TextInputPointerEvent
    pub action: TextInputPointerAction,
}

#[allow(clippy::too_many_arguments)]
fn pointer(
    mut events: EventReader<TextInputPointerEvent>,
    mut last_action: Local<Option<(Entity, f32, usize)>>,
    mut buffers: Query<(&TextInputInactive, Entity, &mut CosmicEditor)>,
    mut font_system: ResMut<CosmicFontSystem>,
    inner_text: InnerText,
    time: Res<Time>,
    helper: TransformHelper,
) {
    for event in events.read() {
        let time = time.elapsed_secs();

        let Some((_, entity, mut editor)) = buffers.iter_mut().find(|(inactive, ..)| !inactive.0)
        else {
            continue;
        };

        let click_count = last_action
            .filter(|(e, t, _)| {
                *e == entity && (*t > time - 0.25 || event.action == TextInputPointerAction::Drag)
            })
            .map(|(_, _, c)| c)
            .unwrap_or(0);

        editor.editor.with_buffer_mut(|b| {
            b.clone_from(&inner_text.computed_text(entity).unwrap().buffer().0)
        });
        editor.editor.shape_as_needed(&mut font_system, false);

        let top_left = helper
            .compute_global_transform(inner_text.inner_entity(entity).unwrap())
            .unwrap()
            .translation()
            .xy()
            - inner_text.computed_node(entity).unwrap().size() * 0.5;
        let relative_position = event.position - top_left;
        let Some(cursor) = editor
            .editor
            .with_buffer(|b| b.hit(relative_position.x, relative_position.y))
        else {
            continue;
        };

        match event.action {
            TextInputPointerAction::Release => (),
            TextInputPointerAction::Press => {
                editor.editor.set_cursor(cursor);
                editor.editor.set_selection(match click_count {
                    0 => Selection::Normal(cursor),
                    1 => Selection::Word(cursor),
                    _ => Selection::Line(cursor),
                });
                *last_action = Some((entity, time, click_count + 1));
            }
            TextInputPointerAction::Drag => {
                if click_count > 0 {
                    editor.editor.set_cursor(cursor);
                }
            }
        }

        editor.update_selection_bounds();
    }
}

fn update_value(
    mut input_query: Query<
        (
            Entity,
            Ref<TextInputValue>,
            &TextInputSettings,
            &CosmicEditor,
        ),
        Or<(Changed<TextInputValue>, Changed<CosmicEditor>)>,
    >,
    inner_text: InnerText,
    mut writer: TextUiWriter,
) {
    for (entity, text_input, settings, editor) in &mut input_query {
        let Some(inner_entity) = inner_text.inner_entity(entity) else {
            continue;
        };

        let mut section_values = section_values(
            &text_input.0,
            editor.selection_bounds,
            settings.mask_character,
        );
        writer.for_each_text(inner_entity, |mut t| *t = section_values.next().unwrap());
    }
}

fn create(
    trigger: Trigger<OnAdd, TextInputValue>,
    mut commands: Commands,
    query: Query<(
        &TextInputTextFont,
        &TextInputTextColor,
        &TextInputValue,
        &TextInputInactive,
        &TextInputSettings,
        &TextInputPlaceholder,
    )>,
) {
    if let Ok((font, color, text_input, inactive, settings, placeholder)) =
        &query.get(trigger.target())
    {
        let value = masked_value(&text_input.0, settings.mask_character);

        let text = commands
            .spawn((
                // pre-selection
                Text::new(value),
                font.0.clone(),
                color.0,
                Node {
                    min_width: Val::Percent(100.0),
                    min_height: Val::Percent(100.0),
                    ..Default::default()
                },
                TextLayout::new_with_linebreak(if settings.multiline {
                    LineBreak::WordBoundary
                } else {
                    LineBreak::NoWrap
                }),
                Name::new("TextInputInner"),
                TextInputInner,
            ))
            .with_children(|parent| {
                // selection
                parent.spawn((TextSpan::default(), font.0.clone(), color.0));
                // post-selection
                parent.spawn((TextSpan::default(), font.0.clone(), color.0));
            })
            .id();

        let selection_hilight = commands
            .spawn((
                Node {
                    position_type: PositionType::Absolute,
                    display: Display::Flex,
                    width: Val::Percent(100.0),
                    height: Val::Percent(100.0),
                    ..Default::default()
                },
                ZIndex(-1),
                TextInputSelection,
            ))
            .id();

        let cursor = commands
            .spawn((
                Node {
                    display: Display::None,
                    width: Val::Px(1f32.max(font.0.font_size * 0.05)),
                    height: Val::Px(font.0.font_size),
                    position_type: PositionType::Absolute,
                    ..Default::default()
                },
                BackgroundColor(*color.0),
                TextInputCursorDisplay,
            ))
            .id();

        let container = commands
            .spawn((
                Node {
                    min_height: Val::Percent(100.0),
                    ..Default::default()
                },
                TextInputContainer,
            ))
            .add_children(&[text, selection_hilight, cursor])
            .id();

        let placeholder_font = placeholder
            .text_font
            .clone()
            .unwrap_or_else(|| font.0.clone());

        let placeholder_color = placeholder
            .text_color
            .unwrap_or_else(|| placeholder_color(&color.0));

        let placeholder_visible = inactive.0 && text_input.0.is_empty();

        let placeholder_text = commands
            .spawn((
                Text::new(&placeholder.value),
                TextLayout::new_with_linebreak(LineBreak::NoWrap),
                placeholder_font,
                placeholder_color,
                Name::new("TextInputPlaceholderInner"),
                TextInputPlaceholderInner,
                if placeholder_visible {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                },
                Node {
                    position_type: PositionType::Absolute,
                    ..default()
                },
            ))
            .id();

        let overflow_container = commands
            .spawn((
                Node {
                    overflow: if settings.multiline {
                        Overflow::scroll()
                    } else {
                        Overflow::scroll_x()
                    },
                    justify_content: JustifyContent::FlexStart,
                    align_items: AlignItems::FlexStart,
                    min_width: Val::Percent(100.),
                    max_width: Val::Percent(100.),
                    min_height: Val::Percent(100.),
                    max_height: Val::Percent(100.),
                    ..default()
                },
                Name::new("TextInputOverflowContainer"),
            ))
            .id();

        commands.entity(overflow_container).add_child(container);
        commands
            .entity(trigger.target())
            .add_children(&[overflow_container, placeholder_text]);

        // Prevent clicks from registering on UI elements underneath the text input.
        commands
            .entity(trigger.target())
            // .insert(FocusPolicy::Block)
            .insert(CosmicEditor::new(&text_input.0));
    }
}

// Shows or hides the cursor based on the text input's [`TextInputInactive`] property.
fn set_positions(
    mut input_query: Query<
        (
            Entity,
            &TextInputSettings,
            &mut TextInputCursorTimer,
            &TextInputInactive,
            &mut CosmicEditor,
        ),
        Or<(
            Changed<TextInputInactive>,
            Changed<TextInputTextFont>,
            Changed<CosmicEditor>,
        )>,
    >,
    mut inner_style_query: Query<
        (&mut Node, &ComputedNode),
        (Without<TextInputCursorDisplay>, With<TextInputContainer>),
    >,

    mut container_query: Query<
        &ComputedNode,
        (Without<TextInputCursorDisplay>, Without<TextInputContainer>),
    >,
    mut inner_text: InnerText,
    children: Query<&Children>,
    mut font_system: ResMut<CosmicFontSystem>,
) {
    let px = |val: Val| match val {
        Val::Px(px) => px,

        _ => 0.0,
    };

    for (entity, settings, mut cursor_timer, inactive, mut editor) in &mut input_query {
        if inactive.0 {
            let Some(cursor_style) = inner_text.cursor_style(entity) else {
                continue;
            };

            cursor_style.0.display = Display::None;
            continue;
        }

        let inverse_scale_factor = inner_text
            .computed_node(entity)
            .map(ComputedNode::inverse_scale_factor)
            .unwrap_or(1.0);

        let Some((mut container_style, child_node)) = children
            .iter_descendants(entity)
            .find(|e| inner_style_query.get(*e).is_ok())
            .and_then(|e| inner_style_query.get_mut(e).ok())
        else {
            continue;
        };

        let Some(parent_node) = children
            .iter_descendants(entity)
            .find(|e| container_query.get(*e).is_ok())
            .and_then(|e| container_query.get_mut(e).ok())
        else {
            continue;
        };

        if font_system.0.db().is_empty() {
            editor.set_changed();
            continue;
        }

        let editor = editor.bypass_change_detection();

        editor.editor.with_buffer_mut(|b| {
            b.clone_from(&inner_text.computed_text(entity).unwrap().buffer().0);
        });
        // we need to reset the cursor position if it's invalid, else shape will fail
        if editor.editor.cursor_position().is_none() {
            editor.editor.action(
                &mut font_system,
                Action::Motion(bevy::text::cosmic_text::Motion::BufferEnd),
            );
        }
        editor.editor.shape_as_needed(&mut font_system, false);

        let cursor_position = IVec2::from(editor.editor.cursor_position().unwrap_or((0, 0)))
            .as_vec2()
            * inverse_scale_factor;

        let child_size = child_node.size();
        let parent_size = parent_node.size();

        let box_pos_x = match container_style.left {
            Val::Px(px) => -px,
            _ => 0.0,
        };

        let box_pos_y = match container_style.top {
            Val::Px(px) => -px,
            _ => 0.0,
        };

        let Some(cursor_style) = inner_text.cursor_style(entity) else {
            continue;
        };

        let relative_cursor_position = cursor_position - Vec2::new(box_pos_x, box_pos_y);
        let cursor_size = Vec2::new(
            px(cursor_style.0.width) + 1.0,
            px(cursor_style.0.height) + 1.0,
        );

        // println!("cs.top: {:?}", container_style.top);
        // println!("box: ({box_pos_x},{box_pos_y}, cursor: {cursor_position}, rcp: {relative_cursor_position}");
        if relative_cursor_position.cmplt(Vec2::ZERO).any()
            || (relative_cursor_position + cursor_size)
                .cmpgt(parent_size)
                .any()
        {
            // println!("update");
            let req_px = parent_size * 0.5 - cursor_position;
            let mut req_px =
                req_px.clamp(parent_size - child_size - cursor_size * Vec2::X, Vec2::ZERO);
            if settings.multiline {
                req_px.x = 0.0;
            }
            container_style.left = Val::Px(req_px.x);
            container_style.top = Val::Px(req_px.y);
        }
        // println!(
        //     "parent_size: {parent_size}, child_size: {child_size}, cursor_position: {cursor_position}, req_unclamped: {}, req_px: {}",
        //     parent_size * 0.5 - cursor_position,
        //     (parent_size * 0.5 - cursor_position)
        //         .clamp(parent_size - child_size - cursor_size * Vec2::X, Vec2::ZERO)
        // );

        cursor_style.0.display = if editor
            .selection_bounds
            .is_some_and(|(start, end)| start != end)
        {
            Display::None
        } else {
            Display::Flex
        };

        cursor_style.0.left = Val::Px(cursor_position.x);
        cursor_style.0.top = Val::Px(cursor_position.y + px(cursor_style.0.height) * 0.1);

        cursor_timer.timer.reset();
    }
}

fn set_selection(
    mut query: Query<(Entity, &mut CosmicEditor, &TextInputSelectionStyle), Changed<CosmicEditor>>,
    children: Query<&Children>,
    sel: Query<&TextInputSelection>,
    mut commands: Commands,
    mut font_system: ResMut<CosmicFontSystem>,
) {
    for (entity, mut editor, style) in query.iter_mut() {
        let Some(selection) = children
            .iter_descendants(entity)
            .find(|c| sel.get(*c).is_ok())
        else {
            continue;
        };

        let editor = editor.bypass_change_detection();

        commands.entity(selection).despawn_related::<Children>();

        if let Some((from, to)) = editor.editor.selection_bounds() {
            let mut segments = Vec::default();

            editor.editor.with_buffer_mut(|b| {
                b.shape_until_cursor(&mut font_system, to, false);

                let mut segment_y = f32::NEG_INFINITY;

                let runs = b
                    .layout_runs()
                    .skip_while(|run| run.line_i < from.line)
                    .take_while(|run| run.line_i <= to.line);

                for run in runs {
                    let glyphs = run
                        .glyphs
                        .iter()
                        .skip_while(|g| run.line_i == from.line && g.start < from.index)
                        .take_while(|g| run.line_i < to.line || g.end <= to.index);

                    for glyph in glyphs {
                        debug!("g: {},{}", glyph.x, glyph.y);

                        if run.line_top + glyph.y != segment_y {
                            segments.push(Vec4::new(
                                glyph.x,
                                run.line_top + glyph.y,
                                glyph.w,
                                run.line_height,
                            ));

                            segment_y = glyph.y;
                        } else {
                            let segment = segments.last_mut().unwrap();

                            segment.z = glyph.x + glyph.w - segment.x;
                        }
                    }
                }
            });

            commands.entity(selection).with_children(|c| {
                for segment in segments {
                    c.spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            left: Val::Px(segment.x.floor()),
                            top: Val::Px(segment.y.floor()),
                            width: Val::Px(segment.z.ceil()),
                            height: Val::Px(segment.w.ceil()),
                            ..Default::default()
                        },
                        BackgroundColor(style.background.unwrap_or(Color::srgb(0.3, 0.3, 1.0))),
                    ));
                }
            });
        }
    }
}

// Blinks the cursor on a timer.
fn blink_cursor(
    mut input_query: Query<(
        Entity,
        &mut TextInputCursorTimer,
        Ref<TextInputInactive>,
        &CosmicEditor,
    )>,
    mut inner_text: InnerText,
    time: Res<Time>,
) {
    for (entity, mut cursor_timer, inactive, editor) in &mut input_query {
        if inactive.0 {
            continue;
        }

        if cursor_timer.is_changed() && cursor_timer.should_reset {
            cursor_timer.timer.reset();
            cursor_timer.should_reset = false;
            continue;
        }

        if !cursor_timer.timer.tick(time.delta()).just_finished() {
            continue;
        }

        let Some(style) = inner_text.cursor_style(entity) else {
            continue;
        };

        style.0.display = match (
            editor
                .selection_bounds
                .is_some_and(|(start, end)| start != end),
            style.0.display,
        ) {
            (false, Display::None) => Display::Flex,
            _ => Display::None,
        }
    }
}

fn show_hide_placeholder(
    input_query: Query<
        (&Children, &TextInputValue, &TextInputInactive),
        Or<(Changed<TextInputValue>, Changed<TextInputInactive>)>,
    >,
    mut vis_query: Query<&mut Visibility, With<TextInputPlaceholderInner>>,
) {
    for (children, text, inactive) in &input_query {
        let mut iter = vis_query.iter_many_mut(children);
        while let Some(mut inner_vis) = iter.fetch_next() {
            inner_vis.set_if_neq(if text.0.is_empty() && inactive.0 {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            });
        }
    }
}

fn update_style(
    mut input_query: Query<
        (
            Entity,
            &TextInputTextFont,
            &TextInputTextColor,
            &TextInputSelectionStyle,
            &mut TextInputInactive,
        ),
        Or<(
            Changed<TextInputInactive>,
            Changed<TextInputTextFont>,
            Changed<TextInputSelectionStyle>,
            Changed<TextInputTextColor>,
        )>,
    >,
    mut inner_text: InnerText,
    mut writer: TextUiWriter,
) {
    for (entity, font, color, selection_style, mut inactive) in &mut input_query {
        let Some(inner_entity) = inner_text.inner_entity(entity) else {
            continue;
        };

        for index in [0, 2] {
            writer.font(inner_entity, index).clone_from(&font.0);
            writer.color(inner_entity, index).clone_from(&color.0);
        }
        writer.font(inner_entity, 1).clone_from(&font.0);
        writer
            .color(inner_entity, 1)
            .0
            .clone_from(selection_style.color.as_ref().unwrap_or(&color.0));

        let Some(cursor) = inner_text.cursor_style(entity) else {
            continue;
        };

        cursor.0.width = Val::Px(1f32.max(font.0.font_size * 0.05));
        cursor.0.height = Val::Px(font.0.font_size);
        cursor.1.0 = *color.0;

        inactive.set_changed()
    }
}

fn masked_value(value: &str, mask: Option<char>) -> String {
    mask.map_or_else(
        || value.to_string(),
        |c| value.chars().map(|_| c).collect::<String>(),
    )
}

fn placeholder_color(color: &TextColor) -> TextColor {
    TextColor(color.with_alpha(color.alpha() * 0.25))
}

fn update_placeholder_style(
    mut placeholder_query: Query<
        (
            &TextInputPlaceholder,
            &TextInputTextFont,
            &TextInputTextColor,
            &Children,
            &mut TextInputInactive,
        ),
        Changed<TextInputPlaceholder>,
    >,

    mut placeholders: Query<
        (&mut Text, &mut TextFont, &mut TextColor),
        With<TextInputPlaceholderInner>,
    >,
) {
    for (placeholder, base_font, base_color, children, mut inactive) in &mut placeholder_query {
        let Some((mut text, mut font, mut color)) = children
            .iter()
            .find(|c| placeholders.get(*c).is_ok())
            .and_then(|c| placeholders.get_mut(c).ok())
        else {
            continue;
        };

        font.clone_from(&base_font.0);
        color.clone_from(&base_color.0);
        text.0 = placeholder.value.clone();

        // mark so other systems update correctly
        inactive.set_changed()
    }
}

fn section_values(
    value: &str,
    bounds: Option<(usize, usize)>,
    mask_character: Option<char>,
) -> impl Iterator<Item = String> {
    let bounds = bounds.map(|(from, to)| {
        let to = to.min(value.len());
        let from = from.min(to);
        (from, to)
    });

    let vec = match bounds {
        Some((start, end)) if start != end => {
            vec![
                masked_value(&value[0..start], mask_character),
                masked_value(&value[start..end], mask_character),
                masked_value(&value[end..], mask_character),
            ]
        }

        _ => {
            vec![
                masked_value(value, mask_character),
                String::default(),
                if value.is_empty() {
                    String::from("\n")
                } else {
                    String::default()
                },
            ]
        }
    };

    vec.into_iter()
}
