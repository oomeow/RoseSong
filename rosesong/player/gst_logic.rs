use crate::error::App;
use crate::player::network::{fetch_and_verify_audio_url, set_pipeline_uri_with_headers};
use crate::player::playlist::{
    get_current_track, load, move_to_next_track, move_to_previous_track, set_current_track_index,
    PlayMode, PLAYLIST,
};
use futures_util::stream::StreamExt;
use gstreamer::prelude::*;
use gstreamer::MessageView;
use gstreamer::Pipeline;
use log::{error, info};
use reqwest::{Client, ClientBuilder};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, Mutex, RwLock};
use tokio::task;

use super::playlist::{get_current_track_index, CURRENT_PLAY_INFO};

pub enum Command {
    Play,
    PlayBvid(String),
    Pause,
    Next,
    Previous,
    Stop,
    SetVolume(String),
    SetPlayMode(PlayMode),
    ReloadPlaylist,
    PlaylistIsEmpty,
}

#[derive(Clone, Debug)]
pub struct Audio {
    pipeline: Arc<Pipeline>,
    volume_ele: Arc<gstreamer::Element>,
    client: Arc<Client>,
    play_mode: Arc<RwLock<PlayMode>>,
    command_receiver: Arc<Mutex<mpsc::Receiver<Command>>>,
    eos_sender: mpsc::Sender<()>,
}

impl Audio {
    pub fn new(
        play_mode: PlayMode,
        command_receiver: Arc<Mutex<mpsc::Receiver<Command>>>,
    ) -> Result<Self, App> {
        gstreamer::init().map_err(|e| App::Init(e.to_string()))?;
        let pipeline = Arc::new(gstreamer::Pipeline::new());
        let volume_ele = Arc::new(
            gstreamer::ElementFactory::make("volume")
                .property("volume", 0.0)
                .build()
                .map_err(|_| App::Element("Failed to create volume Element".to_string()))?,
        );
        let client = Arc::new(
            ClientBuilder::new()
                .timeout(Duration::from_secs(5))
                .build()?,
        );
        let (eos_sender, eos_receiver) = mpsc::channel(1);

        info!("GStreamer created successfully.");
        let audio_player = Self {
            pipeline,
            volume_ele: volume_ele.clone(),
            client,
            play_mode: Arc::new(RwLock::new(play_mode)),
            command_receiver,
            eos_sender,
        };

        audio_player.start_eos_listener(&volume_ele, eos_receiver);

        Ok(audio_player)
    }

    fn start_eos_listener(
        &self,
        volume_ele: &gstreamer::Element,
        mut eos_receiver: mpsc::Receiver<()>,
    ) {
        let pipeline = Arc::clone(&self.pipeline);
        let client = Arc::clone(&self.client);
        let play_mode = Arc::clone(&self.play_mode);

        let volume_ele_ = volume_ele.clone();
        task::spawn(async move {
            while let Some(()) = eos_receiver.recv().await {
                info!("Track finished playing. Handling EOS...");

                let current_play_mode = *play_mode.read().await;
                if current_play_mode != PlayMode::Repeat {
                    if let Err(e) = move_to_next_track(current_play_mode).await {
                        error!("Error moving to next track: {}", e);
                        continue;
                    }
                }

                if let Err(e) = play_track(&pipeline, &volume_ele_, &client).await {
                    error!("Failed to play next track: {}", e);
                }
            }
        });
    }
    /// 渐变调整音量（单位：秒）
    pub fn fade_volume(&self, start: f64, target: f64, duration_sec: u8) {
        let all_step = duration_sec * 10;
        let delta = (target - start) / f64::from(all_step);
        for step in 0..=all_step {
            let new_vol = start + delta * f64::from(step);
            self.volume_ele.set_property("volume", new_vol);
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }

    pub async fn play_playlist(&self) -> Result<(), App> {
        let pipeline = Arc::clone(&self.pipeline);
        let volume_ele = Arc::clone(&self.volume_ele);
        let client = Arc::clone(&self.client);
        let play_mode = Arc::clone(&self.play_mode);
        let command_receiver = Arc::clone(&self.command_receiver);
        let eos_sender = self.eos_sender.clone();

        self.listen_to_bus(&eos_sender.clone())?;
        Audio::listen_for_commands(
            command_receiver,
            pipeline,
            volume_ele,
            client,
            play_mode,
            &eos_sender,
        );

        play_track(&self.pipeline, &self.volume_ele.clone(), &self.client).await?;
        Ok(())
    }

    fn listen_to_bus(&self, eos_sender: &mpsc::Sender<()>) -> Result<(), App> {
        let bus = self
            .pipeline
            .bus()
            .ok_or_else(|| App::Pipeline("Failed to get GStreamer bus".to_string()))?;

        task::spawn({
            let eos_sender = eos_sender.clone();
            bus.stream().for_each(move |msg| {
                let eos_sender = eos_sender.clone();
                async move {
                    match msg.view() {
                        MessageView::Eos(_) => {
                            info!("EOS message received, sending signal.");
                            if eos_sender.send(()).await.is_err() {
                                error!("Failed to send EOS signal");
                            }
                        }
                        MessageView::Error(err) => {
                            error!("Error from GStreamer pipeline: {}", err);
                        }
                        _ => (),
                    }
                }
            })
        });
        Ok(())
    }

    fn listen_for_commands(
        command_receiver: Arc<Mutex<mpsc::Receiver<Command>>>,
        pipeline: Arc<Pipeline>,
        volume_ele: Arc<gstreamer::Element>,
        client: Arc<Client>,
        play_mode: Arc<RwLock<PlayMode>>,
        _eos_sender: &mpsc::Sender<()>,
    ) {
        task::spawn(async move {
            let mut command_receiver = command_receiver.lock().await;
            loop {
                if let Some(command) = command_receiver.recv().await {
                    match command {
                        Command::Play => {
                            info!("Resume playback");
                            if let Err(e) = pipeline.set_state(gstreamer::State::Playing) {
                                error!("Failed to play: {}", e);
                            }
                        }
                        Command::PlayBvid(new_bvid) => {
                            info!("Play {}", new_bvid);
                            if let Err(e) =
                                handle_play_bvid(&new_bvid, &pipeline, &volume_ele, &client).await
                            {
                                error!("Failed to play track: {}", e);
                            }
                        }

                        Command::Pause => {
                            info!("Pause");
                            if let Err(e) = pipeline.set_state(gstreamer::State::Paused) {
                                error!("Failed to pause: {}", e);
                            }
                        }
                        Command::Next => {
                            info!("Play next song");
                            if let Err(e) = handle_next_track(
                                play_mode.clone(),
                                &pipeline,
                                &volume_ele,
                                &client,
                            )
                            .await
                            {
                                error!("Failed to play next track: {}", e);
                            }
                        }
                        Command::Previous => {
                            info!("Play previous song");
                            if let Err(e) = handle_previous_track(
                                play_mode.clone(),
                                &pipeline,
                                &volume_ele,
                                &client,
                            )
                            .await
                            {
                                error!("Failed to play previous track: {}", e);
                            }
                        }
                        Command::Stop => {
                            if let Err(e) = pipeline.set_state(gstreamer::State::Null) {
                                error!("Failed to stop: {}", e);
                            }
                        }
                        Command::SetVolume(vol) => {
                            info!("Set volume to {}", vol);
                            if let Err(e) = handle_volume_change(&volume_ele, vol).await {
                                error!("Failed to set volume: {}", e);
                            }
                        }
                        Command::SetPlayMode(new_mode) => {
                            let mut write_guard = play_mode.write().await;
                            *write_guard = new_mode;
                            let mut current_play_info = CURRENT_PLAY_INFO.write().await;
                            if let Err(e) = current_play_info.set_play_mode(new_mode).await {
                                error!("Failed to set play mode: {}", e);
                            }
                        }
                        Command::ReloadPlaylist => {
                            if let Err(e) =
                                handle_reload_playlist(&pipeline, &volume_ele, &client).await
                            {
                                error!("Failed to reload playlist: {}", e);
                            }
                        }
                        Command::PlaylistIsEmpty => {
                            if let Err(e) =
                                handle_playlist_is_empty(&pipeline, &volume_ele, &client).await
                            {
                                error!("Failed to play track after reloading playlist: {}", e);
                            }
                        }
                    }
                }
            }
        });
    }
}

async fn handle_play_bvid(
    new_bvid: &str,
    pipeline: &Pipeline,
    volume_ele: &gstreamer::Element,
    client: &Client,
) -> Result<(), App> {
    let new_index = {
        let playlist = PLAYLIST.read().await;
        let playlist = playlist.as_ref().unwrap();
        playlist.find_track_index(new_bvid)
    };

    if let Some(index) = new_index {
        set_current_track_index(index).await.ok();
    } else {
        error!("Track with bvid {} not found in the playlist", new_bvid);
    }

    play_track(pipeline, volume_ele, client).await
}

async fn handle_next_track(
    play_mode: Arc<RwLock<PlayMode>>,
    pipeline: &Pipeline,
    volume_ele: &gstreamer::Element,
    client: &Client,
) -> Result<(), App> {
    let current_play_mode = *play_mode.read().await;
    let mode = if current_play_mode == PlayMode::Repeat {
        PlayMode::Loop
    } else {
        current_play_mode
    };
    move_to_next_track(mode).await?;
    play_track(pipeline, volume_ele, client).await
}

async fn handle_previous_track(
    play_mode: Arc<RwLock<PlayMode>>,
    pipeline: &Pipeline,
    volume_ele: &gstreamer::Element,
    client: &Client,
) -> Result<(), App> {
    let current_play_mode = *play_mode.read().await;
    let mode = if current_play_mode == PlayMode::Repeat {
        PlayMode::Loop
    } else {
        current_play_mode
    };
    move_to_previous_track(mode).await?;
    play_track(pipeline, volume_ele, client).await
}

async fn handle_volume_change(volume_ele: &gstreamer::Element, vol: String) -> Result<(), App> {
    log::info!("----------------------> volume <-------------------------");
    let current_volume = volume_ele.property::<f64>("volume");
    log::info!("Current volume before: {}", current_volume);
    let current_volume = (current_volume * 100.0).round() / 100.0;
    log::info!("Current volume after: {}", current_volume);
    let new_volume = match vol.as_str() {
        "up" => {
            let vol = ((current_volume + 0.05) * 100.0).round() / 100.0;
            if vol <= 1.0 {
                vol
            } else {
                1.0
            }
        }
        "down" => {
            let vol = ((current_volume - 0.05) * 100.0).round() / 100.0;
            if vol > 0.0 {
                vol
            } else {
                0.0
            }
        }
        _ => {
            if let Ok(parsed_volume) = vol.parse::<u8>() {
                f64::from(parsed_volume) / 100.0
            } else {
                1.0
            }
        }
    };
    log::info!("set new volume: {}", new_volume);
    volume_ele.set_property("volume", new_volume);
    CURRENT_PLAY_INFO.write().await.set_volume(new_volume).await
}

async fn handle_reload_playlist(
    pipeline: &Pipeline,
    volume_ele: &gstreamer::Element,
    client: &Client,
) -> Result<(), App> {
    let current_index = get_current_track_index().await;
    let current_track = get_current_track().await;

    load().await?;

    let playlist = {
        let playlist_ = PLAYLIST.read().await;
        playlist_.clone().unwrap_or_default()
    };

    let should_play = {
        if let Ok(current_track) = current_track {
            // 这个 if 条件应该是多余的，因为在添加歌曲过程中，已经合并同 bvid 的歌曲信息了，只保留其中一个
            if let Some(new_index) = playlist.find_track_index(&current_track.bvid) {
                set_current_track_index(new_index).await.ok();
                info!(
                    "Current track found in the new playlist, index set to {}",
                    new_index
                );
                false
            } else {
                info!("Current track not found in the new playlist, resetting playback");
                let track_count = playlist.tracks.len();
                let new_index = if current_index < track_count {
                    current_index
                } else {
                    track_count - 1
                };
                set_current_track_index(new_index).await.ok();
                true
            }
        } else {
            false
        }
    };

    if should_play {
        play_track(&pipeline, &volume_ele, &client).await?;
    }

    Ok(())
}

async fn handle_playlist_is_empty(
    pipeline: &Pipeline,
    volume_ele: &gstreamer::Element,
    client: &Client,
) -> Result<(), App> {
    load().await?;
    info!("Set track");
    set_current_track_index(0).await.ok();
    play_track(pipeline, volume_ele, client).await
}

async fn play_track(
    pipeline: &Pipeline,
    volume_ele: &gstreamer::Element,
    client: &Client,
) -> Result<(), App> {
    pipeline
        .set_state(gstreamer::State::Null)
        .map_err(|_| App::State("Failed to set pipeline to Null".to_string()))?;

    for element in pipeline.children() {
        pipeline
            .remove(&element)
            .map_err(|_| App::Element("Failed to remove element from pipeline".to_string()))?;
    }

    pipeline
        .set_state(gstreamer::State::Ready)
        .map_err(|_| App::State("Failed to set pipeline to Ready".to_string()))?;

    let mut retries = 5;
    let play_mode = CURRENT_PLAY_INFO.read().await.play_mode;
    loop {
        if retries == 0 {
            return Err(App::Fetch(
                "Max retries reached for play next song".to_string(),
            ));
        }
        let track = get_current_track().await?;
        if let Ok(url) = fetch_and_verify_audio_url(client, &track.bvid, &track.cid).await {
            set_pipeline_uri_with_headers(pipeline, volume_ele.clone(), &url).await?;
            break;
        }
        log::info!("Failed to fetch audio URL, play next song");
        move_to_next_track(play_mode).await?;
        retries -= 1;
    }

    pipeline
        .set_state(gstreamer::State::Playing)
        .map_err(|_| App::State("Failed to set pipeline to Playing".to_string()))?;
    Ok(())
}
