use std::path::PathBuf;

use clap::Parser;
use clap::Subcommand;
use color_eyre::eyre;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// Path of the Markdown `*.md` file to style.
    path: PathBuf,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Replace fancy (`‘’`, `“”`) quotes with simple (`'`, `"`) quotes.
    Quotes,
}

impl Command {
    fn rewrite(&self, before: String) -> String {
        match *self {
            Self::Quotes => before
                .replace(|c| c == '‘' || c == '’', "'")
                .replace(|c| c == '“' || c == '”', "\""),
        }
    }
}

impl Args {
    fn run(&self) -> eyre::Result<()> {
        let before = fs_err::read_to_string(&self.path)?;
        let after = self.command.rewrite(before);
        fs_err::write(&self.path, after)?;
        Ok(())
    }
}

fn main() -> eyre::Result<()> {
    let args = Args::parse();
    println!("{args:?}");
    args.run()?;
    Ok(())
}
