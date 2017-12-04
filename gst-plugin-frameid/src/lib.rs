
#![crate_type = "cdylib"]

extern crate glib;
#[macro_use]
extern crate gst_plugin;
#[macro_use]
extern crate gstreamer as gst;
extern crate gstreamer_video as gst_video;
extern crate qrcode;
extern crate image;
extern crate quirc;

mod frameid;
mod frameidfilter;

fn plugin_init(plugin: &gst::Plugin) -> bool {
    frameid::register(plugin);
    frameidfilter::register(plugin);
    true
}

plugin_define!(
    b"rsframeid\0",
    b"Rust FrameId Plugin\0",
    plugin_init,
    b"1.0\0",
    b"MIT/X11\0",
    b"rsaudiofx\0",
    b"rsaudiofx\0",
    b"null\0",
    b"2017-12-01\0"
);

