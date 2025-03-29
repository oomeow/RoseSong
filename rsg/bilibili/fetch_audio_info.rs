use std::{io::Write, sync::mpsc};

use crate::error::App;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use reqwest::Client;
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

pub async fn fetch_video_data(client: &Client, bvid: &str) -> Result<VideoData, App> {
    let url = format!("https://api.bilibili.com/x/web-interface/view?bvid={bvid}");
    let response = client.get(&url).send().await.map_err(|e| {
        eprintln!("Failed to send request to {url}: {e}");
        App::HttpRequest(e)
    })?;
    let mut api_response: ApiResponse<VideoData> = response.json().await.map_err(|e| {
        eprintln!("Failed to parse response from {url}: {e}");
        App::HttpRequest(e)
    })?;
    api_response.data.bvid = bvid.to_string();
    Ok(api_response.data)
}

pub async fn fetch_bvids_from_fid(client: &Client, fid: &str) -> Result<Vec<String>, App> {
    let url = format!("https://api.bilibili.com/x/v3/fav/resource/ids?media_id={fid}");
    let response = client.get(&url).send().await.map_err(|e| {
        eprintln!("Failed to send request to {url}: {e}");
        App::HttpRequest(e)
    })?;
    let json: serde_json::Value = response.json().await.map_err(|e| {
        eprintln!("Failed to parse response from {url}: {e}");
        App::HttpRequest(e)
    })?;
    let bvids: Vec<String> = json["data"]
        .as_array()
        .ok_or_else(|| {
            eprintln!("Failed to find 'data' array in response from {url}");
            App::DataParsing("数据中缺少 bvids 数组".to_string())
        })?
        .iter()
        .filter_map(|v| v["bvid"].as_str().map(String::from))
        .collect();

    if bvids.is_empty() {
        return Err(App::InvalidInput(
            "提供的 fid 无效或没有找到相关的视频".to_string(),
        ));
    }

    Ok(bvids)
}

pub async fn fetch_bvids_from_session_id(
    client: &Client,
    season_id: &str,
) -> Result<Vec<String>, App> {
    let url = format!("https://api.bilibili.com/x/space/fav/season/list?season_id={season_id}");
    let response = client.get(&url).send().await.map_err(|e| {
        eprintln!("Failed to send request to {url}: {e}");
        App::HttpRequest(e)
    })?;
    let json: serde_json::Value = response.json().await.map_err(|e| {
        eprintln!("Failed to parse response from {url}: {e}");
        App::HttpRequest(e)
    })?;
    let bvids: Vec<String> = json["data"]["medias"]
        .as_array()
        .ok_or_else(|| {
            eprintln!("Failed to find 'data' array in response from {url}");
            App::DataParsing("数据中缺少 bvids 数组".to_string())
        })?
        .iter()
        .filter_map(|v| v["bvid"].as_str().map(String::from))
        .collect();

    if bvids.is_empty() {
        return Err(App::InvalidInput(
            "提供的 fid 无效或没有找到相关的视频".to_string(),
        ));
    }

    Ok(bvids)
}

pub async fn get_video_data(
    client: &Client,
    fid: Option<&str>,
    bvid: Option<&str>,
    sid: Option<&str>,
) -> Result<Vec<VideoData>, App> {
    let mut video_data_list = Vec::new();

    if let Some(fid) = fid {
        let bvids = fetch_bvids_from_fid(client, fid).await?;
        batch_fetch_audio_info(client, &mut video_data_list, &bvids)?;
    } else if let Some(bvid) = bvid {
        let video_data = fetch_video_data(client, bvid).await?;
        if let Some(season_id) = video_data.season_id.as_ref() {
            print!("该歌曲位于合集中，是否导入该合集? [y/n]: ");
            std::io::stdout().flush().unwrap();
            let mut confirmation = String::new();
            let mut stdin = tokio::io::BufReader::new(tokio::io::stdin());
            stdin
                .read_line(&mut confirmation)
                .await
                .expect("Failed to read line");
            if confirmation.trim().eq_ignore_ascii_case("y") {
                let bvids = fetch_bvids_from_session_id(client, &season_id.to_string()).await?;
                batch_fetch_audio_info(client, &mut video_data_list, &bvids)?;
            } else {
                video_data_list.push(video_data);
            }
        } else {
            video_data_list.push(video_data);
        }
    } else if let Some(season_id) = sid {
        let bvids = fetch_bvids_from_session_id(client, season_id).await?;
        batch_fetch_audio_info(client, &mut video_data_list, &bvids)?;
    } else {
        return Err(App::InvalidInput("请提供正确的 fid 或 bvid".to_string()));
    }

    if video_data_list.is_empty() {
        return Err(App::InvalidInput(
            "提供的 fid 或 bvid 无效或没有找到相关的视频".to_string(),
        ));
    }

    Ok(video_data_list)
}

fn batch_fetch_audio_info(
    client: &Client,
    video_data_list: &mut Vec<VideoData>,
    bvids: &[String],
) -> Result<(), App> {
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
        video_data_list.push(video_data);
    }

    if video_data_list.is_empty() {
        return Err(App::InvalidInput(
            "提供的 bvid 无效或没有找到相关的视频".to_string(),
        ));
    }

    Ok(())
}
