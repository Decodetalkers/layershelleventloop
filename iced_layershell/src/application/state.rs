use crate::application::Application;
use crate::{Appearance, DefaultStyle};
use iced_core::{mouse as IcedMouse, Color, Point, Size};
use iced_graphics::Viewport;
use layershellev::keyboard::ModifiersState;

use crate::event::WindowEvent;
use layershellev::reexport::wp_viewport::WpViewport;

pub struct State<A: Application>
where
    A::Theme: DefaultStyle,
{
    application_scale_factor: f64,
    wayland_scale_factor: f64,
    viewport: Viewport,
    viewport_version: usize,
    theme: A::Theme,
    appearance: Appearance,
    mouse_position: Option<Point>,
    modifiers: ModifiersState,
    wpviewport: WpViewport,
}

impl<A: Application> State<A>
where
    A::Theme: DefaultStyle,
{
    pub fn new(application: &A, window: &layershellev::WindowStateSimple) -> Self {
        let application_scale_factor = application.scale_factor();
        let theme = application.theme();
        let appearance = application.style(&theme);

        let (width, height) = window.main_window().get_size();
        let wayland_scale_factor = 1.;

        let logical_size = Size::new(width, height);
        let viewport = viewport(logical_size, wayland_scale_factor, application_scale_factor);

        let wpviewport = window
            .gen_main_wrapper()
            .viewport
            .expect("iced_layershell need viewport support to better wayland dpi");
        set_wpviewport_destination(&wpviewport, logical_size);
        Self {
            application_scale_factor,
            wayland_scale_factor,
            viewport,
            viewport_version: 0,
            theme,
            appearance,
            mouse_position: None,
            modifiers: ModifiersState::default(),
            wpviewport,
        }
    }

    pub fn modifiers(&self) -> ModifiersState {
        self.modifiers
    }

    pub fn update_view_port(&mut self, width: u32, height: u32, scale: f64) {
        let logical_size = Size::new(width, height);
        if self.logical_size_u32() == logical_size && self.wayland_scale_factor == scale {
            return;
        }
        self.wayland_scale_factor = scale;
        self.resize_viewport(logical_size);
    }

    pub fn viewport(&self) -> &Viewport {
        &self.viewport
    }

    pub fn physical_size(&self) -> Size<u32> {
        self.viewport.physical_size()
    }

    pub fn logical_size(&self) -> Size<f32> {
        self.viewport.logical_size()
    }

    pub fn scale_factor(&self) -> f64 {
        self.viewport.scale_factor()
    }

    pub fn text_color(&self) -> Color {
        self.appearance.text_color
    }

    pub fn background_color(&self) -> Color {
        self.appearance.background_color
    }

    pub fn theme(&self) -> &A::Theme {
        &self.theme
    }

    pub fn cursor(&self) -> IcedMouse::Cursor {
        self.mouse_position
            .map(IcedMouse::Cursor::Available)
            .unwrap_or(IcedMouse::Cursor::Unavailable)
    }

    pub fn update(&mut self, event: &WindowEvent) {
        match event {
            WindowEvent::CursorLeft | WindowEvent::TouchUp { .. } => {
                self.mouse_position = None;
            }
            WindowEvent::CursorMoved { x, y }
            | WindowEvent::CursorEnter { x, y }
            | WindowEvent::TouchMotion { x, y, .. }
            | WindowEvent::TouchDown { x, y, .. } => {
                self.mouse_position = Some(Point::new(*x as f32, *y as f32));
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                self.modifiers = *modifiers;
            }
            WindowEvent::ScaleFactorChanged {
                scale_float,
                scale_u32: _,
            } => {
                self.wayland_scale_factor = *scale_float;
                self.resize_viewport(self.logical_size_u32());
            }
            _ => {}
        }
    }

    pub fn synchronize(&mut self, application: &A) {
        let new_scale_factor = application.scale_factor();
        if self.application_scale_factor != new_scale_factor {
            self.application_scale_factor = new_scale_factor;
            self.resize_viewport(self.logical_size_u32());
        }
        self.theme = application.theme();
        self.appearance = application.style(&self.theme);
    }

    fn resize_viewport(&mut self, logical_size: Size<u32>) {
        self.viewport = viewport(
            logical_size,
            self.wayland_scale_factor,
            self.application_scale_factor,
        );

        self.viewport_version = self.viewport_version.wrapping_add(1);

        set_wpviewport_destination(&self.wpviewport, self.logical_size_u32());
    }

    fn logical_size_u32(&self) -> Size<u32> {
        // physical_size = (orig_logical_size as f64 * factor).ceil()
        // logical_sizea = physical_size as f64 / factor as f32
        // logical_size >= orig_logical_size
        let logical_size = self.viewport.logical_size();
        Size::new(
            logical_size.width.floor() as u32,
            logical_size.height.floor() as u32,
        )
    }
}

fn viewport(
    logical_size: Size<u32>,
    wayland_scale_factor: f64,
    application_scale_factor: f64,
) -> Viewport {
    let factor = wayland_scale_factor * application_scale_factor;
    let physical_size = Size::new(
        (logical_size.width as f64 * factor).ceil() as u32,
        (logical_size.height as f64 * factor).ceil() as u32,
    );
    Viewport::with_physical_size(physical_size, factor)
}

fn set_wpviewport_destination(wpviewport: &WpViewport, logical_size: Size<u32>) {
    if logical_size.width != 0 && logical_size.height != 0 {
        // set_destination(0, 0) will panic
        wpviewport.set_destination(logical_size.width as i32, logical_size.height as i32);
    }
}
