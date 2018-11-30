#![feature(range_contains)]
#![feature(self_struct_ctor)]
#![feature(try_trait)]
// extern crate openssl;
extern crate serde;
#[macro_use]
extern crate serde_derive;
extern crate chrono;
extern crate git2;
extern crate tempfile;
extern crate toml;
extern crate trust_dns_proto;
extern crate trust_dns_resolver;
#[macro_use]
extern crate lazy_static;
extern crate actix;
extern crate actix_web;
extern crate serde_json;
#[macro_use]
extern crate log;
extern crate futures;

use crate::dns::DnsRecordPutter;
use futures::future;
use futures::prelude::*;
use git2::{DiffOptions, Repository, RepositoryState};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::rc::Rc;
use tempfile::TempDir;

pub mod config;
pub mod dns;
mod docker;
pub mod error;
mod ipfs;
mod vcs;

use self::error::BoxFuture;
use self::error::Result;

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}

pub struct LradCli {
    repo: Repository,
    config: config::Config,
}

impl LradCli {
    pub fn try_load(path: &Path) -> Result<Self> {
        let repo = Repository::discover(path)?;
        let config = config::Config::try_from(&repo)?;
        Ok(LradCli { repo, config })
    }

    pub fn try_init(path: &Path) -> Result<Self> {
        debug!("Finding repo...");
        let repo = Repository::discover(path)?;
        debug!("Found repo at {:#?}", repo.path());
        let config = config::Config::default();
        config.write(&repo)?;
        if !repo.status_should_ignore(Path::new(".env"))? {
            warn!("The .env file may accidentally be committed! Please add it to your .gitignore if you plan on using it to store secrets.");
        }
        Ok(LradCli { repo, config })
    }

    pub fn try_push(self) -> BoxFuture<String> {
        if self.repo.state() != RepositoryState::Clean {
            return Box::new(future::err(vcs::VcsError::RepoNotClean.into()));
        } else if self.repo.is_bare() {
            return Box::new(future::err(vcs::VcsError::RepoShouldNotBeBare.into()));
        }
        let repo = Rc::new(self.repo);
        let ipfs_api_server = Rc::new(self.config.ipfs_api_server);
        let dns_provider = Rc::new(self.config.dns_provider);
        Box::new(
            future::result(repo.index().map_err(|err| err.into()))
                .and_then(move |index| {
                    if index.has_conflicts() {
                        return Err(vcs::VcsError::RepoHasConflicts.into());
                    } else if repo
                        .diff_index_to_workdir(
                            Some(&index),
                            Some(DiffOptions::default().ignore_submodules(true)),
                        )?
                        .stats()?
                        .files_changed()
                        != 0
                    {
                        return Err(vcs::VcsError::RepoHasUnstagedChanges.into());
                    }
                    debug!("Repo is clean, good to go!");
                    let repo_path = PathBuf::from(repo.path());
                    Ok(repo_path)
                })
                .and_then(|repo_path| {
                    info!("Converting to bare repo...");
                    let repo_path = repo_path.parent().unwrap();
                    let tmp_dir = TempDir::new()?;
                    let mut bare_repo_path = PathBuf::from(tmp_dir.path());
                    bare_repo_path.push(repo_path.file_name().unwrap());
                    let bare_repo = vcs::clone_bare(repo_path.to_str().unwrap(), &bare_repo_path)?;
                    debug!("Stripping remotes from bare repo.");
                    for remote in bare_repo.remotes()?.iter() {
                        if remote.is_some() {
                            bare_repo.remote_delete(&remote.unwrap())?;
                        }
                    }
                    debug!("Updating server info");
                    Command::new("git")
                        .arg("update-server-info")
                        .current_dir(&bare_repo_path)
                        .output()?;
                    Ok((tmp_dir, bare_repo_path))
                })
                .and_then(move |(tmp_dir, bare_repo_path)| {
                    info!("Adding files to IPFS...");
                    ipfs::IpfsAddRecursive::new(&ipfs_api_server, &bare_repo_path).run()
                })
                .and_then(move |ipfs_add_response| {
                    info!("Updating Cloudflare DNS Record...");
                    let root = ipfs_add_response
                        .iter()
                        .min_by(|a, b| a.name.len().cmp(&b.name.len()))
                        .unwrap();
                    dns_provider.try_put_txt_record(root.hash.clone()).wait()?;
                    Ok(root.hash.clone())
                }),
        )
    }

    // pub fn try_build(&self) -> Result<()> {}
}

pub struct LradDaemon {
    domain_name: String,
}
