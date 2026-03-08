use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "xpo",
    about = "Expose local services via secure tunnels",
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Share {
        port: u16,
        #[arg(short, long)]
        subdomain: Option<String>,
        #[arg(short, long)]
        domain: Option<String>,
    },
    Login,
    Status,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Share {
            port,
            subdomain,
            domain: _,
        } => {
            let sub_display = subdomain
                .as_deref()
                .map(|s| format!("{s}.xpo.sh"))
                .unwrap_or_else(|| "<random>.xpo.sh".to_string());
            println!("  ⚡ Tunnel establishing...");
            println!("  → https://{sub_display} → localhost:{port}");
            println!("\n  Coming soon. Visit https://xpo.sh");
        }
        Commands::Login => {
            println!("  Coming soon. Visit https://xpo.sh");
        }
        Commands::Status => {
            println!("  No active tunnels.");
            println!("  Coming soon. Visit https://xpo.sh");
        }
    }
}
