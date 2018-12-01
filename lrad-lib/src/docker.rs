use actix_web::{client, HttpMessage};
use futures::future;
use futures::prelude::*;
use git2::Repository;
use tar::Builder;
use tokio_uds::UnixStream;

use crate::error::{BoxFuture, Error};
use crate::vcs::VcsError;

use std::sync::mpsc;
use std::time::Duration;

pub fn build_image(repo: &Repository) -> BoxFuture<bool> {
    if repo.is_bare() {
        return Box::new(future::err(VcsError::RepoShouldNotBeBare.into()));
    }
    let repo_path = repo.path().parent().unwrap().to_path_buf();
    debug!("Opening Unix socket");
    Box::new(
        UnixStream::connect("/var/run/docker.sock")
            .map_err(|err| Error::from(err))
            .and_then(move |stream| {
                debug!("Unix stream opened, preparing to send build request");
                let (tx, rx) = mpsc::channel();
                debug!("Building tarball");
                // TODO: convert this to actor and stream contents to request
                let mut ar = Builder::new(Vec::new());
                ar.append_dir_all(".", repo_path).unwrap();
                ar.finish().unwrap();
                debug!("Tarball ready");
                actix::run(move || {
                    client::post("/build")
                        .header("Host", "v1.39")
                        .with_connection(client::Connection::from_stream(stream))
                        .timeout(Duration::from_secs(30))
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
                        .then(move |res| {
                            actix::System::current().stop();
                            tx.send(res).unwrap();
                            Ok(())
                        })
                });
                rx.recv().unwrap()
            }),
    )
}
