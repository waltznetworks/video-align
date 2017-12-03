use glib;
use gst;
use gst::prelude::*;
use gst_base;
use gst_base::prelude::*;
use gst_video;
use gst_video::prelude::*;

use gst_plugin::properties::*;
use gst_plugin::object::*;
use gst_plugin::element::*;
use gst_plugin::base_transform::*;

use std::{cmp, iter, i32, u64};
use std::sync::Mutex;

use qrcode::QrCode;
use image::Luma;

#[derive(Debug, Clone)]
struct Settings {
    pub prefix: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            prefix: None,
        }
    }
}

struct State {
    frame_index: u64,
    info: gst_video::VideoInfo,
}

struct FrameId {
    cat: gst::DebugCategory,
    settings: Mutex<Settings>,
    state: Mutex<Option<State>>,
}

static PROPERTIES: [Property; 1] = [
    Property::String(
        "prefix",
        "Prefix to add to frame index",
        "Prefix added to the qrcode before the frame number",
        None,
        PropertyMutability::ReadWrite,
    ),
];

impl FrameId {
    fn new(_transform: &BaseTransform) -> Self {
        Self {
            cat: gst::DebugCategory::new(
                "rsframeid",
                gst::DebugColorFlags::empty(),
                "Rust FrameId image tagger",
            ),
            settings: Mutex::new(Default::default()),
            state: Mutex::new(None),
        }
    }

    fn class_init(klass: &mut BaseTransformClass) {
        klass.set_metadata(
            "FrameId",
            "Filter/Effect/Video",
            "Adds a qrcode with an id to each frame",
            "Thiago Santos <thiagossantos@gmail.com>",
        );

        let caps = gst::Caps::new_simple(
            "video/x-raw",
            &[
                (
                    "format",
                    &gst::List::new(&[
                        &gst_video::VideoFormat::Rgbx.to_string(),
                    ]),
                ),
                ("width", &gst::IntRange::<i32>::new(0, i32::MAX)),
                ("height", &gst::IntRange::<i32>::new(0, i32::MAX)),
                ("framerate", &gst::FractionRange::new(gst::Fraction::new(0, 1), gst::Fraction::new(i32::MAX, 1))),
            ],
        );
        let src_pad_template = gst::PadTemplate::new(
            "src",
            gst::PadDirection::Src,
            gst::PadPresence::Always,
            &caps,
        );
        klass.add_pad_template(src_pad_template);

        let sink_pad_template = gst::PadTemplate::new(
            "sink",
            gst::PadDirection::Sink,
            gst::PadPresence::Always,
            &caps,
        );
        klass.add_pad_template(sink_pad_template);

        klass.install_properties(&PROPERTIES);

        klass.configure(BaseTransformMode::AlwaysInPlace, false, false);
    }

    fn init(element: &BaseTransform) -> Box<BaseTransformImpl<BaseTransform>> {
        let imp = Self::new(element);
        Box::new(imp)
    }
}

impl ObjectImpl<BaseTransform> for FrameId {
    fn set_property(&self, _obj: &glib::Object, id: u32, value: &glib::Value) {
        let prop = &PROPERTIES[id as usize];

        match *prop {
            Property::String("prefix", ..) => {
                let mut settings = self.settings.lock().unwrap();
                settings.prefix = value.get();
            }
            _ => unimplemented!(),
        }
    }

    fn get_property(&self, _obj: &glib::Object, id: u32) -> Result<glib::Value, ()> {
        let prop = &PROPERTIES[id as usize];

        match *prop {
            Property::String("prefix", ..) => {
                let settings = self.settings.lock().unwrap();
                Ok(settings.prefix.to_value())
            }
            _ => unimplemented!(),
        }
    }
}

impl ElementImpl<BaseTransform> for FrameId {}

impl BaseTransformImpl<BaseTransform> for FrameId {
    fn transform_ip(&self, _element: &BaseTransform, buf: &mut gst::BufferRef) -> gst::FlowReturn {
        let mut state_guard = self.state.lock().unwrap();
        let state = match *state_guard {
            None => return gst::FlowReturn::NotNegotiated,
            Some(ref mut state) => state,
        };

        let mut map = match buf.map_writable() {
            None => return gst::FlowReturn::Error,
            Some(map) => map,
        };

        let mut settings = self.settings.lock().unwrap();
        let mut text = match settings.prefix {
            None => "".to_owned(),
            Some(ref a) => a.clone(),
        };
        text.push_str(&state.frame_index.to_string());
        let code = QrCode::new(text.to_string()).unwrap();
        let image = code.render::<Luma<u8>>().quiet_zone(false).build();

        let dimensions = image.dimensions();
        for y in 0..dimensions.1 {
            for x in 0..dimensions.0 {
                let baseindex : usize = 4 * (x + state.info.width() * y) as usize;
                let pixel = image.get_pixel(x, y);
                map.as_mut_slice()[baseindex] = pixel[0];
                map.as_mut_slice()[baseindex + 1] = pixel[0];
                map.as_mut_slice()[baseindex + 2] = pixel[0];
            }
        }

        state.frame_index += 1;
        gst::FlowReturn::Ok
    }

    fn set_caps(&self, _element: &BaseTransform, incaps: &gst::Caps, outcaps: &gst::Caps) -> bool {
        if incaps != outcaps {
            return false;
        }

        let info = match gst_video::VideoInfo::from_caps(incaps) {
            None => return false,
            Some(info) => info,
        };

        *self.state.lock().unwrap() = Some(State {
            info: info,
            frame_index: 0
        });

        true
    }
}

struct FrameIdStatic;

impl ImplTypeStatic<BaseTransform> for FrameIdStatic {
    fn get_name(&self) -> &str {
        "FrameId"
    }

    fn new(&self, element: &BaseTransform) -> Box<BaseTransformImpl<BaseTransform>> {
        FrameId::init(element)
    }

    fn class_init(&self, klass: &mut BaseTransformClass) {
        FrameId::class_init(klass);
    }
}

pub fn register(plugin: &gst::Plugin) {
    let frameid_static = FrameIdStatic;
    let type_ = register_type(frameid_static);
    gst::Element::register(plugin, "rsframeid", 0, type_);
}
