use crate::error::App;
use rand::rng;
use rand::seq::IteratorRandom;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::LazyLock;
use tokio::io::AsyncWriteExt;
use tokio::sync::RwLock;

// global variables
pub static PLAYLIST: LazyLock<RwLock<Result<Playlist, App>>> =
    LazyLock::new(|| RwLock::new(Ok(Playlist { tracks: Vec::new() })));
pub static CURRENT_PLAY_INFO: LazyLock<RwLock<CurrentPlayInfo>> =
    LazyLock::new(|| RwLock::new(CurrentPlayInfo::default()));

#[derive(Debug, Serialize, Deserialize, Default, Clone, Copy, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum PlayMode {
    #[default]
    Loop,
    Shuffle,
    Repeat,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct CurrentPlayInfo {
    pub index: usize,
    pub play_mode: PlayMode,
    pub track: Option<Track>,
}

impl CurrentPlayInfo {
    pub async fn set_play_mode(&mut self, mode: PlayMode) -> Result<(), App> {
        self.play_mode = mode;
        self.save_to_file().await?;
        Ok(())
    }

    pub async fn set_current(&mut self, index: usize) -> Result<(), App> {
        self.index = index + 1;
        let playlist = Playlist::load_from_file().await?;
        let track = playlist.get_current_track(index)?;
        self.track = Some(track);
        self.save_to_file().await?;
        Ok(())
    }

    pub async fn load_from_file() -> Result<Self, App> {
        log::info!("Loading current play info");
        let file_path = format!(
            "{}/.config/rosesong/current.toml",
            std::env::var("HOME").expect("Failed to get HOME environment variable")
        );
        if !Path::new(&file_path).exists() {
            let default_content = toml::to_string(&CurrentPlayInfo::default()).map_err(|_| {
                App::DataParsing("Failed to serialize default content to TOML".to_string())
            })?;
            tokio::fs::write(&file_path, default_content).await?;
        }
        let content = tokio::fs::read_to_string(file_path).await?;
        let current_play_info: CurrentPlayInfo = toml::from_str(&content)?;
        Ok(current_play_info)
    }

    async fn save_to_file(&self) -> Result<(), App> {
        log::info!("Saving current play info");
        let file_path = format!(
            "{}/.config/rosesong/current.toml",
            std::env::var("HOME").expect("Failed to get HOME environment variable")
        );
        let toml_content = toml::to_string(&self)
            .map_err(|_| App::DataParsing("Failed to serialize tracks to TOML".to_string()))?;
        let mut file = tokio::fs::File::create(&file_path)
            .await
            .map_err(|_| App::Io("Failed to create playlist file".to_string()))?;
        file.write_all(toml_content.as_bytes())
            .await
            .map_err(|_| App::Io("Failed to write playlist file".to_string()))?;
        Ok(())
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Track {
    pub bvid: String,
    pub cid: String,
    pub title: String,
    pub owner: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Playlist {
    pub tracks: Vec<Track>,
}

impl Playlist {
    pub async fn load_from_file() -> Result<Self, App> {
        log::info!("Loading playlist");
        let file_path = format!(
            "{}/.config/rosesong/playlists/playlist.toml",
            std::env::var("HOME").expect("Failed to get HOME environment variable")
        );
        let content = tokio::fs::read_to_string(file_path).await?;
        let playlist: Playlist = toml::from_str(&content)?;
        Ok(playlist)
    }

    pub fn get_current_track(&self, index: usize) -> Result<Track, App> {
        self.tracks
            .get(index)
            .cloned()
            .ok_or_else(|| App::DataParsing("Track index out of bounds".to_string()))
    }

    pub async fn move_to_next_track(&mut self, play_mode: PlayMode) -> Result<usize, App> {
        let current_index = get_current_track_index().await;
        let new_index = match play_mode {
            PlayMode::Loop => (current_index + 1) % self.tracks.len(),
            PlayMode::Shuffle => {
                let mut rng = rng();
                (0..self.tracks.len())
                    .choose(&mut rng)
                    .ok_or_else(|| App::DataParsing("Failed to choose random track".to_string()))?
            }
            PlayMode::Repeat => current_index,
        };
        Ok(new_index)
    }

    pub async fn move_to_previous_track(&mut self, play_mode: PlayMode) -> Result<usize, App> {
        let current_index = get_current_track_index().await;
        let new_index = match play_mode {
            PlayMode::Loop => {
                if current_index == 0 {
                    self.tracks.len() - 1
                } else {
                    current_index - 1
                }
            }
            PlayMode::Shuffle => {
                let mut rng = rng();
                (0..self.tracks.len())
                    .choose(&mut rng)
                    .ok_or_else(|| App::DataParsing("Failed to choose random track".to_string()))?
            }
            PlayMode::Repeat => current_index,
        };
        Ok(new_index)
    }

    pub fn find_track_index(&self, bvid: &str) -> Option<usize> {
        self.tracks.iter().position(|track| track.bvid == bvid)
    }
}

pub async fn get_current_track_index() -> usize {
    CURRENT_PLAY_INFO.read().await.index - 1
}

pub async fn set_current_track_index(index: usize) -> Result<(), App> {
    CURRENT_PLAY_INFO.write().await.set_current(index).await?;
    Ok(())
}

pub async fn load() -> Result<(), App> {
    // playlist
    let playlist = Playlist::load_from_file().await?;
    let mut playlist_lock = PLAYLIST.write().await;
    // Replace the old playlist with the new one
    *playlist_lock = Ok(playlist.clone());

    // current play info
    let mut current_play_info = CurrentPlayInfo::load_from_file().await?;
    if current_play_info.track.is_none() && !playlist.tracks.is_empty() {
        current_play_info
            .set_current(current_play_info.index)
            .await?;
    }
    let mut current_play_info_lock = CURRENT_PLAY_INFO.write().await;
    *current_play_info_lock = current_play_info;

    Ok(())
}

pub async fn get_current_track() -> Result<Track, App> {
    let playlist = PLAYLIST.read().await;
    let playlist = playlist.as_ref().map_err(std::clone::Clone::clone)?;
    let index = get_current_track_index().await;
    playlist.get_current_track(index)
}

pub async fn move_to_next_track(play_mode: PlayMode) -> Result<usize, App> {
    let index = {
        let mut playlist = PLAYLIST.write().await;
        let playlist = playlist.as_mut().map_err(|e| e.clone())?;
        playlist.move_to_next_track(play_mode).await?
    };
    set_current_track_index(index).await?;
    Ok(index)
}

pub async fn move_to_previous_track(play_mode: PlayMode) -> Result<usize, App> {
    let mut playlist = PLAYLIST.write().await;
    let playlist = playlist.as_mut().map_err(|e| e.clone())?;
    let index = playlist.move_to_previous_track(play_mode).await?;
    set_current_track_index(index).await?;
    Ok(index)
}
