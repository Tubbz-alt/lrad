use crate::dns::CloudflareConfig;
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use git2::Repository;

use crate::error::Result;

#[derive(Deserialize, Serialize)]
pub struct Config {
    dns_provider: CloudflareConfig,
}

impl Config {
    fn config_path(repo: &Repository) -> Result<PathBuf> {
        let path = if !repo.is_bare() {
            repo.path()
                .parent()
                .expect(".git should always have a parent folder in a non-bare repo")
        } else {
            repo.path()
        };
        let mut path = PathBuf::from(path);
        path.push(Path::new(".lrad.toml"));
        Ok(path)
    }
    pub fn try_from(repo: &Repository) -> Result<Self> {
        let mut file = File::open(Self::config_path(repo)?)?;
        let metadata = file.metadata()?;
        let mut buf = Vec::with_capacity(metadata.len() as usize);
        let _bytes_read = file.read_to_end(&mut buf)?;
        toml::from_slice(buf.as_slice()).map_err(|err| err.into())
    }

    pub fn write(&self, repo: &Repository) -> Result<()> {
        let config_json_str = toml::to_string(self).unwrap();
        let mut file = File::create(Self::config_path(repo)?)?;
        file.write(config_json_str.as_bytes())?;
        Ok(())
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            dns_provider: CloudflareConfig::default(),
        }
    }
}
