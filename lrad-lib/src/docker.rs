use actix_web::{client, HttpMessage};
use futures::prelude::*;
use git2::Repository;
use percent_encoding::{utf8_percent_encode, QUERY_ENCODE_SET};
use std::collections::HashMap;
use tar::Builder;
use tokio_uds::UnixStream;

use crate::error::Error;
use crate::vcs::VcsError;

use std::time::Duration;

pub fn build_image(
    repo: &Repository,
    image_name: String,
) -> impl Future<Item = bool, Error = Error> {
    let repo_path = repo.path().parent().unwrap().to_path_buf();
    let is_bare = repo.is_bare();
    debug!("Opening Unix socket");
    UnixStream::connect("/var/run/docker.sock")
        .map_err(|err| Error::from(err))
        .and_then(move |stream| {
            if is_bare {
                return Err(VcsError::RepoShouldNotBeBare.into());
            }
            debug!("Unix stream opened, preparing to send build request");
            debug!("Building tarball");
            // TODO: convert this to actor and stream contents to request
            let mut ar = Builder::new(Vec::new());
            ar.append_dir_all(".", &repo_path).unwrap();
            ar.finish().unwrap();
            debug!("Tarball ready");
            Ok((stream, ar))
        })
        .and_then(move |(stream, ar)| {
            client::post(format!(
                "/v1.39/build?t={}",
                utf8_percent_encode(&image_name, QUERY_ENCODE_SET)
            ))
            .header("Host", "lrad")
            .with_connection(client::Connection::from_stream(stream))
            .timeout(Duration::from_secs(3600))
            .body(ar.into_inner().unwrap())
            .map(|x| {
                debug!("Sending Docker build request...");
                x
            })
            .unwrap()
            .send()
            .map_err(|err| Error::from(err))
            .and_then(|res| {
                let is_success = res.status().is_success();
                res.body()
                    .and_then(|bytes| {
                        debug!("Parsing Docker build response... {:?}", bytes);
                        Ok(())
                    })
                    .then(move |_| Ok(is_success))
            })
        })
}

#[derive(Deserialize)]
pub struct CreateContainerResponse {
    #[serde(rename = "Id")]
    pub id: String,
    #[serde(rename = "Warnings")]
    pub warnings: Option<Vec<String>>,
}

#[derive(Serialize)]
struct CreateContainerRequest {
    #[serde(rename = "Image")]
    image: String,
    #[serde(rename = "HostConfig")]
    host_config: Option<HostConfig>,
}

#[derive(Serialize)]
pub struct HostConfig {
    #[serde(rename = "PublishAllPorts")]
    pub publish_all_ports: Option<bool>,
    #[serde(rename = "PortBindings")]
    pub port_bindings: HashMap<String, Vec<PortBinding>>,
}

#[derive(Serialize, Debug)]
pub struct PortBinding {
    #[serde(rename = "HostIp")]
    host_ip: Option<String>,
    #[serde(rename = "HostPort")]
    host_port: String,
}

impl From<&crate::config::PortBinding> for PortBinding {
    fn from(other: &crate::config::PortBinding) -> Self {
        Self {
            host_ip: other.host_ip.map(|x| x.to_string()),
            host_port: other.host_port.to_string(),
        }
    }
}

pub fn create_new_container(
    image: String,
    container_name: Option<String>,
    host_config: Option<HostConfig>,
) -> impl Future<Item = CreateContainerResponse, Error = Error> {
    UnixStream::connect("/var/run/docker.sock")
        .map_err(|err| Error::from(err))
        .and_then(move |stream| {
            client::post("/v1.39/containers/create")
                .header("Host", "lrad")
                .with_connection(client::Connection::from_stream(stream))
                .timeout(Duration::from_secs(30))
                .json(CreateContainerRequest {
                    image,
                    host_config,
                })
                .map(|x| {
                    debug!("Sending Docker create container...");
                    x
                })
                .unwrap()
                .send()
                .map_err(|err| Error::from(err))
                .and_then(|res| res.json().map_err(|err| Error::from(err)))
        })
}

pub fn force_remove_running_container(
    container_id: String,
) -> impl Future<Item = bool, Error = Error> {
    debug!("Opening Unix socket");
    debug!("Preparing to remove container {}", container_id);
    UnixStream::connect("/var/run/docker.sock")
        .map_err(|err| Error::from(err))
        .and_then(move |stream| {
            debug!("Unix stream opened, preparing to send build request");
            client::delete(format!("/v1.39/containers/{}?force=true", container_id))
                .header("Host", "lrad")
                .with_connection(client::Connection::from_stream(stream))
                .timeout(Duration::from_secs(30))
                .finish()
                .map(|x| {
                    debug!("Sending Docker remove containers request...");
                    x
                })
                .unwrap()
                .send()
                .map_err(|err| Error::from(err))
                .and_then(|res| {
                    let is_success = res.status().is_success();
                    res.body()
                        .and_then(|bytes| {
                            debug!("Parsing Docker remove container response... {:?}", bytes);
                            Ok(())
                        })
                        .then(move |_| Ok(is_success))
                })
        })
}

#[derive(Deserialize)]
pub struct ListContainersResponse {
    #[serde(rename = "Id")]
    pub id: String,
    #[serde(rename = "Image")]
    pub image: String,
    #[serde(rename = "State")]
    pub state: String,
}

pub fn list_containers() -> impl Future<Item = Vec<ListContainersResponse>, Error = Error> {
    debug!("Opening Unix socket");
    UnixStream::connect("/var/run/docker.sock")
        .map_err(|err| Error::from(err))
        .and_then(move |stream| {
            debug!("Unix stream opened, preparing to send list request");
            client::get("/v1.39/containers/json")
                .header("Host", "lrad")
                .with_connection(client::Connection::from_stream(stream))
                .timeout(Duration::from_secs(30))
                .finish()
                .map(|x| {
                    debug!("Sending Docker list containers request...");
                    x
                })
                .unwrap()
                .send()
                .map_err(|err| Error::from(err))
                .and_then(|res| res.json().map_err(|err| Error::from(err)))
        })
}

#[derive(Deserialize)]
pub struct ListImagesResponse {
    #[serde(rename = "Id")]
    pub id: String,
    #[serde(rename = "RepoTags")]
    pub repo_tags: Vec<String>,
    #[serde(rename = "Containers")]
    pub containers: i32,
}

pub fn list_images() -> impl Future<Item = Vec<ListImagesResponse>, Error = Error> {
    debug!("Opening Unix socket");
    UnixStream::connect("/var/run/docker.sock")
        .map_err(|err| Error::from(err))
        .and_then(move |stream| {
            debug!("Unix stream opened, preparing to send list request");
            client::get("/v1.39/images/json")
                .header("Host", "lrad")
                .with_connection(client::Connection::from_stream(stream))
                .timeout(Duration::from_secs(30))
                .finish()
                .map(|x| {
                    debug!("Sending Docker list containers request...");
                    x
                })
                .unwrap()
                .send()
                .map_err(|err| Error::from(err))
                .and_then(|res| res.json().map_err(|err| Error::from(err)))
        })
}

pub fn start_container(container_id: String) -> impl Future<Item = bool, Error = Error> {
    debug!("Opening Unix socket");
    UnixStream::connect("/var/run/docker.sock")
        .map_err(|err| Error::from(err))
        .and_then(move |stream| {
            debug!("Unix stream opened, preparing to send start request");
            client::post(format!("/v1.39/containers/{}/start", container_id))
                .header("Host", "lrad")
                .with_connection(client::Connection::from_stream(stream))
                .timeout(Duration::from_secs(30))
                .finish()
                .map(|x| {
                    debug!("Sending Docker start request...");
                    x
                })
                .unwrap()
                .send()
                .map_err(|err| Error::from(err))
                .and_then(|res| {
                    let is_success = res.status().is_success();
                    res.body()
                        .and_then(|bytes| {
                            debug!("Parsing Docker start container response... {:?}", bytes);
                            Ok(())
                        })
                        .then(move |_| Ok(is_success))
                })
        })
}
