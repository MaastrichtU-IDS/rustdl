use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(name = "xtask", about = "rustdl build automation")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Fetch the ORE 2015 Live corpus into ./ontologies/ore-2015-live.
    FetchOre2015,
    /// Refresh the NOTICE file with current third-party license info.
    ThirdPartyLicenses,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::FetchOre2015 => {
            println!("xtask fetch-ore-2015: not implemented yet (Day 26-30 of the 30-day plan).");
        }
        Cmd::ThirdPartyLicenses => {
            println!(
                "xtask third-party-licenses: not implemented yet. \
                 In the meantime run `cargo deny list` or install `cargo about`."
            );
        }
    }
    Ok(())
}
