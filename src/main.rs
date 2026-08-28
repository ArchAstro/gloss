use clap::{Args, Parser, Subcommand};
use gloss::format::LineRange;
use gloss::{AddOptions, App, ChangeScope, CommandOutput, GlossError, LintOptions, UpdateOptions};
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Parser)]
#[command(
    name = "gloss",
    version,
    about = "Attach intent annotations to source edit hunks"
)]
struct Cli {
    #[arg(long, global = true, help = "Emit stable machine-readable JSON")]
    json: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Install hooks, CI validation, generated-file handling, and metadata
    Init,
    /// Attach an explanation to a working-tree edit hunk
    Add(AddArgs),
    /// Validate gloss files without changing them
    Lint(LintArgs),
    /// Apply deterministic header, range, and lifecycle maintenance
    Update(UpdateArgs),
    /// Reconcile annotations and rebuild Git provenance
    Repair,
    /// Show gloss coverage for changed source hunks
    Status,
    /// Manage lightweight Git hooks
    Hook {
        #[command(subcommand)]
        command: HookCommand,
    },
    #[command(name = "__post-commit", hide = true)]
    PostCommit,
    #[command(name = "__post-rewrite", hide = true)]
    PostRewrite {
        #[arg(allow_hyphen_values = true)]
        args: Vec<String>,
    },
}

#[derive(Args)]
struct AddArgs {
    file: PathBuf,
    range: String,
    explanation: String,
    #[arg(long, env = "GLOSS_USER")]
    user: Option<String>,
    #[arg(long, env = "GLOSS_AGENT")]
    agent: Option<String>,
    #[arg(long, env = "GLOSS_SESSION")]
    session: Option<String>,
}

#[derive(Args)]
struct UpdateArgs {
    paths: Vec<PathBuf>,
    #[arg(long, env = "GLOSS_AGENT")]
    editor: Option<String>,
}

#[derive(Args)]
struct LintArgs {
    paths: Vec<PathBuf>,
    #[arg(
        long,
        conflicts_with = "base",
        help = "Validate staged changes for pre-commit"
    )]
    staged: bool,
    #[arg(
        long,
        env = "GLOSS_BASE",
        value_name = "REF",
        help = "Validate committed changes since a CI base ref"
    )]
    base: Option<String>,
    #[arg(
        long,
        help = "Create or update working-tree gloss files, then lint again"
    )]
    fix: bool,
    #[arg(long, env = "GLOSS_AGENT", requires = "fix")]
    editor: Option<String>,
}

#[derive(Subcommand)]
enum HookCommand {
    Install,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(&cli) {
        Ok(output) => {
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&output).unwrap());
            } else {
                println!("{}", output.human);
            }
            let ok = output
                .json
                .get("ok")
                .and_then(|value| value.as_bool())
                .unwrap_or(true);
            if ok {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            }
        }
        Err(error) => {
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&error).unwrap());
            } else {
                eprintln!("{}: {}", error.code.as_str(), error);
            }
            ExitCode::from(1)
        }
    }
}

fn run(cli: &Cli) -> Result<CommandOutput, GlossError> {
    let cwd = std::env::current_dir().map_err(|error| GlossError::io(error, "."))?;
    let app = App::discover(&cwd)?;
    match &cli.command {
        Command::Init => app.init(),
        Command::Add(args) => app.add(
            &args.file,
            LineRange::parse(&args.range)?,
            &args.explanation,
            AddOptions {
                user: args.user.clone(),
                agent: args.agent.clone(),
                session: args.session.clone(),
            },
        ),
        Command::Lint(args) => app.lint(
            &args.paths,
            LintOptions {
                scope: if args.staged {
                    ChangeScope::Staged
                } else if let Some(base) = &args.base {
                    ChangeScope::Base(base.clone())
                } else {
                    ChangeScope::WorkingTree
                },
                fix: args.fix,
                editor: args.editor.clone(),
            },
        ),
        Command::Update(args) => app.update(
            &args.paths,
            UpdateOptions {
                editor: args.editor.clone(),
            },
        ),
        Command::Repair => app.repair(),
        Command::Status => app.status(),
        Command::Hook {
            command: HookCommand::Install,
        } => app.hook_install(),
        Command::PostCommit => app.post_commit(),
        Command::PostRewrite { args: _ } => app.post_rewrite(),
    }
}
