#![feature(range_contains)]
#![feature(try_trait)]
#![feature(box_patterns)]
#[macro_use]
extern crate serde_derive;
#[macro_use]
extern crate log;

use crate::dns::DnsRecordPutter;
use futures::prelude::*;
use futures::{future, stream};
use git2::{build::RepoBuilder, DiffOptions, Repository, RepositoryState};
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

pub use self::dns::DnsTxtRecordResponse;
use self::error::{BoxFuture, Error, ErrorKind, Result};

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}

pub struct LradCli {
    repo: Repository,
    config: config::CliConfig,
}

impl LradCli {
    pub fn try_load(path: &Path) -> Result<Self> {
        let repo = Repository::discover(path)?;
        let config = config::CliConfig::try_from(&repo)?;
        Ok(LradCli { repo, config })
    }

    pub fn try_init(path: &Path) -> Result<Self> {
        debug!("Finding repo...");
        let repo = Repository::discover(path)?;
        debug!("Found repo at {:#?}", repo.path());
        let config = config::CliConfig::default();
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
                .and_then(move |(_tmp_dir, bare_repo_path)| {
                    info!("Adding files to IPFS...");
                    ipfs::IpfsAddRecursive::new(&ipfs_api_server, &bare_repo_path).run()
                })
                .and_then(move |ipfs_add_response| {
                    info!("Updating Cloudflare DNS Record...");
                    let root = ipfs_add_response.iter().last().unwrap();
                    dns_provider.try_put_txt_record(root.hash.clone()).wait()?;

                    Ok(root.hash.clone())
                }),
        )
    }
}

pub struct LradDaemon {
    config: config::DaemonConfig,
}

impl LradDaemon {
    pub fn try_load(path: &Path) -> Result<Self> {
        let config = config::DaemonConfig::try_from(path)?;
        Ok(LradDaemon { config })
    }

    pub fn try_lookup_txt_record(
        &self,
    ) -> impl Future<Item = Option<DnsTxtRecordResponse>, Error = Error> {
        DnsTxtRecordResponse::lookup_txt_record(&self.config.dns_record_name)
        // .or_else(|err| {
        //     match &err {
        //         box ErrorKind::TrustDnsResolveError(resolve_err) => match resolve_err.kind() {
        //             trust_dns_resolver::error::ResolveErrorKind::NoRecordsFound {
        //                 query: _,
        //                 valid_until: _,
        //             } => DnsTxtRecordResponse::lookup_txt_record(&format!(
        //                 "_dnslink.{}",
        //                 &self.config.dns_record_name
        //             )),
        //             _ => future::err(err),
        //         },
        //         _ => future::err(err),
        //     }
        // })
    }

    pub fn try_deploy(&self) -> BoxFuture<bool> {
        let dns_record_name = self.config.dns_record_name.get("_dnslink.".len()..);

        if dns_record_name.is_none() {
            return Box::new(future::ok(false));
        }
        let dns_record_name = String::from(dns_record_name.unwrap());
        Box::new(
            future::result(TempDir::new())
                .map_err(|err| -> Error { err.into() })
                .and_then(move |tmp_dir| {
                    debug!("Cloning git repo with dns record {}", dns_record_name);

                    Command::new("git")
                        .arg("clone")
                        .arg(format!("http://localhost:8080/ipns/{}", dns_record_name))
                        .arg("--single-branch")
                        .current_dir(tmp_dir.path())
                        .output()?;
                    let mut repo_path = tmp_dir.path().to_path_buf();
                    repo_path.push(dns_record_name.to_string());
                    let repo = Repository::discover(repo_path)?;
                    Ok((tmp_dir, repo, format!("{}:latest", dns_record_name)))
                })
                .and_then(|(tmp_dir, repo, image_name)| {
                    docker::build_image(&repo, image_name.clone()).map(|x| (x, image_name, tmp_dir))
                })
                .and_then(|(ok, image_name, _tmp_dir)| {
                    debug!("Creating docker container");
                    docker::create_new_container(image_name.clone(), None).map(|x| (x, image_name))
                })
                .and_then(|(create_container_response, image_name)| {
                    debug!("Listing docker images");
                    docker::list_images()
                        .map(|images| (create_container_response, image_name, images))
                })
                .and_then(|(create_container_response, image_name, images)| {
                    debug!("Listing existing docker images");
                    docker::list_containers().map(|containers| {
                        (create_container_response, image_name, images, containers)
                    })
                })
                .and_then(
                    move |(create_container_response, image_name, images, containers)| {
                        debug!("Removing old docker container(s)");
                        let removable_image_ids: Vec<String> = images
                            .iter()
                            .filter(|image| image.repo_tags.contains(&image_name))
                            .map(|image| image.id.clone())
                            .collect();

                        // TODO: This currently deletes all docker containers, need to selectively delete the ones of interest.
                        // let containers_to_remove: Vec<docker::ListContainersResponse> = containers.iter().filter(|container| {
                        //     container.id != create_container_response.id // && removable_image_ids.contains(&container.image)
                        // }).collect();
                        stream::iter_ok(containers)
                            .and_then(|container| {
                                docker::force_remove_running_container(container.id.clone())
                            })
                            .collect()
                            .map(|x| (x, create_container_response))
                    },
                )
                .and_then(|(_removed, create_container_response)| {
                    debug!("Starting new docker container");
                    docker::start_container(create_container_response.id)
                }),
        )
    }
}
