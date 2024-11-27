#![doc = include_str!("../README.md")]
pub mod actions;
pub mod application;
mod clipboard;
mod conversion;
mod error;
mod event;
pub mod multi_window;
mod proxy;
mod sandbox;

pub mod settings;

pub mod reexport {
    pub use layershellev::reexport::wayland_client::wl_keyboard;
    pub use layershellev::reexport::Anchor;
    pub use layershellev::reexport::KeyboardInteractivity;
    pub use layershellev::reexport::Layer;
    pub use layershellev::LayerOutputSetting;
    pub use layershellev::NewLayerShellSettings;
}

use actions::{LayershellCustomActions, LayershellCustomActionsWithIdAndInfo};
use settings::Settings;

use iced_runtime::Task;

pub use iced_layershell_macros::to_layer_message;

pub use error::Error;

use iced::{Color, Element, Theme};
use iced_futures::Subscription;

pub use sandbox::LayerShellSandbox;

/// The appearance of a program.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Appearance {
    /// The background [`Color`] of the application.
    pub background_color: Color,

    /// The default text [`Color`] of the application.
    pub text_color: Color,
}

/// The default style of a [`Application`].
pub trait DefaultStyle {
    /// Returns the default style of a [`Appearance`].
    fn default_style(&self) -> Appearance;
}

impl DefaultStyle for Theme {
    fn default_style(&self) -> Appearance {
        default(self)
    }
}

/// The default [`Appearance`] of a [`Application`] with the built-in [`Theme`].
pub fn default(theme: &Theme) -> Appearance {
    let palette = theme.extended_palette();

    Appearance {
        background_color: palette.background.base.color,
        text_color: palette.background.base.text,
    }
}

// layershell application
pub trait Application: Sized {
    /// The [`Executor`] that will run commands and subscriptions.
    ///
    /// The [default executor] can be a good starting point!
    ///
    /// [`Executor`]: Self::Executor
    /// [default executor]: iced::executor::Default
    type Executor: iced::Executor;

    /// The type of __messages__ your [`Application`] will produce.
    type Message: std::fmt::Debug + Send;

    /// The theme of your [`Application`].
    type Theme: Default + DefaultStyle;

    /// The data needed to initialize your [`Application`].
    type Flags;

    /// Initializes the [`Application`] with the flags provided to
    /// [`run`] as part of the [`Settings`].
    ///
    /// Here is where you should return the initial state of your app.
    ///
    /// Additionally, you can return a [`Task`] if you need to perform some
    /// async action in the background on startup. This is useful if you want to
    /// load state from a file, perform an initial HTTP request, etc.
    ///
    /// [`run`]: Self::run
    fn new(flags: Self::Flags) -> (Self, Task<Self::Message>);

    /// Returns the current title of the [`Application`].
    ///
    /// This title can be dynamic! The runtime will automatically update the
    /// title of your application when necessary.
    fn namespace(&self) -> String;

    /// Handles a __message__ and updates the state of the [`Application`].
    ///
    /// This is where you define your __update logic__. All the __messages__,
    /// produced by either user interactions or commands, will be handled by
    /// this method.
    ///
    /// Any [`Task`] returned will be executed immediately in the background.
    fn update(&mut self, message: Self::Message) -> Task<Self::Message>;

    /// Returns the widgets to display in the [`Application`].
    ///
    /// These widgets can produce __messages__ based on user interaction.
    fn view(&self) -> Element<'_, Self::Message, Self::Theme, iced::Renderer>;

    /// Returns the current [`Theme`] of the [`Application`].
    ///
    /// [`Theme`]: Self::Theme
    fn theme(&self) -> Self::Theme {
        Self::Theme::default()
    }

    /// Returns the current `Style` of the [`Theme`].
    ///
    /// [`Theme`]: Self::Theme
    fn style(&self, theme: &Self::Theme) -> Appearance {
        theme.default_style()
    }

    /// Returns the event [`Subscription`] for the current state of the
    /// application.
    ///
    /// A [`Subscription`] will be kept alive as long as you keep returning it,
    /// and the __messages__ produced will be handled by
    /// [`update`](#tymethod.update).
    ///
    /// By default, this method returns an empty [`Subscription`].
    fn subscription(&self) -> Subscription<Self::Message> {
        Subscription::none()
    }

    /// Returns the scale factor of the [`Application`].
    ///
    /// It can be used to dynamically control the size of the UI at runtime
    /// (i.e. zooming).
    ///
    /// For instance, a scale factor of `2.0` will make widgets twice as big,
    /// while a scale factor of `0.5` will shrink them to half their size.
    ///
    /// By default, it returns `1.0`.
    fn scale_factor(&self) -> f64 {
        1.0
    }

    /// Runs the [`Application`].
    ///
    /// On native platforms, this method will take control of the current thread
    /// until the [`Application`] exits.
    ///
    /// On the web platform, this method __will NOT return__ unless there is an
    /// [`Error`] during startup.
    ///
    /// [`Error`]: crate::Error
    fn run(settings: Settings<Self::Flags>) -> Result<(), error::Error>
    where
        Self: 'static,
        Self::Message: 'static + TryInto<LayershellCustomActions, Error = Self::Message>,
    {
        #[allow(clippy::needless_update)]
        let renderer_settings = iced_graphics::Settings {
            default_font: settings.default_font,
            default_text_size: settings.default_text_size,
            antialiasing: if settings.antialiasing {
                Some(iced_graphics::Antialiasing::MSAAx4)
            } else {
                None
            },
            ..iced_graphics::Settings::default()
        };

        application::run::<Instance<Self>, Self::Executor, iced_renderer::Compositor>(
            settings,
            renderer_settings,
        )
    }
}

struct Instance<A: Application>(A);

impl<A> iced_runtime::Program for Instance<A>
where
    A: Application,
{
    type Message = A::Message;
    type Theme = A::Theme;
    type Renderer = iced_renderer::Renderer;

    fn update(&mut self, message: Self::Message) -> Task<Self::Message> {
        self.0.update(message)
    }

    fn view(&self) -> Element<'_, Self::Message, Self::Theme, Self::Renderer> {
        self.0.view()
    }
}

impl<A> application::Application for Instance<A>
where
    A: Application,
    A::Message: 'static + TryInto<LayershellCustomActions, Error = A::Message>,
{
    type Flags = A::Flags;

    fn new(flags: Self::Flags) -> (Self, Task<A::Message>) {
        let (app, command) = A::new(flags);

        (Instance(app), command)
    }

    fn namespace(&self) -> String {
        self.0.namespace()
    }

    fn theme(&self) -> A::Theme {
        self.0.theme()
    }

    fn style(&self, theme: &Self::Theme) -> Appearance {
        self.0.style(theme)
    }

    fn subscription(&self) -> Subscription<Self::Message> {
        self.0.subscription()
    }

    fn scale_factor(&self) -> f64 {
        self.0.scale_factor()
    }
}

pub trait MultiApplication: Sized {
    /// The [`Executor`] that will run commands and subscriptions.
    ///
    /// The [default executor] can be a good starting point!
    ///
    /// [`Executor`]: Self::Executor
    /// [default executor]: iced::executor::Default
    type Executor: iced::Executor;

    /// The type of __messages__ your [`Application`] will produce.
    type Message: std::fmt::Debug + Send;

    /// The data needed to initialize your [`Application`].
    type Flags;

    type WindowInfo;

    type Theme: Default + DefaultStyle;

    /// Initializes the [`Application`] with the flags provided to
    /// [`run`] as part of the [`Settings`].
    ///
    /// Here is where you should return the initial state of your app.
    ///
    /// Additionally, you can return a [`Task`] if you need to perform some
    /// async action in the background on startup. This is useful if you want to
    /// load state from a file, perform an initial HTTP request, etc.
    ///
    /// [`run`]: Self::run
    fn new(flags: Self::Flags) -> (Self, Task<Self::Message>);

    /// Returns the current title of the `window` of the [`Application`].
    ///
    /// This title can be dynamic! The runtime will automatically update the
    /// title of your window when necessary.
    fn namespace(&self) -> String;

    fn id_info(&self, _id: iced_core::window::Id) -> Option<Self::WindowInfo> {
        None
    }

    fn set_id_info(&mut self, _id: iced_core::window::Id, _info: Self::WindowInfo) {}
    fn remove_id(&mut self, _id: iced_core::window::Id) {}
    /// Handles a __message__ and updates the state of the [`Application`].
    ///
    /// This is where you define your __update logic__. All the __messages__,
    /// produced by either user interactions or commands, will be handled by
    /// this method.
    ///
    /// Any [`Task`] returned will be executed immediately in the background.
    fn update(&mut self, message: Self::Message) -> Task<Self::Message>;

    /// Returns the widgets to display in the `window` of the [`Application`].
    ///
    /// These widgets can produce __messages__ based on user interaction.
    fn view(
        &self,
        window: iced::window::Id,
    ) -> Element<'_, Self::Message, Self::Theme, iced::Renderer>;

    /// Returns the current [`Theme`] of the `window` of the [`Application`].
    ///
    /// [`Theme`]: Self::Theme
    #[allow(unused_variables)]
    fn theme(&self) -> Self::Theme {
        Self::Theme::default()
    }

    /// Returns the current `Style` of the [`Theme`].
    ///
    /// [`Theme`]: Self::Theme
    fn style(&self, theme: &Self::Theme) -> Appearance {
        theme.default_style()
    }

    /// Returns the event [`Subscription`] for the current state of the
    /// application.
    ///
    /// A [`Subscription`] will be kept alive as long as you keep returning it,
    /// and the __messages__ produced will be handled by
    /// [`update`](#tymethod.update).
    ///
    /// By default, this method returns an empty [`Subscription`].
    fn subscription(&self) -> Subscription<Self::Message> {
        Subscription::none()
    }

    /// Returns the scale factor of the `window` of the [`Application`].
    ///
    /// It can be used to dynamically control the size of the UI at runtime
    /// (i.e. zooming).
    ///
    /// For instance, a scale factor of `2.0` will make widgets twice as big,
    /// while a scale factor of `0.5` will shrink them to half their size.
    ///
    /// By default, it returns `1.0`.
    #[allow(unused_variables)]
    fn scale_factor(&self, window: iced::window::Id) -> f64 {
        1.0
    }

    /// Runs the multi-window [`Application`].
    ///
    /// On native platforms, this method will take control of the current thread
    /// until the [`Application`] exits.
    ///
    /// On the web platform, this method __will NOT return__ unless there is an
    /// [`Error`] during startup.
    ///
    /// [`Error`]: crate::Error
    fn run(settings: Settings<Self::Flags>) -> Result<(), error::Error>
    where
        Self: 'static,
        <Self as MultiApplication>::WindowInfo: Clone,
        Self::Message: 'static
            + TryInto<LayershellCustomActionsWithIdAndInfo<Self::WindowInfo>, Error = Self::Message>,
    {
        #[allow(clippy::needless_update)]
        let renderer_settings = iced_graphics::Settings {
            default_font: settings.default_font,
            default_text_size: settings.default_text_size,
            antialiasing: if settings.antialiasing {
                Some(iced_graphics::Antialiasing::MSAAx4)
            } else {
                None
            },
            ..iced_graphics::Settings::default()
        };

        multi_window::run::<MultiInstance<Self>, Self::Executor, iced_renderer::Compositor>(
            settings,
            renderer_settings,
        )
    }
}

struct MultiInstance<A: MultiApplication>(A);

impl<A> iced_runtime::multi_window::Program for MultiInstance<A>
where
    A: MultiApplication,
{
    type Message = A::Message;
    type Theme = A::Theme;
    type Renderer = iced_renderer::Renderer;

    fn update(&mut self, message: Self::Message) -> Task<Self::Message> {
        self.0.update(message)
    }

    fn view(
        &self,
        window: iced::window::Id,
    ) -> Element<'_, Self::Message, Self::Theme, Self::Renderer> {
        self.0.view(window)
    }
}

impl<A> multi_window::Application for MultiInstance<A>
where
    A: MultiApplication,
{
    type Flags = A::Flags;

    type WindowInfo = A::WindowInfo;

    fn new(flags: Self::Flags) -> (Self, Task<A::Message>) {
        let (app, command) = A::new(flags);

        (MultiInstance(app), command)
    }

    fn namespace(&self) -> String {
        self.0.namespace()
    }

    fn theme(&self) -> A::Theme {
        self.0.theme()
    }

    fn style(&self, theme: &Self::Theme) -> Appearance {
        self.0.style(theme)
    }

    fn subscription(&self) -> Subscription<Self::Message> {
        self.0.subscription()
    }

    fn scale_factor(&self, window: iced::window::Id) -> f64 {
        self.0.scale_factor(window)
    }

    fn id_info(&self, id: iced_core::window::Id) -> Option<Self::WindowInfo> {
        self.0.id_info(id)
    }

    fn set_id_info(&mut self, id: iced_core::window::Id, info: Self::WindowInfo) {
        self.0.set_id_info(id, info)
    }
    fn remove_id(&mut self, id: iced_core::window::Id) {
        self.0.remove_id(id)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use iced::widget::text;

    struct TestApp {
        counter: i32,
        scale_factor: f64,
        namespace: String,
    }

    #[derive(Debug)]
    enum TestMessage {
        Increment,
        Decrement,
    }

    impl Application for TestApp {
        type Executor = iced::executor::Default;
        type Message = TestMessage;
        type Theme = Theme;
        type Flags = (i32, f64, String);

        fn new(flags: Self::Flags) -> (Self, Task<Self::Message>) {
            let (counter, scale_factor, namespace) = flags;
            (
                Self {
                    counter,
                    scale_factor,
                    namespace,
                },
                Task::none(),
            )
        }

        fn namespace(&self) -> String {
            self.namespace.clone()
        }

        fn update(&mut self, message: Self::Message) -> Task<Self::Message> {
            match message {
                TestMessage::Increment => self.counter += 1,
                TestMessage::Decrement => self.counter -= 1,
            }
            Task::none()
        }

        fn view(&self) -> Element<'_, Self::Message, Self::Theme, iced::Renderer> {
            text("Test").into()
        }

        fn scale_factor(&self) -> f64 {
            self.scale_factor
        }
    }

    // Test default appearance
    #[test]
    fn test_default_appearance() {
        let theme = Theme::default();
        let appearance = theme.default_style();
        assert_eq!(appearance.background_color, Color::WHITE);
        assert_eq!(appearance.text_color, Color::BLACK);
    }

    // Test namespace
    #[test]
    fn test_namespace() {
        let app = TestApp::new((0, 1.0, "Test namespace".into())).0;
        assert_eq!(app.namespace(), "Test namespace");
    }

    // Test scale factor
    #[test]
    fn test_scale_factor() {
        let app = TestApp::new((0, 2.0, "Test scale factor".into())).0;
        assert_eq!(app.scale_factor(), 2.0);
    }

    // Test update increment
    #[test]
    fn test_update_increment() {
        let mut app = TestApp::new((0, 1.0, "Test Update".into())).0;
        let _ = app.update(TestMessage::Increment);
        assert_eq!(app.counter, 1);
    }

    // Test update decrement
    #[test]
    fn test_update_decrement() {
        let mut app = TestApp::new((5, 1.0, "Test Update".into())).0;
        let _ = app.update(TestMessage::Decrement);
        assert_eq!(app.counter, 4);
    }
}
