use std::path::PathBuf;

use crate::{
    error::AppError,
    model::{CurrentPlayInfo, Playlist},
};

pub async fn init_dir() -> Result<(), AppError> {
    let home_dir = std::env::var("HOME")?;
    let logs_dir = PathBuf::from(format!("{home_dir}/.config/rosesong/logs"));
    if !logs_dir.exists() {
        tokio::fs::create_dir_all(&logs_dir).await?;
    }
    let playlists_dir = PathBuf::from(format!("{home_dir}/.config/rosesong/playlists"));
    if !playlists_dir.exists() {
        tokio::fs::create_dir_all(&playlists_dir).await?;
    }
    let playlist_path = playlists_dir.join("playlist.toml");
    if !playlist_path.exists() {
        let content = toml::to_string(&Playlist::default())
            .map_err(|_| AppError::DataParsing("Failed to serialize tracks to TOML".to_string()))?;
        tokio::fs::write(&playlist_path, content).await?;
    }
    Ok(())
}

pub fn app_dir() -> Result<PathBuf, AppError> {
    let home_dir = std::env::var("HOME")?;
    let app_dir = PathBuf::from(format!("{home_dir}/.config/rosesong"));
    Ok(app_dir)
}

pub fn logs_dir() -> Result<PathBuf, AppError> {
    let app_dir = app_dir()?;
    Ok(app_dir.join("logs"))
}

pub fn playlist_dir() -> Result<PathBuf, AppError> {
    let app_dir = app_dir()?;
    Ok(app_dir.join("playlists"))
}

pub fn playlist_file() -> Result<PathBuf, AppError> {
    let playlist_dir = playlist_dir()?;
    Ok(playlist_dir.join("playlist.toml"))
}

pub fn current_play_info_file() -> Result<PathBuf, AppError> {
    let app_dir = app_dir()?;
    Ok(app_dir.join("current.toml"))
}

pub async fn save_playlist_to_file(playlist: &Playlist) -> Result<(), AppError> {
    init_dir().await?;
    let file_path = playlist_file()?;
    let content = toml::to_string(playlist)
        .map_err(|_| AppError::DataParsing("Failed to serialize Playlist to TOML".to_string()))?;
    tokio::fs::write(&file_path, content).await?;
    Ok(())
}

pub async fn save_current_play_info(current_play_info: &CurrentPlayInfo) -> Result<(), AppError> {
    init_dir().await?;
    let file_path = current_play_info_file()?;
    let content = toml::to_string(current_play_info).map_err(|_| {
        AppError::DataParsing("Failed to serialize CurrentPlayInfo to TOML".to_string())
    })?;
    tokio::fs::write(&file_path, content).await?;
    Ok(())
}

pub async fn get_playlist() -> Option<Playlist> {
    let file_path = playlist_file().ok()?;
    let content = tokio::fs::read_to_string(&file_path).await.ok()?;
    toml::from_str::<Playlist>(&content).ok()
}

pub async fn get_current_play_info() -> Option<CurrentPlayInfo> {
    let file_path = current_play_info_file().ok()?;
    let content = tokio::fs::read_to_string(&file_path).await.ok()?;
    toml::from_str::<CurrentPlayInfo>(&content).ok()
}

pub async fn is_playlist_empty() -> Result<bool, AppError> {
    let playlist = get_playlist().await;
    Ok(playlist.is_none() || playlist.is_some() && playlist.unwrap().tracks.is_empty())
}
