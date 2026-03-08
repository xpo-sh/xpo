use clap::{Args, Parser, Subcommand};

mod dev;

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
    Dev(DevArgs),
    Login,
    Status,
}

#[derive(Args)]
#[command(args_conflicts_with_subcommands = true)]
struct DevArgs {
    #[command(subcommand)]
    command: Option<DevCommands>,

    port: Option<u16>,

    #[arg(short, long)]
    name: Option<String>,
}

#[derive(Subcommand)]
enum DevCommands {
    #[command(about = "One-time setup: generate CA, trust it, configure port forwarding")]
    Setup,
    #[command(about = "Stop local HTTPS proxy and clean up")]
    Stop,
    #[command(about = "Check setup status and diagnose issues")]
    Doctor,
    #[command(about = "Remove CA, untrust it, and remove port forwarding")]
    Uninstall,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Dev(args) => match args.command {
            Some(DevCommands::Setup) => dev::setup::run(),
            Some(DevCommands::Stop) => {
                println!("  Coming soon.");
                Ok(())
            }
            Some(DevCommands::Doctor) => {
                println!("  Coming soon.");
                Ok(())
            }
            Some(DevCommands::Uninstall) => dev::uninstall::run(),
            None => {
                if let Some(port) = args.port {
                    let name = args.name.unwrap_or_else(|| "localhost".to_string());
                    dev::proxy::run(port, &name).await
                } else {
                    println!("  Usage: xpo dev <port> -n <name>");
                    println!("         xpo dev setup");
                    println!("         xpo dev stop");
                    Ok(())
                }
            }
        },
        Commands::Share {
            port,
            subdomain,
            domain: _,
        } => {
            let sub_display = subdomain
                .as_deref()
                .map(|s| format!("{s}.xpo.sh"))
                .unwrap_or_else(|| "<random>.xpo.sh".to_string());
            println!("  Tunnel establishing...");
            println!("  https://{sub_display} -> localhost:{port}");
            println!("\n  Coming soon. Visit https://xpo.sh");
            Ok(())
        }
        Commands::Login => {
            println!("  Coming soon. Visit https://xpo.sh");
            Ok(())
        }
        Commands::Status => {
            println!("  No active tunnels.");
            println!("  Coming soon. Visit https://xpo.sh");
            Ok(())
        }
    };

    if let Err(e) = result {
        eprintln!("  Error: {e}");
        std::process::exit(1);
    }
}
