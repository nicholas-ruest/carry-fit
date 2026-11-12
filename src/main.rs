use carry_fit::{load_manifest, optimize, render_markdown, render_text};
use clap::{Parser, ValueEnum};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(version, about)]
struct Args {
    /// TOML packing manifest.
    manifest: PathBuf,

    /// Output format.
    #[arg(long, value_enum, default_value_t = Format::Text)]
    format: Format,

    /// Write to a file instead of stdout.
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Replace an existing output file.
    #[arg(long, requires = "output")]
    force: bool,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum Format {
    Text,
    Json,
    Markdown,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let manifest = load_manifest(&args.manifest)?;
    let plan = optimize(&manifest)?;
    let output = match args.format {
        Format::Text => render_text(&plan),
        Format::Json => format!("{}\n", serde_json::to_string_pretty(&plan)?),
        Format::Markdown => render_markdown(&plan),
    };
    if let Some(path) = args.output {
        let mut options = std::fs::OpenOptions::new();
        options.write(true);
        if args.force {
            options.create(true).truncate(true);
        } else {
            options.create_new(true);
        }
        options.open(&path)?.write_all(output.as_bytes())?;
    } else {
        print!("{output}");
    }
    Ok(())
}

use std::io::Write as _;
