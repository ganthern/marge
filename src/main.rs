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

#[derive(Parser, Debug)]
struct AppArgs {
    #[arg(long, default_value = "marge.toml")]
    config: String,
}

#[derive(Deserialize, Debug)]
struct AppConfig {
    branch: String,
    owner: String,
    repo: String,
    milestone: String,
    cmd: String,
    #[serde(default)]
    token: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = init().await?;

    let instance = Octocrab::builder().personal_token(config.token).build()?;
    let commit_urls = get_pulls(&instance, &config.owner, &config.repo)
        .await?
        .into_iter()
        .filter_map(|pull| pull.commits_url)
        .map(|url| url.as_str().to_owned());

    let commits_futures = commit_urls.map(|url| read_commits(&instance, url));
    let commits_nested = futures::future::join_all(commits_futures).await;
    let commits: Vec<Commit> = commits_nested.into_iter()
    .filter(|c| c.is_ok())
    .map(|c| c.unwrap())
    .flatten()
    .collect::<Vec<Commit>>();

    let commit_messages = commits.into_iter().map(|c| c.commit.message).collect::<Vec<String>>();


    println!("stuff {:#?}", commit_messages);

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

async fn get_milestones(
    instance: &Octocrab,
    owner: &str,
    repo: &str,
) -> anyhow::Result<Vec<Milestone>> {
    let path = format!("/repos/{}/{}/milestones", owner, repo);
    instance
        .get(path, None::<&()>)
        .await
        .context(format!("Failed to read milestones from {}", repo))
}

async fn get_milestone(instance: &Octocrab, config: &AppConfig) -> anyhow::Result<Milestone> {
    let milestones: Vec<Milestone> = get_milestones(&instance, &config.owner, &config.repo).await?;
    milestones
        .into_iter()
        .find(|m| m.title == config.milestone)
        .context(format!(
            "/repos/{}/{} has no milestone {}",
            config.owner, config.repo, config.milestone
        ))
}

async fn get_issues_for_milestone(
    instance: &Octocrab,
    owner: &str,
    repo: &str,
    milestone: &Milestone,
) -> anyhow::Result<Vec<Issue>> {
    instance
        .issues(owner, repo)
        .list()
        .milestone(milestone.number)
        .state(params::State::All)
        .per_page(100)
        .page(1u8)
        .send()
        .await
        .context(format!(
            "could not get issues for milestone {} in {}/{}",
            milestone.title, owner, repo
        ))
        .map(|p: Page<Issue>| p.items)
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

fn collect_pull_request_links(issues: Vec<Issue>) -> Vec<Url> {
    println!("{}", issues.len());
    issues
        .into_iter()
        .filter_map(|i| i.pull_request.map(|prl| prl.url))
        .collect()
}
