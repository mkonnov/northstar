// Copyright (c) 2019 - 2020 ESRLabs
//
//   Licensed under the Apache License, Version 2.0 (the "License");
//   you may not use this file except in compliance with the License.
//   You may obtain a copy of the License at
//
//      http://www.apache.org/licenses/LICENSE-2.0
//
//   Unless required by applicable law or agreed to in writing, software
//   distributed under the License is distributed on an "AS IS" BASIS,
//   WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//   See the License for the specific language governing permissions and
//   limitations under the License.

use crate::download::download_updates;
use anyhow::Result;
use async_std::{net::TcpStream, path::PathBuf, prelude::*};
use chrono::Local;
use log::*;
use north_common::manifest::Version;
use std::{collections::HashMap, io::Write};
use structopt::StructOpt;

mod download;

const REMOTE_UPDATE_SERVER: &str = "http://localhost:8080";
const DEFAULT_CONSOLE_ADDRESS: &str = "127.0.0.1:4242";

fn init_logging() {
    if std::env::var("RUST_LOG").is_err() {
        std::env::set_var("RUST_LOG", "info,downloader=debug");
    }
    env_logger::builder()
        .format(|buf, record| {
            writeln!(
                buf,
                "{} [{}] - {}",
                Local::now().format("%Y-%m-%dT%H:%M:%S%.3f"),
                record.level(),
                record.args()
            )
        })
        .init();
}

#[derive(Debug, StructOpt)]
#[structopt(name = "downloader", about = "Downloader")]
pub struct Opt {
    /// Directory to download images into
    #[structopt(long)]
    download_dir: PathBuf,

    /// Console address
    #[structopt(long)]
    console_address: Option<String>,
}

async fn get_response_for_msg(msg: &str, stream: &mut async_std::net::TcpStream) -> Result<String> {
    let mut buf = vec![0u8; 1024];
    stream.write_all(msg.as_bytes()).await?;
    let n = stream.read(&mut buf).await?;
    Ok(String::from_utf8_lossy(&buf[..n]).into_owned())
}

#[async_std::main]
async fn main() -> Result<()> {
    init_logging();
    let opt = Opt::from_args();
    let mut stream = TcpStream::connect(DEFAULT_CONSOLE_ADDRESS).await?;
    info!("Connected to {}", &stream.peer_addr()?);

    let msg = "version-info\n";
    trace!("<- {}", msg);
    let response = get_response_for_msg(msg, &mut stream).await?;
    trace!("-> {}\n", response);
    let versions: Vec<(String, Version, String)> = serde_json::from_str(&response)?;
    info!("we received versions: {:?}", versions);
    let mut version_map: HashMap<String, Version> = HashMap::new();
    versions.iter().fold(&mut version_map, |acc, x| {
        acc.insert(x.0.clone(), x.1.clone());
        acc
    });
    let download_res = download_updates(&version_map, &opt.download_dir).await?;
    info!("{}", download_res);
    let update_msg = format!("update-with {}\n", opt.download_dir.display());
    let update_response = get_response_for_msg(&update_msg, &mut stream).await?;
    debug!("update response: {}", update_response);

    Ok(())
}
