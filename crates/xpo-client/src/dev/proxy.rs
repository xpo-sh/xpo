use crate::dev::{ca, hosts};
use console::style;
use rustls_pki_types::{CertificateDer, PrivateKeyDer};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::TlsAcceptor;
use xpo_core::error_page::ErrorPage;
use xpo_tui::app::{BannerInfo, TuiApp};
use xpo_tui::event::AppEvent;
use xpo_tui::model::RequestLog;

static REQUEST_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

pub async fn run(
    port: u16,
    name: &str,
    max_logs: usize,
    visible_rows: usize,
) -> Result<(), Box<dyn std::error::Error>> {
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

    let (cert_pem, key_pem) = ca::ensure_leaf_cert(&domain)?;
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

    #[cfg(target_os = "macos")]
    {
        if !crate::dev::setup::verify_pf_runtime_state() {
            eprintln!(
                "  {} Port forwarding rules not active, reloading...",
                style("\u{2192}").dim()
            );
            if crate::dev::setup::auto_reload_pf().is_err() {
                eprintln!(
                    "  {} Could not reload pfctl rules. Run: sudo pfctl -ef /etc/pf.conf",
                    style("\u{2717}").red().bold()
                );
            }
        }
    }

    let use_tui = TuiApp::check_terminal_size();
    let (app_tx, events) = TuiApp::create_channel();
    let quit_flag = Arc::new(AtomicBool::new(false));

    let tui_handle = if use_tui {
        let banner = BannerInfo {
            title: "xpo dev".to_string(),
            url: format!("https://{domain}"),
            target: format!("localhost:{port}"),
            extra_lines: vec![],
            has_qr: false,
            qr_url: None,
        };
        let qf = quit_flag.clone();
        Some(std::thread::spawn(move || {
            let mut terminal = match TuiApp::init_terminal() {
                Ok(t) => t,
                Err(_) => return,
            };
            let mut app = TuiApp::new(banner, max_logs, visible_rows);

            loop {
                let _ = terminal.draw(|frame| xpo_tui::render::draw(frame, &app));

                match events.next() {
                    Ok(event) => {
                        app.handle_event(event);
                        if app.should_quit {
                            qf.store(true, Ordering::Relaxed);
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }

            let summary = app.summary_line();
            drop(terminal);
            TuiApp::restore_terminal();
            print!("\r\x1b[2K");
            println!("{summary}");
        }))
    } else {
        legacy_print_banner(&domain, port);
        None
    };

    spawn_pf_health_check(quit_flag.clone(), app_tx.clone());

    let domain_clone = domain.clone();
    let quit_notify = Arc::new(tokio::sync::Notify::new());
    let quit_notify2 = quit_notify.clone();

    let qf_check = quit_flag.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            if qf_check.load(Ordering::Relaxed) {
                quit_notify2.notify_one();
                break;
            }
        }
    });

    loop {
        tokio::select! {
            result = listener.accept() => {
                match result {
                    Ok((stream, _)) => {
                        let acceptor = acceptor.clone();
                        let tx = app_tx.clone();
                        tokio::spawn(handle_connection(acceptor, stream, port, tx));
                    }
                    Err(e) => {
                        eprintln!("  {} Accept error: {e}", style("\u{2717}").red());
                    }
                }
            }
            _ = tokio::signal::ctrl_c() => {
                break;
            }
            _ = quit_notify.notified() => {
                break;
            }
        }
    }

    let _ = hosts::remove(&domain_clone);
    drop(app_tx);

    if let Some(handle) = tui_handle {
        let _ = handle.join();
    } else {
        eprint!("\r\x1b[2K");
        println!("  {} Cleaning up...", style("\u{2192}").dim());
        println!("  {} Stopped", style("\u{2713}").green().bold());
        println!();
    }

    Ok(())
}

#[cfg(target_os = "macos")]
fn spawn_pf_health_check(quit_flag: Arc<AtomicBool>, event_tx: std::sync::mpsc::Sender<AppEvent>) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
        interval.tick().await;
        loop {
            interval.tick().await;
            if quit_flag.load(Ordering::Relaxed) {
                break;
            }
            let active = crate::dev::setup::verify_pf_runtime_state();
            let _ = event_tx.send(AppEvent::PfStatus(active));
            if !active {
                let _ = crate::dev::setup::auto_reload_pf();
                let reloaded = crate::dev::setup::verify_pf_runtime_state();
                let _ = event_tx.send(AppEvent::PfStatus(reloaded));
            }
        }
    });
}

#[cfg(not(target_os = "macos"))]
fn spawn_pf_health_check(
    _quit_flag: Arc<AtomicBool>,
    _event_tx: std::sync::mpsc::Sender<AppEvent>,
) {
}

fn legacy_print_banner(domain: &str, port: u16) {
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

async fn handle_connection(
    acceptor: TlsAcceptor,
    tcp_stream: TcpStream,
    upstream_port: u16,
    event_tx: std::sync::mpsc::Sender<AppEvent>,
) {
    if let Err(e) = proxy_connection(acceptor, tcp_stream, upstream_port, &event_tx).await {
        let msg = e.to_string();
        if !msg.contains("connection reset")
            && !msg.contains("broken pipe")
            && !msg.contains("unexpected eof")
            && !msg.contains("early eof")
            && !msg.contains("close_notify")
            && !msg.contains("CertificateUnknown")
        {
            eprintln!("  {} {msg}", style("\u{2717}").red().dim());
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

fn parse_headers(raw: &[u8]) -> Vec<(String, String)> {
    let s = String::from_utf8_lossy(raw);
    let mut headers = Vec::new();
    for line in s.split("\r\n").skip(1) {
        if line.is_empty() {
            break;
        }
        if let Some((key, val)) = line.split_once(':') {
            headers.push((key.trim().to_string(), val.trim().to_string()));
        }
    }
    headers
}

#[allow(clippy::too_many_arguments)]
fn send_request_log(
    tx: &std::sync::mpsc::Sender<AppEvent>,
    method: &str,
    path: &str,
    status: u16,
    duration: std::time::Duration,
    req_headers: Vec<(String, String)>,
    resp_headers: Vec<(String, String)>,
    body_size: u64,
) {
    let log = RequestLog {
        id: REQUEST_COUNTER.fetch_add(1, Ordering::Relaxed),
        timestamp: time::OffsetDateTime::now_utc(),
        method: method.to_string(),
        path: path.to_string(),
        status,
        duration_ms: duration.as_millis() as u64,
        request_headers: req_headers,
        response_headers: resp_headers,
        body_preview: None,
        body_size,
    };
    let _ = tx.send(AppEvent::Request(log));
}

async fn proxy_connection(
    acceptor: TlsAcceptor,
    tcp_stream: TcpStream,
    upstream_port: u16,
    event_tx: &std::sync::mpsc::Sender<AppEvent>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let start = std::time::Instant::now();

    let tls_stream = acceptor.accept(tcp_stream).await?;
    let (mut tls_read, mut tls_write) = tokio::io::split(tls_stream);

    let mut buf = Vec::with_capacity(8192);
    let mut chunk = [0u8; 4096];
    loop {
        let n = tls_read.read(&mut chunk).await?;
        if n == 0 {
            return Ok(());
        }
        buf.extend_from_slice(&chunk[..n]);
        if buf.windows(4).any(|w| w == b"\r\n\r\n") {
            break;
        }
        if buf.len() > 65536 {
            break;
        }
    }

    let header_end = buf
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .unwrap_or(buf.len());
    let headers_with_sep = &buf[..header_end + 4];
    let body_remainder = &buf[header_end + 4..];

    let header_str = String::from_utf8_lossy(headers_with_sep);
    let first_line_end = header_str.find("\r\n").unwrap_or(header_str.len());
    let parts: Vec<&str> = header_str[..first_line_end].split_whitespace().collect();
    let method = parts.first().copied().unwrap_or("???").to_string();
    let path = parts.get(1).copied().unwrap_or("/").to_string();

    let is_ws_upgrade = header_str
        .to_ascii_lowercase()
        .contains("upgrade: websocket");
    let req_headers = parse_headers(headers_with_sep);
    let rewritten = rewrite_host_header(headers_with_sep, upstream_port);

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
            send_request_log(
                event_tx,
                &method,
                &path,
                502,
                start.elapsed(),
                req_headers,
                vec![],
                0,
            );
            return Ok(());
        }
    };

    let (mut up_read, mut up_write) = upstream.split();

    up_write.write_all(&rewritten).await?;
    if !body_remainder.is_empty() {
        up_write.write_all(body_remainder).await?;
    }

    let captured_status = Arc::new(std::sync::Mutex::new(String::from("---")));
    let captured_resp_headers: Arc<std::sync::Mutex<Vec<(String, String)>>> =
        Arc::new(std::sync::Mutex::new(vec![]));
    let captured_body_size: Arc<std::sync::atomic::AtomicU64> =
        Arc::new(std::sync::atomic::AtomicU64::new(0));
    let status_for_copy = captured_status.clone();
    let resp_headers_for_copy = captured_resp_headers.clone();
    let body_size_for_copy = captured_body_size.clone();

    let client_to_server = async {
        tokio::io::copy(&mut tls_read, &mut up_write).await?;
        Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
    };
    let server_to_client = async {
        let mut resp_buf = Vec::with_capacity(8192);
        let mut chunk = [0u8; 4096];

        let first_read = tokio::time::timeout(
            std::time::Duration::from_secs(if is_ws_upgrade { 30 } else { 10 }),
            up_read.read(&mut chunk),
        )
        .await;

        match first_read {
            Ok(Ok(0)) | Ok(Err(_)) | Err(_) => {
                let (code, title, msg) = if first_read.is_err() {
                    (
                        504,
                        "Gateway Timeout",
                        format!("<b>localhost:{upstream_port}</b> did not respond"),
                    )
                } else {
                    (
                        502,
                        "Bad Gateway",
                        format!("Connection to <b>localhost:{upstream_port}</b> lost"),
                    )
                };
                let resp = error_page(code, title, &msg, "Server may be restarting");
                let _ = tls_write.write_all(resp.as_bytes()).await;
                *status_for_copy.lock().unwrap() = code.to_string();
                return Ok(());
            }
            Ok(Ok(n)) => {
                resp_buf.extend_from_slice(&chunk[..n]);
            }
        }

        let resp_str = String::from_utf8_lossy(&resp_buf);
        let status = resp_str
            .split_whitespace()
            .nth(1)
            .unwrap_or("???")
            .to_string();
        *status_for_copy.lock().unwrap() = status;

        *resp_headers_for_copy.lock().unwrap() = parse_headers(&resp_buf);
        if let Some(cl) = parse_headers(&resp_buf)
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("content-length"))
        {
            body_size_for_copy.store(cl.1.parse().unwrap_or(0), Ordering::Relaxed);
        }

        let _ = tls_write.write_all(&resp_buf).await;
        let _ = tokio::io::copy(&mut up_read, &mut tls_write).await;

        Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
    };

    let _ = tokio::try_join!(client_to_server, server_to_client);

    let duration = start.elapsed();
    let status_str = captured_status.lock().unwrap().clone();
    let status_code: u16 = status_str.parse().unwrap_or(0);
    let resp_headers = captured_resp_headers.lock().unwrap().clone();
    let body_size = captured_body_size.load(Ordering::Relaxed);

    send_request_log(
        event_tx,
        &method,
        &path,
        status_code,
        duration,
        req_headers,
        resp_headers,
        body_size,
    );

    Ok(())
}

fn rewrite_host_header(raw: &[u8], port: u16) -> Vec<u8> {
    let header_end = match raw.windows(4).position(|w| w == b"\r\n\r\n") {
        Some(pos) => pos,
        None => return raw.to_vec(),
    };

    let header_bytes = &raw[..header_end];
    let header_str = String::from_utf8_lossy(header_bytes);
    let first_crlf = match header_str.find("\r\n") {
        Some(pos) => pos,
        None => return raw.to_vec(),
    };

    let is_ws_upgrade = header_str
        .to_ascii_lowercase()
        .contains("upgrade: websocket");

    let mut patched = Vec::with_capacity(raw.len() + 64);
    patched.extend_from_slice(&header_bytes[..first_crlf + 2]);

    if !is_ws_upgrade {
        patched.extend_from_slice(b"Connection: close\r\n");
    }

    for line in header_str[first_crlf + 2..].split("\r\n") {
        if line.is_empty() {
            continue;
        }
        let lower = line.to_ascii_lowercase();
        if lower.starts_with("host:") {
            patched.extend_from_slice(format!("Host: localhost:{port}\r\n").as_bytes());
        } else if !is_ws_upgrade && lower.starts_with("connection:") {
            continue;
        } else {
            patched.extend_from_slice(line.as_bytes());
            patched.extend_from_slice(b"\r\n");
        }
    }

    patched.extend_from_slice(b"\r\n");
    patched
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

    fn test_event_tx() -> std::sync::mpsc::Sender<AppEvent> {
        let (tx, _rx) = std::sync::mpsc::channel();
        tx
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
        let event_tx = test_event_tx();
        let proxy_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let proxy_addr = proxy_listener.local_addr().unwrap();

        tokio::spawn(async move {
            let (stream, _) = proxy_listener.accept().await.unwrap();
            let _ = proxy_connection(acceptor, stream, upstream_port, &event_tx).await;
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
        let event_tx = test_event_tx();
        let proxy_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let proxy_addr = proxy_listener.local_addr().unwrap();

        tokio::spawn(async move {
            let (stream, _) = proxy_listener.accept().await.unwrap();
            let _ = proxy_connection(acceptor, stream, 19999, &event_tx).await;
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
    fn rolling_log_via_event_channel() {
        let (tx, rx) = std::sync::mpsc::channel::<AppEvent>();
        let mut state = xpo_tui::model::TuiState::new(10, 10);

        for i in 0..15 {
            send_request_log(
                &tx,
                "GET",
                &format!("/page{i}"),
                200,
                std::time::Duration::from_millis(10),
                vec![],
                vec![],
                0,
            );
        }

        while let Ok(event) = rx.try_recv() {
            if let AppEvent::Request(req) = event {
                state.push_request(req);
            }
        }

        assert_eq!(state.requests.len(), 10);
        assert_eq!(state.requests.back().unwrap().path, "/page14");
        assert_ne!(state.requests.front().unwrap().path, "/page0");
    }
}
