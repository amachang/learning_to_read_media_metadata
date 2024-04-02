use std::{path::{Path, PathBuf}, collections::HashMap};
use anyhow::{anyhow, Result};
use gstreamer as gst;
use gst::prelude::*;

use clap::Parser;

#[derive(Debug, Parser)]
struct Args {
    video_paths: Vec<PathBuf>,
}

fn main() -> Result<()> {
    let args = Args::parse();

    gst::init().expect("prerequisite");

    let pipeline = gst::Pipeline::new();
    let filesrc = gst::ElementFactory::make("filesrc").name("src").build()?;
    let decodebin = gst::ElementFactory::make("decodebin").build()?;
    let fakesink = gst::ElementFactory::make("fakesink").build()?;
    pipeline.add_many(&[&filesrc, &decodebin, &fakesink])?;

    decodebin.connect_pad_added(move |_decodebin, src_pad| {
        let src_pad = src_pad.clone();
        let sink_pad = fakesink.static_pad("sink").expect("must have sink pad");
        if sink_pad.is_linked() {
            return;
        }
        src_pad.link(&sink_pad).unwrap();
    });
    filesrc.link(&decodebin)?;

    for video_path in args.video_paths {
        let tags = get_tags(&pipeline, &video_path)?;
        println!("Tags: {:?}", tags);
    }

    Ok(())
}

fn get_tags(pipeline: &gst::Pipeline, video_path: impl AsRef<Path>) -> Result<HashMap<String, String>> {
    let video_path = video_path.as_ref();
    let filesrc = pipeline.by_name("src").expect("must have filesrc");
    filesrc.set_property("location", video_path);

    pipeline.set_state(gst::State::Paused)?;

    let mut tag_map = HashMap::new();
    let bus = pipeline.bus().expect("pipeline must have bus");
    let result = 'message_loop: loop {
        let Some(msg) = bus.timed_pop_filtered(gst::ClockTime::NONE, &[gst::MessageType::Tag, gst::MessageType::Error, gst::MessageType::Eos, gst::MessageType::AsyncDone]) else {
            break Err(anyhow!("No message from bus"));
        };

        use gst::MessageView;
        match msg.view() {
            MessageView::Error(err) => {
                break Err(anyhow!(
                    "Error from {:?}: {} ({:?})",
                    msg.src().map(|s| s.path_string()),
                    err.error(),
                    err.details()
                ));
            },
            MessageView::AsyncDone(_) => {
                break Ok(tag_map);
            },
            MessageView::Eos(_) => {
                break Err(anyhow!("Face EOS before async (prerolled) done message"));
            },
            MessageView::Tag(tag) => {
                let tags = tag.tags();
                for (tag_name, tag_value) in tags.iter() {
                    let tag_name = tag_name.to_string();
                    let tag_value = match tag_value.get::<String>() {
                        Err(err) => break 'message_loop Err(anyhow!("Failed to get string value from tag({}): {:?} ({})", tag_name, tag_value, err)),
                        Ok(tag_value) => tag_value,
                    };
                
                    tag_map.insert(tag_name, tag_value);
                }
            },
            _ => unreachable!(),
        }
    };

    pipeline.set_state(gst::State::Null)?;

    result
}
