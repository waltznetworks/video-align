extern crate gstreamer as gst;
use gst::prelude::*;
extern crate gstreamer_video as gst_video;
extern crate gstreamer_app as gst_app;

extern crate glib;
extern crate qrcode;
extern crate image;

use qrcode::QrCode;
use image::Luma;

extern crate failure;
use failure::Error;

use std::thread;
use std::error::Error as StdError;

#[macro_use]
extern crate failure_derive;

#[derive(Debug, Fail)]
#[fail(display = "Missing element {}", _0)]
struct MissingElement(&'static str);

#[derive(Debug, Fail)]
#[fail(display = "Received error from {}: {} (debug: {:?})", src, error, debug)]
struct ErrorMessage {
    src: String,
    error: String,
    debug: Option<String>,
    #[cause] cause: glib::Error,
}

const WIDTH: usize = 1280;
const HEIGHT: usize = 720;

fn create_pipeline() -> Result<(gst::Pipeline, gst_app::AppSrc), Error> {
    gst::init()?;

    let pipeline = gst::Pipeline::new(None);
    // Prepending
    let src = gst::ElementFactory::make("appsrc", None).ok_or(MissingElement("appsrc"))?;
    let timeoverlay = gst::ElementFactory::make("timeoverlay", None).ok_or(MissingElement("timeoverlay"))?;
    let srcconv = gst::ElementFactory::make("videoconvert", None).ok_or(MissingElement("videoconvert"))?;
    let srccapsfilter = gst::ElementFactory::make("capsfilter", None).ok_or(MissingElement("capsfilter"))?;
    let srcenc = gst::ElementFactory::make("x264enc", None).ok_or(MissingElement("x264enc"))?;

    // Decoding a file
    let uridec = gst::ElementFactory::make("uridecodebin", None).ok_or(MissingElement("uridecodebin"))?;

    // mixing it up together
    let concat = gst::ElementFactory::make("concat", None).ok_or(MissingElement("concat"))?;
    let mux =
        gst::ElementFactory::make("mp4mux", None).ok_or(MissingElement("mp4mux"))?;
    let sink =
        gst::ElementFactory::make("filesink", None).ok_or(MissingElement("filesink"))?;

    // Source and destination
    uridec.set_property("uri", &glib::Value::from("file:///home/thiagoss/Videos/sintel_trailer-720p.mp4")).unwrap();
    sink.set_property("location", &glib::Value::from("/tmp/videoalign.mp4")).unwrap();

    timeoverlay.set_property("font-desc", &glib::Value::from("monospaced")).unwrap();
    timeoverlay.set_property("silent", &true).unwrap();
    srccapsfilter.set_property("caps", &gst::Caps::from_string("video/x-raw, format=(string)I420"));

    pipeline.add_many(&[&src, &timeoverlay, &srcconv, &srccapsfilter, &srcenc, &concat, &uridec, &mux, &sink])?;
    gst::Element::link_many(&[&src, &timeoverlay, &srcconv, &srccapsfilter, &srcenc])?;
    gst::Element::link_many(&[&mux, &sink])?;

    let pipeline_clone = pipeline.clone();
    let concat_clone = concat.clone();
    uridec.connect_pad_added(move |_, src_pad| {
        if !src_pad.get_current_caps().unwrap().get_structure(0).unwrap().get_name().contains("video") {
            return;
        }
        let queue = gst::ElementFactory::make("queue", None).unwrap();
        let enc = gst::ElementFactory::make("x264enc", None).unwrap();
        // blitting to the decoded file
        let frameidcf = gst::ElementFactory::make("capsfilter", None).unwrap();
        let videoconvert = gst::ElementFactory::make("videoconvert", None).unwrap();
        let frameid = gst::ElementFactory::make("rsframeid", None).unwrap();
        let frameidvideoconvert = gst::ElementFactory::make("videoconvert", None).unwrap();

        frameidcf.set_property("caps", &gst::Caps::from_string("video/x-raw, format=(string)I420"));

        let pipeline = &pipeline_clone;
        let concat = &concat_clone;

        pipeline.add(&queue).unwrap();
        pipeline.add(&frameidcf).unwrap();
        pipeline.add(&videoconvert).unwrap();
        pipeline.add(&frameid).unwrap();
        pipeline.add(&frameidvideoconvert).unwrap();
        pipeline.add(&enc).unwrap();
        gst::Element::link_many(&[&queue, &videoconvert, &frameid, &frameidvideoconvert, &frameidcf, &enc]).unwrap();
        enc.sync_state_with_parent().unwrap();
        frameidcf.sync_state_with_parent().unwrap();
        frameidvideoconvert.sync_state_with_parent().unwrap();
        frameid.sync_state_with_parent().unwrap();
        videoconvert.sync_state_with_parent().unwrap();
        queue.sync_state_with_parent().unwrap();
        frameid.set_property("prefix", &"f:".to_owned());

        let concatpad = concat.get_request_pad("sink_%u").unwrap();
        assert_eq!(enc.get_static_pad("src").unwrap().link(&concatpad), gst::PadLinkReturn::Ok);

        let sink_pad = queue.get_static_pad("sink").unwrap();
        assert_eq!(src_pad.link(&sink_pad), gst::PadLinkReturn::Ok);

        // Prepare the last concat
        let videotestsrc = gst::ElementFactory::make("videotestsrc", None).unwrap();
        let lastframeid = gst::ElementFactory::make("rsframeid", None).unwrap();
        let lastvideoconvert = gst::ElementFactory::make("videoconvert", None).unwrap();
        let lastcapsfilter = gst::ElementFactory::make("capsfilter", None).unwrap();
        let lastenc = gst::ElementFactory::make("x264enc", None).unwrap();

        lastcapsfilter.set_property("caps", &gst::Caps::from_string("video/x-raw, format=(string)I420, width=(int)1280, height=(int)720, framerate=(fraction)24/1"));
        videotestsrc.set_property("pattern", &1);
        videotestsrc.set_property("num-buffers", &300);
        lastframeid.set_property("prefix", &"e:".to_owned());

        pipeline.add(&videotestsrc);
        pipeline.add(&lastframeid);
        pipeline.add(&lastvideoconvert);
        pipeline.add(&lastcapsfilter);
        pipeline.add(&lastenc);
        gst::Element::link_many(&[&videotestsrc, &lastframeid, &lastvideoconvert, &lastcapsfilter, &lastenc]).unwrap();
        let lastconcat = concat.get_request_pad("sink_%u").unwrap();
        assert_eq!(lastenc.get_static_pad("src").unwrap().link(&lastconcat), gst::PadLinkReturn::Ok);
        lastenc.sync_state_with_parent().unwrap();
        lastcapsfilter.sync_state_with_parent().unwrap();
        lastvideoconvert.sync_state_with_parent().unwrap();
        lastframeid.sync_state_with_parent().unwrap();
        videotestsrc.sync_state_with_parent().unwrap();
    });

    let mux_sinkpad = mux.get_request_pad("video_%u").unwrap();
    let concat_srcpad = concat.get_static_pad("src").unwrap();
    assert_eq!(concat_srcpad.link(&mux_sinkpad), gst::PadLinkReturn::Ok);

    assert_eq!(srcenc.get_static_pad("src").unwrap().link(&concat.get_request_pad("sink_%u").unwrap()), gst::PadLinkReturn::Ok);

    let appsrc = src.clone()
        .dynamic_cast::<gst_app::AppSrc>()
        .expect("Source element is expected to be an appsrc!");

    let info = gst_video::VideoInfo::new(gst_video::VideoFormat::Rgbx, WIDTH as u32, HEIGHT as u32)
        .fps(gst::Fraction::new(24, 1))
        .build()
        .expect("Failed to create video info");

    appsrc.set_caps(&info.to_caps().unwrap());
    appsrc.set_property_format(gst::Format::Time);
    appsrc.set_max_bytes(1);
    appsrc.set_property_block(true);

    Ok((pipeline, appsrc))
}

fn main_loop(pipeline: gst::Pipeline, appsrc: gst_app::AppSrc) -> Result<(), Error> {
    pipeline.set_state(gst::State::Playing).into_result()?;

    thread::spawn(move || {
        for i in 0..100 {
            let mut buffer = gst::Buffer::with_size(WIDTH * HEIGHT * 4).unwrap();
            {
                let buffer = buffer.get_mut().unwrap();
                let pts = (i * 41666 * gst::USECOND);
                let mut codetext : String = "s:".to_owned();
                codetext.push_str(&pts.to_string());
                let code = QrCode::new(codetext.to_string()).unwrap();
                let image = code.render::<Luma<u8>>().quiet_zone(false).build();

                buffer.set_pts(pts);

                let mut data = buffer.map_writable().unwrap();

                for p in data.as_mut_slice().chunks_mut(4) {
                    assert_eq!(p.len(), 4);
                    p[0] = 0;
                    p[1] = 0;
                    p[2] = 0;
                    p[3] = 0;
                }

                // draw our qrcode on the top left
                let dimensions = image.dimensions();
                for x in 0..dimensions.0 {
                    for y in 0..dimensions.1 {
                        let baseindex : usize = 4 * (WIDTH * (x as usize) + (y as usize));
                        let pixel = image.get_pixel(x, y);
                        data.as_mut_slice()[baseindex+0] = pixel[0];
                        data.as_mut_slice()[baseindex+1] = pixel[0];
                        data.as_mut_slice()[baseindex+2] = pixel[0];
                    }
                }
            }

            if appsrc.push_buffer(buffer) != gst::FlowReturn::Ok {
                break;
            }
        }

        let _ = appsrc.end_of_stream();
    });

    let bus = pipeline
        .get_bus()
        .expect("Pipeline without bus. Shouldn't happen!");

    while let Some(msg) = bus.timed_pop(gst::CLOCK_TIME_NONE) {
        use gst::MessageView;

        match msg.view() {
            MessageView::Eos(..) => break,
            MessageView::Error(err) => {
                pipeline.set_state(gst::State::Null).into_result()?;
                Err(ErrorMessage {
                    src: msg.get_src()
                        .map(|s| s.get_path_string())
                        .unwrap_or_else(|| String::from("None")),
                    error: err.get_error().description().into(),
                    debug: err.get_debug(),
                    cause: err.get_error(),
                })?;
            }
            _ => (),
        }
    }

    pipeline.set_state(gst::State::Null).into_result()?;

    Ok(())
}

fn main() {
    match create_pipeline().and_then(|(pipeline, appsrc)| main_loop(pipeline, appsrc)) {
        Ok(r) => r,
        Err(e) => eprintln!("Error! {}", e),
    }
}
