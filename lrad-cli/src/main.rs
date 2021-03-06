#[macro_use]
extern crate clap;
#[macro_use]
extern crate log;

use lrad::{error::Result, LradCli};

use futures::prelude::*;
use std::env;

fn main() -> Result<()> {
    let dotenv_res = dotenv::dotenv();
    if env::var("RUST_LOG").is_err() {
        env::set_var("RUST_LOG", "lrad=info,lrad-cli=info");
    }
    env_logger::init();
    if dotenv_res.is_ok() {
        // TODO: Add a config option for this
        warn!("A .env file was found and environment variables were loaded from it. If you do not want this behavior, change it in the config file.");
    }
    let matches = clap_app!(LRAD =>
        (version: crate_version!())
        (author: crate_authors!())
        (about: "An update framework for hobbyist SBCs")
        (@arg CONFIG: -c --config +takes_value "Sets a custom config file")
        (@subcommand init =>
            (about: "Initializes the current git repo with a .lrad.toml configuration file.")
        )
        (@subcommand push =>
            (about: "Pushes this git repo to IPFS and updates the DNS link record in Cloudflare.")
        )
        (@subcommand daemon =>
            (about: "Starts daemon to deploy packages with")
        )
    )
    .get_matches();

    if let Some(_matches) = matches.subcommand_matches("init") {
        let current_dir = env::current_dir()?;
        LradCli::try_init(&current_dir)?;
        info!("Successfully initialized! Please make sure to store any secrets securely.");
        Ok(())
    } else if let Some(_matches) = matches.subcommand_matches("push") {
        let current_dir = env::current_dir()?;
        let lrad = LradCli::try_load(&current_dir)?;
        lrad.try_push().and_then(|hash| {
            info!("Successfully pushed to IPFS! You can try cloning it from your local IPFS gateway: http://localhost:8080/ipfs/{}", hash);
            Ok(())
        }).wait()
    } else {
        Ok(())
    }
}
