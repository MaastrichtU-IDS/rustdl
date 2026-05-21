use clap::Parser;

#[derive(Parser, Debug)]
#[command(name = "rustdl", version, about = "OWL DL reasoner (rustdl)")]
struct Cli {
    /// Print version information and exit.
    #[arg(long)]
    info: bool,
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();
    if cli.info {
        println!("rustdl {} (scaffold)", env!("CARGO_PKG_VERSION"));
        println!("No reasoning implemented yet — see strategy v2 Phase 0.");
    } else {
        println!("rustdl CLI — no commands wired yet. Try --help or --info.");
    }
    Ok(())
}
