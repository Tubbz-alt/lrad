use git2::{build::RepoBuilder, Repository};
use std::path::Path;

pub fn clone_bare(url: &str, into: &Path) -> Result<Repository, git2::Error> {
    RepoBuilder::new().bare(true).clone(url, into)
}

#[derive(Debug)]
pub enum VcsError {
    RepoShouldNotBeBare,
    RepoNotClean,
}
