use clap::{Args, Parser, Subcommand};

mod dev;
mod tunnel;

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
        #[arg(long, default_value = "10")]
        logs: usize,
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

    #[arg(long, default_value = "10")]
    logs: usize,
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
            Some(DevCommands::Stop) => dev::stop::run(),
            Some(DevCommands::Doctor) => {
                println!("  Coming soon.");
                Ok(())
            }
            Some(DevCommands::Uninstall) => dev::uninstall::run(),
            None => {
                if let Some(port) = args.port {
                    let name = args.name.unwrap_or_else(|| "localhost".to_string());
                    if !is_valid_name(&name) {
                        eprintln!(
                            "  {} Invalid name: '{}'. Use lowercase letters, digits, and hyphens (a-z, 0-9, -).",
                            console::style("✗").red().bold(),
                            name
                        );
                        std::process::exit(1);
                    }
                    dev::proxy::run(port, &name, args.logs).await
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
            logs,
        } => {
            let server = std::env::var("XPO_SERVER").unwrap_or_else(|_| "localhost:8081".into());
            tunnel::run(port, subdomain, &server, logs).await
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

fn is_valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 63
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !name.starts_with('-')
        && !name.ends_with('-')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_names() {
        assert!(is_valid_name("myapp"));
        assert!(is_valid_name("my-app"));
        assert!(is_valid_name("app123"));
        assert!(is_valid_name("a"));
        assert!(is_valid_name("my-cool-app-2"));
        assert!(is_valid_name("localhost"));
        assert!(is_valid_name(&"a".repeat(63)));
    }

    #[test]
    fn invalid_names() {
        assert!(!is_valid_name(""));
        assert!(!is_valid_name("-myapp"));
        assert!(!is_valid_name("myapp-"));
        assert!(!is_valid_name("MyApp"));
        assert!(!is_valid_name("my_app"));
        assert!(!is_valid_name("my app"));
        assert!(!is_valid_name("my.app"));
        assert!(!is_valid_name("../hack"));
        assert!(!is_valid_name("../../etc/passwd"));
        assert!(!is_valid_name(&"a".repeat(64)));
    }
}
