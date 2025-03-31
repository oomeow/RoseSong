use crate::bilibili::fetch_audio_url::fetch_audio_url;
use crate::error::App;
use glib::object::ObjectExt;
use gstreamer::prelude::{ElementExt, ElementExtManual, GstBinExtManual, PadExt};
use gstreamer::Pipeline;
use log::{error, info};
use reqwest::header::{ACCEPT, RANGE, USER_AGENT};
use reqwest::Client;
use tokio::time::{sleep, Duration};

pub async fn verify_audio_url(client: &Client, url: &str) -> Result<bool, App> {
    let response = client
        .get(url)
        .header(USER_AGENT, "Mozilla/5.0 BiliDroid/..* (bbcallen@gmail.com)")
        .header(ACCEPT, "*/*")
        .header(RANGE, "bytes=0-1024")
        .header("Referer", "https://www.bilibili.com")
        .send()
        .await
        .map_err(|e| App::Network(e.to_string()))?;

    Ok(response.status().is_success())
}

pub async fn fetch_and_verify_audio_url(
    client: &Client,
    bvid: &str,
    cid: &str,
) -> Result<String, App> {
    const MAX_RETRIES: u32 = 3;
    const INITIAL_RETRY_DELAY: Duration = Duration::from_secs(1);
    let mut retry_delay = INITIAL_RETRY_DELAY;

    for attempt in 1..=MAX_RETRIES {
        match fetch_audio_url(client, bvid, cid).await {
            Ok(url) => match verify_audio_url(client, &url).await {
                Ok(true) => return Ok(url),
                Ok(false) => {
                    info!("Verification failed for URL: {}", url);
                }
                Err(e) => {
                    error!("Error verifying URL: {}", e);
                }
            },
            Err(e) => {
                error!("Error fetching audio URL: {}", e);
            }
        }
        if attempt < MAX_RETRIES {
            info!("Retrying... Attempt {}/{}", attempt, MAX_RETRIES);
            sleep(retry_delay).await;
            // Exponential backoff
            retry_delay *= 2;
        }
    }

    Err(App::Fetch(
        "Max retries reached for fetching and verifying audio URL".to_string(),
    ))
}

pub async fn set_pipeline_uri_with_headers(
    pipeline: &Pipeline,
    volume_ele: gstreamer::Element,
    url: &str,
) -> Result<(), App> {
    let source = gstreamer::ElementFactory::make("souphttpsrc")
        .build()
        .map_err(|_| App::Element("Failed to create souphttpsrc element".to_string()))?;
    source.set_property("location", url);

    let mut headers = gstreamer::Structure::new_empty("headers");
    headers.set(
        "User-Agent",
        "Mozilla/5.0 BiliDroid/..* (bbcallen@gmail.com)",
    );
    headers.set("Referer", "https://www.bilibili.com");
    source.set_property("extra-headers", &headers);
    source.set_property("timeout", 5u32);

    // 创建 queue 作为缓存
    let queue = gstreamer::ElementFactory::make("queue")
        .build()
        .map_err(|_| App::Element("Failed to create queue element".to_string()))?;
    queue.set_property("max-size-buffers", 100u32);
    queue.set_property("max-size-time", 5 * gstreamer::ClockTime::SECOND);

    let decodebin = gstreamer::ElementFactory::make("decodebin")
        .build()
        .map_err(|_| App::Element("Failed to create decodebin element".to_string()))?;

    pipeline
        .add_many([&source, &queue, &decodebin])
        .map_err(|_| App::Pipeline("Failed to add elements to pipeline".to_string()))?;
    source
        .link(&queue)
        .map_err(|_| App::Link("Failed to link source to queue".to_string()))?;
    queue
        .link(&decodebin)
        .map_err(|_| App::Link("Failed to link queue to decodebin".to_string()))?;

    let pipeline_weak = pipeline.downgrade();

    decodebin.connect_pad_added(move |_, src_pad| {
        if let Some(pipeline) = pipeline_weak.upgrade() {
            let queue = gstreamer::ElementFactory::make("queue")
                .build()
                .expect("Failed to create queue element");

            let audioconvert = gstreamer::ElementFactory::make("audioconvert")
                .build()
                .expect("Failed to create audioconvert element");
            let audioresample = gstreamer::ElementFactory::make("audioresample")
                .build()
                .expect("Failed to create audioresample element");
            let autoaudiosink = gstreamer::ElementFactory::make("autoaudiosink")
                .build()
                .expect("Failed to create autoaudiosink element");

            pipeline
                .add_many([
                    &queue,
                    &audioconvert,
                    &audioresample,
                    &volume_ele,
                    &autoaudiosink,
                ])
                .expect("Failed to add elements to pipeline");

            queue
                .sync_state_with_parent()
                .expect("Failed to sync_state_with_parent for queue");
            audioconvert
                .sync_state_with_parent()
                .expect("Failed to sync_state_with_parent for audioconvert");
            audioresample
                .sync_state_with_parent()
                .expect("Failed to sync_state_with_parent for audioresample");
            volume_ele
                .sync_state_with_parent()
                .expect("Failed to sync_state_with_parent for volume");
            autoaudiosink
                .sync_state_with_parent()
                .expect("Failed to sync_state_with_parent for autoaudiosink");

            audioconvert
                .link(&audioresample)
                .expect("Failed to link audioconvert to audioresample");
            audioresample
                .link(&volume_ele)
                .expect("Failed to link audioresample to volume");
            volume_ele
                .link(&autoaudiosink)
                .expect("Failed to link volume to autoaudiosink");

            let queue_src_pad = queue.static_pad("src").expect("Failed to get static pad");
            let audio_sink_pad = audioconvert
                .static_pad("sink")
                .expect("Failed to get static pad");
            queue_src_pad
                .link(&audio_sink_pad)
                .expect("Failed to link pads");

            let queue_sink_pad = queue.static_pad("sink").expect("Failed to get static pad");
            src_pad.link(&queue_sink_pad).expect("Failed to link pads");

            info!("Pipeline elements linked successfully");
        } else {
            error!("Failed to upgrade pipeline reference");
        }
    });

    pipeline
        .set_state(gstreamer::State::Playing)
        .map_err(|_| App::State("Failed to set pipeline to Playing".to_string()))?;
    Ok(())
}
