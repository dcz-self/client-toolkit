#[macro_use]
mod protocol_macro;

pub mod zwp_input_method_v3 {
    wayland_protocol!("./protocols/input-method-unstable-v2.xml", [wayland_protocols::wp::text_input::zv3, wayland_protocols::xdg::shell]);
}