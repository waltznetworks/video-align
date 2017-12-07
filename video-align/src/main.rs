extern crate gstreamer as gst;
use gst::prelude::*;
extern crate gstreamer_video as gst_video;
extern crate gstreamer_app as gst_app;
extern crate gstreamer_sys as gst_ffi;
extern crate glib_sys as glib_ffi;
extern crate libc;

extern crate glib;
use glib::translate::*;
use glib::signal::SignalHandlerId;
macro_rules! callback_guard {
    () => (
                let _guard = ::glib::CallbackGuard::new();
                    )
}

extern crate failure;
use failure::Error;

use std::env;
use std::error::Error as StdError;
use std::boxed::Box as Box_;
use std::mem::transmute;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};

#[macro_use]
extern crate failure_derive;

#[derive(Debug, Fail)]
#[fail(display = "Missing element {}", _0)]
struct MissingElement(&'static str);

#[derive(Debug, Clone)]
struct Cropping {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32
}

#[derive(Debug)]
struct Config {
    reference: String,
    capture: String,
    cropping : Cropping,
}

impl Config {
    fn new(args: &[String]) -> Result<Config, &'static str> {
        let reference = args[1].clone();
        let capture = args[2].clone();
        let mut cropping = Cropping {left: 0, top: 0, right: 0, bottom: 0};
        if args.len() > 3 {
            cropping.left = args[3].parse::<i32>().unwrap();
        }
        if args.len() > 4 {
            cropping.top = args[4].parse::<i32>().unwrap();
        }
        if args.len() > 5 {
            cropping.right = args[5].parse::<i32>().unwrap();
        }
        if args.len() > 6 {
            cropping.bottom = args[6].parse::<i32>().unwrap();
        }

        Ok(Config { reference, capture, cropping } )
    }
}

unsafe extern "C" fn code_detected_trampoline(this: *mut gst_ffi::GstElement, object: *mut libc::c_char, f: glib_ffi::gpointer) -> bool {
    callback_guard!();
    let f: &&(Fn(&gst::Element, &str) -> bool + Send + 'static) = transmute(f);
        f(&from_glib_borrow(this), &String::from_glib_none(object))
}

fn connect_to_code_detected<F: Fn(&gst::Element, &str) -> bool + Send + 'static>(el : &gst::Element, f: F) -> SignalHandlerId {
    unsafe {
        let f: Box_<Box_<Fn(&gst::Element, &str) -> bool + Send + 'static>> = Box_::new(Box_::new(f));
            glib::signal::connect(el.to_glib_none().0, "code-detected",
                transmute(code_detected_trampoline as usize), Box_::into_raw(f) as *mut _)
    }
}

fn setup_pipeline(path : &String, cropping : &Cropping, codes : Arc<Mutex<HashSet<String>>>, filter_by_codes : bool) -> gst::Pipeline {
    let pipeline = gst::Pipeline::new(None);
    let uridec = gst::ElementFactory::make("uridecodebin", None).ok_or(MissingElement("uridecodebin")).unwrap();

    uridec.set_property("uri", &glib::Value::from(path)).unwrap();
    pipeline.add(&uridec).unwrap();

    let path_clone = path.clone();
    let pipeline_clone = pipeline.clone();
    let cropping_clone = cropping.clone();
    uridec.connect_pad_added(move |_, src_pad| {
        // FIXME post an error message if any of those fail instead of just doing unwrap()
        if !src_pad.get_current_caps().unwrap().get_structure(0).unwrap().get_name().contains("video") {
            return;
        }
        let queue = gst::ElementFactory::make("queue", None).unwrap();
        let videoconvert = gst::ElementFactory::make("videoconvert", None).unwrap();
        let zbar = gst::ElementFactory::make("zbar", None).unwrap();
        let videoconvert2 = gst::ElementFactory::make("videoconvert", None).unwrap();
        let videocrop = gst::ElementFactory::make("videocrop", None).unwrap();
        let capsfilter = gst::ElementFactory::make("capsfilter", None).unwrap();
        let filesink = gst::ElementFactory::make("filesink", None).unwrap();
        let codesclone = codes.clone();
        if !filter_by_codes {
            connect_to_code_detected(&zbar, move |_el, code| {
                println!("Code: {:?}", code);
                let mut ret = code.starts_with("f:");
                if ret {
                    let mut codesdata = codesclone.lock().unwrap();
                    let codestr = code.to_owned();
                    if !codesdata.contains(&codestr) {
                        codesdata.insert(codestr);
                    } else {
                        ret = false; // repeated frame
                    }
                }
                !ret
            });
        } else {
            connect_to_code_detected(&zbar, move |_el, code| {
                let mut ret = code.starts_with("f:");
                if ret {
                    let mut codesdata = codesclone.lock().unwrap();
                    let codestr = code.to_owned();
                    if codesdata.contains(&codestr) {
                       codesdata.remove(&codestr);
                    } else {
                        ret = false; // frame is not present in our codes list
                    }
                }
                println!("Code: {:?} {:?}", code, ret);
                !ret
            });
        }

        filesink.set_property("location", &(path_clone[7..].to_owned() + ".I420")).unwrap();
        capsfilter.set_property("caps", &gst::Caps::from_string("video/x-raw, format=(string)I420")).unwrap();
        videocrop.set_property("top", &cropping_clone.top).unwrap();
        videocrop.set_property("left", &cropping_clone.left).unwrap();
        videocrop.set_property("right", &cropping_clone.right).unwrap();
        videocrop.set_property("bottom", &cropping_clone.bottom).unwrap();

        let pipeline = &pipeline_clone;
        pipeline.add_many(&[&queue, &videoconvert, &zbar, &videoconvert2, &videocrop,
                          &capsfilter, &filesink]).unwrap();;
        gst::Element::link_many(&[&queue, &videoconvert, &zbar, &videoconvert2,
                                &videocrop, &capsfilter, &filesink]).unwrap();

        filesink.sync_state_with_parent().unwrap();
        capsfilter.sync_state_with_parent().unwrap();
        videocrop.sync_state_with_parent().unwrap();
        videoconvert2.sync_state_with_parent().unwrap();
        zbar.sync_state_with_parent().unwrap();
        videoconvert.sync_state_with_parent().unwrap();
        queue.sync_state_with_parent().unwrap();

        let queue_sink_pad = queue.get_static_pad("sink").unwrap();
        assert_eq!(src_pad.link(&queue_sink_pad), gst::PadLinkReturn::Ok);
    });


    pipeline
}

fn analyze_capture(config : &Config, codes : &Arc<Mutex<HashSet<String>>>) {
    let pipeline = setup_pipeline(&config.capture, &config.cropping, codes.clone(), false);
    pipeline.set_state(gst::State::Playing).into_result().unwrap();

    let bus = pipeline
        .get_bus()
        .expect("Pipeline without bus. Shouldn't happen!");

    while let Some(msg) = bus.timed_pop(gst::CLOCK_TIME_NONE) {
        use gst::MessageView;

        match msg.view() {
            MessageView::Eos(..) => break,
            MessageView::Error(err) => {
                pipeline.set_state(gst::State::Null).into_result().unwrap();
                println!("Error");
            }
            _ => (),
        }
    }

    pipeline.set_state(gst::State::Null).into_result().unwrap();

    println!("Codes: {:?}", codes.lock().unwrap());
}

fn extract_reference(config : &Config, codes : &Arc<Mutex<HashSet<String>>>) {
    let pipeline = setup_pipeline(&config.reference, &config.cropping, codes.clone(), true);
    pipeline.set_state(gst::State::Playing).into_result().unwrap();

    let bus = pipeline
        .get_bus()
        .expect("Pipeline without bus. Shouldn't happen!");

    while let Some(msg) = bus.timed_pop(gst::CLOCK_TIME_NONE) {
        use gst::MessageView;

        match msg.view() {
            MessageView::Eos(..) => break,
            MessageView::Error(err) => {
                pipeline.set_state(gst::State::Null).into_result().unwrap();
                println!("Error");
            }
            _ => (),
        }
    }

    pipeline.set_state(gst::State::Null).into_result().unwrap();
}

fn main() {
    gst::init().unwrap();
    let args: Vec<String> = env::args().collect();
    let config = Config::new(&args).unwrap();

    let codes = Arc::new(Mutex::new(HashSet::<String>::new()));
    analyze_capture(&config, &codes);
    extract_reference(&config, &codes);
}
