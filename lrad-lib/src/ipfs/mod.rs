use crate::error::Result;
use curl::easy::{Easy, Form};
use std::path::Path;
use std::path::PathBuf;

#[derive(Deserialize, Serialize)]
pub struct IpfsApiServerConfig {
    host: String,
    port: u16,
}

impl Default for IpfsApiServerConfig {
    fn default() -> Self {
        Self {
            host: String::from("localhost"),
            port: 5001,
        }
    }
}

#[derive(Serialize)]
pub struct IpfsAddRecursive<'a> {
    pub path: PathBuf,
    pub recursive: Option<bool>,
    #[serde(rename = "wrap-with-directory")]
    pub wrap_with_directory: Option<bool>,
    config: &'a IpfsApiServerConfig,
}

#[derive(Deserialize, Debug)]
pub struct IpfsAddResponse {
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "Hash")]
    pub hash: String,
    #[serde(rename = "Bytes")]
    pub bytes: Option<usize>,
    #[serde(rename = "Size")]
    pub size: String,
}

impl<'a> IpfsAddRecursive<'a> {
    pub fn new(config: &'a IpfsApiServerConfig, path: &Path) -> Self {
        Self {
            path: PathBuf::from(path),
            recursive: Some(true),
            wrap_with_directory: None,
            config,
        }
    }

    pub fn run(&self) -> Result<Vec<IpfsAddResponse>> {
        let mut handle = Easy::new();
        handle.post(true)?;
        handle.url(
            format!(
                "http://{}:{}/api/v0/add?recursive={}",
                self.config.host,
                self.config.port,
                self.recursive.unwrap_or(false)
            )
            .as_str(),
        )?;
        let mut form = Form::new();
        Self::walk_dir_to_form(&self.path.parent().unwrap(), &self.path, &mut form)?;
        handle.httppost(form)?;
        let mut dst = Vec::new();
        {
            let mut transfer = handle.transfer();
            transfer.write_function(|data| {
                dst.extend_from_slice(data);
                Ok(data.len())
            })?;
            transfer.perform()?;
        }
        // TODO: IPFS outputs invalid JSON, confirmed! A list of {}s is invalid
        // For now, unsafely converting this to valid JSON because the outptu of IPFS
        // is trusted.
        let dst_str = String::from(std::str::from_utf8(dst.as_slice())?);
        let mut responses: Vec<IpfsAddResponse> = Vec::new();
        for response in dst_str.split_terminator('\n') {
            responses.push(serde_json::from_str(&response)?);
        }
        responses
            .iter()
            .for_each(|res| debug!("Received response {:?}", res));
        Ok(responses)
    }

    fn walk_dir_to_form(root: &Path, path: &Path, form: &mut Form) -> Result<()> {
        for entry in path.read_dir()? {
            let entry = entry?;
            let absolute_entry_path = entry.path();
            if absolute_entry_path.is_dir() {
                debug!("Looking at directory {}", absolute_entry_path.to_str().unwrap());
                Self::walk_dir_to_form(root, &absolute_entry_path, form)?;
            } else {
                let path_buf: PathBuf = absolute_entry_path.to_path_buf();
                let relative_entry_path = path_buf.strip_prefix(root).unwrap();
                debug!(
                    "Adding file {} to send to IPFS",
                    relative_entry_path.to_str().unwrap()
                );
                let mut part = form.part("path");
                part.file(&absolute_entry_path);
                part.filename(relative_entry_path.to_str().unwrap());
                part.add()?;
            }
        }
        Ok(())
    }
}
