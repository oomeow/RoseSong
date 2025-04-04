mod bilibili;
mod dbus;
mod error;
mod player;
mod temp_dbus;

use crate::error::App;
use crate::player::playlist::PlayMode;
use crate::player::Audio;
use flexi_logger::{Cleanup, Criterion, Duplicate, FileSpec, Logger, Naming};
use log::{error, warn};
use player::playlist::{load, CURRENT_PLAY_INFO};
use std::path::Path;
use std::process;
use std::sync::Arc;
use tikv_jemallocator::Jemalloc;
use tokio::fs;
use tokio::{
    sync::{mpsc, watch, Mutex},
    task,
};

#[global_allocator]
static GLOBAL: Jemalloc = Jemalloc;

#[tokio::main]
async fn main() -> Result<(), App> {
    let home_dir = std::env::var("HOME").map_err(|e| {
        App::Io(
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("Failed to get HOME environment variable: {e}"),
            )
            .to_string(),
        )
    })?;

    // Define the required directories
    let required_dirs = [
        format!("{home_dir}/.config/rosesong/logs"),
        format!("{home_dir}/.config/rosesong/playlists"),
    ];

    // Ensure all directories exist
    for dir in &required_dirs {
        fs::create_dir_all(dir).await?;
    }

    // Check if playlist.toml exists, if not, create an empty one
    let playlist_path = format!("{home_dir}/.config/rosesong/playlists/playlist.toml");
    if !Path::new(&playlist_path).exists() {
        fs::write(&playlist_path, "").await?;
    }

    // Logger setup
    Logger::try_with_str("info")?
        .format(|w, _, record| {
            write!(
                w,
                "{} [{}:{}] {}",
                record.level(),
                record.module_path().unwrap_or("<unknown>"),
                record.line().unwrap_or(0),
                record.args()
            )
        })
        .log_to_file(FileSpec::default().directory(&required_dirs[0]))
        // .duplicate_to_stdout(Duplicate::All) // for debug
        .rotate(
            Criterion::Size(1_000_000),
            Naming::Timestamps,
            Cleanup::KeepLogFiles(3),
        )
        .duplicate_to_stderr(Duplicate::None)
        .start()?;

    // Check if the playlist is empty
    {
        let playlist_content = fs::read_to_string(&playlist_path).await?;
        if playlist_content.trim().is_empty() {
            warn!("Current playlist is empty");
            let (stop_sender, stop_receiver) = watch::channel(());
            let _ = start_temp_dbus_listener(stop_sender).await;
            wait_for_stop_signal(stop_receiver).await;
            let playlist_content = fs::read_to_string(&playlist_path).await?;
            if playlist_content.trim().is_empty() {
                process::exit(0);
            }
        }
    }

    load().await?;
    let (stop_sender, stop_receiver) = watch::channel(());
    let play_mode = CURRENT_PLAY_INFO.read().await.play_mode;
    let _audio_player = start_player_and_dbus_listener(play_mode, &stop_sender)?;
    wait_for_stop_signal(stop_receiver).await;
    process::exit(0);
}

async fn wait_for_stop_signal(mut stop_receiver: watch::Receiver<()>) {
    stop_receiver.changed().await.unwrap();
}

async fn start_temp_dbus_listener(
    stop_signal: watch::Sender<()>,
) -> Result<(), Box<dyn std::error::Error>> {
    let stop_receiver = stop_signal.subscribe();

    task::spawn({
        let stop_signal = stop_signal.clone();
        async move {
            let result = temp_dbus::run_temp_dbus_server(stop_signal).await;
            if let Err(e) = result {
                error!("Temp DBus listener error: {}", e);
            }
        }
    });

    // Wait for the stop signal
    wait_for_stop_signal(stop_receiver).await;

    Ok(())
}

fn start_player_and_dbus_listener(
    play_mode: PlayMode,
    stop_signal: &watch::Sender<()>,
) -> Result<Audio, App> {
    let (command_sender, command_receiver) = mpsc::channel(1);

    let audio_player = Audio::new(play_mode, Arc::new(Mutex::new(command_receiver)))?;

    task::spawn({
        let command_sender = command_sender.clone();
        let stop_signal = stop_signal.clone();
        async move {
            let _ = dbus::run_dbus_server(command_sender, stop_signal).await;
        }
    });

    task::spawn({
        let audio_player = audio_player.clone();
        async move {
            audio_player.play_playlist().await.unwrap();
            #[allow(clippy::cast_precision_loss)]
            let volume = CURRENT_PLAY_INFO.read().await.volume as f64 / 100.0;
            audio_player.fade_volume(0.0, volume, 3);
        }
    });

    Ok(audio_player)
}
