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
            && !msg.contains("early eof")
            && !msg.contains("close_notify")
        {
            eprintln!("  {} {msg}", style("✗").red().dim());
        }
    }
}

fn error_page(status_code: u16, title: &str, message: &str, hint: &str) -> String {
    let body = format!(
        "<!DOCTYPE html>\
        <html><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\
        <title>{status_code} {title}</title>\
        <style>\
        *{{margin:0;padding:0;box-sizing:border-box}}\
        body{{font-family:'JetBrains Mono','Fira Code','SF Mono',Menlo,Consolas,monospace;\
        display:flex;align-items:center;justify-content:center;height:100vh;\
        background:#0a0a0f;color:#e2e2e8}}\
        .c{{text-align:center}}\
        .code{{font-size:96px;font-weight:800;line-height:1;color:#1e1e2e}}\
        .msg{{margin:16px 0 0;font-size:15px}}\
        .msg b{{color:#22d3ee}}\
        .hint{{color:#555570;font-size:13px;margin:8px 0 0}}\
        .brand{{position:fixed;bottom:24px;color:#555570;font-size:12px}}\
        .brand span{{color:#22d3ee;font-weight:600}}\
        @media(prefers-color-scheme:light){{\
        body{{background:#f5f6f8;color:#111827}}\
        .code{{color:#e2e4e9}}\
        .msg b{{color:#0891b2}}\
        .hint{{color:#6b7280}}\
        .brand{{color:#6b7280}}\
        .brand span{{color:#0891b2}}\
        }}\
        </style></head>\
        <body><div class=\"c\">\
        <p class=\"code\">{status_code}</p>\
        <p class=\"msg\">{message}</p>\
        <p class=\"hint\">{hint}</p>\
        </div>\
        <div class=\"brand\"><span>xpo</span> dev</div>\
        </body></html>"
    );
    let status_text = match status_code {
        502 => "Bad Gateway",
        504 => "Gateway Timeout",
        _ => title,
    };
    format!(
        "HTTP/1.1 {status_code} {status_text}\r\n\
        Content-Type: text/html; charset=utf-8\r\n\
        Content-Length: {}\r\n\
        Connection: close\r\n\r\n{}",
        body.len(),
        body
    )
}

fn log_request(method: &str, path: &str, status: &str, duration: std::time::Duration) {
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

    println!(
        "  {} {} {} {}",
        style(method).bold(),
        path,
        styled_status,
        style(format!("{}ms", duration.as_millis())).dim()
    );
}

async fn proxy_connection(
    acceptor: TlsAcceptor,
    tcp_stream: TcpStream,
    upstream_port: u16,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let start = std::time::Instant::now();

    let tls_stream = acceptor.accept(tcp_stream).await?;
    let (mut tls_read, mut tls_write) = tokio::io::split(tls_stream);

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

    let req_str = String::from_utf8_lossy(&request_line);
    let parts: Vec<&str> = req_str.split_whitespace().collect();
    let method = parts.first().copied().unwrap_or("???").to_string();
    let path = parts.get(1).copied().unwrap_or("/").to_string();

    let mut upstream = match TcpStream::connect(("127.0.0.1", upstream_port)).await {
        Ok(s) => s,
        Err(_) => {
            let resp = error_page(
                502,
                "Bad Gateway",
                &format!("Cannot reach <b>localhost:{upstream_port}</b>"),
                "Make sure your dev server is running",
            );
            let _ = tls_write.write_all(resp.as_bytes()).await;
            let _ = tls_write.shutdown().await;
            log_request(&method, &path, "502", start.elapsed());
            return Ok(());
        }
    };

    let (mut up_read, mut up_write) = upstream.split();

    up_write.write_all(&request_line).await?;

    let captured_status = Arc::new(std::sync::Mutex::new(String::from("---")));
    let status_for_copy = captured_status.clone();

    let client_to_server = async {
        tokio::io::copy(&mut tls_read, &mut up_write).await?;
        Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
    };
    let server_to_client = async {
        let timeout = tokio::time::timeout(std::time::Duration::from_secs(2), async {
            let mut resp_first_line = Vec::with_capacity(128);
            let mut b = [0u8; 1];
            loop {
                if up_read.read_exact(&mut b).await.is_err() {
                    return resp_first_line;
                }
                resp_first_line.push(b[0]);
                if resp_first_line.ends_with(b"\r\n") {
                    return resp_first_line;
                }
                if resp_first_line.len() > 1024 {
                    return resp_first_line;
                }
            }
        })
        .await;

        let resp_first_line = match timeout {
            Ok(line) => line,
            Err(_) => {
                let resp = error_page(
                    504,
                    "Gateway Timeout",
                    &format!("<b>localhost:{upstream_port}</b> did not respond"),
                    "Server may be restarting",
                );
                let _ = tls_write.write_all(resp.as_bytes()).await;
                *status_for_copy.lock().unwrap() = "504".to_string();
                return Ok(());
            }
        };

        if resp_first_line.is_empty() {
            let resp = error_page(
                502,
                "Bad Gateway",
                &format!("Connection to <b>localhost:{upstream_port}</b> lost"),
                "Server may have stopped",
            );
            let _ = tls_write.write_all(resp.as_bytes()).await;
            *status_for_copy.lock().unwrap() = "502".to_string();
            return Ok(());
        }

        let _ = tls_write.write_all(&resp_first_line).await;

        let resp_str = String::from_utf8_lossy(&resp_first_line);
        let status = resp_str
            .split_whitespace()
            .nth(1)
            .unwrap_or("???")
            .to_string();

        *status_for_copy.lock().unwrap() = status;

        let _ = tokio::io::copy(&mut up_read, &mut tls_write).await;

        Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
    };

    let _ = tokio::try_join!(client_to_server, server_to_client);

    let duration = start.elapsed();
    let status_str = captured_status.lock().unwrap().clone();

    log_request(&method, &path, &status_str, duration);

    Ok(())
}
