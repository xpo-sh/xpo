use crate::dev::{ca, setup};
use xpo_tui::widgets::doctor::{render_doctor_table, CheckStatus, DoctorCheck};

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut checks = Vec::new();

    checks.push(DoctorCheck {
        name: "Root CA".to_string(),
        status: if ca::ca_exists() {
            CheckStatus::Pass
        } else {
            CheckStatus::Fail
        },
        detail: String::new(),
    });

    checks.push(DoctorCheck {
        name: "CA trusted".to_string(),
        status: if setup::is_ca_trusted() {
            CheckStatus::Pass
        } else {
            CheckStatus::Fail
        },
        detail: String::new(),
    });

    collect_port_forwarding_checks(&mut checks);
    collect_port_reachable_check(&mut checks);
    collect_hosts_check(&mut checks);

    render_doctor_table(&checks)?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn collect_port_forwarding_checks(checks: &mut Vec<DoctorCheck>) {
    let anchor_exists = std::path::Path::new("/etc/pf.anchors/com.xpo").exists();
    checks.push(DoctorCheck {
        name: "Anchor file".to_string(),
        status: if anchor_exists {
            CheckStatus::Pass
        } else {
            CheckStatus::Fail
        },
        detail: if anchor_exists {
            String::new()
        } else {
            "missing".to_string()
        },
    });

    let pf_configured = std::fs::read_to_string("/etc/pf.conf")
        .map(|c| c.contains("rdr-anchor \"com.xpo\""))
        .unwrap_or(false);
    checks.push(DoctorCheck {
        name: "pf.conf".to_string(),
        status: if pf_configured {
            CheckStatus::Pass
        } else {
            CheckStatus::Fail
        },
        detail: if pf_configured {
            String::new()
        } else {
            "missing anchor".to_string()
        },
    });
}

#[cfg(target_os = "linux")]
fn collect_port_forwarding_checks(checks: &mut Vec<DoctorCheck>) {
    let active = setup::is_port_forwarding_active();
    checks.push(DoctorCheck {
        name: "iptables forwarding".to_string(),
        status: if active {
            CheckStatus::Pass
        } else {
            CheckStatus::Fail
        },
        detail: if active {
            String::new()
        } else {
            "not active".to_string()
        },
    });
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn collect_port_forwarding_checks(checks: &mut Vec<DoctorCheck>) {
    checks.push(DoctorCheck {
        name: "Port forwarding".to_string(),
        status: CheckStatus::Warn,
        detail: "N/A on this platform".to_string(),
    });
}

fn collect_port_reachable_check(checks: &mut Vec<DoctorCheck>) {
    let reachable = std::net::TcpStream::connect_timeout(
        &std::net::SocketAddr::from(([127, 0, 0, 1], 443)),
        std::time::Duration::from_millis(500),
    )
    .is_ok();

    checks.push(DoctorCheck {
        name: "Port 443 reachable".to_string(),
        status: if reachable {
            CheckStatus::Pass
        } else {
            CheckStatus::Warn
        },
        detail: if reachable {
            String::new()
        } else {
            "not reachable".to_string()
        },
    });
}

fn collect_hosts_check(checks: &mut Vec<DoctorCheck>) {
    let hosts = std::fs::read_to_string("/etc/hosts").unwrap_or_default();
    let xpo_entries: Vec<&str> = hosts.lines().filter(|l| l.ends_with("# xpo")).collect();

    if xpo_entries.is_empty() {
        checks.push(DoctorCheck {
            name: "/etc/hosts".to_string(),
            status: CheckStatus::Warn,
            detail: "no entries".to_string(),
        });
    } else {
        let domains: Vec<&str> = xpo_entries
            .iter()
            .filter_map(|l| l.split_whitespace().nth(1))
            .collect();
        checks.push(DoctorCheck {
            name: "/etc/hosts".to_string(),
            status: CheckStatus::Pass,
            detail: domains.join(", "),
        });
    }
}
