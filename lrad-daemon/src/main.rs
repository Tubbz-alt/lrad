#[macro_use]
extern crate clap;
#[macro_use]
extern crate log;

use ::actix::prelude::*;
use ::actix::System;
use futures::prelude::*;
use lrad::{
    dns::DnsTxtRecordResponse,
    error::{Error, Result},
    LradDaemon,
};

use std::env;
use std::path::Path;
use std::time::{Duration, Instant};

const CONFIG_FILE_PATH: &'static str = "/etc/lrad/lrad.toml";

fn main() -> Result<()> {
    if env::var("RUST_LOG").is_err() {
        env::set_var("RUST_LOG", "lrad=info,lrad-daemon=info");
    }
    env_logger::init();
    debug!("Loading configuration from {}", CONFIG_FILE_PATH);
    let daemon = LradDaemon::try_load(Path::new(CONFIG_FILE_PATH))?;
    info!("Daemon ready!");

    let sys = System::new("lrad-daemon");
    DaemonActor {
        daemon,
        record: None,
    }
    .start();

    sys.run();
    Ok(())
}

struct DnsLookup;
struct Deploy;

impl Message for Deploy {
    type Result = Result<()>;
}

impl Message for DnsLookup {
    type Result = Result<()>;
}

struct DaemonActor {
    daemon: LradDaemon,
    record: Option<DnsTxtRecordResponse>,
}

impl Actor for DaemonActor {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Context<Self>) {
        ctx.notify(DnsLookup {})
    }
}

impl Handler<DnsLookup> for DaemonActor {
    type Result = ResponseActFuture<Self, (), Error>;

    fn handle(&mut self, msg: DnsLookup, ctx: &mut Context<Self>) -> Self::Result {
        Box::new(
            actix::fut::wrap_future::<_, Self>(self.daemon.try_lookup_txt_record()).map(
                |new_record, actor, ctx| {
                    info!("Received new DNS record, checking if a deployment is necessary.");
                    if new_record != actor.record {
                        ctx.notify(Deploy {});
                    }
                    ctx.notify_later(DnsLookup {}, Duration::from_secs(300));
                },
            ),
        )
    }
}

impl Handler<Deploy> for DaemonActor {
    type Result = Result<()>;

    fn handle(&mut self, msg: Deploy, ctx: &mut Context<Self>) -> Self::Result {
        info!("Deploying updated code from IPFS.");
        Arbiter::spawn(
            self.daemon
                .try_deploy()
                .map(|res| {
                    info!("Successfully deployed!");
                    res
                })
                .map_err(|err| {
                    error!("Error while deploying {:?}", err);
                    err
                })
                .then(|x| Ok(())),
        );
        Ok(())
    }
}
