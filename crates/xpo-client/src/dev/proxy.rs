use crate::dev::{ca, hosts};
use console::style;
use rustls_pki_types::{CertificateDer, PrivateKeyDer};
use std::collections::VecDeque;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::TlsAcceptor;
use xpo_core::error_page::ErrorPage;

struct LogState {
    entries: VecDeque<String>,
    displayed: usize,
    max: usize,
}

impl LogState {
    fn new(max: usize) -> Self {
        Self {
            entries: VecDeque::new(),
            displayed: 0,
            max,
        }
    }
}

fn print_banner(domain: &str, port: u16) {
    let d = "\x1b[2m";
    let b = "\x1b[1m";
    let c = "\x1b[36;1m";
    let r = "\x1b[0m";

    let url = format!("https://{domain}");
    let target = format!("localhost:{port}");

    let line1 = "xpo dev";
    let line2 = format!("{url} -> {target}");
    let line3 = "Ctrl+C to stop";

    let inner = line1.len().max(line2.len()).max(line3.len()) + 4;
    let border = "\u{2500}".repeat(inner);
    let empty = " ".repeat(inner);

    let pad1 = inner - line1.len() - 2;
    let pad2 = inner - line2.len() - 2;
    let pad3 = inner - line3.len() - 2;

    println!();
    println!("  {d}\u{256d}{border}\u{256e}{r}");
    println!("  {d}\u{2502}{r}{empty}{d}\u{2502}{r}");
    println!(
        "  {d}\u{2502}{r}  {b}{line1}{r}{}{d}\u{2502}{r}",
        " ".repeat(pad1)
    );
    println!("  {d}\u{2502}{r}{empty}{d}\u{2502}{r}");
    println!(
        "  {d}\u{2502}{r}  {c}{url}{r} -> {target}{}{d}\u{2502}{r}",
        " ".repeat(pad2)
    );
    println!("  {d}\u{2502}{r}{empty}{d}\u{2502}{r}");
    println!(
        "  {d}\u{2502}{r}  {d}{line3}{r}{}{d}\u{2502}{r}",
        " ".repeat(pad3)
    );
    println!("  {d}\u{2502}{r}{empty}{d}\u{2502}{r}");
    println!("  {d}\u{2570}{border}\u{256f}{r}");
    println!();
}

pub async fn run(port: u16, name: &str, max_logs: usize) -> Result<(), Box<dyn std::error::Error>> {
    let domain = format!("{name}.test");

    let status = std::process::Command::new("sudo")
        .args(["-v", "-n"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    if status.is_err() || !status.unwrap().success() {
        let status = std::process::Command::new("sudo").arg("-v").status()?;
        if !status.success() {
            return Err("sudo authentication failed".into());
        }
    }

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

    let (cert_pem, key_pem) = ca::generate_leaf_cert(&domain)?;
    hosts::add(&domain)?;

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

    let listener = TcpListener::bind("127.0.0.1:10443").await?;
    let http_listener = TcpListener::bind("127.0.0.1:10080").await?;

    let http_domain = domain.clone();
    tokio::spawn(async move {
        loop {
            if let Ok((mut stream, _)) = http_listener.accept().await {
                let d = http_domain.clone();
                tokio::spawn(async move {
                    let mut buf = [0u8; 1024];
                    let _ = stream.read(&mut buf).await;
                    let resp = format!(
                        "HTTP/1.1 301 Moved Permanently\r\nLocation: https://{d}/\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    );
                    let _ = stream.write_all(resp.as_bytes()).await;
                });
            }
        }
    });

    print_banner(&domain, port);

    let log_state = Arc::new(std::sync::Mutex::new(LogState::new(max_logs)));
    let domain_clone = domain.clone();
    loop {
        tokio::select! {
            result = listener.accept() => {
                match result {
                    Ok((stream, _)) => {
                        let acceptor = acceptor.clone();
                        let ls = log_state.clone();
                        tokio::spawn(handle_connection(acceptor, stream, port, ls));
                    }
                    Err(e) => {
                        eprintln!("  {} Accept error: {e}", style("✗").red());
                    }
                }
            }
            _ = tokio::signal::ctrl_c() => {
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

async fn handle_connection(
    acceptor: TlsAcceptor,
    tcp_stream: TcpStream,
    upstream_port: u16,
    log_state: Arc<std::sync::Mutex<LogState>>,
) {
    if let Err(e) = proxy_connection(acceptor, tcp_stream, upstream_port, &log_state).await {
        let msg = e.to_string();
        if !msg.contains("connection reset")
            && !msg.contains("broken pipe")
            && !msg.contains("unexpected eof")
            && !msg.contains("early eof")
            && !msg.contains("close_notify")
            && !msg.contains("CertificateUnknown")
        {
            eprintln!("  {} {msg}", style("✗").red().dim());
        }
    }
}

fn error_page(status_code: u16, title: &str, message: &str, hint: &str) -> String {
    let page = ErrorPage::new(status_code, message)
        .title(title)
        .hint(hint)
        .brand("<span>xpo</span> dev")
        .raw_html_message();
    let body = page.render_html();
    let status_text = page.status_text();
    format!(
        "HTTP/1.1 {status_code} {status_text}\r\n\
        Content-Type: text/html; charset=utf-8\r\n\
        Content-Length: {}\r\n\
        Connection: close\r\n\r\n{body}",
        body.len(),
    )
}

fn log_request(
    state: &Arc<std::sync::Mutex<LogState>>,
    method: &str,
    path: &str,
    status: &str,
    duration: std::time::Duration,
) {
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

    let ms = format!("{}ms", duration.as_millis());
    let suffix = format!(" {} {:>6}", status, ms);
    let term_width = terminal_size::terminal_size()
        .map(|(w, _)| w.0 as usize)
        .unwrap_or(80);
    let prefix = format!("  {:<6} ", method);
    let max_path = term_width.saturating_sub(prefix.len() + suffix.len());
    let display_path = if path.len() > max_path && max_path > 3 {
        format!("{}...", &path[..max_path - 3])
    } else {
        path.to_string()
    };
    let pad = term_width.saturating_sub(prefix.len() + display_path.len() + suffix.len());
    let line = format!(
        "  {:<6} {}{:>pad$}{} {:>6}",
        style(method).bold(),
        display_path,
        "",
        styled_status,
        style(ms).dim(),
        pad = pad
    );

    let mut state = state.lock().unwrap();
    state.entries.push_back(line);
    if state.max > 0 && state.entries.len() > state.max {
        state.entries.pop_front();
    }

    if state.displayed > 0 {
        print!("\x1b[{}A\x1b[J", state.displayed);
    }
    for entry in &state.entries {
        println!("{entry}");
    }
    use std::io::Write;
    std::io::stdout().flush().ok();
    state.displayed = state.entries.len();
}

async fn proxy_connection(
    acceptor: TlsAcceptor,
    tcp_stream: TcpStream,
    upstream_port: u16,
    log_state: &Arc<std::sync::Mutex<LogState>>,
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

    let mut upstream = match TcpStream::connect(("localhost", upstream_port)).await {
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
            log_request(log_state, &method, &path, "502", start.elapsed());
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
        let timeout = tokio::time::timeout(std::time::Duration::from_secs(5), async {
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

    log_request(log_state, &method, &path, &status_str, duration);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_page_502_structure() {
        let page = error_page(
            502,
            "Bad Gateway",
            "Cannot reach <b>localhost:3000</b>",
            "Make sure your dev server is running",
        );
        assert!(page.starts_with("HTTP/1.1 502 Bad Gateway\r\n"));
        assert!(page.contains("Content-Type: text/html"));
        assert!(page.contains("Content-Length:"));
        assert!(page.contains("<!DOCTYPE html>"));
        assert!(page.contains("<p class=\"code\">502</p>"));
        assert!(page.contains("localhost:3000"));
        assert!(page.contains("xpo"));
    }

    #[test]
    fn error_page_504_structure() {
        let page = error_page(
            504,
            "Gateway Timeout",
            "<b>localhost:3000</b> did not respond",
            "Server may be restarting",
        );
        assert!(page.starts_with("HTTP/1.1 504 Gateway Timeout\r\n"));
        assert!(page.contains("<p class=\"code\">504</p>"));
        assert!(page.contains("did not respond"));
    }

    #[test]
    fn error_page_has_dark_and_light_theme() {
        let page = error_page(502, "Bad Gateway", "test", "hint");
        assert!(page.contains("background:#0a0a0f"));
        assert!(page.contains("prefers-color-scheme:light"));
        assert!(page.contains("background:#f5f6f8"));
    }

    #[test]
    fn error_page_content_length_matches_body() {
        let page = error_page(502, "Bad Gateway", "test", "hint");
        let parts: Vec<&str> = page.splitn(2, "\r\n\r\n").collect();
        let headers = parts[0];
        let body = parts[1];
        let cl: usize = headers
            .lines()
            .find(|l| l.starts_with("Content-Length:"))
            .unwrap()
            .split(':')
            .nth(1)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        assert_eq!(cl, body.len());
    }

    #[tokio::test]
    async fn http_redirect_response() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let domain = "myapp.test".to_string();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 1024];
            let _ = stream.read(&mut buf).await;
            let resp = format!(
                "HTTP/1.1 301 Moved Permanently\r\nLocation: https://{domain}/\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            );
            stream.write_all(resp.as_bytes()).await.unwrap();
        });

        let mut stream = TcpStream::connect(addr).await.unwrap();
        stream
            .write_all(b"GET / HTTP/1.1\r\nHost: myapp.test\r\n\r\n")
            .await
            .unwrap();

        let mut buf = vec![0u8; 1024];
        let n = stream.read(&mut buf).await.unwrap();
        let response = String::from_utf8_lossy(&buf[..n]);

        assert!(response.starts_with("HTTP/1.1 301"));
        assert!(response.contains("Location: https://myapp.test/"));
    }

    fn test_log_state() -> Arc<std::sync::Mutex<LogState>> {
        Arc::new(std::sync::Mutex::new(LogState::new(10)))
    }

    fn test_tls_pair() -> (
        TlsAcceptor,
        tokio_rustls::TlsConnector,
        CertificateDer<'static>,
    ) {
        let key_pair = rcgen::KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256).unwrap();
        let params = rcgen::CertificateParams::new(vec!["localhost".to_string()]).unwrap();
        let cert = params.self_signed(&key_pair).unwrap();

        let cert_der = CertificateDer::from(cert.der().to_vec());
        let key_der = PrivateKeyDer::from(rustls_pki_types::PrivatePkcs8KeyDer::from(
            key_pair.serialize_der(),
        ));

        let server_config = rustls::ServerConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(vec![cert_der.clone()], key_der)
        .unwrap();

        let mut root_store = rustls::RootCertStore::empty();
        root_store.add(cert_der.clone()).unwrap();
        let client_config = rustls::ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_root_certificates(root_store)
        .with_no_client_auth();

        (
            TlsAcceptor::from(Arc::new(server_config)),
            tokio_rustls::TlsConnector::from(Arc::new(client_config)),
            cert_der,
        )
    }

    #[tokio::test]
    async fn e2e_proxy_200() {
        let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let upstream_port = upstream.local_addr().unwrap().port();

        tokio::spawn(async move {
            let (mut stream, _) = upstream.accept().await.unwrap();
            let mut buf = vec![0u8; 4096];
            let _ = stream.read(&mut buf).await;
            let resp = "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok";
            stream.write_all(resp.as_bytes()).await.unwrap();
            stream.shutdown().await.ok();
        });

        let (acceptor, connector, _) = test_tls_pair();
        let log_state = test_log_state();
        let ls = log_state.clone();
        let proxy_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let proxy_addr = proxy_listener.local_addr().unwrap();

        tokio::spawn(async move {
            let (stream, _) = proxy_listener.accept().await.unwrap();
            let _ = proxy_connection(acceptor, stream, upstream_port, &ls).await;
        });

        let tcp = TcpStream::connect(proxy_addr).await.unwrap();
        let server_name = rustls_pki_types::ServerName::try_from("localhost").unwrap();
        let mut tls = connector.connect(server_name, tcp).await.unwrap();

        tls.write_all(b"GET /hello HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();

        let mut response = Vec::new();
        let mut buf = [0u8; 4096];
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(2);
        loop {
            match tokio::time::timeout_at(deadline, tls.read(&mut buf)).await {
                Ok(Ok(0)) => break,
                Ok(Ok(n)) => response.extend_from_slice(&buf[..n]),
                _ => break,
            }
        }
        let resp_str = String::from_utf8_lossy(&response);

        assert!(resp_str.contains("200 OK"));
        assert!(resp_str.contains("ok"));
    }

    #[tokio::test]
    async fn e2e_proxy_502_upstream_down() {
        let (acceptor, connector, _) = test_tls_pair();
        let log_state = test_log_state();
        let ls = log_state.clone();
        let proxy_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let proxy_addr = proxy_listener.local_addr().unwrap();

        tokio::spawn(async move {
            let (stream, _) = proxy_listener.accept().await.unwrap();
            let _ = proxy_connection(acceptor, stream, 19999, &ls).await;
        });

        let tcp = TcpStream::connect(proxy_addr).await.unwrap();
        let server_name = rustls_pki_types::ServerName::try_from("localhost").unwrap();
        let mut tls = connector.connect(server_name, tcp).await.unwrap();

        tls.write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();

        let mut response = Vec::new();
        let _ = tls.read_to_end(&mut response).await;
        let resp_str = String::from_utf8_lossy(&response);

        assert!(resp_str.contains("502 Bad Gateway"));
        assert!(resp_str.contains("Cannot reach"));
    }

    #[test]
    fn rolling_log_keeps_max_10() {
        let state = test_log_state();
        for i in 0..15 {
            log_request(
                &state,
                "GET",
                &format!("/page{i}"),
                "200",
                std::time::Duration::from_millis(10),
            );
        }
        let s = state.lock().unwrap();
        assert_eq!(s.entries.len(), 10);
        assert!(s.entries.back().unwrap().contains("/page14"));
        assert!(!s.entries.front().unwrap().contains("/page0"));
    }
}
