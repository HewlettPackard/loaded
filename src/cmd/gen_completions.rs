use crate::cli::Loaded;
use anyhow::Result;
use clap::CommandFactory;
use clap_complete::{generate, generate_to, Shell};

pub fn generate_completions(shell: Shell, out_dir: Option<String>) -> Result<()> {
    let mut cli = Loaded::command();

    match out_dir {
        Some(out_dir) => {
            generate_to(shell, &mut cli, env!("CARGO_PKG_NAME"), &out_dir)?;
        }
        None => {
            generate(
                shell,
                &mut cli,
                env!("CARGO_PKG_NAME"),
                &mut std::io::stdout(),
            );
        }
    }
    Ok(())
}
