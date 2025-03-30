mod bilibili;
mod error;

extern crate colored;

use bilibili::fetch_audio_info::get_tracks;
use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::{generate, Shell};
use colored::Colorize;
use error::App;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::{fmt::Display, io::Write};
use tokio::{fs, io::AsyncBufReadExt, io::AsyncWriteExt, process::Command};
use zbus::{proxy, Connection};

type StdResult<T> = std::result::Result<T, App>;

#[proxy(
    interface = "org.rosesong.Player",
    default_service = "org.rosesong.Player",
    default_path = "/org/rosesong/Player"
)]
trait MyPlayer {
    async fn play(&self) -> zbus::Result<()>;
    async fn play_bvid(&self, bvid: &str) -> zbus::Result<()>;
    async fn pause(&self) -> zbus::Result<()>;
    async fn next(&self) -> zbus::Result<()>;
    async fn previous(&self) -> zbus::Result<()>;
    async fn stop(&self) -> zbus::Result<()>;
    async fn set_mode(&self, mode: &str) -> zbus::Result<()>;
    async fn playlist_change(&self) -> zbus::Result<()>;
    async fn test_connection(&self) -> zbus::Result<()>;
    async fn playlist_is_empty(&self) -> zbus::Result<()>;
}

#[derive(Parser)]
#[command(
    name = "rsg",
    about = "Control the rosesong player.",
    version = "1.0.0"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    #[command(about = "播放指定歌曲或继续播放")]
    Play(PlayCommand),

    #[command(about = "暂停播放")]
    Pause,

    #[command(about = "播放下一首歌曲")]
    Next,

    #[command(about = "播放上一首歌曲")]
    Prev,

    #[command(about = "停止 RoseSong")]
    Stop,

    #[command(about = "设置播放模式")]
    Mode(ModeCommand),

    #[command(about = "添加歌曲到播放列表")]
    Add(AddCommand),

    #[command(about = "在播放列表中查找歌曲")]
    Find(FindCommand),

    #[command(about = "从播放列表中删除歌曲")]
    Delete(DeleteCommand),

    #[command(about = "显示播放列表")]
    List,

    #[command(about = "启动 RoseSong")]
    Start,

    #[command(about = "显示当前播放的歌曲信息")]
    Status,

    #[command(about = "生成对应的 shell 命令补全")]
    GenerateCompletion(ShellCommand),
}

#[derive(Parser)]
struct ShellCommand {
    #[arg(
        short = 's',
        long = "shell",
        long_help = "要生成的 shell 命令补全类型文件, 需要重新初始化补全系统\r\n例如(zsh)：rsg generate-completion --shell zsh > /usr/local/share/zsh/site-functions/_rsg && compinit\r\n"
    )]
    shell: Shell,
}

#[derive(Parser)]
struct PlayCommand {
    #[arg(short = 'b', long = "bvid", help = "要播放的 bvid")]
    bvid: Option<String>,
}

#[derive(Parser)]
struct ModeCommand {
    #[arg(short = 'l', long = "loop", action = clap::ArgAction::SetTrue, help = "设置播放模式为循环播放")]
    loop_mode: bool,
    #[arg(short = 's', long = "shuffle", action = clap::ArgAction::SetTrue, help = "设置播放模式为随机播放")]
    shuffle_mode: bool,
    #[arg(short = 'r', long = "repeat", action = clap::ArgAction::SetTrue, help = "设置播放模式为单曲循环")]
    repeat_mode: bool,
}

#[derive(Parser)]
struct AddCommand {
    #[arg(short = 'f', long = "fid", help = "要导入的收藏夹 ID")]
    fid: Option<String>,
    #[arg(short = 'b', long = "bvid", help = "要导入的 bvid")]
    bvid: Option<String>,
    #[arg(short = 's', long = "sid", help = "要导入的合集 ID")]
    sid: Option<String>,
}

#[derive(Parser)]
struct FindCommand {
    #[arg(short = 'b', long = "bvid", help = "按 bvid 查找")]
    bvid: Option<String>,
    #[arg(short = 'c', long = "cid", help = "按 cid 查找")]
    cid: Option<String>,
    #[arg(short = 't', long = "title", help = "按标题查找")]
    title: Option<String>,
    #[arg(short = 'o', long = "owner", help = "按作者查找")]
    owner: Option<String>,
}

#[derive(Parser)]
struct DeleteCommand {
    #[arg(short = 'b', long = "bvid", help = "按 bvid 删除")]
    bvid: Option<String>,
    #[arg(short = 'c', long = "cid", help = "按 cid 删除")]
    cid: Option<String>,
    #[arg(short = 'o', long = "owner", help = "按作者删除")]
    owner: Option<String>,
    #[arg(short = 'a', long = "all", help = "删除所有曲目")]
    all: bool,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Hash)]
struct Track {
    bvid: String,
    cid: String,
    title: String,
    owner: String,
}

#[derive(Default, Serialize, Deserialize)]
struct Playlist {
    tracks: Vec<Track>,
}

#[derive(Deserialize)]
struct CurrentPlayInfo {
    index: usize,
    play_mode: PlayMode,
    track: Option<Track>,
}

#[derive(Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum PlayMode {
    Loop,
    Shuffle,
    Repeat,
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

#[tokio::main]
async fn main() -> StdResult<()> {
    let cli = Cli::parse();
    let connection = Connection::session().await?;
    let proxy = MyPlayerProxy::new(&connection).await?;
    handle_command(cli, proxy).await
}

async fn handle_command(cli: Cli, proxy: MyPlayerProxy<'_>) -> StdResult<()> {
    match cli.command {
        Commands::Play(play_cmd) => handle_play_command(play_cmd, &proxy).await,
        Commands::Pause => handle_pause_command(&proxy).await,
        Commands::Next => handle_next_command(&proxy).await,
        Commands::Prev => handle_previous_command(&proxy).await,
        Commands::Stop => handle_stop_command(&proxy).await,
        Commands::Mode(mode_cmd) => handle_mode_command(mode_cmd, &proxy).await,
        Commands::Add(add_cmd) => add_tracks(add_cmd.fid, add_cmd.bvid, add_cmd.sid, &proxy).await,
        Commands::Delete(delete_cmd) => {
            delete_tracks(
                delete_cmd.bvid,
                delete_cmd.cid,
                delete_cmd.owner,
                delete_cmd.all,
                &proxy,
            )
            .await
        }
        Commands::Find(find_cmd) => {
            find_track(find_cmd.bvid, find_cmd.cid, find_cmd.title, find_cmd.owner).await
        }
        Commands::List => display_playlist().await,
        Commands::Start => start_rosesong(&proxy).await,
        Commands::Status => display_status(&proxy).await,
        Commands::GenerateCompletion(shell_cmd) => generate_completion(shell_cmd.shell),
    }
}

async fn handle_play_command(play_cmd: PlayCommand, proxy: &MyPlayerProxy<'_>) -> StdResult<()> {
    if !is_rosesong_running(proxy).await? {
        println!("{}", "rosesong 没有处于运行状态".red());
    } else if is_playlist_empty().await? {
        println!("{}", "当前播放列表为空，请先添加歌曲".red());
    } else if let Some(bvid) = play_cmd.bvid {
        proxy.play_bvid(&bvid).await?;
        println!("播放指定 bvid");
    } else {
        proxy.play().await?;
        println!("继续播放");
    }
    Ok(())
}

async fn handle_pause_command(proxy: &MyPlayerProxy<'_>) -> StdResult<()> {
    if !is_rosesong_running(proxy).await? {
        println!("{}", "rosesong 没有处于运行状态".red());
    } else if is_playlist_empty().await? {
        println!("{}", "当前播放列表为空，请先添加歌曲".red());
    } else {
        proxy.pause().await?;
        println!("暂停播放");
    }
    Ok(())
}

async fn handle_next_command(proxy: &MyPlayerProxy<'_>) -> StdResult<()> {
    if !is_rosesong_running(proxy).await? {
        println!("{}", "rosesong 没有处于运行状态".red());
    } else if is_playlist_empty().await? {
        println!("{}", "当前播放列表为空，请先添加歌曲".red());
    } else {
        proxy.next().await?;
        println!("播放下一首");
    }
    Ok(())
}

async fn handle_previous_command(proxy: &MyPlayerProxy<'_>) -> StdResult<()> {
    if !is_rosesong_running(proxy).await? {
        println!("{}", "rosesong 没有处于运行状态".red());
    } else if is_playlist_empty().await? {
        println!("{}", "当前播放列表为空，请先添加歌曲".red());
    } else {
        proxy.previous().await?;
        println!("播放上一首");
    }
    Ok(())
}

async fn handle_stop_command(proxy: &MyPlayerProxy<'_>) -> StdResult<()> {
    if is_rosesong_running(proxy).await? {
        proxy.stop().await?;
        println!("rosesong 已退出");
    } else {
        println!("{}", "rosesong 没有处于运行状态".red());
    }
    Ok(())
}

async fn handle_mode_command(mode_cmd: ModeCommand, proxy: &MyPlayerProxy<'_>) -> StdResult<()> {
    if !is_rosesong_running(proxy).await? {
        println!("{}", "rosesong 没有处于运行状态".red());
    } else if is_playlist_empty().await? {
        println!("{}", "当前播放列表为空，请先添加歌曲".red());
    } else if mode_cmd.loop_mode {
        proxy.set_mode("Loop").await?;
        println!("设置为循环播放");
    } else if mode_cmd.shuffle_mode {
        proxy.set_mode("Shuffle").await?;
        println!("设置为随机播放");
    } else if mode_cmd.repeat_mode {
        proxy.set_mode("Repeat").await?;
        println!("设置为单曲循环");
    } else {
        println!("{}", "没有这个播放模式".red());
    }
    Ok(())
}

async fn is_rosesong_running(proxy: &MyPlayerProxy<'_>) -> StdResult<bool> {
    match proxy.test_connection().await {
        Ok(()) => Ok(true),
        Err(_) => Ok(false),
    }
}

async fn is_playlist_empty() -> StdResult<bool> {
    let playlist_path = initialize_directories().await?.join("playlist.toml");
    if !playlist_path.exists() {
        return Ok(true);
    }
    let content = fs::read_to_string(&playlist_path).await.map_err(App::Io)?;
    Ok(content.trim().is_empty())
}

async fn initialize_directories() -> StdResult<PathBuf> {
    let home_dir = std::env::var("HOME")?;
    let playlists_dir = PathBuf::from(format!("{home_dir}/.config/rosesong/playlists"));
    let required_dirs = [&playlists_dir];
    for dir in required_dirs {
        fs::create_dir_all(dir).await?;
    }
    // let playlist_path = format!("{home_dir}/.config/rosesong/playlists/playlist.toml");
    let playlist_path = playlists_dir.join("playlist.toml");
    if !playlist_path.exists() {
        fs::write(&playlist_path, "").await?;
    }
    Ok(playlists_dir)
}

async fn start_rosesong(proxy: &MyPlayerProxy<'_>) -> StdResult<()> {
    if is_rosesong_running(proxy).await? {
        println!("{}", "RoseSong 当前已经处于运行状态".yellow());
        return Ok(());
    }

    let current_exe_path = std::env::current_exe()?;
    let exe_dir = current_exe_path.parent().ok_or_else(|| {
        App::InvalidInput("Failed to get the directory of the executable".to_string())
    })?;
    let rosesong_path = exe_dir.join("rosesong");

    if !rosesong_path.exists() {
        return Err(App::InvalidInput(
            "rosesong executable not found in the same directory".to_string(),
        ));
    }

    let child = Command::new(rosesong_path).spawn().map_err(App::Io)?;
    println!(
        "{}",
        format!(
            "RoseSong 成功启动，进程 ID: {}",
            child.id().unwrap_or_default().to_string().green()
        )
        .blue()
    );
    Ok(())
}

async fn add_tracks(
    fid: Option<String>,
    bvid: Option<String>,
    cid: Option<String>,
    proxy: &MyPlayerProxy<'_>,
) -> StdResult<()> {
    let playlist_path = initialize_directories().await?.join("playlist.toml");
    let old_content = fs::read_to_string(&playlist_path).await.unwrap_or_default();
    import_favorite_or_bvid_or_cid(fid, bvid, cid).await?;
    let new_content = fs::read_to_string(&playlist_path).await.unwrap_or_default();
    if old_content != new_content {
        if let Ok(is_running) = is_rosesong_running(proxy).await {
            if is_running {
                proxy.playlist_change().await?;
            }
        }
    }
    Ok(())
}

async fn import_favorite_or_bvid_or_cid(
    fid: Option<String>,
    bvid: Option<String>,
    sid: Option<String>,
) -> StdResult<()> {
    let client = reqwest::Client::new();
    let playlist_path = initialize_directories().await?.join("playlist.toml");
    println!("正在获取相关信息");
    let new_tracks = get_tracks(&client, fid, bvid, sid).await?;

    let mut tracks = Vec::new();
    if playlist_path.exists() {
        let content = fs::read_to_string(&playlist_path).await.map_err(App::Io)?;
        let playlist = toml::from_str::<Playlist>(&content).unwrap_or_default();
        tracks.extend(playlist.tracks);
    }

    let new_tracks_bvid: Vec<String> = new_tracks.iter().map(|t| t.bvid.clone()).collect();
    tracks.retain(|t| !new_tracks_bvid.contains(&t.bvid));
    tracks.extend(new_tracks);

    let playlist = Playlist { tracks };
    let toml_content = toml::to_string(&playlist)
        .map_err(|_| App::DataParsing("Failed to serialize tracks to TOML".to_string()))?;
    let mut file = fs::File::create(&playlist_path).await.map_err(App::Io)?;
    file.write_all(toml_content.as_bytes())
        .await
        .map_err(App::Io)?;
    println!("{}", "导入成功".green());
    Ok(())
}

async fn delete_tracks(
    bvid: Option<String>,
    cid: Option<String>,
    owner: Option<String>,
    all: bool,
    proxy: &MyPlayerProxy<'_>,
) -> StdResult<()> {
    let playlist_path = initialize_directories().await?.join("playlist.toml");
    let old_content = fs::read_to_string(&playlist_path).await.unwrap_or_default();
    perform_deletion(bvid, cid, owner, all).await?;
    let new_content = fs::read_to_string(&playlist_path).await.unwrap_or_default();
    if old_content != new_content {
        if let Ok(is_running) = is_rosesong_running(proxy).await {
            if is_running {
                if is_playlist_empty().await? {
                    proxy.playlist_is_empty().await?;
                } else {
                    proxy.playlist_change().await?;
                }
            }
        }
    }
    Ok(())
}

async fn perform_deletion(
    bvid: Option<String>,
    cid: Option<String>,
    owner: Option<String>,
    all: bool,
) -> StdResult<()> {
    let playlist_path = initialize_directories().await?.join("playlist.toml");
    if !playlist_path.exists() {
        println!("{}", "播放列表文件不存在".red());
        return Ok(());
    }
    if all {
        print!("即将清空播放列表，是否确认删除所有歌曲？[y/n]: ");
        std::io::stdout().flush().unwrap();
        let mut confirmation = String::new();
        let mut stdin = tokio::io::BufReader::new(tokio::io::stdin());
        stdin
            .read_line(&mut confirmation)
            .await
            .expect("Failed to read line");
        if confirmation.trim().eq_ignore_ascii_case("y") {
            fs::write(&playlist_path, "").await.map_err(App::Io)?;
            println!("{}", "播放列表已清空".green());
        } else {
            println!("{}", "取消清空操作".yellow());
        }
        return Ok(());
    }
    let content = fs::read_to_string(&playlist_path).await.map_err(App::Io)?;
    let mut playlist: Playlist = toml::from_str(&content)
        .map_err(|_| App::DataParsing("Failed to parse playlist.toml".to_string()))?;
    let mut tracks_to_delete: Vec<Track> = Vec::new();
    if let Some(bvid) = bvid {
        tracks_to_delete.extend(
            playlist
                .tracks
                .iter()
                .filter(|track| track.bvid == bvid)
                .cloned(),
        );
    }
    if let Some(cid) = cid {
        tracks_to_delete.extend(
            playlist
                .tracks
                .iter()
                .filter(|track| track.cid == cid)
                .cloned(),
        );
    }
    if let Some(owner) = owner {
        tracks_to_delete.extend(
            playlist
                .tracks
                .iter()
                .filter(|track| track.owner.contains(&owner))
                .cloned(),
        );
    }
    if tracks_to_delete.is_empty() {
        println!("{}", "没有找到符合条件的track".black());
        return Ok(());
    }
    print!(
        "即将删除 {} 首歌曲，是否确认删除？[y/n]: ",
        tracks_to_delete.len()
    );
    std::io::stdout().flush().unwrap();
    let mut confirmation = String::new();
    let mut stdin = tokio::io::BufReader::new(tokio::io::stdin());
    stdin
        .read_line(&mut confirmation)
        .await
        .expect("Failed to read line");
    if confirmation.trim().eq_ignore_ascii_case("y") {
        playlist
            .tracks
            .retain(|track| !tracks_to_delete.contains(track));
        let toml_content = toml::to_string(&playlist)
            .map_err(|_| App::DataParsing("Failed to serialize tracks to TOML".to_string()))?;
        let mut file = fs::File::create(&playlist_path).await.map_err(App::Io)?;
        file.write_all(toml_content.as_bytes())
            .await
            .map_err(App::Io)?;
        println!("{}", "删除成功".green());
    } else {
        println!("{}", "取消删除操作".yellow());
    }
    Ok(())
}

async fn find_track(
    bvid: Option<String>,
    cid: Option<String>,
    title: Option<String>,
    owner: Option<String>,
) -> StdResult<()> {
    let playlist_path = initialize_directories().await?.join("playlist.toml");
    if !playlist_path.exists() {
        println!("{}", "播放列表文件不存在".red());
        return Ok(());
    }
    let content = fs::read_to_string(&playlist_path).await.map_err(App::Io)?;
    let playlist: Playlist = toml::from_str(&content)
        .map_err(|_| App::DataParsing("Failed to parse playlist.toml".to_string()))?;
    let mut results = playlist.tracks.clone();
    if let Some(bvid) = bvid {
        results.retain(|track| track.bvid == bvid);
    }
    if let Some(cid) = cid {
        results.retain(|track| track.cid == cid);
    }
    if let Some(title) = title {
        results.retain(|track| track.title.contains(&title));
    }
    if let Some(owner) = owner {
        results.retain(|track| track.owner.contains(&owner));
    }
    if results.is_empty() {
        println!("没有找到符合条件的track");
    } else {
        for track in results {
            println!(
                "{}: {}, {} {}, {} {}, {} {}",
                "bvid:".black(),
                track.bvid.yellow(),
                "cid:".black(),
                track.cid,
                "title:".black(),
                track.title.cyan(),
                "owner:".black(),
                track.owner
            );
        }
    }
    Ok(())
}

async fn display_playlist() -> StdResult<()> {
    let playlist_path = initialize_directories().await?.join("playlist.toml");
    if !playlist_path.exists() {
        eprintln!("播放列表文件不存在");
        return Ok(());
    }
    let content = fs::read_to_string(&playlist_path).await.map_err(App::Io)?;
    let playlist: Playlist = toml::from_str(&content)
        .map_err(|_| App::DataParsing("Failed to parse playlist.toml".to_string()))?;
    let tracks = playlist.tracks;
    let total_tracks = tracks.len();
    let page_size = 10;
    let total_pages = total_tracks.div_ceil(page_size);
    let mut current_page = 1;
    loop {
        let start = (current_page - 1) * page_size;
        let end = (start + page_size).min(total_tracks);
        for (i, track) in tracks[start..end].iter().enumerate() {
            println!(
                "{:<2}. {} {}, {} {}, {} {}, {} {}",
                start + i + 1,
                "bvid:".black(),
                track.bvid.yellow(),
                "cid:".black(),
                track.cid,
                "title:".black(),
                track.title.cyan(),
                "owner:".black(),
                track.owner
            );
        }
        print!(
            "{}",
            format!(
                "当前第 {} 页, 请输入页码 (1-{})，或输入 'q' 退出：",
                current_page.to_string().green(),
                total_pages
            )
            .blue()
        );
        std::io::stdout().flush().unwrap();
        let mut input = String::new();
        let mut stdin = tokio::io::BufReader::new(tokio::io::stdin());
        stdin
            .read_line(&mut input)
            .await
            .expect("Failed to read line");
        if input.trim().eq_ignore_ascii_case("q") {
            break;
        }
        match input.trim().parse::<usize>() {
            Ok(page) if page >= 1 && page <= total_pages => current_page = page,
            _ => println!(
                "{}",
                "无效的输入，请输入有效的页码或 'q' 退出".red().on_black()
            ),
        }
        println!("\n");
    }
    Ok(())
}

async fn display_status(proxy: &MyPlayerProxy<'_>) -> Result<(), App> {
    let current_file_path = format!(
        "{}/.config/rosesong/current.toml",
        std::env::var("HOME").expect("Failed to get HOME environment variable")
    );
    let content = fs::read_to_string(&current_file_path)
        .await
        .map_err(App::Io)?;
    let current_play_info = toml::from_str::<CurrentPlayInfo>(&content)
        .map_err(|_| App::DataParsing("Failed to parse current.toml".to_string()))?;

    println!("{}", "[rosesong 信息]".blue().bold().on_black());
    let is_running = is_rosesong_running(proxy).await?;
    let running_status = if is_running {
        "正在运行".green()
    } else {
        "未运行".red()
    };
    println!("运行状态: {running_status}");

    println!(
        "播放模式：{}",
        current_play_info.play_mode.to_string().cyan()
    );

    let playlist_path = initialize_directories().await?.join("playlist.toml");
    let mut playlist = Playlist::default();
    if playlist_path.exists() {
        let content = fs::read_to_string(&playlist_path).await.map_err(App::Io)?;
        playlist = toml::from_str::<Playlist>(&content).unwrap_or_default();
    }
    let is_playlist_empty = is_playlist_empty().await?;
    let playlist_status = if is_playlist_empty {
        "空".red()
    } else {
        format!("共 {} 首歌曲", playlist.tracks.len().to_string().cyan()).normal()
    };
    println!("播放列表: {playlist_status}\n");

    let track_info = current_play_info.track;
    if let Some(track) = track_info {
        println!("{}", "[当前歌曲信息]".blue().bold().on_black());
        println!("索引：{}", current_play_info.index.to_string().yellow());
        println!("BV号：{}", track.bvid.to_string().yellow());
        println!("标题：{}", track.title.yellow());
        println!("up主：{}", track.owner.yellow());
    }
    Ok(())
}

fn generate_completion(shell: Shell) -> Result<(), App> {
    let mut cmd = Cli::command();
    let bin_name = cmd.get_name().to_string();
    generate(shell, &mut cmd, bin_name, &mut std::io::stdout());
    Ok(())
}
