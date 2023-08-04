use anyhow::Context;
use clap::Parser;
use octocrab::{
    models::{commits::Commit, issues::Issue, pulls::PullRequest, timelines::Milestone},
    params, Octocrab, Page,
};
use serde::Deserialize;
use tokio;
use toml;
use url::Url;
mod remote;
use crate::remote::get_remotes;
/*
app state machine

INI initializing



*/

#[derive(Parser, Debug)]
struct AppArgs {
    #[arg(long, default_value = "marge.toml")]
    config: String,
    #[arg(long, default_value = "main")]
    branch: String,
}

#[derive(Deserialize, Debug)]
struct AppConfig {
    branch: String,
    cmd: String,
    #[serde(default)]
    token: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = init().await?;
    let remotes = get_remotes()?;

    let instance = Octocrab::builder().personal_token(config.token).build()?;

    println!("stuff {:#?}", remotes);

    Ok(())
}

async fn read_commits(instance: &Octocrab, url: String) -> anyhow::Result<Vec<Commit>> {
    instance.get(url, None::<&()>).await.context("")
}

async fn init() -> anyhow::Result<AppConfig> {
    let args = AppArgs::parse();
    let mut config = parse_config(&args.config).await?;
    let token = get_token(".token").await?;
    config.token = token;
    Ok(config)
}

async fn parse_config(file_path: &str) -> anyhow::Result<AppConfig> {
    let contents_bytes = tokio::fs::read(file_path)
        .await
        .context("coudl not read config")?;
    let contents = std::str::from_utf8(&contents_bytes).context("config is not valid utf-8")?;
    toml::from_str(&contents).context("could not parse config")
}

async fn get_token(file_path: &str) -> anyhow::Result<String> {
    let contents_bytes = tokio::fs::read(file_path)
        .await
        .context("could not read token")?;
    let contents = std::str::from_utf8(&contents_bytes).context("token is not valid utf8")?;
    Ok(contents.to_owned())
}

async fn get_pulls(
    instance: &Octocrab,
    owner: &str,
    repo: &str,
) -> anyhow::Result<Vec<PullRequest>> {
    instance
        .pulls(owner, repo)
        .list()
        .state(params::State::Open)
        .per_page(100)
        .page(1u8)
        .send()
        .await
        .context(format!("could not get pulls for repo {}/{}", owner, repo))
        .map(|p: Page<PullRequest>| p.items)
}
