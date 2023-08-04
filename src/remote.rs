use std::hash::{Hash, Hasher};
use regex::Regex;
use std::process::Command;
use anyhow::Context;
use std::collections::HashSet;

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

impl Eq for Remote {
}

impl PartialEq<Remote> for Remote {
    fn eq(&self, other: &Remote) -> bool { 
        self.name == other.name
     }
}

impl Hash for Remote {
    fn hash<H>(&self, hasher: &mut H) where H: Hasher { 
        self.name.hash(hasher)
     }
}


/** get the remotes of the git repository in the current wd */
pub fn get_remotes() -> anyhow::Result<Vec<Remote>> {
    let remote_re = Regex::new(r"(?xm)           # verbose / multiline
        ^([[:alpha:]]*)                          # remote name at line start
        \s*                                      # eat whitespace
        (?:git@github\.com:|https://github.com/) # eat start of URL
        ([[:alpha:]-_\d]*)                       # remote owner
        /                                        # eat /
        ([[:alpha:]-_\d]*)                       # remote repo
        \.git                                    # eat .git
        \s*                                      # eat whitespace
        \((?:fetch|push)\)$                      # eat (fetch) or (push) at line end
    ").unwrap();
    let output = Command::new("git")
    .args(["remote", "-v"])
    .output()
    .context("could not run git remote")?;

    // check if we got 128 -> no git remote
    let out = String::from_utf8(output.stdout).context("output not valid utf-8")?;
    let mut set: HashSet<Remote> = HashSet::new();
    let remotes = remote_re.captures_iter(&out).map(|caps| {
        let (_, [name, owner, repo]) = caps.extract();
        Remote {name: name.to_owned(), owner: owner.to_owned(), repo: repo.to_owned()}
    });
    set.extend(remotes);
    Ok(set.into_iter().collect())
}
