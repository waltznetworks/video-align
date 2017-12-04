use glib;
use gst;
use gst::prelude::*;
use gst_video;

use gst_plugin::properties::*;
use gst_plugin::object::*;
use gst_plugin::element::*;
use gst_plugin::base_transform::*;

use std::i32;
use std::str;
use std::sync::Mutex;

use image::GrayImage;
use image::DynamicImage;
use quirc::QrCoder;

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
    info: gst_video::VideoInfo,
}

struct FrameIdFilter {
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

impl FrameIdFilter {
    fn new(_transform: &BaseTransform) -> Self {
        Self {
            cat: gst::DebugCategory::new(
                "rsframeidfilter",
                gst::DebugColorFlags::empty(),
                "Rust FrameId Filter image plugin",
            ),
            settings: Mutex::new(Default::default()),
            state: Mutex::new(None),
        }
    }

    fn class_init(klass: &mut BaseTransformClass) {
        klass.set_metadata(
            "FrameIdFilter",
            "Filter/Video",
            "Drops frames that don't have an specific frameid qrcode prefix and reports the ids of frames it finds",
            "Thiago Santos <thiagossantos@gmail.com>",
        );

        let caps = gst::Caps::new_simple(
            "video/x-raw",
            &[
                (
                    "format",
                    &gst::List::new(&[
                        &gst_video::VideoFormat::Rgb.to_string(),
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

    fn inspect_codes(&self, image : GrayImage) -> (gst::FlowReturn, String) {
        let settings = self.settings.lock().unwrap();

        let mut quirc = QrCoder::new().unwrap();
        let width  = image.width();
        let height = image.height();
        let codes  = quirc.codes(&image, width, height).unwrap();

        for code in codes {
            match code {
                Ok(code) => {
                    let s = match str::from_utf8(&code.payload) {
                        Ok(v) => v,
                        Err(e) => panic!("Invalid UTF-8 sequence: {}", e),
                    };
                    println!("Code: {:?}", s);
                    match settings.prefix {
                        Some(ref p) => {
                            if s.starts_with(p) {
                                return (gst::FlowReturn::Ok, s.to_owned());
                            }
                        },
                        None => return (gst::FlowReturn::Ok, s.to_owned())
                    }
                }
                Err(err) => println!("{:?}", err),
            }
        }

        // Drop
        (gst::FlowReturn::CustomSuccess, "".to_owned())
    }

}

impl ObjectImpl<BaseTransform> for FrameIdFilter {
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

impl ElementImpl<BaseTransform> for FrameIdFilter {}

impl BaseTransformImpl<BaseTransform> for FrameIdFilter {
    fn transform_ip(&self, element: &BaseTransform, buf: &mut gst::BufferRef) -> gst::FlowReturn {
        let mut state_guard = self.state.lock().unwrap();
        let state = match *state_guard {
            None => return gst::FlowReturn::NotNegotiated,
            Some(ref mut state) => state,
        };

        let map = match buf.map_readable() {
            None => return gst::FlowReturn::Error,
            Some(map) => map,
        };

        let mut image = DynamicImage::new_luma8(state.info.width(), state.info.height()).to_luma();
        let dimensions = image.dimensions();
        for y in 0..dimensions.1 {
            for x in 0..dimensions.0 {
                let baseindex : usize = 3 * (x + state.info.width() * y) as usize;
                let pixel = image.get_pixel_mut(x, y);
                pixel[0] = map.as_slice()[baseindex]/3 + map.as_slice()[baseindex+1]/3 + map.as_slice()[baseindex+2]/3;
            }
        }
        let ret = self.inspect_codes(image);

        match ret.0 {
            gst::FlowReturn::Ok => {
                let structure = gst::Structure::new("frameid-found", &[
                    ("frameid", &ret.1)]);
                element.post_message(&gst::Message::new_element(structure).src(Some(element)).build());
            },
            _ => {}
        }

        ret.0
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
        });

        true
    }
}

struct FrameIdFilterStatic;

impl ImplTypeStatic<BaseTransform> for FrameIdFilterStatic {
    fn get_name(&self) -> &str {
        "FrameIdFilter"
    }

    fn new(&self, element: &BaseTransform) -> Box<BaseTransformImpl<BaseTransform>> {
        FrameIdFilter::init(element)
    }

    fn class_init(&self, klass: &mut BaseTransformClass) {
        FrameIdFilter::class_init(klass);
    }
}

pub fn register(plugin: &gst::Plugin) {
    let frameid_static = FrameIdFilterStatic;
    let type_ = register_type(frameid_static);
    gst::Element::register(plugin, "rsframeidfilter", 0, type_);
}
