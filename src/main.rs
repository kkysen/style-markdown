use std::path::PathBuf;

use clap::Parser;
use clap::Subcommand;
use color_eyre::eyre;
use itertools::Itertools;
use regex::Captures;
use regex::Regex;

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

    /// Delete large embedded images (i.e. `<data:image/[^>]*>` HTML elements).
    EmbeddedImages,

    /// Delete extra spaces after `\[[^\]]: `, such as footnotes.
    ExtraRefSpaces,

    /// Simplify `[URL](URL)`s as `<URL>`.
    SimplifyUrls,

    /// Add semantic line breaks as best as possible.
    SemanticLineBreaks,
}

impl Command {
    fn rewrite(&self, before: String) -> String {
        match *self {
            Self::Quotes => {
                let after = before
                    .replace(|c| "‘’".contains(c), "'")
                    .replace(|c| "“”".contains(c), "\"");
                after
            }
            Self::EmbeddedImages => {
                let regex = Regex::new(r"<data:image/[^>]*>").unwrap();
                let after = regex.split(&before).join("TODO");
                after
            }
            Self::ExtraRefSpaces => {
                let regex = Regex::new(r"(\[[^\]]*\]: ) *").unwrap();
                let after = regex
                    .replace_all(&before, |captures: &Captures| captures[1].to_string())
                    .into_owned();
                after
            }
            Self::SimplifyUrls => {
                let regex = Regex::new(r"\[(?<text>[^\]]*)\]\((?<link>[^)]*)\)").unwrap();
                let after = regex
                    .replace_all(&before, |captures: &Captures| {
                        let (full, [text, link]) = captures.extract();
                        if text.replace('\\', "") == link {
                            format!("<{link}>")
                        } else {
                            full.to_string()
                        }
                    })
                    .into_owned();
                after
            }
            Self::SemanticLineBreaks => {
                let regex = Regex::new(r"(?<punctuation>[.!?;]) +").unwrap();
                let after = before
                    .split('\n')
                    .map(|line| {
                        if line.len() <= 80 {
                            // return vec![line.to_owned()];
                            return line.to_owned();
                        }
                        regex
                            .replace_all(line, |captures: &Captures| {
                                let (_, [punctuation]) = captures.extract();
                                format!("{punctuation}\n")
                            })
                            .into_owned()
                        // line.split(|c| ".!?;".contains(c)).collect()
                    })
                    .join("\n");
                after
            }
        }
    }
}

impl Args {
    fn run(&self) -> eyre::Result<()> {
        let before = fs_err::read_to_string(&self.path)?;
        let mut after = self.command.rewrite(before);
        if !after.ends_with("\n") {
            after.push_str("\n");
        }
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
