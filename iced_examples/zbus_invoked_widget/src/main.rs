use futures::future::pending;
use iced::widget::{button, column, container, text, text_input};
use iced::window::Id;
use iced::{Element, Length, Task as Command, Theme};
use iced_layershell::actions::{
    LayershellCustomActionsWithIdAndInfo, LayershellCustomActionsWithInfo,
};
use iced_runtime::window::Action as WindowAction;
use iced_runtime::Action;

use iced_layershell::reexport::{
    Anchor, KeyboardInteractivity, Layer, LayerOutputSetting, NewLayerShellSettings,
};
use iced_layershell::settings::{LayerShellSettings, Settings, StartMode};
use iced_layershell::MultiApplication;
use zbus::{connection, interface};

use futures::channel::mpsc::Sender;

type LaLaShellIdAction = LayershellCustomActionsWithIdAndInfo<()>;
type LalaShellAction = LayershellCustomActionsWithInfo<()>;

struct Counter {
    window_shown: bool,
    text: String,
}
pub fn main() -> Result<(), iced_layershell::Error> {
    Counter::run(Settings {
        layer_settings: LayerShellSettings {
            start_mode: StartMode::Background,
            ..Default::default()
        },
        ..Default::default()
    })
}

#[derive(Debug, Clone)]
enum Message {
    NewWindow,
    TextInput(String),
    CloseWindow(Id),
}

impl TryInto<LaLaShellIdAction> for Message {
    type Error = Self;
    fn try_into(self) -> Result<LayershellCustomActionsWithIdAndInfo<()>, Self::Error> {
        match self {
            Self::NewWindow => Ok(LaLaShellIdAction::new(
                None,
                LalaShellAction::NewLayerShell((
                    NewLayerShellSettings {
                        size: None,
                        exclusive_zone: None,
                        anchor: Anchor::Right | Anchor::Top | Anchor::Left | Anchor::Bottom,
                        layer: Layer::Top,
                        margin: Some((100, 100, 100, 100)),
                        keyboard_interactivity: KeyboardInteractivity::OnDemand,
                        output_setting: LayerOutputSetting::None,
                        ..Default::default()
                    },
                    (),
                )),
            )),
            _ => Err(self),
        }
    }
}

impl MultiApplication for Counter {
    type Message = Message;
    type Flags = ();
    type Theme = Theme;
    type Executor = iced::executor::Default;
    type WindowInfo = ();

    fn set_id_info(&mut self, _id: iced_runtime::core::window::Id, _info: Self::WindowInfo) {
        self.window_shown = true;
    }

    fn remove_id(&mut self, _id: iced_runtime::core::window::Id) {
        self.window_shown = false;
    }

    fn new(_flags: Self::Flags) -> (Self, Command<Self::Message>) {
        (
            Self {
                window_shown: false,
                text: "type something".to_string(),
            },
            Command::none(),
        )
    }

    fn namespace(&self) -> String {
        String::from("Counter - Iced")
    }
    fn view(&self, id: iced::window::Id) -> Element<Message> {
        container(
            column![
                container(button(text("hello")).on_press(Message::CloseWindow(id)))
                    .width(Length::Fill)
                    .center_x(Length::Fill),
                container(
                    text_input("hello", &self.text)
                        .on_input(Message::TextInput)
                        .padding(10)
                        .width(300.)
                )
                .width(Length::Fill)
                .center_x(Length::Fill),
            ]
            .padding(10.),
        )
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .into()
    }
    fn subscription(&self) -> iced::Subscription<Self::Message> {
        iced::Subscription::run(|| {
            iced::stream::channel(100, |sender| async move {
                // setup the object server
                let _connection = connection::Builder::session()
                    .unwrap()
                    .name("zbus.iced.MyGreeter1")
                    .unwrap()
                    .serve_at("/org/zbus/MyGreeter1", Greeter { sender })
                    .unwrap()
                    .build()
                    .await
                    .unwrap();
                pending::<()>().await;
                unreachable!()
            })
        })
    }
    fn update(&mut self, message: Self::Message) -> Command<Self::Message> {
        match message {
            Message::CloseWindow(id) => {
                iced_runtime::task::effect(Action::Window(WindowAction::Close(id)))
            }
            Message::NewWindow => {
                if self.window_shown {
                    return Command::none();
                }
                Command::done(message)
            }
            Message::TextInput(text) => {
                self.text = text;
                Command::none()
            }
        }
    }
}

struct Greeter {
    sender: Sender<Message>,
}

#[interface(name = "org.zbus.MyGreeter1")]
impl Greeter {
    async fn say_hello(&mut self, name: &str) -> String {
        self.sender.try_send(Message::NewWindow).ok();
        format!("Hello {}!", name)
    }
}
