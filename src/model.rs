use std::fmt::Display;

use colored::Colorize;
use rand::seq::IteratorRandom;
use serde::{Deserialize, Serialize};

use crate::{error::AppError, utils::save_current_play_info};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Copy)]
#[serde(rename_all = "lowercase")]
pub enum PlayMode {
    Loop,
    Shuffle,
    Repeat,
}

impl From<String> for PlayMode {
    fn from(s: String) -> Self {
        match s.to_lowercase().as_str() {
            "loop" => PlayMode::Loop,
            "shuffle" => PlayMode::Shuffle,
            "repeat" => PlayMode::Repeat,
            _ => PlayMode::Loop,
        }
    }
}

impl Display for PlayMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PlayMode::Loop => write!(f, "顺序循环"),
            PlayMode::Shuffle => write!(f, "随机"),
            PlayMode::Repeat => write!(f, "单曲循环"),
        }
    }
}
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Track {
    pub bvid: String,
    pub cid: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sid: Option<String>,
    pub title: String,
    pub owner: String,
}

impl Track {
    pub fn to_println_string(&self) -> String {
        format!(
            "{} {}, {} {}, {} {}, {} {}",
            "bvid:".black(),
            self.bvid.yellow(),
            "cid:".black(),
            self.cid,
            "title:".black(),
            self.title.cyan(),
            "owner:".black(),
            self.owner
        )
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Season {
    pub id: String,
    pub title: String,
    pub cover: String,
    pub intro: String,
    pub owner: String,
}

#[derive(Default, Serialize, Deserialize, Clone)]
pub struct Playlist {
    pub tracks: Vec<Track>,
    pub seasons: Vec<Season>,
}

impl Playlist {
    pub fn find_tracks_in_season(&self, sid: &str) -> Vec<Track> {
        self.tracks
            .clone()
            .into_iter()
            .filter(|t| t.sid == Some(sid.to_string()))
            .collect::<Vec<Track>>()
    }
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
    pub async fn set_play_mode(&mut self, mode: PlayMode) -> Result<(), AppError> {
        self.play_mode = mode;
        save_current_play_info(self).await?;
        Ok(())
    }

    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    pub async fn set_volume(&mut self, volume: f64) -> Result<(), AppError> {
        self.volume = (volume * 100.0) as usize;
        save_current_play_info(self).await?;
        Ok(())
    }

    pub async fn set_current(&mut self, index: usize) -> Result<(), AppError> {
        self.index = index;
        let track = self.current_tracks.get(index).cloned();
        self.track = track;
        save_current_play_info(self).await?;
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

    pub async fn move_to_next_track(&mut self) -> Result<(), AppError> {
        let current_index = self.index;
        let current_tracks_len = self.current_tracks.len();
        log::info!(
            "move to next track, current index: {}, current tracks len: {}",
            current_index,
            current_tracks_len
        );
        let new_index = match self.play_mode {
            PlayMode::Loop => (current_index + 1) % current_tracks_len,
            PlayMode::Shuffle => {
                let mut rng = rand::rng();
                (0..current_tracks_len).choose(&mut rng).ok_or_else(|| {
                    AppError::DataParsing("Failed to choose random track".to_string())
                })?
            }
            PlayMode::Repeat => current_index,
        };
        log::info!("move to next track, new index: {}", new_index);
        self.index = new_index;
        self.track = self.current_tracks.get(new_index).cloned();
        save_current_play_info(self).await?;
        Ok(())
    }

    pub async fn move_to_previous_track(&mut self) -> Result<(), AppError> {
        let current_index = self.index;
        let current_tracks_len = self.current_tracks.len();
        let new_index = match self.play_mode {
            PlayMode::Loop => {
                if current_index == 0 {
                    current_tracks_len - 1
                } else {
                    current_index - 1
                }
            }
            PlayMode::Shuffle => {
                let mut rng = rand::rng();
                (0..current_tracks_len).choose(&mut rng).ok_or_else(|| {
                    AppError::DataParsing("Failed to choose random track".to_string())
                })?
            }
            PlayMode::Repeat => current_index,
        };
        self.index = new_index;
        self.track = self.current_tracks.get(new_index).cloned();
        save_current_play_info(self).await?;
        Ok(())
    }
}
