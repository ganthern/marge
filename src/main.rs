use anyhow::Context;
use clap::Parser;
use octocrab::{
    models::{commits::Commit, issues::Issue, pulls::PullRequest, timelines::Milestone},
    params, Octocrab, Page,
};
use serde::Deserialize;
use tokio;
use toml;
mod git;
use crate::git::{get_remotes, get_pulls};

#[derive(Parser, Debug)]
struct AppArgs {
    #[arg(long, default_value = "main", help = "the branch to merge the PRs to")]
    branch: String,
    #[arg(long, default_value = ".token", help = "git API token to use")]
    token: String,
    #[arg( help = "the command to run to validate each rebased branch")]
    cmd: String,

}

#[derive(Debug)]
struct AppConfig {
    args: AppArgs,
    token: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = init().await?;
    let remotes = get_remotes()?;

    let instance = Octocrab::builder().personal_token(config.token).build()?;
    let pulls = get_pulls(&instance, &remotes[0].owner, &remotes[0].repo).await?;

    println!("stuff {:#?}", pulls);
    println!("varg: {}", config.args.cmd);
    Ok(())
}

async fn init() -> anyhow::Result<AppConfig> {
    let args = AppArgs::try_parse().context("could not parse args")?;
    let token = get_token(&args.token).await?;
    Ok(AppConfig {args, token})
}

async fn get_token(file_path: &str) -> anyhow::Result<String> {
    let contents_bytes = tokio::fs::read(file_path)
        .await
        .context("could not read token")?;
    let contents = std::str::from_utf8(&contents_bytes).context("token is not valid utf8")?;
    Ok(contents.to_owned())
}
