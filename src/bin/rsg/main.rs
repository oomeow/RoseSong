mod bilibili;

use bilibili::fetch_audio_info::get_tracks;
use clap::builder::PossibleValue;
use clap::{CommandFactory, Parser, Subcommand, ValueEnum};
use clap_complete::{generate, Shell};
use colored::Colorize;
use rosesong::error::AppError;
use rosesong::model::{Playlist, Track};
use rosesong::utils::{
    get_current_play_info, get_playlist, init_dir, is_playlist_empty, playlist_file,
    save_playlist_to_file,
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::io::Write;
use tokio::{fs, io::AsyncBufReadExt, process::Command};
use zbus::{proxy, Connection};

type StdResult<T> = std::result::Result<T, AppError>;

#[proxy(
    interface = "org.rosesong.Player",
    default_service = "org.rosesong.Player",
    default_path = "/org/rosesong/Player"
)]
trait MyPlayer {
    async fn play(&self) -> zbus::Result<()>;
    async fn play_bvid(&self, bvid: &str) -> zbus::Result<()>;
    async fn play_sid(&self, sid: &str) -> zbus::Result<()>;
    async fn play_all(&self) -> zbus::Result<()>;
    async fn pause(&self) -> zbus::Result<()>;
    async fn next(&self) -> zbus::Result<()>;
    async fn previous(&self) -> zbus::Result<()>;
    async fn stop(&self) -> zbus::Result<()>;
    async fn set_volume(&self, vol: &str) -> zbus::Result<()>;
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
    #[arg(
        long = "generate",
        value_name = "shell",
        value_enum,
        hide_possible_values = true,
        next_line_help = true,
        long_help = "要生成的 shell 命令补全类型文件, 需要重新初始化补全系统
例如(zsh)：rsg --generate=zsh > /usr/local/share/zsh/site-functions/_rsg && compinit
允许的 shell 类型：zsh, fish, bash, powershell, elvish"
    )]
    generator: Option<Shell>,
    #[arg(long = "listall", value_enum, help = "显示全部歌曲")]
    list_all: Option<ListAllType>,
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Serialize, Deserialize, Clone)]
enum ListAllType {
    Song,
    Season,
}

impl ValueEnum for ListAllType {
    fn value_variants<'a>() -> &'a [Self] {
        &[ListAllType::Song, ListAllType::Season]
    }

    fn to_possible_value(&self) -> Option<PossibleValue> {
        Some(match self {
            ListAllType::Song => PossibleValue::new("song"),
            ListAllType::Season => PossibleValue::new("season"),
        })
    }
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

    #[command(about = "设置音量大小")]
    Vol(VolumeCommand),

    #[command(about = "设置播放模式")]
    Mode(ModeCommand),

    #[command(about = "添加歌曲到歌曲列表")]
    Add(AddCommand),

    #[command(about = "在歌曲列表中查找歌曲")]
    Find(FindCommand),

    #[command(about = "从歌曲列表中删除歌曲")]
    Delete(DeleteCommand),

    #[command(about = "显示歌曲列表")]
    List(ListCommand),

    #[command(about = "更新所有合集")]
    Update,

    #[command(about = "启动 RoseSong")]
    Start,

    #[command(about = "显示当前播放的歌曲信息")]
    Status,
}

#[derive(Parser)]
struct PlayCommand {
    #[arg(short = 'a', long = "all", action = clap::ArgAction::SetTrue, help = "播放全部歌曲")]
    all: bool,
    #[arg(short = 'b', long = "bvid", help = "要播放的 bvid")]
    bvid: Option<String>,
    #[arg(short = 's', long = "sid", help = "要播放的合集 ID")]
    sid: Option<String>,
}

#[derive(Parser)]
struct VolumeCommand {
    #[arg(short = 'u', long = "up", action = clap::ArgAction::SetTrue, help = "音量增加 5%")]
    up: bool,
    #[arg(short = 'd', long = "down", action = clap::ArgAction::SetTrue, help = "音量减少 5%")]
    down: bool,
    #[arg(short = 'v', long = "value", help = "设置音量大小 [0~100]")]
    value: Option<usize>,
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
    // #[arg(short = 'c', long = "cid", help = "按 cid 查找")]
    // cid: Option<String>,
    #[arg(short = 't', long = "title", help = "按标题查找")]
    title: Option<String>,
    #[arg(short = 'o', long = "owner", help = "按作者查找")]
    owner: Option<String>,
}

#[derive(Parser)]
struct DeleteCommand {
    #[arg(short = 'b', long = "bvid", help = "按 bvid 删除")]
    bvid: Option<String>,
    #[arg(short = 's', long = "sid", help = "按合集 ID 删除")]
    sid: Option<String>,
    #[arg(short = 'o', long = "owner", help = "按作者删除")]
    owner: Option<String>,
    #[arg(short = 'a', long = "all", help = "删除所有曲目")]
    all: bool,
}

#[derive(Parser)]
struct ListCommand {
    #[arg(short = 's', action = clap::ArgAction::SetTrue, help = "显示所有合集")]
    season: bool,
}

#[tokio::main]
async fn main() -> StdResult<()> {
    init_dir().await?;
    let cli = Cli::parse();
    let connection = Connection::session().await?;
    let proxy = MyPlayerProxy::new(&connection).await?;
    handle_command(cli, proxy).await
}

async fn handle_command(cli: Cli, proxy: MyPlayerProxy<'_>) -> StdResult<()> {
    if let Some(shell) = cli.generator {
        generate_completion(shell);
        return Ok(());
    }
    if let Some(list_all_type) = cli.list_all {
        handle_list_all(list_all_type).await?;
        return Ok(());
    }

    if let Some(cmd) = cli.command {
        match cmd {
            Commands::Play(play_cmd) => handle_play_command(play_cmd, &proxy).await,
            Commands::Pause => handle_pause_command(&proxy).await,
            Commands::Next => handle_next_command(&proxy).await,
            Commands::Prev => handle_previous_command(&proxy).await,
            Commands::Stop => handle_stop_command(&proxy).await,
            Commands::Vol(vol_cmd) => handle_volume_command(vol_cmd, &proxy).await,
            Commands::Mode(mode_cmd) => handle_mode_command(mode_cmd, &proxy).await,
            Commands::Add(add_cmd) => handle_add_command(add_cmd, &proxy).await,
            Commands::Delete(del_cmd) => handle_delete_command(del_cmd, &proxy).await,
            Commands::Find(find_cmd) => handle_find_command(find_cmd).await,
            Commands::List(list_cmd) => display_playlist(list_cmd).await,
            Commands::Update => update_season(&proxy).await,
            Commands::Start => start_rosesong(&proxy).await,
            Commands::Status => display_status(&proxy).await,
        }
    } else {
        display_status(&proxy).await
    }
}

async fn handle_play_command(play_cmd: PlayCommand, proxy: &MyPlayerProxy<'_>) -> StdResult<()> {
    if !is_rosesong_running(proxy).await? {
        println!("{}", "rosesong 没有处于运行状态".red());
    } else if is_playlist_empty().await? {
        println!("{}", "当前歌曲列表为空，请先添加歌曲".red());
    } else if let Some(bvid) = play_cmd.bvid {
        proxy.play_bvid(&bvid).await?;
        println!("播放指定 bvid");
    } else if let Some(sid) = play_cmd.sid {
        proxy.play_sid(&sid).await?;
        println!("播放指定合集");
    } else if play_cmd.all {
        proxy.play_all().await?;
        println!("播放全部歌曲");
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
        println!("{}", "当前歌曲列表为空，请先添加歌曲".red());
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
        println!("{}", "当前歌曲列表为空，请先添加歌曲".red());
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
        println!("{}", "当前歌曲列表为空，请先添加歌曲".red());
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

async fn handle_volume_command(vol_cmd: VolumeCommand, proxy: &MyPlayerProxy<'_>) -> StdResult<()> {
    if !is_rosesong_running(proxy).await? {
        println!("{}", "rosesong 没有处于运行状态".red());
    } else if is_playlist_empty().await? {
        println!("{}", "当前歌曲列表为空，请先添加歌曲".red());
    } else if vol_cmd.up {
        proxy.set_volume("up").await?;
        println!("增加 5% 音量");
    } else if vol_cmd.down {
        proxy.set_volume("down").await?;
        println!("减少 5% 音量");
    } else if let Some(value) = vol_cmd.value {
        if value > 100 {
            println!("{}", "音量不能超过 100".red());
        } else {
            proxy.set_volume(&value.to_string()).await?;
            println!("设置音量为 {value}%");
        }
    }
    Ok(())
}

async fn handle_mode_command(mode_cmd: ModeCommand, proxy: &MyPlayerProxy<'_>) -> StdResult<()> {
    if !is_rosesong_running(proxy).await? {
        println!("{}", "rosesong 没有处于运行状态".red());
    } else if is_playlist_empty().await? {
        println!("{}", "当前歌曲列表为空，请先添加歌曲".red());
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

async fn start_rosesong(proxy: &MyPlayerProxy<'_>) -> StdResult<()> {
    if is_rosesong_running(proxy).await? {
        println!("{}", "RoseSong 当前已经处于运行状态".yellow());
        return Ok(());
    }

    let current_exe_path = std::env::current_exe()?;
    let exe_dir = current_exe_path.parent().ok_or_else(|| {
        AppError::InvalidInput("Failed to get the directory of the executable".to_string())
    })?;
    let rosesong_path = exe_dir.join("rosesong");

    if !rosesong_path.exists() {
        return Err(AppError::InvalidInput(
            "rosesong executable not found in the same directory".to_string(),
        ));
    }

    let child = Command::new(rosesong_path).spawn()?;
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

async fn reload_playlist(proxy: &MyPlayerProxy<'_>) -> StdResult<()> {
    let is_running = is_rosesong_running(proxy).await?;
    if is_running {
        if is_playlist_empty().await? {
            proxy.playlist_is_empty().await?;
        } else {
            proxy.playlist_change().await?;
        }
    } else if let Some(mut cur_play_info) = get_current_play_info().await {
        if let Some(cur_track) = cur_play_info.get_current_track() {
            let playlist = get_playlist().await.unwrap_or_default();
            if let Some(sid) = cur_play_info.playing_sid.as_ref() {
                let tracks = playlist.find_tracks_in_season(sid);
                cur_play_info.current_tracks = tracks;
            } else {
                cur_play_info.current_tracks = playlist.tracks;
            }
            let cur_bvid = cur_track.bvid;
            let new_index = cur_play_info
                .current_tracks
                .clone()
                .iter()
                .position(|t| t.bvid == cur_bvid);
            match new_index {
                Some(new_index) => {
                    cur_play_info.set_current(new_index).await?;
                }
                None => {
                    cur_play_info.set_current(cur_play_info.index).await?;
                }
            }
        }
    }
    Ok(())
}

async fn handle_add_command(add_cmd: AddCommand, proxy: &MyPlayerProxy<'_>) -> StdResult<()> {
    let playlist_path = playlist_file()?;
    let old_content = fs::read_to_string(&playlist_path).await.unwrap_or_default();
    println!("正在获取相关信息");
    import_favorite_or_bvid_or_sid(add_cmd.fid, add_cmd.bvid, add_cmd.sid).await?;
    println!("{}", "导入成功".green());
    let new_content = fs::read_to_string(&playlist_path).await.unwrap_or_default();
    if old_content != new_content {
        reload_playlist(proxy).await?;
    }
    Ok(())
}

async fn import_favorite_or_bvid_or_sid(
    fid: Option<String>,
    bvid: Option<String>,
    sid: Option<String>,
) -> StdResult<()> {
    let client = reqwest::Client::new();
    let (new_tracks, new_season) = get_tracks(&client, fid, bvid, sid).await?;

    let mut tracks = Vec::new();
    let mut seasons = Vec::new();
    let playlist = get_playlist().await;
    if let Some(playlist) = playlist {
        tracks.extend(playlist.tracks);
        seasons.extend(playlist.seasons);
    }
    // update tracks
    let new_tracks_bvid: Vec<String> = new_tracks.iter().map(|t| t.bvid.clone()).collect();
    tracks.retain(|t| !new_tracks_bvid.contains(&t.bvid));
    tracks.extend(new_tracks);
    // update seasons
    if let Some(new_season) = new_season {
        seasons.retain(|s| s.id != new_season.id);
        seasons.push(new_season);
    }

    let playlist = Playlist { tracks, seasons };
    save_playlist_to_file(&playlist).await?;
    Ok(())
}

async fn handle_delete_command(del_cmd: DeleteCommand, proxy: &MyPlayerProxy<'_>) -> StdResult<()> {
    let playlist_path = playlist_file()?;
    if !playlist_path.exists() {
        println!("{}", "歌曲列表文件不存在".red());
        return Ok(());
    }
    let old_content = fs::read_to_string(&playlist_path).await.unwrap_or_default();
    perform_deletion(del_cmd.bvid, del_cmd.sid, del_cmd.owner, del_cmd.all).await?;
    let new_content = fs::read_to_string(&playlist_path).await.unwrap_or_default();
    if old_content != new_content {
        reload_playlist(proxy).await?;
    }
    Ok(())
}

async fn perform_deletion(
    bvid: Option<String>,
    sid: Option<String>,
    owner: Option<String>,
    all: bool,
) -> StdResult<()> {
    if all {
        print!("即将清空歌曲列表，是否确认删除所有歌曲？[y/n]: ");
        std::io::stdout().flush().unwrap();
        let mut confirmation = String::new();
        let mut stdin = tokio::io::BufReader::new(tokio::io::stdin());
        stdin
            .read_line(&mut confirmation)
            .await
            .expect("Failed to read line");
        if confirmation.trim().eq_ignore_ascii_case("y") {
            save_playlist_to_file(&Playlist::default()).await?;
            println!("{}", "歌曲列表已清空".green());
        } else {
            println!("{}", "取消清空操作".yellow());
        }
        return Ok(());
    }

    let mut playlist = get_playlist().await.unwrap_or_default();
    let mut tracks_to_delete: Vec<Track> = Vec::new();

    // bvid
    if let Some(bvid) = bvid {
        tracks_to_delete.extend(
            playlist
                .tracks
                .iter()
                .filter(|track| track.bvid == bvid)
                .cloned(),
        );
    }
    // sid
    if sid.is_some() {
        tracks_to_delete.extend(
            playlist
                .tracks
                .iter()
                .filter(|track| track.sid == sid)
                .cloned(),
        );
    }
    // owner
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
        // retain tracks
        playlist
            .tracks
            .retain(|track| !tracks_to_delete.contains(track));
        // retain seasons
        let exist_seasons = playlist
            .tracks
            .iter()
            .filter_map(|t| t.sid.clone())
            .collect::<HashSet<String>>();
        playlist.seasons.retain(|s| exist_seasons.contains(&s.id));
        // save to file
        save_playlist_to_file(&playlist).await?;
        println!("{}", "删除成功".green());
    } else {
        println!("{}", "取消删除操作".yellow());
    }
    Ok(())
}

async fn handle_find_command(find_cmd: FindCommand) -> StdResult<()> {
    let playlist = get_playlist().await;
    if let Some(playlist) = playlist {
        let mut results = playlist.tracks;
        if let Some(bvid) = find_cmd.bvid {
            results.retain(|track| track.bvid == bvid);
        }
        if let Some(title) = find_cmd.title {
            results.retain(|track| track.title.contains(&title));
        }
        if let Some(owner) = find_cmd.owner {
            results.retain(|track| track.owner.contains(&owner));
        }
        if results.is_empty() {
            println!("没有找到符合条件的 track");
        } else {
            let list = results.iter().map(|t| t.to_println_string()).collect();
            show_list_page(list).await;
        }
    } else {
        println!("{}", "歌曲列表文件不存在".red());
    }
    Ok(())
}

async fn display_playlist(list_cmd: ListCommand) -> StdResult<()> {
    let is_empty = is_playlist_empty().await?;
    if is_empty {
        println!("{}", "歌曲列表为空".red());
        return Ok(());
    }
    if let Some(playlist) = get_playlist().await {
        if list_cmd.season {
            let seasons = playlist.seasons;
            let list = seasons.iter().map(|s| s.to_println_string()).collect();
            show_list_page(list).await;
        } else {
            let tracks = playlist.tracks;
            let list = tracks.iter().map(|t| t.to_println_string()).collect();
            show_list_page(list).await;
        }
    }
    Ok(())
}

async fn update_season(proxy: &MyPlayerProxy<'_>) -> StdResult<()> {
    if let Some(mut playlist) = get_playlist().await {
        // clean all season songs
        let retain_tracks = playlist
            .tracks
            .clone()
            .into_iter()
            .filter(|t| t.sid.is_none())
            .collect::<Vec<Track>>();
        playlist.tracks = retain_tracks;
        save_playlist_to_file(&playlist).await?;
        // starting update season songs
        for season in playlist.seasons {
            println!("更新合集：{}", season.title.blue());
            if let Err(e) = import_favorite_or_bvid_or_sid(None, None, Some(season.id)).await {
                eprintln!(
                    "{}",
                    format!("更新合集[{}]失败：{}", season.title.blue(), e).red()
                );
            }
        }
        reload_playlist(proxy).await?;
        println!("{}", "合集更新成功".green());
    }
    Ok(())
}

async fn show_list_page(list: Vec<String>) {
    let total_tracks = list.len();
    let page_size = 10;
    let total_pages = total_tracks.div_ceil(page_size);
    let mut current_page = 1;
    if list.len() <= 15 {
        for (i, item) in list.iter().enumerate() {
            println!("{:<2}. {}", i + 1, item);
        }
    } else {
        loop {
            let start = (current_page - 1) * page_size;
            let end = (start + page_size).min(total_tracks);
            for (i, line) in list[start..end].iter().enumerate() {
                println!("{:<2}. {}", start + i + 1, line);
            }
            print!(
                "{}",
                format!(
                    "当前第 {} 页, 请输入页码 [1-{}], 或输入 'q' 退出：",
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
                _ => println!("{}", "无效的输入，请输入有效的页码或 'q' 退出".red()),
            }
            println!("\n");
        }
    }
}

async fn display_status(proxy: &MyPlayerProxy<'_>) -> Result<(), AppError> {
    // play list
    let playlist = get_playlist().await.unwrap_or_default();
    let is_playlist_empty = is_playlist_empty().await?;
    // current play info
    let current_play_info = get_current_play_info().await.unwrap_or_default();
    let mut current_play_season = None;
    if let Some(sid) = current_play_info.playing_sid.clone() {
        let season = playlist.seasons.iter().find(|s| s.id == sid).cloned();
        current_play_season = season;
    }

    // show info
    println!("{}", "[rosesong 信息]".blue().bold());
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

    println!(
        "音量大小：{}",
        format!("{}%", current_play_info.volume).cyan()
    );

    let play_status = {
        if let Some(season) = &current_play_season {
            let season_name = season.title.clone().yellow();
            format!("仅播放合集 [{season_name}]").cyan()
        } else {
            "全部歌曲".cyan()
        }
    };
    println!("播放列表状态：{play_status}");

    let playlist_status = if is_playlist_empty {
        "空".red()
    } else {
        format!("共 {} 首", playlist.tracks.len().to_string().cyan()).normal()
    };
    println!("全部歌曲: {playlist_status}\n");

    if let Some(season) = current_play_season {
        let current_tracks_length = if is_playlist_empty {
            "空".red()
        } else {
            format!(
                "共 {} 首歌曲",
                current_play_info.current_tracks.len().to_string().yellow()
            )
            .normal()
        };
        println!("{}", "[当前合集信息]".blue().bold());
        println!("ID：{}", season.id.to_string().yellow());
        println!("全部歌曲：{current_tracks_length}");
        println!("标题：{}", season.title.to_string().yellow());
        println!("简介：{}", season.intro.to_string().yellow());
        println!("up主：{}\n", season.owner.yellow());
    }
    let track_info = current_play_info.track;
    if let Some(track) = track_info {
        println!("{}", "[当前歌曲信息]".blue().bold());
        println!(
            "当前播放列表索引：{}",
            current_play_info.index.to_string().yellow()
        );
        println!("BV号：{}", track.bvid.to_string().yellow());
        println!("标题：{}", track.title.yellow());
        println!("up主：{}", track.owner.yellow());
    }
    Ok(())
}

fn generate_completion(shell: Shell) {
    let mut cmd = Cli::command();
    let bin_name = cmd.get_name().to_string();
    generate(shell, &mut cmd, bin_name, &mut std::io::stdout());
}

async fn handle_list_all(list_all_type: ListAllType) -> StdResult<()> {
    let is_empty = is_playlist_empty().await?;
    if is_empty {
        return Ok(());
    }
    if let Some(playlist) = get_playlist().await {
        match list_all_type {
            ListAllType::Song => {
                let tracks = playlist.tracks;
                for track in tracks {
                    println!("{},{} - {}", track.bvid, track.title, track.owner);
                }
            }
            ListAllType::Season => {
                let seasons = playlist.seasons;
                for season in seasons {
                    let total = playlist
                        .tracks
                        .iter()
                        .filter(|t| t.sid == Some(season.id.clone()))
                        .count();
                    println!("{},{}  [共{}首]", season.id, season.title, total);
                }
            }
        }
    }
    Ok(())
}
