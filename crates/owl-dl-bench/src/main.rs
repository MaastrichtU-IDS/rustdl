use std::path::PathBuf;

use clap::Parser;

#[derive(Parser, Debug)]
#[command(name = "owl-dl-bench", version, about = "rustdl benchmark harness")]
struct Cli {
    /// Directory of ontologies to benchmark.
    #[arg(long)]
    corpus: Option<PathBuf>,

    /// Output JSONL file for raw results.
    #[arg(long, default_value = "bench-results/results.jsonl")]
    out: PathBuf,
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    let cli = Cli::parse();
    println!(
        "owl-dl-bench scaffold — corpus={:?}, out={}",
        cli.corpus,
        cli.out.display()
    );
    println!("Real benchmark loop lands in Day 21-25 of the 30-day plan.");
    Ok(())
}
