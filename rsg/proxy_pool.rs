use reqwest::Proxy;
use serde::Deserialize;

use crate::{error::App, StdResult};

#[derive(Debug, Deserialize)]
struct ProxiesResponse {
    data: Proxies,
}

#[derive(Debug, Deserialize)]
struct Proxies {
    proxies: Vec<ProxyItem>,
}

#[derive(Debug, Deserialize)]
struct ProxyItem {
    // id: String,
    ip: String,
    port: u16,
    #[serde(rename = "type")]
    proxy_type: ProxyType,
    // country: String,
    // response_time: f64,
    // last_check: String,
    // status: isize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
enum ProxyType {
    Http,
    Https,
}

impl ProxyItem {
    fn to_proxy(&self) -> Proxy {
        match self.proxy_type {
            ProxyType::Http => Proxy::http(format!("http://{}:{}", self.ip, self.port)).unwrap(),
            ProxyType::Https => Proxy::https(format!("https://{}:{}", self.ip, self.port)).unwrap(),
        }
    }
}

pub async fn get_proxy() -> StdResult<Proxy> {
    let url = "https://proxy.scdn.io/api/proxy_list.php?page=1&per_page=100&type=HTTP&country=%E4%B8%AD%E5%9B%BD";
    let response = reqwest::Client::new()
        .get(url)
        .header("Content-Type", "application/json")
        .send()
        .await?;
    let response = response.json::<ProxiesResponse>().await?;
    let proxies = response.data.proxies;
    let rand_index = rand::random_range(0..=proxies.len());
    let proxy = proxies.get(rand_index);
    if let Some(proxy) = proxy {
        Ok(proxy.to_proxy())
    } else {
        Err(App::NoProxy)
    }
}
