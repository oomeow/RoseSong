use crate::error::App;
use rand::rng;
use rand::seq::IteratorRandom;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;
use tokio::io::AsyncWriteExt;
use tokio::sync::RwLock;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Track {
    pub bvid: String,
    pub cid: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Playlist {
    pub current: usize,
    pub tracks: Vec<Track>,
}

impl Playlist {
    pub async fn load_from_file(file_path: &str) -> Result<Self, App> {
        log::info!("Loading playlist");
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

    pub fn move_to_next_track(&mut self, play_mode: PlayMode) -> Result<usize, App> {
        let current_index = self.current;
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
        self.current = new_index;
        Ok(new_index)
    }

    pub fn move_to_previous_track(&mut self, play_mode: PlayMode) -> Result<usize, App> {
        let current_index = self.current;
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
        self.current = new_index;
        Ok(new_index)
    }

    pub fn find_track_index(&self, bvid: &str) -> Option<usize> {
        self.tracks.iter().position(|track| track.bvid == bvid)
    }
}

pub static PLAYLIST: LazyLock<RwLock<Result<Playlist, App>>> = LazyLock::new(|| {
    RwLock::new(Ok(Playlist {
        current: 0,
        tracks: Vec::new(),
    }))
});
// pub static CURRENT_TRACK_INDEX: LazyLock<AtomicUsize> = LazyLock::new(|| AtomicUsize::new(0));

pub async fn load(file_path: &str) -> Result<(), App> {
    let playlist = Playlist::load_from_file(file_path).await?;
    let mut playlist_lock = PLAYLIST.write().await;
    *playlist_lock = Ok(playlist); // Replace the old playlist with the new one
    Ok(())
}

pub async fn get_current_track() -> Result<Track, App> {
    let playlist = PLAYLIST.read().await;
    let playlist = playlist.as_ref().map_err(std::clone::Clone::clone)?;
    let index = playlist.current;
    playlist.get_current_track(index)
}

pub async fn move_to_next_track(play_mode: PlayMode) -> Result<usize, App> {
    let mut playlist = PLAYLIST.write().await;
    let playlist = playlist.as_mut().map_err(|e| e.clone())?;
    let index = playlist.move_to_next_track(play_mode)?;
    save_playlist(&playlist).await?;
    Ok(index)
}

pub async fn move_to_previous_track(play_mode: PlayMode) -> Result<usize, App> {
    let mut playlist = PLAYLIST.write().await;
    let playlist = playlist.as_mut().map_err(|e| e.clone())?;
    let index = playlist.move_to_previous_track(play_mode)?;
    save_playlist(&playlist).await?;
    Ok(index)
}

pub async fn set_current_track_index(index: usize) -> Result<(), App> {
    let mut playlist = PLAYLIST.write().await;
    let playlist = playlist.as_mut().map_err(|e| e.clone())?;
    playlist.current = index;
    save_playlist(&playlist).await?;
    Ok(())
}

pub async fn save_playlist(playlist: &Playlist) -> Result<(), App> {
    let playlist_path = format!(
        "{}/.config/rosesong/playlists/playlist.toml",
        std::env::var("HOME").expect("Failed to get HOME environment variable")
    );
    let toml_content = toml::to_string(playlist)
        .map_err(|_| App::DataParsing("Failed to serialize tracks to TOML".to_string()))?;
    let mut file = tokio::fs::File::create(&playlist_path)
        .await
        .map_err(|_| App::Io("Failed to create playlist file".to_string()))?;
    file.write_all(toml_content.as_bytes())
        .await
        .map_err(|_| App::Io("Failed to write playlist file".to_string()))?;
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PlayMode {
    Loop,
    Shuffle,
    Repeat,
}
