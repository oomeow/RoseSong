use crate::error::App;
use rand::rng;
use rand::seq::IteratorRandom;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::LazyLock;
use tokio::io::AsyncWriteExt;
use tokio::sync::RwLock;

// global variables
pub static PLAYLIST: LazyLock<RwLock<Result<Playlist, App>>> = LazyLock::new(|| {
    RwLock::new(Ok(Playlist {
        tracks: Vec::new(),
        seasons: Vec::new(),
    }))
});
pub static CURRENT_PLAY_INFO: LazyLock<RwLock<CurrentPlayInfo>> =
    LazyLock::new(|| RwLock::new(CurrentPlayInfo::default()));

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum PlayMode {
    Loop,
    Shuffle,
    Repeat,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CurrentPlayInfo {
    pub index: usize,
    pub volume: usize,
    pub play_mode: PlayMode,
    pub track: Option<Track>,
    pub playing_sid: Option<String>,
    pub current_tracks: Vec<Track>,
}

impl Default for CurrentPlayInfo {
    fn default() -> Self {
        Self {
            index: 0,
            volume: 100,
            play_mode: PlayMode::Loop,
            track: None,
            playing_sid: None,
            current_tracks: Vec::new(),
        }
    }
}

impl CurrentPlayInfo {
    pub async fn set_play_mode(&mut self, mode: PlayMode) -> Result<(), App> {
        self.play_mode = mode;
        self.save_to_file().await?;
        Ok(())
    }

    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    pub async fn set_volume(&mut self, volume: f64) -> Result<(), App> {
        self.volume = (volume * 100.0) as usize;
        self.save_to_file().await?;
        Ok(())
    }

    pub async fn set_current(&mut self, index: usize) -> Result<(), App> {
        self.index = index + 1;
        let track = self.current_tracks.get(index).cloned();
        self.track = track;
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

    pub fn get_current_track(&self) -> Option<Track> {
        self.current_tracks.get(self.index).cloned()
    }

    pub fn find_track_index(&self, bvid: &str) -> Option<usize> {
        self.current_tracks
            .iter()
            .position(|track| track.bvid == bvid)
    }

    pub async fn move_to_next_track(&mut self, play_mode: PlayMode) -> Result<(), App> {
        let current_index = self.index;
        let current_tracks_len = self.current_tracks.len();
        let new_index = match play_mode {
            PlayMode::Loop => (current_index + 1) % current_tracks_len,
            PlayMode::Shuffle => {
                let mut rng = rng();
                (0..current_tracks_len)
                    .choose(&mut rng)
                    .ok_or_else(|| App::DataParsing("Failed to choose random track".to_string()))?
            }
            PlayMode::Repeat => current_index,
        };
        self.index = new_index;
        self.track = self.current_tracks.get(new_index).cloned();
        self.save_to_file().await?;
        Ok(())
    }

    pub async fn move_to_previous_track(&mut self, play_mode: PlayMode) -> Result<(), App> {
        let current_index = self.index;
        let current_tracks_len = self.current_tracks.len();
        let new_index = match play_mode {
            PlayMode::Loop => {
                if current_index == 0 {
                    current_tracks_len - 1
                } else {
                    current_index - 1
                }
            }
            PlayMode::Shuffle => {
                let mut rng = rng();
                (0..current_tracks_len)
                    .choose(&mut rng)
                    .ok_or_else(|| App::DataParsing("Failed to choose random track".to_string()))?
            }
            PlayMode::Repeat => current_index,
        };
        self.index = new_index;
        self.track = self.current_tracks.get(new_index).cloned();
        self.save_to_file().await?;
        Ok(())
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Track {
    pub bvid: String,
    pub cid: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sid: Option<String>,
    pub title: String,
    pub owner: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Season {
    pub id: String,
    pub title: String,
    pub cover: Option<String>,
    pub intro: String,
    pub owner: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Playlist {
    pub tracks: Vec<Track>,
    pub seasons: Vec<Season>,
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

    pub fn find_tracks_in_season(&self, sid: &str) -> Vec<Track> {
        self.tracks
            .clone()
            .into_iter()
            .filter(|t| t.sid == Some(sid.to_string()))
            .collect::<Vec<Track>>()
    }
}

pub async fn get_current_track_index() -> usize {
    CURRENT_PLAY_INFO.read().await.index
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
    // 初始化播放列表
    let tracks = if let Some(sid) = current_play_info.playing_sid.clone() {
        let tracks = playlist.find_tracks_in_season(&sid);
        current_play_info.current_tracks.clone_from(&tracks);
        tracks
    } else {
        let tracks = playlist.tracks.clone();
        current_play_info.current_tracks.clone_from(&tracks);
        tracks
    };
    let index = current_play_info.index;
    if !tracks.is_empty() {
        if index < tracks.len() {
            current_play_info.track = tracks.get(index).cloned();
        } else {
            current_play_info.track = tracks.first().cloned();
        }
    }
    current_play_info.save_to_file().await?;
    // replace the old current play info with new one
    let mut current_play_info_lock = CURRENT_PLAY_INFO.write().await;
    *current_play_info_lock = current_play_info;

    Ok(())
}

pub async fn get_play_mode() -> Result<PlayMode, App> {
    let current_play_info = CURRENT_PLAY_INFO.read().await;
    Ok(current_play_info.play_mode)
}

pub async fn get_current_track() -> Result<Track, App> {
    let current_play_info = CURRENT_PLAY_INFO.read().await;
    current_play_info
        .get_current_track()
        .ok_or(App::DataParsing("Failed to get current track".to_string()))
}

pub async fn move_to_next_track(play_mode: PlayMode) -> Result<(), App> {
    let mut current_play_info = CURRENT_PLAY_INFO.write().await;
    current_play_info.move_to_next_track(play_mode).await?;
    Ok(())
}

pub async fn move_to_previous_track(play_mode: PlayMode) -> Result<(), App> {
    let mut current_play_info = CURRENT_PLAY_INFO.write().await;
    current_play_info.move_to_previous_track(play_mode).await?;
    Ok(())
}

pub async fn update_current_play_tracks(
    sid: Option<String>,
    tracks: Vec<Track>,
) -> Result<(), App> {
    let mut current_play_info = CURRENT_PLAY_INFO.write().await;
    let play_mode = current_play_info.play_mode;
    let new_index = match play_mode {
        PlayMode::Shuffle => {
            let mut rng = rng();
            (0..tracks.len())
                .choose(&mut rng)
                .ok_or_else(|| App::DataParsing("Failed to choose random track".to_string()))?
        }
        _ => 0,
    };
    current_play_info.index = new_index;
    current_play_info.playing_sid = sid;
    current_play_info.current_tracks.clone_from(&tracks);
    current_play_info.track = tracks.get(new_index).cloned();
    current_play_info.save_to_file().await?;
    Ok(())
}
