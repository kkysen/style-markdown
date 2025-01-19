use std::borrow::Cow;
use std::path::PathBuf;

use clap::Parser;
use clap::Subcommand;
use color_eyre::eyre;
use itertools::Itertools;
use regex::Captures;
use regex::Regex;

fn main() -> eyre::Result<()> {
    let args = Args::parse();
    println!("{args:?}");
    args.run()?;
    Ok(())
}

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// Path of the Markdown `*.md` file to style.
    path: PathBuf,

    #[command(subcommand)]
    command: Command,
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

    /// Canonicalize "through-running" words, always hyphenating and always putting "through" before "run".
    ThroughRunning,
}

impl Command {
    fn rewrite(&self, before: String) -> String {
        let rewrite = match *self {
            Self::Quotes => canonicalize_quotes,
            Self::EmbeddedImages => remove_embedded_images,
            Self::ExtraRefSpaces => remove_extra_ref_spaces,
            Self::SimplifyUrls => simplify_urls,
            Self::SemanticLineBreaks => add_semantic_line_breaks,
            Self::ThroughRunning => canonicalize_through_running,
        };
        rewrite(before)
    }
}

fn canonicalize_quotes(before: String) -> String {
    let after = before
        .replace(|c| "‘’".contains(c), "'")
        .replace(|c| "“”".contains(c), "\"");
    after
}

fn remove_embedded_images(before: String) -> String {
    let data_image = Regex::new(r"<data:image/[^>]*>").unwrap();
    let after = data_image.split(&before).join("TODO");
    after
}

fn remove_extra_ref_spaces(before: String) -> String {
    let ref_with_spaces = Regex::new(r"(\[[^\]]*\]: ) *").unwrap();
    let after = ref_with_spaces
        .replace_all(&before, |captures: &Captures| captures[1].to_string())
        .into_owned();
    after
}

fn simplify_urls(before: String) -> String {
    let link = Regex::new(r"\[(?<text>[^\]]*)\]\((?<link>[^)]*)\)").unwrap();
    let after = link
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

fn add_semantic_line_breaks(before: String) -> String {
    let max_line_length = 100;

    // First, split each original line at the given punctuation regex.
    // Then rejoin lines before it gets longer than the line length.
    fn add_line_breaks<'a>(
        punctuation_regex: &str,
        line: &'a str,
        max_line_length: usize,
    ) -> Cow<'a, str> {
        let punctuation = Regex::new(&format!("(?<punctuation>{punctuation_regex}) +")).unwrap();
        // Don't break headings.
        let is_heading = || line.trim_ascii_start().starts_with('#');
        // Early optimization.
        if line.len() < max_line_length || is_heading() {
            return Cow::Borrowed(line);
        }
        let with_all_line_breaks = punctuation
            // Replace punctuation plus space with punctuation plus newline,
            // thus adding line breaks at all punctuation.
            .replace_all(line, |captures: &Captures| {
                let (_, [punctuation]) = captures.extract();
                format!("{punctuation}\n")
            });
        // For simplicity, the above is implemented by
        // replacing the spaces after punctuation with a newline,
        // so now split again to get the lines.
        let fully_split_lines = with_all_line_breaks.split('\n');
        // Newlines are manually added here.
        let mut rejoined_lines = Vec::new();
        let mut current_line_length = 0;
        for line in fully_split_lines {
            if current_line_length == 0 {
                // It could be too long, but we can't split it anymore by punctuation.
                rejoined_lines.push(line);
                current_line_length = line.len();
            } else if current_line_length + line.len() < max_line_length {
                // There's room to join a line, so join it with a space.
                rejoined_lines.push(" ");
                rejoined_lines.push(line);
                current_line_length += line.len();
            } else {
                // The line is too long, so keep it split.
                rejoined_lines.push("\n");
                rejoined_lines.push(line);
                current_line_length = line.len();
            }
        }
        Cow::Owned(rejoined_lines.concat())
    }

    let after = before
        .split_terminator('\n')
        .map(|line| {
            add_line_breaks(r"[.!?;:]", line, max_line_length)
                .split_terminator('\n')
                .map(|line| add_line_breaks(r"[,)\]]", line, max_line_length))
                .join("\n")
        })
        .join("\n");
    after
}

fn canonicalize_through_running(before: String) -> String {
    let after = before
        .replace("through running", "through-running")
        .replace("running through", "through-running")
        .replace("through run", "through-run")
        .replace("run through", "through-run");
    after
}
