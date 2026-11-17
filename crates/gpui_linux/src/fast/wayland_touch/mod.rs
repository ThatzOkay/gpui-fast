mod contacts;

use super::{SerialKind, WaylandClientState, WaylandClientStatePtr};
use crate::linux::wayland::window::WaylandWindowStatePtr;
use contacts::Contacts;
use gpui::{PlatformInput, TouchEvent, point, px};
use wayland_client::{
    Connection, Dispatch, Proxy, QueueHandle,
    protocol::{wl_seat, wl_touch},
};

#[derive(Default)]
pub(super) struct State {
    device: Option<wl_touch::WlTouch>,
    pub contacts: Contacts<WaylandWindowStatePtr>,
}

type Events = Vec<(WaylandWindowStatePtr, TouchEvent)>;

pub(super) fn release(state: &mut State) -> Events {
    if let Some(device) = state.device.take() {
        device.release();
    }
    state.contacts.cancel();
    state.contacts.frame()
}

pub(super) fn capabilities(
    state: &mut WaylandClientState,
    seat: &wl_seat::WlSeat,
    capabilities: wl_seat::Capability,
    qh: &QueueHandle<WaylandClientStatePtr>,
) -> Events {
    if capabilities.contains(wl_seat::Capability::Touch) {
        if state.fast_touch.device.is_none() {
            state.fast_touch.device = Some(seat.get_touch(qh, ()));
            log::info!("Wayland touchscreen input enabled");
        }
        Vec::new()
    } else {
        release(&mut state.fast_touch)
    }
}

pub(super) fn dispatch(events: Events) {
    for (window, event) in events {
        window.handle_input(PlatformInput::Touch(event));
    }
}

impl Dispatch<wl_touch::WlTouch, ()> for WaylandClientStatePtr {
    fn event(
        this: &mut Self,
        _: &wl_touch::WlTouch,
        event: wl_touch::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let client = this.get_client();
        let mut state = client.borrow_mut();
        let events = match event {
            wl_touch::Event::Down {
                serial,
                surface,
                id,
                x,
                y,
                ..
            } => {
                if let Some(window) =
                    crate::fast::composition::wayland::get_input_window(&mut state, &surface.id())
                {
                    state.serial_tracker.update_with_focused_surface(
                        SerialKind::MousePress,
                        serial,
                        Some(window.surface()),
                    );
                    state
                        .fast_touch
                        .contacts
                        .down(id, window, point(px(x as f32), px(y as f32)));
                }
                Vec::new()
            }
            wl_touch::Event::Motion { id, x, y, .. } => {
                state
                    .fast_touch
                    .contacts
                    .motion(id, point(px(x as f32), px(y as f32)));
                Vec::new()
            }
            wl_touch::Event::Up { id, .. } => {
                state.fast_touch.contacts.up(id);
                Vec::new()
            }
            wl_touch::Event::Frame => state.fast_touch.contacts.frame(),
            wl_touch::Event::Cancel => {
                state.fast_touch.contacts.cancel();
                state.fast_touch.contacts.frame()
            }
            _ => Vec::new(),
        };
        drop(state);
        dispatch(events);
    }
}
