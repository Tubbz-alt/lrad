#![feature(range_contains)]
#![feature(self_struct_ctor)]
// extern crate openssl;
extern crate serde;
#[macro_use]
extern crate serde_derive;
extern crate chrono;
extern crate git2;
extern crate tempfile;
extern crate toml;
// extern crate trust_dns_proto;
// extern crate trust_dns_resolver;
#[macro_use]
extern crate lazy_static;
extern crate curl;
extern crate serde_json;
#[macro_use]
extern crate log;

use crate::dns::DnsRecordPutter;
use git2::{Repository, RepositoryState};
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;

pub mod config;
pub mod dns;
pub mod error;
mod ipfs;
mod vcs;

use self::error::Result;

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}

pub struct Lrad {
    repo: Repository,
    config: config::Config,
}

impl Lrad {
    pub fn try_load(path: &Path) -> Result<Self> {
        let repo = Repository::discover(path)?;
        let config = config::Config::try_from(&repo)?;
        Ok(Lrad { repo, config })
    }

    pub fn try_init(path: &Path) -> Result<Self> {
        debug!("Finding repo...");
        let repo = Repository::discover(path)?;
        debug!("Found repo at {:#?}", repo.path());
        let config = config::Config::default();
        config.write(&repo)?;
        Ok(Lrad { repo, config })
    }

    pub fn try_deploy(&self) -> Result<()> {
        if self.repo.state() != RepositoryState::Clean {
            return Err(vcs::VcsError::RepoNotClean.into());
        } else if self.repo.is_bare() {
            return Err(vcs::VcsError::RepoShouldNotBeBare.into());
        }
        info!("Converting to bare repo...");
        let tmp_dir = TempDir::new()?;
        let repo_path = self.repo.path().parent().unwrap();
        let mut bare_repo_path = PathBuf::from(tmp_dir.path());
        bare_repo_path.push(repo_path.file_name().unwrap());
        let bare_repo = vcs::clone_bare(repo_path.to_str().unwrap(), &bare_repo_path)?;
        Command::new("git").arg("update-server-info").current_dir(&bare_repo_path).output()?;
        info!("Adding to IPFS...");
        let ipfs_add_all = ipfs::IpfsAddRecursive::new(&self.config.ipfs_api_server, &bare_repo_path);
        let ipfs_add_response = ipfs_add_all.run()?;
        let root = ipfs_add_response
            .iter()
            .min_by(|a, b| a.name.len().cmp(&b.name.len()))
            .unwrap();
        info!("Added to IPFS with hash {}", root.hash);
        info!("Updating Cloudflare DNS Record...");
        self.config.dns_provider.try_put_txt_record(root.hash.clone())?;
        Ok(())
    }
}
