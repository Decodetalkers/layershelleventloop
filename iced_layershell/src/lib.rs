pub mod application;

pub mod actions;
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
    pub use layershellev::NewLayerShellSettings;
}

use settings::Settings;

pub use error::Error;

use iced::Element;
use iced_futures::Subscription;
use iced_runtime::Command;
use iced_style::application::StyleSheet;

pub use sandbox::LayerShellSandbox;

// layershell application
pub trait Application: Sized {
    /// The [`Executor`] that will run commands and subscriptions.
    ///
    /// The [default executor] can be a good starting point!
    ///
    /// [`Executor`]: Self::Executor
    /// [default executor]: crate::executor::Default
    type Executor: iced::Executor;

    /// The type of __messages__ your [`Application`] will produce.
    type Message: std::fmt::Debug + Send;

    /// The theme of your [`Application`].
    type Theme: Default + StyleSheet;

    /// The data needed to initialize your [`Application`].
    type Flags;

    /// Initializes the [`Application`] with the flags provided to
    /// [`run`] as part of the [`Settings`].
    ///
    /// Here is where you should return the initial state of your app.
    ///
    /// Additionally, you can return a [`Command`] if you need to perform some
    /// async action in the background on startup. This is useful if you want to
    /// load state from a file, perform an initial HTTP request, etc.
    ///
    /// [`run`]: Self::run
    fn new(flags: Self::Flags) -> (Self, Command<Self::Message>);

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
    /// Any [`Command`] returned will be executed immediately in the background.
    fn update(&mut self, message: Self::Message) -> Command<Self::Message>;

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
    fn style(&self) -> <Self::Theme as StyleSheet>::Style {
        <Self::Theme as StyleSheet>::Style::default()
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
    {
        #[allow(clippy::needless_update)]
        let renderer_settings = iced_renderer::Settings {
            default_font: settings.default_font,
            default_text_size: settings.default_text_size,
            antialiasing: if settings.antialiasing {
                Some(iced_graphics::Antialiasing::MSAAx4)
            } else {
                None
            },
            ..iced_renderer::Settings::default()
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

    fn update(&mut self, message: Self::Message) -> Command<Self::Message> {
        self.0.update(message)
    }

    fn view(&self) -> Element<'_, Self::Message, Self::Theme, Self::Renderer> {
        self.0.view()
    }
}

impl<A> application::Application for Instance<A>
where
    A: Application,
{
    type Flags = A::Flags;

    fn new(flags: Self::Flags) -> (Self, Command<A::Message>) {
        let (app, command) = A::new(flags);

        (Instance(app), command)
    }

    fn namespace(&self) -> String {
        self.0.namespace()
    }

    fn theme(&self) -> A::Theme {
        self.0.theme()
    }

    fn style(&self) -> <A::Theme as StyleSheet>::Style {
        self.0.style()
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
    /// [default executor]: crate::executor::Default
    type Executor: iced::Executor;

    /// The type of __messages__ your [`Application`] will produce.
    type Message: std::fmt::Debug + Send;

    /// The data needed to initialize your [`Application`].
    type Flags;

    type WindowInfo;

    type Theme: Default + StyleSheet;

    /// Initializes the [`Application`] with the flags provided to
    /// [`run`] as part of the [`Settings`].
    ///
    /// Here is where you should return the initial state of your app.
    ///
    /// Additionally, you can return a [`Command`] if you need to perform some
    /// async action in the background on startup. This is useful if you want to
    /// load state from a file, perform an initial HTTP request, etc.
    ///
    /// [`run`]: Self::run
    fn new(flags: Self::Flags) -> (Self, Command<Self::Message>);

    /// Returns the current title of the `window` of the [`Application`].
    ///
    /// This title can be dynamic! The runtime will automatically update the
    /// title of your window when necessary.
    fn namespace(&self) -> String;

    fn id_info(&self, _id: iced_core::window::Id) -> Option<&Self::WindowInfo> {
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
    /// Any [`Command`] returned will be executed immediately in the background.
    fn update(&mut self, message: Self::Message) -> Command<Self::Message>;

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
    fn style(&self) -> <Self::Theme as StyleSheet>::Style {
        <Self::Theme as StyleSheet>::Style::default()
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
        <Self as MultiApplication>::WindowInfo: Clone + std::fmt::Debug,
    {
        #[allow(clippy::needless_update)]
        let renderer_settings = iced_renderer::Settings {
            default_font: settings.default_font,
            default_text_size: settings.default_text_size,
            antialiasing: if settings.antialiasing {
                Some(iced_graphics::Antialiasing::MSAAx4)
            } else {
                None
            },
            ..iced_renderer::Settings::default()
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

    fn update(&mut self, message: Self::Message) -> Command<Self::Message> {
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

    fn new(flags: Self::Flags) -> (Self, Command<A::Message>) {
        let (app, command) = A::new(flags);

        (MultiInstance(app), command)
    }

    fn namespace(&self) -> String {
        self.0.namespace()
    }

    fn theme(&self) -> A::Theme {
        self.0.theme()
    }

    fn style(&self) -> <Self::Theme as StyleSheet>::Style {
        self.0.style()
    }

    fn subscription(&self) -> Subscription<Self::Message> {
        self.0.subscription()
    }

    fn scale_factor(&self, window: iced::window::Id) -> f64 {
        self.0.scale_factor(window)
    }

    fn id_info(&self, id: iced_core::window::Id) -> Option<&Self::WindowInfo> {
        self.0.id_info(id)
    }

    fn set_id_info(&mut self, id: iced_core::window::Id, info: Self::WindowInfo) {
        self.0.set_id_info(id, info)
    }
    fn remove_id(&mut self, id: iced_core::window::Id) {
        self.0.remove_id(id)
    }
}
