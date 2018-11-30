use actix_web::client;
use futures::prelude::*;
use url::Url;
use percent_encoding::{percent_encode, QUERY_ENCODE_SET};

use crate::error::{BoxFuture, Error};

pub fn build_image(git_remote: String) -> BoxFuture<bool> {
    let mut url = Url::parse(
        "unix:///var/run/docker.sock/v1.39/images/build"
    ).unwrap();
    let encoded_git_remote = percent_encode(format!("remote={}", git_remote).as_bytes(), QUERY_ENCODE_SET).to_string();
    url.set_query(Some(encoded_git_remote));
    debug!("URL is {:?}", url);
    // let (tx, rx) = mpsc::channel();
    Box::new(
        client::post(url)
            .finish()
            .map(|x| {
                debug!("Sending Docker build request...");
                x
            })
            .unwrap()
            .send()
            .map_err(|err| Error::from(err))
            .and_then(|res| {
                debug!("Parsing Docker build response...");
                Ok(res.status().is_success())
            })
            // .then(move |res| {
            //     tx.send(res).unwrap();
            //     actix::System::current().stop();
            //     Ok(())
            // }),
    )
    // Box::new(future::result(rx.recv().unwrap()))
}
