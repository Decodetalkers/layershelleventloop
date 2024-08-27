use futures::future::pending;
use iced::widget::{button, text};
use iced::window::Id;
use iced::{Command, Element, Theme};
use iced_layershell::actions::{
    LayershellCustomActionsWithIdAndInfo, LayershellCustomActionsWithInfo,
};
use iced_runtime::command::Action;
use iced_runtime::window::Action as WindowAction;

use iced_layershell::reexport::{Anchor, KeyboardInteractivity, Layer, NewLayerShellSettings};
use iced_layershell::settings::Settings;
use iced_layershell::MultiApplication;
use zbus::{interface, ConnectionBuilder};

use futures::channel::mpsc::Sender;

type LaLaShellIdAction = LayershellCustomActionsWithIdAndInfo<()>;
type LalaShellAction = LayershellCustomActionsWithInfo<()>;

struct Counter {
    window_shown: bool,
}
pub fn main() -> Result<(), iced_layershell::Error> {
    Counter::run(Settings::default())
}
#[derive(Debug, Clone)]
enum Message {
    NewWindow,
    CloseWindow(Id),
}
impl MultiApplication for Counter {
    type Message = Message;
    type Flags = ();
    type Theme = Theme;
    type Executor = iced::executor::Default;
    type WindowInfo = ();

    const BACKGROUND_MODE: bool = true;

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
            },
            Command::none(),
        )
    }

    fn namespace(&self) -> String {
        String::from("Counter - Iced")
    }
    fn view(&self, id: iced::window::Id) -> Element<Message> {
        button(text("hello"))
            .on_press(Message::CloseWindow(id))
            .into()
    }
    fn subscription(&self) -> iced::Subscription<Self::Message> {
        iced::subscription::channel(std::any::TypeId::of::<()>(), 100, |sender| async move {
            // setup the object server
            let _connection = ConnectionBuilder::session()
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
    }
    fn update(&mut self, message: Self::Message) -> Command<Self::Message> {
        match message {
            Message::CloseWindow(id) => Command::single(Action::Window(WindowAction::Close(id))),
            Message::NewWindow => {
                if self.window_shown {
                    return Command::none();
                }
                Command::single(
                    LaLaShellIdAction::new(
                        iced::window::Id::MAIN,
                        LalaShellAction::NewLayerShell((
                            NewLayerShellSettings {
                                size: None,
                                exclusive_zone: None,
                                anchor: Anchor::Right | Anchor::Top | Anchor::Left | Anchor::Bottom,
                                layer: Layer::Top,
                                margin: Some((100, 100, 100, 100)),
                                keyboard_interactivity: KeyboardInteractivity::None,
                                use_last_output: false,
                            },
                            (),
                        )),
                    )
                    .into(),
                )
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