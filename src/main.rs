use clap::Parser;
use serde::Deserialize;
use std::fs;
use toml;
use octocrab;
use octocrab::issues::I
use tokio;

#[derive(Debug)]
enum AppErr {
    ConfigNotFound,
    InvalidConfig,
}

#[derive(Parser, Debug)]
struct AppArgs {
    #[arg(long, default_value="marge.toml")]
    config: String
}


#[derive(Deserialize, Debug)]
struct AppConfig {
    target: String,
    milestone: String,
    cmd: String,
}


#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let args = AppArgs::parse();
    let config = parse_config(args.config);
    println!("Hello, {:?}!", config);
    Ok(())
}

fn parse_config(file_path: String) -> Result<AppConfig, AppErr> {
    let contents = fs::read_to_string(file_path).map_err(|_| AppErr::ConfigNotFound)?;
    toml::from_str(&contents).map_err(|_| AppErr::InvalidConfig)
}

fn get_milestone_issues(milestone: &str) -> Result<(), ()> {
    let octocrab = octocrab::Octocrab::builder()
        .build()?;

    match octocrab
        .search()
        .issues_and_pull_requests("tokei is:pr")
        .send()
        .await
    {
        Ok(page) => println!("{page:#?}"),
        Err(error) => println!("{error:#?}"),
    }

    Ok(())
}