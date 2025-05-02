use crate::compositor::Surface;
use crate::globals::GlobalData;

use log::warn;

use std::num::Wrapping;
use std::sync::Mutex;

use wayland_client::globals::{BindError, GlobalList};
use wayland_client::protocol::wl_seat::WlSeat;
use wayland_client::WEnum;

use wayland_client::{Connection, Dispatch, Proxy, QueueHandle};
use wayland_protocols::xdg::shell::client::xdg_popup::XdgPopup;
use wayland_protocols::wp::text_input::zv3::client::zwp_text_input_v3::{
    ChangeCause, ContentHint, ContentPurpose,
};

use wl_input_method::input_method::xx as zwp_input_method_v3;

pub use zwp_input_method_v3::client::xx_input_method_v1::XxInputMethodV1 as ZwpInputMethodV2;
pub use zwp_input_method_v3::client::xx_input_popup_surface_v2::XxInputPopupSurfaceV2;
use zwp_input_method_v3::client::{
    xx_input_method_manager_v2::{self as zwp_input_method_manager_v2, XxInputMethodManagerV2 as ZwpInputMethodManagerV2},
    xx_input_method_v1 as zwp_input_method_v2,
    xx_input_popup_surface_v2,
};

#[derive(Debug)]
pub struct Rectangle {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

#[derive(Debug)]
pub struct InputMethodManager {
    manager: ZwpInputMethodManagerV2,
}

impl InputMethodManager {
    /// Bind `zwp_input_method_v2` global, if it exists
    pub fn bind<D>(globals: &GlobalList, qh: &QueueHandle<D>) -> Result<Self, BindError>
    where
        D: Dispatch<ZwpInputMethodManagerV2, GlobalData> + 'static,
    {
        let manager = globals.bind(qh, 1..=1, GlobalData)?;
        Ok(Self { manager })
    }

    /// Request a new input zwp_input_method_v2 object associated with a given
    /// seat.
    pub fn get_input_method<State>(&self, qh: &QueueHandle<State>, seat: &WlSeat) -> InputMethod
    where
        State: Dispatch<ZwpInputMethodV2, InputMethodData, State> + 'static,
    {
        InputMethod {
            input_method: self.manager.get_input_method(
                seat,
                qh,
                InputMethodData::new(seat.clone()),
            ),
        }
    }
}

impl<D> Dispatch<zwp_input_method_manager_v2::XxInputMethodManagerV2, GlobalData, D>
    for InputMethodManager
where
    D: Dispatch<zwp_input_method_manager_v2::XxInputMethodManagerV2, GlobalData>
        + InputMethodHandler,
{
    fn event(
        _data: &mut D,
        _manager: &zwp_input_method_manager_v2::XxInputMethodManagerV2,
        _event: zwp_input_method_manager_v2::Event,
        _: &GlobalData,
        _conn: &Connection,
        _qh: &QueueHandle<D>,
    ) {
        unreachable!()
    }
}

#[derive(Debug)]
pub struct InputMethod {
    input_method: ZwpInputMethodV2,
}

impl InputMethod {
    pub fn set_preedit_string(&self, text: String, cursor: CursorPosition) {
        // TODO: should this enforce indices on codepoint boundaries?
        let (start, end) = match cursor {
            CursorPosition::Hidden => (-1, -1),
            CursorPosition::Visible { start, end } => (
                // This happens only for cursor values in the upper usize range.
                // Such values are most likely bugs already,
                // so it's not a problem if one of the cursors weirdly lands at 0 sometimes.
                start.try_into().unwrap_or(0),
                end.try_into().unwrap_or(0),
            ),
        };
        self.input_method.set_preedit_string(text, start, end)
    }

    pub fn commit_string(&self, text: String) {
        self.input_method.commit_string(text)
    }

    pub fn delete_surrounding_text(&self, before_length: u32, after_length: u32) {
        // TODO: this has 2 separate behaviours:
        // one when preedit text is supported,
        // and a completely different one when it is not supported
        // and the input method doesn't know what bytes it deletes.
        // Not sure how or whether this should be reflected here.
        self.input_method.delete_surrounding_text(before_length, after_length)
    }

    pub fn commit(&self) {
        let data = self.input_method.data::<InputMethodData>().unwrap();
        let inner = data.inner.lock().unwrap();
        self.input_method.commit(inner.serial.0)
    }

    pub fn get_popup(&self, popup: &XdgPopup) {
        self.input_method.get_popup(popup)
    }

    pub fn get_input_popup_surface<D>(
        &self,
        qh: &QueueHandle<D>,
        surface: impl Into<Surface>,
    ) -> Popup
        where D: Dispatch<XxInputPopupSurfaceV2, PopupData> + 'static
    {
        let surface = surface.into();
        Popup {
            popup: self.input_method.get_input_popup_surface(
                surface.wl_surface(),
                qh,
                PopupData{ inner: Mutex::new(PopupDataInner{}) },
            ),
            surface,
        }
    }
}

#[derive(Debug)]
pub struct InputMethodData {
    seat: WlSeat,

    inner: Mutex<InputMethodDataInner>,
}

impl InputMethodData {
    /// Create the new touch data associated with the given seat.
    pub fn new(seat: WlSeat) -> Self {
        Self {
            seat,
            inner: Mutex::new(InputMethodDataInner {
                pending_state: Default::default(),
                current_state: Default::default(),
                serial: Wrapping(0),
            }),
        }
    }

    /// Get the associated seat from the data.
    pub fn seat(&self) -> &WlSeat {
        &self.seat
    }
}

#[derive(Debug)]
struct InputMethodDataInner {
    pending_state: InputMethodEventState,
    current_state: InputMethodEventState,
    serial: Wrapping<u32>,
}

/// Stores incoming interface state.
#[derive(Debug, Clone, PartialEq)]
pub struct InputMethodEventState {
    pub surrounding: SurroundingText,
    pub content_purpose: ContentPurpose,
    pub content_hint: ContentHint,
    pub text_change_cause: ChangeCause,
    pub active: Active,
}

impl Default for InputMethodEventState {
    fn default() -> Self {
        Self {
            surrounding: SurroundingText::default(),
            content_hint: ContentHint::empty(),
            content_purpose: ContentPurpose::Normal,
            text_change_cause: ChangeCause::InputMethod,
            active: Active::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CursorPosition {
    Hidden,
    Visible { start: usize, end: usize },
}

#[derive(Default, Clone, Debug, PartialEq)]
pub struct SurroundingText {
    pub text: String,
    pub cursor: u32,
    pub anchor: u32,
}

/// State machine for determining the capabilities of a text input
#[derive(Clone, Debug, Copy, PartialEq)]
pub enum Active {
    Inactive,
    NegotiatingCapabilities { surrounding_text: bool, content_type: bool },
    Active { surrounding_text: bool, content_type: bool },
}

impl Default for Active {
    fn default() -> Self {
        Self::Inactive
    }
}

impl Active {
    fn with_active(self) -> Self {
        match self {
            Self::Inactive => {
                Self::NegotiatingCapabilities { content_type: false, surrounding_text: false }
            }
            other => other,
        }
    }

    fn with_surrounding_text(self) -> Self {
        match self {
            Self::Inactive => Self::Inactive,
            Self::NegotiatingCapabilities { content_type, .. } => {
                Self::NegotiatingCapabilities { content_type, surrounding_text: true }
            }
            active @ Self::Active { .. } => active,
        }
    }

    fn with_content_type(self) -> Self {
        match self {
            Self::Inactive => Self::Inactive,
            Self::NegotiatingCapabilities { surrounding_text, .. } => {
                Self::NegotiatingCapabilities { content_type: true, surrounding_text }
            }
            active @ Self::Active { .. } => active,
        }
    }

    fn with_done(self) -> Self {
        match self {
            Self::Inactive => Self::Inactive,
            Self::NegotiatingCapabilities { surrounding_text, content_type } => {
                Self::Active { content_type, surrounding_text }
            }
            active @ Self::Active { .. } => active,
        }
    }
}


use wayland_client::protocol::wl_surface;


// FIXME: is Clone the right thing here? XdgPopup clones an inner Arc. What happens when cloning XxInputPopupSurfaceV2? Is that equivalent to cloning Arc?
#[derive(Debug, PartialEq, Eq)]
pub struct Popup {
    popup: XxInputPopupSurfaceV2,
    surface: Surface,
}

impl Popup {
    pub fn wl_surface(&self) -> &wl_surface::WlSurface {
        &self.surface.wl_surface()
    }
}

impl<D> Dispatch<XxInputPopupSurfaceV2, PopupData, D>
    for Popup
where
    D: Dispatch<XxInputPopupSurfaceV2, PopupData>
        + InputMethodHandler,
{
    fn event(
        data: &mut D,
        popup: &XxInputPopupSurfaceV2,
        event: xx_input_popup_surface_v2::Event,
        _: &PopupData,
        _conn: &Connection,
        qh: &QueueHandle<D>,
    ) {
        use xx_input_popup_surface_v2::Event;

        match event {
            Event::TextInputRectangle{x, y, width, height} => {
                // TODO: this should be sent to the client after InputMethod.commit()
                data.handle_text_input_rectangle(qh, &popup, Rectangle {x, y, width, height})
            },
            _ => unreachable!(),
        };
    }
}

#[derive(Debug)]
pub struct PopupData {
    inner: Mutex<PopupDataInner>,
}

#[derive(Debug)]
struct PopupDataInner {
    //rectangle: Rectangle,
}

#[macro_export]
macro_rules! delegate_input_method_v3 {
    ($(@<$( $lt:tt $( : $clt:tt $(+ $dlt:tt )* )? ),+>)? $ty: ty) => {
        $crate::reexports::client::delegate_dispatch!($(@< $( $lt $( : $clt $(+ $dlt )* )? ),+ >)? $ty: [
            $crate::reexports::wl_input_method::input_method::xx::client::xx_input_method_manager_v2::XxInputMethodManagerV2: $crate::globals::GlobalData
        ] => $crate::seat::input_method_v3::InputMethodManager);
        $crate::reexports::client::delegate_dispatch!($(@< $( $lt $( : $clt $(+ $dlt )* )? ),+ >)? $ty: [
            $crate::reexports::wl_input_method::input_method::xx::client::xx_input_method_v1::XxInputMethodV1: $crate::seat::input_method_v3::InputMethodData
        ] => $crate::seat::input_method_v3::InputMethod);
        $crate::reexports::client::delegate_dispatch!($(@< $( $lt $( : $clt $(+ $dlt )* )? ),+ >)? $ty: [
            $crate::reexports::wl_input_method::input_method::xx::client::xx_input_popup_surface_v2::XxInputPopupSurfaceV2: $crate::seat::input_method_v3::PopupData
        ] => $crate::seat::input_method_v3::Popup);
    };
}

pub trait InputMethodDataExt: Send + Sync {
    fn input_method_data(&self) -> &InputMethodData;
}

impl InputMethodDataExt for InputMethodData {
    fn input_method_data(&self) -> &InputMethodData {
        self
    }
}

pub trait InputMethodHandler: Sized {
    fn handle_done(
        &self,
        qh: &QueueHandle<Self>,
        input_method: &ZwpInputMethodV2,
        state: &InputMethodEventState,
    );
    fn handle_unavailable(&self, qh: &QueueHandle<Self>, input_method: &ZwpInputMethodV2);
    /// Remember this doesn't take effect until handle_done.
    fn handle_text_input_rectangle(
        &self,
        qh: &QueueHandle<Self>,
        popup: &XxInputPopupSurfaceV2,
        text_input_rectangle: Rectangle,
    );
}

impl<D, U> Dispatch<ZwpInputMethodV2, U, D> for InputMethod
where
    D: Dispatch<ZwpInputMethodV2, U> + InputMethodHandler,
    U: InputMethodDataExt,
{
    fn event(
        data: &mut D,
        input_method: &ZwpInputMethodV2,
        event: zwp_input_method_v2::Event,
        udata: &U,
        _conn: &Connection,
        qh: &QueueHandle<D>,
    ) {
        let mut imdata: std::sync::MutexGuard<'_, InputMethodDataInner> =
            udata.input_method_data().inner.lock().unwrap();

        use zwp_input_method_v2::Event;

        match event {
            Event::Activate => {
                imdata.pending_state = InputMethodEventState {
                    active: imdata.pending_state.active.with_active(),
                    ..Default::default()
                };
            }
            Event::Deactivate => {
                imdata.pending_state = Default::default();
            }
            Event::SurroundingText { text, cursor, anchor } => {
                imdata.pending_state = InputMethodEventState {
                    active: imdata.pending_state.active.with_surrounding_text(),
                    surrounding: SurroundingText { text, cursor, anchor },
                    ..imdata.pending_state.clone()
                }
            }
            Event::TextChangeCause { cause } => {
                imdata.pending_state = InputMethodEventState {
                    text_change_cause: match cause {
                        WEnum::Value(cause) => cause,
                        WEnum::Unknown(value) => {
                            warn!(
                                "Unknown `text_change_cause`: {}. Assuming not input method.",
                                value
                            );
                            ChangeCause::Other
                        }
                    },
                    ..imdata.pending_state.clone()
                }
            }
            Event::ContentType { hint, purpose } => {
                imdata.pending_state = InputMethodEventState {
                    active: imdata.pending_state.active.with_content_type(),
                    content_hint: match hint {
                        WEnum::Value(hint) => hint,
                        WEnum::Unknown(value) => {
                            warn!(
                                "Unknown content hints: 0b{:b}, ignoring.",
                                ContentHint::from_bits_retain(value)
                                    - ContentHint::from_bits_truncate(value)
                            );
                            ContentHint::from_bits_truncate(value)
                        }
                    },
                    content_purpose: match purpose {
                        WEnum::Value(v) => v,
                        WEnum::Unknown(value) => {
                            warn!("Unknown `content_purpose`: {}. Assuming `normal`.", value);
                            ContentPurpose::Normal
                        }
                    },
                    ..imdata.pending_state.clone()
                }
            }
            Event::Done => {
                imdata.pending_state = InputMethodEventState {
                    active: imdata.pending_state.active.with_done(),
                    ..imdata.pending_state.clone()
                };
                imdata.current_state = imdata.pending_state.clone();
                imdata.serial += 1;
                data.handle_done(qh, &input_method, &imdata.current_state)
            }
            Event::Unavailable => data.handle_unavailable(qh, &input_method),
            _ => unreachable!(),
        };
    }
}

#[cfg(test)]
mod test {
    use super::*;

    struct Handler {}

    impl InputMethodHandler for Handler {
        fn handle_done(
            &self,
            qh: &QueueHandle<Self>,
            input_method: &ZwpInputMethodV2,
            state: &InputMethodEventState,
        ) {}
        
        fn handle_unavailable(&self, qh: &QueueHandle<Self>, input_method: &ZwpInputMethodV2) {}
    }

    delegate_input_method_v3!(Handler);

    fn assert_is_manager_delegate<T>()
        where T: wayland_client::Dispatch<crate::seat::unstable::zwp_input_method_v3::client::xx_input_method_manager_v2::XxInputMethodManagerV2, crate::globals::GlobalData>,
    {
    }

    fn assert_is_delegate<T>()
        where T: wayland_client::Dispatch<crate::seat::unstable::zwp_input_method_v3::client::xx_input_method_v1::XxInputMethodV1, InputMethodData>,
    {
    }
    
    fn assert_is_popup_delegate<T>()
        where T: wayland_client::Dispatch<crate::seat::unstable::zwp_input_method_v3::client::xx_input_popup_surface_v2::XxInputPopupSurfaceV2, PopupData>,
    {
    }

    #[test]
    fn test_valid_assignment() {
        assert_is_manager_delegate::<Handler>();
        assert_is_delegate::<Handler>();
    }
}
