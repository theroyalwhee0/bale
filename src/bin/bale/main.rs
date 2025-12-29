use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    version = concat!(
        env!("CARGO_PKG_VERSION"),
        "\n",
        "  Built:   ",
        env!("BUILD_DATETIME_ISO"),
        "\n",
        "  Profile: ",
        env!("EXPECT_PROFILE"),
    ),
    about
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Create a file or update its modification time.
    Touch {
        /// The file to touch.
        path: PathBuf,
    },
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Command::Touch { path } => {
            if let Err(e) = bale::touch(&path) {
                eprintln!("error: {e}");
                std::process::exit(1);
            }
        }
    }
}
