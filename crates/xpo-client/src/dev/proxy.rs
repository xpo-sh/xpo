use crate::dev::{ca, hosts};
use console::style;
use rustls_pki_types::{CertificateDer, PrivateKeyDer};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::TlsAcceptor;

pub async fn run(port: u16, name: &str) -> Result<(), Box<dyn std::error::Error>> {
    if !ca::ca_exists() {
        eprintln!(
            "  {} Run {} first",
            style("✗").red().bold(),
            style("xpo dev setup").cyan()
        );
        std::process::exit(1);
    }

    let domain = format!("{name}.test");

    // Cache sudo credentials upfront, keep alive for cleanup
    println!("  {} sudo required for /etc/hosts", style("○").dim());
    let status = std::process::Command::new("sudo").arg("-v").status()?;
    if !status.success() {
        return Err("sudo authentication failed".into());
    }
    // Background task: refresh sudo every 4 minutes
    tokio::spawn(async {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(240));
        loop {
            interval.tick().await;
            let _ = std::process::Command::new("sudo")
                .args(["-v", "-n"])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
        }
    });

    let sp = indicatif::ProgressBar::new_spinner();
    sp.set_style(
        indicatif::ProgressStyle::default_spinner()
            .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"])
            .template("  {spinner:.cyan} {msg}")
            .unwrap(),
    );
    sp.enable_steady_tick(std::time::Duration::from_millis(80));

    sp.set_message(format!("Generating certificate for {domain}..."));
    let (cert_pem, key_pem) = ca::generate_leaf_cert(&domain)?;

    sp.set_message(format!("Adding {domain} to /etc/hosts..."));
    hosts::add(&domain)?;

    sp.set_message("Starting HTTPS proxy...".to_string());

    let certs: Vec<CertificateDer<'static>> =
        rustls_pemfile::certs(&mut cert_pem.as_bytes()).collect::<Result<Vec<_>, _>>()?;

    let key: PrivateKeyDer<'static> = rustls_pemfile::private_key(&mut key_pem.as_bytes())?
        .ok_or("No private key found in PEM")?;

    let config = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()?
    .with_no_client_auth()
    .with_single_cert(certs, key)?;

    let acceptor = TlsAcceptor::from(Arc::new(config));

    let listener = TcpListener::bind("0.0.0.0:10443").await?;

    sp.finish_and_clear();

    println!();
    println!("  {}", style("xpo dev").bold());
    println!();
    println!(
        "  {} {}  →  localhost:{}",
        style("→").green().bold(),
        style(format!("https://{domain}")).cyan().bold(),
        port
    );
    println!();
    println!("  {}", style("Ctrl+C to stop").dim());
    println!();

    let domain_clone = domain.clone();
    loop {
        tokio::select! {
            result = listener.accept() => {
                match result {
                    Ok((stream, _)) => {
                        let acceptor = acceptor.clone();
                        tokio::spawn(handle_connection(acceptor, stream, port));
                    }
                    Err(e) => {
                        eprintln!("  {} Accept error: {e}", style("✗").red());
                    }
                }
            }
            _ = tokio::signal::ctrl_c() => {
                // \r + clear line to hide ^C
                eprint!("\r\x1b[2K");
                println!("  {} Cleaning up...", style("→").dim());
                let _ = hosts::remove(&domain_clone);
                println!("  {} Stopped", style("✓").green().bold());
                println!();
                break;
            }
        }
    }

    Ok(())
}

async fn handle_connection(acceptor: TlsAcceptor, tcp_stream: TcpStream, upstream_port: u16) {
    if let Err(e) = proxy_connection(acceptor, tcp_stream, upstream_port).await {
        let msg = e.to_string();
        if !msg.contains("connection reset")
            && !msg.contains("broken pipe")
            && !msg.contains("unexpected eof")
            && !msg.contains("close_notify")
        {
            eprintln!("  {} {msg}", style("✗").red().dim());
        }
    }
}

async fn proxy_connection(
    acceptor: TlsAcceptor,
    tcp_stream: TcpStream,
    upstream_port: u16,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let start = std::time::Instant::now();

    let tls_stream = acceptor.accept(tcp_stream).await?;
    let (mut tls_read, mut tls_write) = tokio::io::split(tls_stream);

    let mut upstream = TcpStream::connect(("127.0.0.1", upstream_port)).await?;
    let (mut up_read, mut up_write) = upstream.split();

    let mut request_line = Vec::with_capacity(512);
    let mut byte = [0u8; 1];
    loop {
        tls_read.read_exact(&mut byte).await?;
        request_line.push(byte[0]);
        if request_line.ends_with(b"\r\n") {
            break;
        }
        if request_line.len() > 8192 {
            break;
        }
    }

    up_write.write_all(&request_line).await?;

    let req_str = String::from_utf8_lossy(&request_line);
    let parts: Vec<&str> = req_str.split_whitespace().collect();
    let method = parts.first().copied().unwrap_or("???");
    let path = parts.get(1).copied().unwrap_or("/");

    let client_to_server = tokio::io::copy(&mut tls_read, &mut up_write);
    let server_to_client = async {
        let mut resp_first_line = Vec::with_capacity(128);
        let mut b = [0u8; 1];
        loop {
            up_read.read_exact(&mut b).await?;
            resp_first_line.push(b[0]);
            if resp_first_line.ends_with(b"\r\n") {
                break;
            }
            if resp_first_line.len() > 1024 {
                break;
            }
        }
        tls_write.write_all(&resp_first_line).await?;

        let resp_str = String::from_utf8_lossy(&resp_first_line);
        let status = resp_str
            .split_whitespace()
            .nth(1)
            .unwrap_or("???")
            .to_string();

        let _ = tokio::io::copy(&mut up_read, &mut tls_write).await;

        Ok::<String, Box<dyn std::error::Error + Send + Sync>>(status)
    };

    let (_, status_result) = tokio::try_join!(
        async {
            client_to_server.await?;
            Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
        },
        server_to_client
    )?;

    let duration = start.elapsed();
    let status = &status_result;
    let status_code: u16 = status.parse().unwrap_or(0);

    let styled_status = if status_code >= 500 {
        style(status).red().bold()
    } else if status_code >= 400 {
        style(status).yellow()
    } else if status_code >= 300 {
        style(status).cyan()
    } else if status_code >= 200 {
        style(status).green()
    } else {
        style(status).dim()
    };

    let styled_method = style(method).bold();

    println!(
        "  {} {} {} {}",
        styled_method,
        path,
        styled_status,
        style(format!("{}ms", duration.as_millis())).dim()
    );

    Ok(())
}
