use clap::{Args, Parser, Subcommand};

mod auth;
mod dev;
mod error_page;
mod tunnel;
mod uninstall;
mod update;

#[derive(Parser)]
#[command(
    name = "xpo",
    about = "Expose local services via secure tunnels",
    version,
    after_help = "Run 'xpo <command> --help' for more info"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    #[command(about = "Expose a local port to the internet")]
    Share {
        port: u16,
        #[arg(short, long)]
        subdomain: Option<String>,
        #[arg(short, long)]
        domain: Option<String>,
        #[arg(long, default_value = "500")]
        log_max: usize,
        #[arg(long, default_value = "10")]
        log_visible: usize,
        #[arg(long)]
        cors: bool,
    },
    #[command(about = "Local HTTPS development with .test domains")]
    Dev(DevArgs),
    #[command(about = "Authenticate with GitHub or Google")]
    Login {
        #[arg(long)]
        provider: Option<String>,
    },
    #[command(about = "Clear session")]
    Logout,
    #[command(about = "Show login info")]
    Status,
    #[command(about = "Check setup status and diagnose issues")]
    Doctor,
    #[command(about = "Update xpo to the latest version")]
    Update,
    #[command(about = "Remove all xpo data from your system")]
    Uninstall,
}

#[derive(Args)]
#[command(args_conflicts_with_subcommands = true)]
struct DevArgs {
    #[command(subcommand)]
    command: Option<DevCommands>,

    port: Option<u16>,

    #[arg(short, long)]
    name: Option<String>,

    #[arg(long, default_value = "500")]
    log_max: usize,
    #[arg(long, default_value = "10")]
    log_visible: usize,
}

#[derive(Subcommand)]
enum DevCommands {
    #[command(about = "One-time setup: generate CA, trust it, configure port forwarding")]
    Setup,
    #[command(about = "Stop local HTTPS proxy and clean up")]
    Stop,
    #[command(about = "Remove CA, untrust it, and remove port forwarding")]
    Uninstall,
}

#[tokio::main]
async fn main() {
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("failed to install crypto provider");

    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Dev(args) => match args.command {
            Some(DevCommands::Setup) => dev::setup::run(),
            Some(DevCommands::Stop) => dev::stop::run(),
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
                    match ensure_dev_setup() {
                        Ok(()) => {
                            dev::proxy::run(port, &name, args.log_max, args.log_visible).await
                        }
                        Err(e) => Err(e),
                    }
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
            log_max,
            log_visible,
            cors,
        } => {
            let server =
                std::env::var("XPO_SERVER").unwrap_or_else(|_| "ws.xpo.sh:8081".to_string());
            tunnel::run(port, subdomain, &server, log_max, log_visible, cors).await
        }
        Commands::Login { provider } => {
            let provider = provider.unwrap_or_else(|| {
                let items = vec!["GitHub", "Google"];
                let selection =
                    dialoguer::Select::with_theme(&dialoguer::theme::ColorfulTheme::default())
                        .with_prompt("  Select provider")
                        .items(&items)
                        .default(0)
                        .interact()
                        .unwrap_or(0);
                items[selection].to_lowercase()
            });
            auth::login(&provider).await
        }
        Commands::Logout => {
            let mut config = xpo_core::config::Config::load().unwrap_or_default();
            config.clear_tokens();
            let _ = config.save();
            println!("  {} Logged out.", console::style("✓").green().bold());
            Ok(())
        }
        Commands::Doctor => dev::doctor::run(),
        Commands::Update => update::run().await,
        Commands::Uninstall => uninstall::run(),
        Commands::Status => {
            let config = xpo_core::config::Config::load().unwrap_or_default();
            if config.is_authenticated() && !config.is_expired() {
                let provider = config
                    .auth
                    .provider
                    .as_deref()
                    .map(|p| {
                        let label = p[..1].to_uppercase() + &p[1..];
                        format!(" ({})", console::style(label).dim())
                    })
                    .unwrap_or_default();
                println!(
                    "  {} Logged in as {}{}",
                    console::style("✓").green().bold(),
                    console::style(config.auth.email.as_deref().unwrap_or("unknown")).cyan(),
                    provider
                );
            } else {
                println!("  Not logged in. Run: xpo login");
            }
            Ok(())
        }
    };

    if let Err(e) = result {
        eprintln!("  Error: {e}");
        std::process::exit(1);
    }
}

fn ensure_dev_setup() -> Result<(), Box<dyn std::error::Error>> {
    if dev::ca::ca_exists()
        && dev::setup::is_ca_trusted()
        && dev::setup::is_port_forwarding_active()
    {
        return Ok(());
    }
    println!(
        "  {} Running first-time setup...",
        console::style("→").dim()
    );
    dev::setup::run()
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
