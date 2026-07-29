mod cli;
mod recipe;

use clap::Parser;

fn main() {
    if let Err(error) = cli::Cli::parse().run() {
        eprintln!("sidecraft-textures: {error}");
        std::process::exit(1);
    }
}
