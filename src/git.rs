use anyhow::{anyhow, Context};
use clap::Parser;
use futures::{future::Fuse, FutureExt};
use octocrab::{
    models::{commits::Commit, issues::Issue, pulls::PullRequest, timelines::Milestone},
    params, Octocrab, Page,
};
use regex::Regex;
use std::collections::HashSet;
use std::future::Future;
use std::hash::{Hash, Hasher};
use std::pin::Pin;
use tokio::sync::mpsc::Receiver;
use tui_logger::TuiWidgetState;

use crate::merge_candidate::{MergeCandidate, MergeCandidateState};
use crate::{events::AppEvent, merge_candidate::Successor, AppArgs, AppConfig, Screen};
use tokio::process::Command;

enum GitReturn {
    Ok = 0,
    NotGitRepo = 128,
}

#[derive(Debug)]
pub struct Remote {
    pub name: String,
    pub owner: String,
    pub repo: String,
}

impl Eq for Remote {}

impl PartialEq<Remote> for Remote {
    fn eq(&self, other: &Remote) -> bool {
        self.name == other.name
    }
}

impl Hash for Remote {
    fn hash<H>(&self, hasher: &mut H)
    where
        H: Hasher,
    {
        self.name.hash(hasher)
    }
}

/** get the remotes of the git repository in the current wd */
async fn get_remotes() -> anyhow::Result<Vec<Remote>> {
    let remote_re = Regex::new(
        r"(?xm)           # verbose syntax / multiline
        ^([[:alpha:]]*)                          # remote name at line start
        \s*                                      # eat whitespace
        (?:git@github\.com:|https://github.com/) # eat start of URL
        ([[:alpha:]-_\d]*)                       # remote owner
        /                                        # eat /
        ([[:alpha:]-_\d]*)                       # remote repo
        \.git                                    # eat .git
        \s*                                      # eat whitespace
        \((?:fetch|push)\)$                      # eat (fetch) or (push) at line end
    ",
    )
    .unwrap();
    let output = Command::new("git")
        .args(["remote", "-v"])
        .output()
        .await
        .context("could not run git remote")?;

    // check if we got 128 -> no git remote
    let out = String::from_utf8(output.stdout).context("output not valid utf-8")?;
    let mut set: HashSet<Remote> = HashSet::new();
    let remotes = remote_re.captures_iter(&out).map(|caps| {
        let (_, [name, owner, repo]) = caps.extract();
        Remote {
            name: name.to_owned(),
            owner: owner.to_owned(),
            repo: repo.to_owned(),
        }
    });
    set.extend(remotes);

    return if set.len() > 0 {
        Ok(set.into_iter().collect())
    } else {
        Err(anyhow!("not enough remotes!"))
    };
}

fn is_repo_clean() -> Receiver<anyhow::Result<bool>> {
    let (tx, rx) = tokio::sync::mpsc::channel(1);

    tokio::spawn(async move {
        let result = Command::new("git")
            .args(["status", "--porcelain"])
            .output()
            .await;

        let _ = match result {
            Ok(output) => {
                if output.stdout.is_empty() {
                    tx.send(Ok(true))
                } else {
                    tx.send(Ok(false))
                }
            }
            Err(e) => tx.send(Err(e).context("could not check repo")),
        }
        .await;
    });

    rx
}

#[derive(PartialEq)]
pub enum ActivePane {
    List,
    Log,
}

pub enum AppState {
    CheckingRepo(Receiver<anyhow::Result<bool>>),
    WaitingForCleanRepo,
    GettingPulls,
    WaitingForSort,
    Failed,
}

/// the main app struct
pub struct Marge {
    pub app_state: Box<AppState>,
    pub instance: Octocrab,
    pub remote: Remote,
    pub cmd: String,
    pub branch: String,
    pub merge_head: Successor,
    pub active_pane: ActivePane,
    pub last_event: AppEvent,
    pub log_state: TuiWidgetState,
}

impl Marge {
    pub async fn try_transition(&mut self) -> anyhow::Result<()> {
        let old_state = std::mem::replace(self.app_state.as_mut(), AppState::Failed);

        let _ = std::mem::replace(
            self.app_state.as_mut(),
            match old_state {
                AppState::CheckingRepo(rx) => transition_checking(rx).await,
                AppState::WaitingForCleanRepo => AppState::WaitingForCleanRepo,
                AppState::GettingPulls => AppState::GettingPulls,
                AppState::WaitingForSort => todo!(),
                AppState::Failed => todo!(),
            },
        );

        Ok(())
    }

    pub async fn get_pulls(self: &Self) -> anyhow::Result<Vec<PullRequest>> {
        let owner = &self.remote.owner;
        let repo = &self.remote.repo;
        self.instance
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

    pub async fn try_init() -> anyhow::Result<Marge> {
        let (config, remotes) = futures::future::try_join(get_config(), get_remotes()).await?;
        let instance = Octocrab::builder().personal_token(config.token).build()?;
        let remote = find_remote(remotes, &config.args.remote)?;

        let log_state = TuiWidgetState::new()
            .set_default_display_level(log::LevelFilter::Info)
            .set_level_for_target("debug", log::LevelFilter::Debug)
            .set_level_for_target("error", log::LevelFilter::Error)
            .set_level_for_target("warn", log::LevelFilter::Warn)
            .set_level_for_target("info", log::LevelFilter::Info);

        Ok(Marge {
            app_state: Box::new(AppState::CheckingRepo(is_repo_clean())),
            remote,
            instance,
            cmd: config.args.cmd,
            branch: config.args.branch,
            merge_head: None,
            active_pane: ActivePane::List,
            last_event: AppEvent::Tick,
            log_state,
        })
    }
}

fn find_remote(mut remotes: Vec<Remote>, target: &str) -> anyhow::Result<Remote> {
    let default_remote = remotes.pop().expect("should have a remote");
    remotes
        .into_iter()
        .find(|r| r.name == target)
        .or_else(|| {
            if default_remote.name == target {
                Some(default_remote)
            } else {
                None
            }
        })
        .context(format!("could not find remote {}", target))
}

async fn get_config() -> anyhow::Result<AppConfig> {
    let args = AppArgs::try_parse()?;
    let token = get_token(&args.token).await?;
    Ok(AppConfig { args, token })
}

async fn get_token(file_path: &str) -> anyhow::Result<String> {
    let contents_bytes = tokio::fs::read(file_path)
        .await
        .context("could not read token")?;
    let contents = std::str::from_utf8(&contents_bytes).context("token is not valid utf8")?;
    Ok(contents.to_owned())
}

/** transition from the repo checking state */
async fn transition_checking(mut rx: Receiver<anyhow::Result<bool>>) -> AppState {
    {
        let ready = futures::future::ready(()).fuse();
        let nxt = rx.recv().fuse();

        futures::pin_mut!(ready, nxt);

        futures::select! {
            maybe_clean = nxt => {
                if let Some(Ok(is_clean)) = maybe_clean {
                    return if is_clean {AppState::GettingPulls} else {AppState::WaitingForCleanRepo}
                } else {
                    return AppState::Failed
                }
            },
            _ = ready => (),
        };
    }

    AppState::CheckingRepo(rx)
}
