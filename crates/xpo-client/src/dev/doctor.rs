use crate::dev::{ca, setup};
use console::style;

fn pass(msg: &str) {
    println!("  {} {msg}", style("✓").green().bold());
}

fn warn(msg: &str) {
    println!("  {} {msg}", style("!").yellow().bold());
}

fn fail(msg: &str) {
    println!("  {} {msg}", style("✗").red().bold());
}

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    println!();
    println!("  {}", style("xpo dev doctor").bold());
    println!();

    let mut all_pass = true;

    if ca::ca_exists() {
        pass("Root CA");
    } else {
        fail("Root CA");
        all_pass = false;
    }

    if setup::is_ca_trusted() {
        pass("CA trusted");
    } else {
        fail("CA not trusted");
        all_pass = false;
    }

    check_port_forwarding(&mut all_pass);
    check_port_reachable(&mut all_pass);
    check_hosts_entries();

    println!();
    if all_pass {
        println!("  {} All checks passed", style("✓").green().bold());
    } else {
        println!(
            "  {} Issues found {}",
            style("!").yellow().bold(),
            style("(auto-fixed on next xpo dev)").dim()
        );
    }
    println!();

    Ok(())
}

#[cfg(target_os = "macos")]
fn check_port_forwarding(all_pass: &mut bool) {
    if std::path::Path::new("/etc/pf.anchors/com.xpo").exists() {
        pass("Anchor file");
    } else {
        fail("Anchor file missing");
        *all_pass = false;
    }

    let pf_configured = std::fs::read_to_string("/etc/pf.conf")
        .map(|c| c.contains("rdr-anchor \"com.xpo\""))
        .unwrap_or(false);
    if pf_configured {
        pass("pf.conf");
    } else {
        fail("pf.conf missing anchor");
        *all_pass = false;
    }
}

#[cfg(target_os = "linux")]
fn check_port_forwarding(all_pass: &mut bool) {
    if setup::is_port_forwarding_active() {
        pass("iptables forwarding");
    } else {
        fail("Port forwarding not active");
        *all_pass = false;
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn check_port_forwarding(_all_pass: &mut bool) {
    warn("Port forwarding N/A");
}

fn check_port_reachable(all_pass: &mut bool) {
    let reachable = std::net::TcpStream::connect_timeout(
        &std::net::SocketAddr::from(([127, 0, 0, 1], 443)),
        std::time::Duration::from_millis(500),
    )
    .is_ok();

    if reachable {
        pass("Port 443 reachable");
    } else {
        warn("Port 443 not reachable");
        *all_pass = false;
    }
}

fn check_hosts_entries() {
    let hosts = std::fs::read_to_string("/etc/hosts").unwrap_or_default();
    let xpo_entries: Vec<&str> = hosts.lines().filter(|l| l.ends_with("# xpo")).collect();

    if xpo_entries.is_empty() {
        println!("  {} No /etc/hosts entries", style("·").dim());
    } else {
        let domains: Vec<&str> = xpo_entries
            .iter()
            .filter_map(|l| l.split_whitespace().nth(1))
            .collect();
        pass(&format!("/etc/hosts: {}", domains.join(", ")));
    }
}
