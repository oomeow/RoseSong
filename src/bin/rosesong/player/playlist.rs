use rand::rng;
use rand::seq::IteratorRandom;
use rosesong::{
    error::AppError,
    model::{CurrentPlayInfo, PlayMode, Playlist, Track},
    utils::{get_current_play_info, get_playlist, save_current_play_info},
};
use std::sync::LazyLock;
use tokio::sync::RwLock;

// global variables
pub static PLAYLIST: LazyLock<RwLock<Result<Playlist, AppError>>> = LazyLock::new(|| {
    RwLock::new(Ok(Playlist {
        tracks: Vec::new(),
        seasons: Vec::new(),
    }))
});
pub static CURRENT_PLAY_INFO: LazyLock<RwLock<CurrentPlayInfo>> =
    LazyLock::new(|| RwLock::new(CurrentPlayInfo::default()));

pub async fn set_current_track_index(index: usize) -> Result<(), AppError> {
    CURRENT_PLAY_INFO.write().await.set_current(index).await?;
    Ok(())
}

pub async fn load() -> Result<(), AppError> {
    // playlist
    let playlist = get_playlist().await.unwrap_or_default();
    let mut playlist_lock = PLAYLIST.write().await;
    // Replace the old playlist with the new one
    *playlist_lock = Ok(playlist.clone());

    // current play info
    let mut current_play_info = get_current_play_info().await.unwrap_or_default();
    // 初始化播放列表
    let tracks = if let Some(sid) = current_play_info.playing_sid.clone() {
        let mut tracks_ = playlist.find_tracks_in_season(&sid);
        if tracks_.is_empty() {
            // 当前播放合集里的歌曲已经全部清空了, 改为播放全部歌曲
            current_play_info.playing_sid = None;
            tracks_ = playlist.tracks;
        }
        tracks_
    } else {
        playlist.tracks
    };
    current_play_info.current_tracks.clone_from(&tracks);

    let index = current_play_info.index;
    if tracks.is_empty() {
        current_play_info.index = 0;
        current_play_info.track = None;
    } else if index < tracks.len() {
        current_play_info.track = tracks.get(index).cloned();
    } else if index == tracks.len() {
        current_play_info.index = 0;
        current_play_info.track = tracks.first().cloned();
    } else {
        current_play_info.move_to_next_track().await?;
    }

    save_current_play_info(&current_play_info).await?;
    // replace the old current play info with new one
    let mut current_play_info_lock = CURRENT_PLAY_INFO.write().await;
    *current_play_info_lock = current_play_info;

    Ok(())
}

pub async fn get_current_track() -> Result<Track, AppError> {
    let current_play_info = CURRENT_PLAY_INFO.read().await;
    current_play_info
        .get_current_track()
        .ok_or(AppError::DataParsing(
            "Failed to get current track".to_string(),
        ))
}

pub async fn move_to_next_track() -> Result<(), AppError> {
    let mut current_play_info = CURRENT_PLAY_INFO.write().await;
    current_play_info.move_to_next_track().await?;
    Ok(())
}

pub async fn move_to_previous_track() -> Result<(), AppError> {
    let mut current_play_info = CURRENT_PLAY_INFO.write().await;
    current_play_info.move_to_previous_track().await?;
    Ok(())
}

pub async fn update_current_play_tracks(
    sid: Option<String>,
    tracks: Vec<Track>,
) -> Result<(), AppError> {
    let mut current_play_info = CURRENT_PLAY_INFO.write().await;
    let play_mode = current_play_info.play_mode;
    let new_index = match play_mode {
        PlayMode::Shuffle => {
            if tracks.is_empty() {
                0
            } else {
                let mut rng = rng();
                (0..tracks.len()).choose(&mut rng).ok_or_else(|| {
                    AppError::DataParsing("Failed to choose random track".to_string())
                })?
            }
        }
        _ => 0,
    };
    current_play_info.index = new_index;
    current_play_info.playing_sid = sid;
    if tracks.is_empty() {
        current_play_info.current_tracks = Vec::new();
        current_play_info.track = None;
    } else {
        current_play_info.current_tracks.clone_from(&tracks);
        current_play_info.track = tracks.get(new_index).cloned();
    }
    save_current_play_info(&current_play_info).await?;
    Ok(())
}
