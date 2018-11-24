extern crate lrad;
#[macro_use]
extern crate clap;

use clap::{App, Arg, SubCommand};
use lrad::{error::Result, Lrad};

use std::env;

fn main() -> Result<()> {
    let matches = clap_app!(LRAD =>
        (version: crate_version!())
        (author: crate_authors!())
        (about: "An update framework for hobyist SBCs")
        (@arg CONFIG: -c --config +takes_value "Sets a custom config file")
        (@subcommand init =>
            (about: "Initializes the current git repo with a .lrad.toml configuration file.")
        )
        (@subcommand deploy =>
            (about: "Adds this git repo to IPFS and updates the DNS link record in Cloudflare.")
        )
    )
    .get_matches();
    if let Some(_matches) = matches.subcommand_matches("init") {
        let current_dir = env::current_dir()?;
        let lrad = Lrad::try_init(&current_dir)?;
        println!("Successfully initialized!");
    } else if let Some(_matches) = matches.subcommand_matches("deploy") {
        let current_dir = env::current_dir()?;
        let lrad = Lrad::try_load(&current_dir)?;
        lrad.try_deploy()?;
        println!("Successfully deployed!");
    }
    Ok(())
}
