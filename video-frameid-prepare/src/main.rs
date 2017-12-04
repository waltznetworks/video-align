extern crate gstreamer as gst;
use gst::prelude::*;
extern crate gstreamer_video as gst_video;
extern crate gstreamer_app as gst_app;

extern crate glib;

extern crate failure;
use failure::Error;

use std::env;
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

#[derive(Debug)]
struct Config {
    input: String,
    output: String,
}

impl Config {
    fn new(args: &[String]) -> Result<Config, &'static str> {
        let input = args[1].clone();
        let output = args[2].clone();

        Ok(Config {input, output})
    }
}

// TODO This should be discovered by the input file
const WIDTH: usize = 1280;
const HEIGHT: usize = 720;
const FRAMERATE: usize = 24;

fn setup_prepend_branch(pipeline : &gst::Pipeline, sink_pad : gst::Pad) -> Result<bool, Error> {
    let src = gst::ElementFactory::make("videotestsrc", None).ok_or(MissingElement("videotestsrc"))?;
    let frameid = gst::ElementFactory::make("rsframeid", None).ok_or(MissingElement("rsframeid"))?;
    let srcconv = gst::ElementFactory::make("videoconvert", None).ok_or(MissingElement("videoconvert"))?;
    let srccapsfilter = gst::ElementFactory::make("capsfilter", None).ok_or(MissingElement("capsfilter"))?;
    let srcenc = gst::ElementFactory::make("x264enc", None).ok_or(MissingElement("x264enc"))?;

    src.set_property("num-buffers", &300)?;
    frameid.set_property("prefix", &"s:".to_owned())?;
    srccapsfilter.set_property("caps", &gst::Caps::from_string(&format!("video/x-raw, format=(string)I420, width=(int){}, height=(int){}, framerate=(fraction){}/1",
            WIDTH, HEIGHT, FRAMERATE)))?;

    pipeline.add_many(&[&src, &frameid, &srcconv, &srccapsfilter, &srcenc])?;
    gst::Element::link_many(&[&src, &frameid, &srcconv, &srccapsfilter, &srcenc])?;

    assert_eq!(srcenc.get_static_pad("src").unwrap().link(&sink_pad), gst::PadLinkReturn::Ok);

    Ok(true)
}

fn setup_decoder_branch(pipeline : &gst::Pipeline, sink_pad : gst::Pad, config : &Config) -> Result<bool, Error> {
    let uridec = gst::ElementFactory::make("uridecodebin", None).ok_or(MissingElement("uridecodebin"))?;

    uridec.set_property("uri", &glib::Value::from(&config.input))?;
    pipeline.add(&uridec)?;

    let pipeline_clone = pipeline.clone();
    uridec.connect_pad_added(move |_, src_pad| {
        // FIXME post an error message if any of those fail instead of just doing unwrap()
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

        frameidcf.set_property("caps", &gst::Caps::from_string("video/x-raw, format=(string)I420")).unwrap();

        let pipeline = &pipeline_clone;

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
        frameid.set_property("prefix", &"f:".to_owned()).unwrap();

        assert_eq!(enc.get_static_pad("src").unwrap().link(&sink_pad), gst::PadLinkReturn::Ok);

        let queue_sink_pad = queue.get_static_pad("sink").unwrap();
        assert_eq!(src_pad.link(&queue_sink_pad), gst::PadLinkReturn::Ok);

    });

    Ok(true)
}

fn setup_append_branch(pipeline : &gst::Pipeline, sink_pad : gst::Pad) -> Result<bool, Error> {
    // Prepare the last concat
    let videotestsrc = gst::ElementFactory::make("videotestsrc", None).ok_or(MissingElement("videotestsrc"))?;
    let lastframeid = gst::ElementFactory::make("rsframeid", None).ok_or(MissingElement("rsframeid"))?;
    let lastvideoconvert = gst::ElementFactory::make("videoconvert", None).ok_or(MissingElement("videoconvert"))?;
    let lastcapsfilter = gst::ElementFactory::make("capsfilter", None).ok_or(MissingElement("capsfilter"))?;
    let lastenc = gst::ElementFactory::make("x264enc", None).ok_or(MissingElement("x264enc"))?;

    lastcapsfilter.set_property("caps", &gst::Caps::from_string(&format!("video/x-raw, format=(string)I420, width=(int){}, height=(int){}, framerate=(fraction){}/1",
            WIDTH, HEIGHT, FRAMERATE)))?;
    videotestsrc.set_property("num-buffers", &300)?;
    lastframeid.set_property("prefix", &"e:".to_owned())?;

    pipeline.add_many(&[&videotestsrc, &lastframeid, &lastvideoconvert, &lastcapsfilter, &lastenc])?;
    gst::Element::link_many(&[&videotestsrc, &lastframeid, &lastvideoconvert, &lastcapsfilter, &lastenc])?;

    assert_eq!(lastenc.get_static_pad("src").unwrap().link(&sink_pad), gst::PadLinkReturn::Ok);
    lastenc.sync_state_with_parent()?;
    lastcapsfilter.sync_state_with_parent()?;
    lastvideoconvert.sync_state_with_parent()?;
    lastframeid.sync_state_with_parent()?;
    videotestsrc.sync_state_with_parent()?;

    Ok(true)
}

fn create_pipeline(config : Config) -> Result<(gst::Pipeline), Error> {
    gst::init()?;

    let pipeline = gst::Pipeline::new(None);

    // end of the pipeline responsible of mixing it up together
    let concat = gst::ElementFactory::make("concat", None).ok_or(MissingElement("concat"))?;
    let mux =
        gst::ElementFactory::make("mp4mux", None).ok_or(MissingElement("mp4mux"))?;
    let sink =
        gst::ElementFactory::make("filesink", None).ok_or(MissingElement("filesink"))?;

    pipeline.add_many(&[&concat, &mux, &sink])?;
    gst::Element::link_many(&[&mux, &sink])?;

    // Source and destination
    sink.set_property("location", &config.output).unwrap();

    setup_prepend_branch(&pipeline, concat.get_request_pad("sink_%u").unwrap())?;
    setup_decoder_branch(&pipeline, concat.get_request_pad("sink_%u").unwrap(), &config)?;
    setup_append_branch(&pipeline, concat.get_request_pad("sink_%u").unwrap())?;

    let mux_sinkpad = mux.get_request_pad("video_%u").unwrap();
    let concat_srcpad = concat.get_static_pad("src").unwrap();
    assert_eq!(concat_srcpad.link(&mux_sinkpad), gst::PadLinkReturn::Ok);

    Ok(pipeline)
}

fn main_loop(pipeline: gst::Pipeline) -> Result<(), Error> {
    pipeline.set_state(gst::State::Playing).into_result()?;

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
    let args: Vec<String> = env::args().collect();
    let config = Config::new(&args).unwrap();

    match create_pipeline(config).and_then(|pipeline| main_loop(pipeline)) {
        Ok(r) => r,
        Err(e) => eprintln!("Error! {}", e),
    }
}
