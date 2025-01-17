use std::borrow::Cow;
use std::env;
use std::path::PathBuf;
use std::process;
use std::process::Output;

use clap::Parser;
use clap::Subcommand;
use color_eyre::eyre;
use color_eyre::eyre::ensure;
use color_eyre::eyre::Context;
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

    /// `git commit` the changes.
    #[arg(long)]
    commit: bool,

    #[command(subcommand)]
    command: Command,
}

impl Args {
    fn run(&self) -> eyre::Result<()> {
        let git = || process::Command::new("git");
        if self.commit {
            // `git status --porcelain` should be empty; no current changes
            run_command(
                git().args(["status", "--porcelain"]),
                &[&check_status, &check_empty_stdout],
            )?;
            assert!({
                let output = git().args(["status", "--porcelain"]).output()?;
                output.status.success() && output.stdout.is_empty()
            });
        }
        let before = fs_err::read_to_string(&self.path)?;
        let mut after = self.command.rewrite(before);
        if !after.ends_with("\n") {
            after.push_str("\n");
        }
        fs_err::write(&self.path, after)?;
        if self.commit {
            // `git add {self.path}`
            run_command(git().arg("add").arg(&self.path), &[&check_status])?;
            let cmd = env::args()
                .map(|arg| {
                    if arg.contains(' ') {
                        format!("'{}'", arg.replace('\'', r"\'"))
                    } else {
                        arg
                    }
                })
                .join(" ");
            let msg = format!("run `{cmd}`");
            // `git commit -m "run `{cmd}`"`
            run_command(git().args(["commit", "-m", &msg]), &[&check_status])?;
        }
        Ok(())
    }
}

fn run_command(
    cmd: &mut process::Command,
    checks: &[&dyn Fn(&mut Output) -> eyre::Result<()>],
) -> eyre::Result<()> {
    println!("> {cmd:?}");
    cmd.output()
        .map_err(eyre::Error::from) // into eyre
        .and_then(|mut output| {
            for check in checks {
                check(&mut output)?;
            }
            Ok(())
        })
        .wrap_err_with(|| format!("error running: {cmd:?}"))
}

fn check_status(output: &mut Output) -> eyre::Result<()> {
    ensure!(
        output.status.success(),
        "exited with {}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}

fn check_empty_stdout(output: &mut Output) -> eyre::Result<()> {
    ensure!(
        output.stdout.is_empty(),
        "expected empty stdout: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    Ok(())
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

    /// Move footnotes to always after punctuation.
    FootnotesAfterPunctuation,
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
            Self::FootnotesAfterPunctuation => move_footnotes_after_punctuation,
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

    /// First, split each original line at the given punctuation regex.
    /// Then rejoin lines before it gets longer than the line length.
    ///
    /// `separator_regex` should have either a `before` or `after` capture name
    /// depending on if it should go before or after the line break.
    fn add_line_breaks<'a>(
        separator_regex: &str,
        line: &'a str,
        max_line_length: usize,
    ) -> Cow<'a, str> {
        let punctuation = Regex::new(separator_regex).unwrap();
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
                if let Some(before) = captures.name("before") {
                    format!("{}\n", before.as_str())
                } else if let Some(after) = captures.name("after") {
                    format!("\n{}", after.as_str())
                } else {
                    panic!("captures supposed to have either `before` xor `after` group, but is {captures:?}");
                }
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

    // These are chosen somewhat subjectively.
    // Usually they should be coordinating and subordinating conjunctions.
    let line_starting_words = ["because", "that", "rather than", "of how", "in order to"];
    let line_starting_words_regex = line_starting_words
        .iter()
        // Sort by more words first, so that they take priority in the regex.
        .map(|conjunction| conjunction.split(' ').collect::<Vec<_>>())
        .sorted_by(|a, b| a.len().cmp(&b.len()).reverse())
        .map(|words| words.join(" "))
        .join("|");

    let outer_separators_regex = r"(?<before>[.!?;:]) +";
    let inner_separators_regex =
        &format!(r"(?<before>[,)\]]) +| +(?<after>\(|\[|{line_starting_words_regex})");

    let after = before
        .split_terminator('\n')
        .map(|line| {
            add_line_breaks(&outer_separators_regex, line, max_line_length)
                .split_terminator('\n')
                .map(|line| add_line_breaks(inner_separators_regex, line, max_line_length))
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

fn move_footnotes_after_punctuation(before: String) -> String {
    let regex = Regex::new(r"(?<footnote>\[\^[^\]]*\])(?<punctuation>[.!?;,])").unwrap();
    let after = regex.replace_all(&before, |captures: &Captures| {
        let (_, [footnote, punctuation]) = captures.extract();
        format!("{punctuation}{footnote}")
    });
    after.into_owned()
}

#[cfg(test)]
mod tests {
    use crate::add_semantic_line_breaks;
    use crate::canonicalize_quotes;
    use crate::canonicalize_through_running;
    use crate::move_footnotes_after_punctuation;
    use crate::remove_embedded_images;
    use crate::remove_extra_ref_spaces;
    use crate::simplify_urls;

    #[test]
    fn test_canonicalize_quotes() {
        let before = "‘’, “”";
        let after = "'', \"\"";
        assert_eq!(canonicalize_quotes(before.into()), after);
    }

    #[test]
    fn test_remove_embedded_images() {
        let before = "[image1]: <data:image/png;base64,iVBORw0KGgoAAAAN>

[image2]: <data:image/png;base64,iVBORw0KGgoAAAANS>";
        let after = "[image1]: TODO

[image2]: TODO";
        assert_eq!(remove_embedded_images(before.into()), after);
    }

    #[test]
    fn test_remove_extra_ref_spaces() {
        let before = "[^2]:    hello";
        let after = "[^2]: hello";
        assert_eq!(remove_extra_ref_spaces(before.into()), after);
    }

    #[test]
    fn test_simplify_urls() {
        let before = r"[URL](URL), [URL\_2](URL_2)";
        let after = "<URL>, <URL_2>";
        assert_eq!(simplify_urls(before.into()), after);
    }

    #[test]
    fn test_add_semantic_line_breaks() {
        let before = "
# A Not-So-Capital Plan Part 2: The Future is Electric

Metro-North's M8 can run on catenary power (left[^M8-catenary-pantograph-citation]) or on either over- or under-running third rails (shoe seen at right[^M8-third-rail-shoe-citation]).

## Introduction

In major cities all across the globe, electric trains form the backbone of urban transportation. The benefits of electrification are simply too great to ignore. Electric trains accelerate faster, reduce overall journey times, and provide a higher-quality passenger experience than their diesel-powered counterparts, all while being cheaper to run and maintain. Electric trains are also a powerful tool for decarbonization: they can easily run on non-carbon fuel sources and produce no local pollution. It is rare that a single technology can reduce both pollution and costs while also actually improving service, but electric rail can accomplish just that. That is why the future of rail is electric around both the country and the world.
        ";
        let after = "
# A Not-So-Capital Plan Part 2: The Future is Electric

Metro-North's M8 can run on catenary power (left[^M8-catenary-pantograph-citation])
or on either over- or under-running third rails (shoe seen at right[^M8-third-rail-shoe-citation]).

## Introduction

In major cities all across the globe, electric trains form the backbone of urban transportation.
The benefits of electrification are simply too great to ignore.
Electric trains accelerate faster, reduce overall journey times,
and provide a higher-quality passenger experience than their diesel-powered counterparts,
all while being cheaper to run and maintain.
Electric trains are also a powerful tool for decarbonization:
they can easily run on non-carbon fuel sources and produce no local pollution.
It is rare that a single technology can reduce both pollution and costs while also actually improving service,
but electric rail can accomplish just that.
That is why the future of rail is electric around both the country and the world.
        ";
        assert_eq!(add_semantic_line_breaks(before.into()), after);
    }

    #[test]
    fn test_canonicalize_through_running() {
        let before = "through-running, through running, running through, through-run, through run, run through";
        let after = "through-running, through-running, through-running, through-run, through-run, through-run";
        assert_eq!(canonicalize_through_running(before.into()), after);
    }

    #[test]
    fn test_move_footnotes_after_punctuation() {
        let before = "[^1].";
        let after = ".[^1]";
        assert_eq!(move_footnotes_after_punctuation(before.into()), after);
    }
}
