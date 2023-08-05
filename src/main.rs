use anyhow::{Context};
use clap::Parser;
use octocrab::Octocrab;
use tokio;
mod git;
use crate::git::{get_remotes, get_pulls};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about)]
#[command(
    help_template = "{about-section} \n {usage-heading} \n\t {usage} \n\n {all-args} \n\n {name} v{version} ({author})"
)]
/// marge helps you merge your PRs
/// 
/// will get the PRs for the current git repositories' github page,
/// then ask for a desired order to merge them in. after that, each branch will in turn be
/// 
/// * checked out
///
/// * rebased onto its predecessor
/// 
/// * validated with the command passed to marge
/// 
/// * force-pushed to github
/// 
/// if any step fails, marge will pause and notify so you can fix your stuff 
/// before telling her to continue.
struct AppArgs {
    #[arg(long, short, default_value = "main")]
    /// the branch to rebase the PR chain onto
    branch: String,
    #[arg(long, short, default_value = ".token")]
    /// file to read the github API token from
    token: String,
    #[arg()]
    /// the sh command line marge should run to validate each rebased branch
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
    let args = AppArgs::parse();
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
