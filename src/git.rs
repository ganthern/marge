use anyhow::{anyhow, Context};
use clap::Parser;
use crossterm::event::{KeyEvent, KeyCode};
use futures::FutureExt;
use log::info;
use octocrab::{
    models::{pulls::PullRequest},
    params, Octocrab, Page,
};
use regex::Regex;
use std::{collections::HashSet, hash::Hasher, hash::Hash};
use tokio::sync::mpsc::Receiver;
use tui_logger::TuiWidgetState;

use crate::{events::AppEvent, merge_candidate::{Successor, MergeCandidate}, AppArgs, AppConfig};
use tokio::process::Command;

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

async fn get_pulls(remote: &Remote, instance: &Octocrab) -> anyhow::Result<Vec<PullRequest>> {
    let owner = &remote.owner;
    let repo = &remote.repo;
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

async fn checkout_branch(branchname: &str) -> Receiver<anyhow::Result<()>> {
    let (tx, rx) = tokio::sync::mpsc::channel(1);
    log::info!("running git checkout");
    let b = branchname.to_owned();
    tokio::spawn(async move {
        let result = Command::new("git")
            .args(["checkout", &b])
            .output()
            .await;
        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
        let _ = match result {
            Ok(output) => {
                info!("{}", std::str::from_utf8(&output.stdout).unwrap_or("<invalid utf8 output>"));
                tx.send(Ok(()))
            }
            Err(e) => tx.send(Err(e).context("could not check repo")),
        }
        .await;
    });

    rx
}

async fn pull_remote() -> Receiver<anyhow::Result<()>> {
    let (tx, rx) = tokio::sync::mpsc::channel(1);
    log::info!("running git pull");
    tokio::spawn(async move {
        let result = Command::new("git")
            .args(["pull"])
            .output()
            .await;
        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
        let _ = match result {
            Ok(output) => {
                info!("{}", std::str::from_utf8(&output.stdout).unwrap_or("<invalid utf8 output>"));
                tx.send(Ok(()))
            }
            Err(e) => tx.send(Err(e).context("could not check repo")),
        }
        .await;
    });

    rx
}

fn is_repo_clean() -> Receiver<anyhow::Result<bool>> {
    let (tx, rx) = tokio::sync::mpsc::channel(1);
    log::info!("running git status");

    tokio::spawn(async move {
        let result = Command::new("git")
            .args(["status", "--porcelain"])
            .output()
            .await;
        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
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

#[derive(Debug)]
pub enum AppState {
    CheckingRepo(Receiver<anyhow::Result<bool>>),
    WaitingForCleanRepo,
    CheckingOutTargetBranch(Receiver<anyhow::Result<()>>),
    PullingRemote(Receiver<anyhow::Result<()>>),
    GettingPulls,
    WaitingForSort(Vec<MergeCandidate>),
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
                AppState::CheckingRepo(rx) => transition_checking(rx, &self.branch).await,
                AppState::WaitingForCleanRepo => transition_waiting_clean(&self.last_event),
                AppState::CheckingOutTargetBranch(rx) => transition_checking_out_target(rx).await,
                AppState::PullingRemote(rx) => transition_pull_remote(rx).await,
                AppState::GettingPulls => transition_getting_pulls(&self.remote, &self.instance).await,
                AppState::WaitingForSort(c) => transition_waiting_sort(c),
                AppState::Failed => todo!(),
            },
        );

        Ok(())
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
async fn transition_checking(mut rx: Receiver<anyhow::Result<bool>>, branchname: &str) -> AppState {
    {
        let ready = futures::future::ready(()).fuse();
        let nxt = rx.recv().fuse();

        futures::pin_mut!(ready, nxt);

        futures::select! {
            maybe_clean = nxt => {
                if let Some(Ok(is_clean)) = maybe_clean {
                    return if is_clean {AppState::CheckingOutTargetBranch(checkout_branch(branchname).await)} else {AppState::WaitingForCleanRepo}
                } else {
                    return AppState::Failed
                }
            },
            _ = ready => (),
        };
    }

    AppState::CheckingRepo(rx)
}


/** transition out of the waiting for clean repo state */
fn transition_waiting_clean(last_event: &AppEvent) -> AppState {
    match last_event {
        AppEvent::Input(KeyEvent {
            code: KeyCode::Char(' '),
            ..
        }) => AppState::CheckingRepo(is_repo_clean()),
        AppEvent::Error(_) => AppState::Failed,
        _ => AppState::WaitingForCleanRepo,
    }
}

async fn transition_checking_out_target(mut rx: Receiver<anyhow::Result<()>>) -> AppState {
    {
        let ready = futures::future::ready(()).fuse();
        let nxt = rx.recv().fuse();

        futures::pin_mut!(ready, nxt);

        futures::select! {
            maybe_clean = nxt => {
                if let Some(Ok(_)) = maybe_clean {
                    return AppState::PullingRemote(pull_remote().await);
                } else {
                    return AppState::Failed;
                }
            },
            _ = ready => (),
        };
    }

    // still waiting for the checkout...
    AppState::CheckingOutTargetBranch(rx)
}

async fn transition_pull_remote(mut rx: Receiver<anyhow::Result<()>>) -> AppState {
    {
        let ready = futures::future::ready(()).fuse();
        let nxt = rx.recv().fuse();

        futures::pin_mut!(ready, nxt);

        futures::select! {
            maybe_clean = nxt => {
                if let Some(Ok(_)) = maybe_clean {
                    return AppState::GettingPulls;
                } else {
                    return AppState::Failed;
                }
            },
            _ = ready => (),
        };
    }

    // still waiting for the checkout...
    AppState::PullingRemote(rx)
}



async fn transition_getting_pulls(remote: &Remote, instance: &Octocrab) -> AppState {
    if let Ok(pulls) = get_pulls(remote, instance).await {

    let candidates = pulls
        .into_iter()
        .map(|p| MergeCandidate::new(p))
        .collect();

        AppState::WaitingForSort(candidates)
    } else {
        AppState::GettingPulls
    }
}

fn transition_waiting_sort(candidates: Vec<MergeCandidate>) -> AppState {
    AppState::WaitingForSort(candidates)
}