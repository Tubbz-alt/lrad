use crate::error::Result;
use curl::easy::{Easy, Form, List};
use std::fs;
use std::io::Read;
use std::path::Path;
use std::path::PathBuf;
use std::str::from_utf8;

#[derive(Serialize)]
pub struct IpfsAddRecursive {
    pub path: PathBuf,
    pub recursive: Option<bool>,
    #[serde(rename = "wrap-with-directory")]
    pub wrap_with_directory: Option<bool>,
}

#[derive(Deserialize)]
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

impl IpfsAddRecursive {
    pub fn new(path: &Path) -> Self {
        Self {
            path: PathBuf::from(path),
            recursive: Some(true),
            wrap_with_directory: None,
        }
    }

    // TODO: Read up on doing this recursively
    pub fn run(&self) -> Result<Vec<IpfsAddResponse>> {
        let mut handle = Easy::new();
        handle.post(true)?;
        handle.url(
            format!(
                "http://localhost:5001/api/v0/add?recursive={}",
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
        println!("{}", from_utf8(dst.as_slice()).unwrap());
        let response: Vec<IpfsAddResponse> = serde_json::from_slice(dst.as_slice())?;
        Ok(response)
    }

    fn walk_dir_to_form(root: &Path, path: &Path, form: &mut Form) -> Result<()> {
        for entry in path.read_dir()? {
            let entry = entry?;
            let absolute_entry_path = entry.path();
            if absolute_entry_path.is_dir() {
                // let path_buf: PathBuf = absolute_entry_path.to_path_buf();
                // let relative_entry_path = path_buf.strip_prefix(root).unwrap();
                // println!("Adding directory {}", relative_entry_path.to_str().unwrap());
                // let mut part = form.part("path");
                // part.file(&absolute_entry_path);
                // part.filename(relative_entry_path.to_str().unwrap());
                // part.add()?;
                Self::walk_dir_to_form(root, &absolute_entry_path, form)?;
            } else {
                let path_buf: PathBuf = absolute_entry_path.to_path_buf();
                let relative_entry_path = path_buf.strip_prefix(root).unwrap();
                println!("Adding file {}", relative_entry_path.to_str().unwrap());
                let mut part = form.part("path");
                part.file(&absolute_entry_path);
                part.filename(relative_entry_path.to_str().unwrap());
                part.add()?;
            }
        }
        Ok(())
    }
}
