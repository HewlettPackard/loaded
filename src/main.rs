#![warn(clippy::pedantic)]
#![allow(
    clippy::style,
    clippy::module_name_repetitions,
    clippy::missing_errors_doc
)]

pub mod cli;
mod cmd;
pub(crate) mod connection;
mod engine;
pub mod stats;
mod stream;
mod util;
pub mod worker;

use crate::cli::{Loaded, LoadedCmd};
use anyhow::{bail, Result};
use clap::Parser;

fn main() -> Result<()> {
    env_logger::init();
    let args = Loaded::parse();
    match args.loaded {
        LoadedCmd::GenCompletions { shell, out_dir } => {
            cmd::gen_completions::generate_completions(shell, out_dir)?;
        }
        LoadedCmd::Run(args) => {
            if args.connections < args.threads {
                bail!(
                    "Connections ({}) cannot be less than the number of threads ({}).",
                    args.connections,
                    args.threads
                )
            }
            cmd::run::run(&args)?;
        }
    }

    Ok(())
}
