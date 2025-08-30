use std::{io::Write, sync::mpsc};

use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use reqwest::Client;
use rosesong::{
    error::AppError,
    model::{Season, Track},
};
use serde::Deserialize;
use tokio::io::AsyncBufReadExt;

#[derive(Deserialize)]
pub struct Owner {
    pub name: String,
}

#[derive(Deserialize)]
pub struct VideoData {
    pub bvid: String,
    pub title: String,
    pub cid: i64,
    pub owner: Owner,
    pub season_id: Option<i64>,
    pub ugc_season: Option<UgcSeason>,
}

#[derive(Deserialize)]
pub struct UgcSeason {
    pub id: i64,
    pub title: String,
    pub cover: String,
    pub intro: String,
    pub sections: Vec<Section>,
}

#[derive(Deserialize)]
pub struct Section {
    pub season_id: i64,
    pub episodes: Vec<Episode>,
}

#[derive(Deserialize)]
pub struct Episode {
    pub cid: i64,
    pub bvid: String,
    pub title: String,
}

impl VideoData {
    pub fn to_season(&self) -> Option<Season> {
        self.ugc_season.as_ref().map(|ugc_season| Season {
            id: ugc_season.id.to_string(),
            title: ugc_season.title.clone(),
            cover: ugc_season.cover.clone(),
            intro: ugc_season.intro.clone(),
            owner: self.owner.name.clone(),
        })
    }

    pub fn to_tracks_by_season(&self) -> Vec<Track> {
        let mut tracks = Vec::new();
        if let Some(ugc_season) = &self.ugc_season {
            for section in &ugc_season.sections {
                for episode in &section.episodes {
                    tracks.push(Track {
                        bvid: episode.bvid.clone(),
                        cid: episode.cid.to_string(),
                        sid: Some(section.season_id.to_string()),
                        title: episode.title.clone(),
                        owner: self.owner.name.clone(),
                    });
                }
            }
        }
        tracks
    }

    pub fn to_track(&self) -> Track {
        Track {
            bvid: self.bvid.clone(),
            cid: self.cid.to_string(),
            sid: None,
            title: self.title.clone(),
            owner: self.owner.name.clone(),
        }
    }
}

#[derive(Deserialize)]
struct ApiResponse<T> {
    data: T,
}

pub fn create_progress_bar(total: u64) -> ProgressBar {
    let pb = ProgressBar::new(total);
    pb.set_style(
        ProgressStyle::default_bar()
            .template(
                "{spinner:.green} [{elapsed_precise}] [{bar:40.blue}] ({pos}/{len}, ETA {eta})",
            )
            .unwrap()
            .progress_chars("█▓▒░  "),
    );
    pb
}

// 可通过该方法获取合集里的所有视频信息 (ugc_season -> sections -> episodes(合集里的所有视频数组对象))
pub async fn fetch_video_data(client: &Client, bvid: &str) -> Result<VideoData, AppError> {
    let url = format!("https://api.bilibili.com/x/web-interface/view?bvid={bvid}");
    let response = client.get(&url).send().await.map_err(|e| {
        eprintln!("Failed to send request to {url}: {e}");
        AppError::HttpRequest(e)
    })?;
    let mut api_response: ApiResponse<VideoData> = response.json().await.map_err(|e| {
        eprintln!("Failed to parse response from {url}: {e}");
        AppError::HttpRequest(e)
    })?;
    api_response.data.bvid = bvid.to_string();
    Ok(api_response.data)
}

pub async fn fetch_bvids_from_fid(client: &Client, fid: &str) -> Result<Vec<String>, AppError> {
    let url = format!("https://api.bilibili.com/x/v3/fav/resource/ids?media_id={fid}");
    let response = client.get(&url).send().await.map_err(|e| {
        eprintln!("Failed to send request to {url}: {e}");
        AppError::HttpRequest(e)
    })?;
    let json: serde_json::Value = response.json().await.map_err(|e| {
        eprintln!("Failed to parse response from {url}: {e}");
        AppError::HttpRequest(e)
    })?;
    let bvids: Vec<String> = json["data"]
        .as_array()
        .ok_or({
            eprintln!("Failed to find 'data' array in response from {url}");
            AppError::DataParsing("数据中缺少 bvids 数组".to_string())
        })?
        .iter()
        .filter_map(|v| v["bvid"].as_str().map(String::from))
        .collect();

    if bvids.is_empty() {
        return Err(AppError::InvalidInput(
            "提供的 fid 无效或没有找到相关的视频".to_string(),
        ));
    }

    Ok(bvids)
}

pub async fn fetch_bvids_from_session_id(
    client: &Client,
    season_id: &str,
) -> Result<Vec<String>, AppError> {
    let url = format!("https://api.bilibili.com/x/space/fav/season/list?season_id={season_id}");
    let response = client.get(&url).send().await.map_err(|e| {
        eprintln!("Failed to send request to {url}: {e}");
        AppError::HttpRequest(e)
    })?;
    let json: serde_json::Value = response.json().await.map_err(|e| {
        eprintln!("Failed to parse response from {url}: {e}");
        AppError::HttpRequest(e)
    })?;
    let bvids: Vec<String> = json["data"]["medias"]
        .as_array()
        .ok_or_else(|| {
            eprintln!("Failed to find 'data' array in response from {url}");
            AppError::DataParsing("数据中缺少 bvids 数组".to_string())
        })?
        .iter()
        .filter_map(|v| v["bvid"].as_str().map(String::from))
        .collect();

    if bvids.is_empty() {
        return Err(AppError::InvalidInput(
            "提供的 fid 无效或没有找到相关的视频".to_string(),
        ));
    }

    Ok(bvids)
}

pub async fn get_tracks(
    client: &Client,
    fid: Option<String>,
    bvid: Option<String>,
    sid: Option<String>,
) -> Result<(Vec<Track>, Option<Season>), AppError> {
    let mut track_list = Vec::new();
    let mut season = None;

    if let Some(fid) = fid {
        let bvids = fetch_bvids_from_fid(client, &fid).await?;
        batch_fetch_audio_info(client, &mut track_list, &bvids)?;
    } else if let Some(bvid) = bvid {
        let video_data = fetch_video_data(client, &bvid).await?;
        if video_data.season_id.is_some() {
            print!(
                "该歌曲位于合集 [{}] 中，是否导入该合集? [y/n]: ",
                video_data
                    .ugc_season
                    .as_ref()
                    .map(|i| i.title.clone())
                    .unwrap_or_default()
            );
            std::io::stdout().flush().unwrap();
            let mut confirmation = String::new();
            let mut stdin = tokio::io::BufReader::new(tokio::io::stdin());
            stdin
                .read_line(&mut confirmation)
                .await
                .expect("Failed to read line");
            if confirmation.trim().eq_ignore_ascii_case("y") {
                track_list.extend(video_data.to_tracks_by_season());
            } else {
                track_list.push(video_data.to_track());
            }
            season = video_data.to_season();
        } else {
            track_list.push(video_data.to_track());
        }
    } else if let Some(season_id) = sid {
        let bvids = fetch_bvids_from_session_id(client, &season_id).await?;
        let video_data = fetch_video_data(client, &bvids[0]).await?;
        track_list.extend(video_data.to_tracks_by_season());
        season = video_data.to_season();
    } else {
        return Err(AppError::InvalidInput(
            "请提供正确的 fid 或 bvid 或 sid".to_string(),
        ));
    }

    if track_list.is_empty() {
        return Err(AppError::InvalidInput(
            "提供的 bvid 或 fid 或 sid 无效或没有找到相关的视频".to_string(),
        ));
    }

    Ok((track_list, season))
}

fn batch_fetch_audio_info(
    client: &Client,
    track_list: &mut Vec<Track>,
    bvids: &[String],
) -> Result<(), AppError> {
    let (send, recv) = mpsc::channel();
    let m = MultiProgress::new();
    let batch_bvids = bvids
        .chunks(100)
        .map(<[String]>::to_vec)
        .collect::<Vec<Vec<String>>>();

    for batch in batch_bvids {
        let pb = m.add(create_progress_bar(batch.clone().len() as u64));
        let task_data = batch.clone();
        let send_ = send.clone();
        let client_ = client.clone();
        tokio::spawn(async move {
            for task_bvid in task_data {
                let video_data = fetch_video_data(&client_, &task_bvid).await.unwrap();
                send_.send(video_data).unwrap();
                pb.inc(1);
            }
            pb.finish_and_clear();
        });
    }

    drop(send);

    for video_data in recv {
        track_list.extend(video_data.to_tracks_by_season());
    }

    if track_list.is_empty() {
        return Err(AppError::InvalidInput(
            "提供的 bvid 无效或没有找到相关的视频".to_string(),
        ));
    }

    Ok(())
}
