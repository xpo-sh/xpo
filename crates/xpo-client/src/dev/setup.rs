use crate::dev::ca;
use std::io::Write;
use std::process::{Command, Stdio};

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    println!("  xpo dev setup\n");

    step_generate_ca()?;
    step_trust_ca()?;
    step_port_forwarding()?;

    println!("\n  Setup complete! Run: xpo dev 3000 -n myapp");
    Ok(())
}

fn step_generate_ca() -> Result<(), Box<dyn std::error::Error>> {
    if ca::ca_exists() {
        println!("  1. Root CA already exists");
        println!("     {}", ca::ca_cert_path().display());
    } else {
        println!("  1. Generating root CA...");
        ca::generate_ca()?;
        println!("     {}", ca::ca_cert_path().display());
        println!("     Root CA created (P-256 ECDSA, valid 10 years)");
    }
    Ok(())
}

fn step_trust_ca() -> Result<(), Box<dyn std::error::Error>> {
    println!("\n  2. Trusting CA in system keychain...");
    println!("     sudo required");

    trust_ca_platform()?;

    println!("     CA trusted");
    Ok(())
}

#[cfg(target_os = "macos")]
fn trust_ca_platform() -> Result<(), Box<dyn std::error::Error>> {
    let status = Command::new("sudo")
        .args([
            "security",
            "add-trusted-cert",
            "-d",
            "-r",
            "trustRoot",
            "-k",
            "/Library/Keychains/System.keychain",
        ])
        .arg(ca::ca_cert_path())
        .status()?;

    if !status.success() {
        return Err("Failed to trust CA in macOS keychain".into());
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn trust_ca_platform() -> Result<(), Box<dyn std::error::Error>> {
    let dest = "/usr/local/share/ca-certificates/xpo-ca.crt";

    let status = Command::new("sudo")
        .args(["cp", &ca::ca_cert_path().to_string_lossy(), dest])
        .status()?;

    if !status.success() {
        return Err("Failed to copy CA cert".into());
    }

    let status = Command::new("sudo")
        .args(["update-ca-certificates"])
        .status()?;

    if !status.success() {
        return Err("Failed to update CA certificates".into());
    }
    Ok(())
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn trust_ca_platform() -> Result<(), Box<dyn std::error::Error>> {
    println!("     Manual trust required: add {} to your trust store", ca::ca_cert_path().display());
    Ok(())
}

fn step_port_forwarding() -> Result<(), Box<dyn std::error::Error>> {
    println!("\n  3. Setting up port forwarding (443 -> 10443, 80 -> 10080)...");
    println!("     sudo required");

    setup_port_forwarding_platform()?;

    println!("     Port forwarding active");
    Ok(())
}

#[cfg(target_os = "macos")]
fn setup_port_forwarding_platform() -> Result<(), Box<dyn std::error::Error>> {
    let anchor = "\
rdr pass on lo0 inet proto tcp from any to any port 443 -> 127.0.0.1 port 10443
rdr pass on lo0 inet proto tcp from any to any port 80 -> 127.0.0.1 port 10080
";

    let mut child = Command::new("sudo")
        .args(["tee", "/etc/pf.anchors/xpo"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()?;

    child
        .stdin
        .take()
        .unwrap()
        .write_all(anchor.as_bytes())?;

    let status = child.wait()?;
    if !status.success() {
        return Err("Failed to write pf anchor".into());
    }

    let pf_conf = std::fs::read_to_string("/etc/pf.conf")?;
    if !pf_conf.contains("pf.anchors/xpo") {
        let mut lines: Vec<&str> = pf_conf.lines().collect();

        let insert_pos = lines
            .iter()
            .rposition(|l| l.starts_with("rdr-anchor") || l.starts_with("load anchor"))
            .map(|i| i + 1)
            .unwrap_or(lines.len());

        lines.insert(insert_pos, "rdr-anchor \"xpo\"");
        lines.insert(insert_pos + 1, "load anchor \"xpo\" from \"/etc/pf.anchors/xpo\"");

        let new_conf = lines.join("\n") + "\n";

        let mut child = Command::new("sudo")
            .args(["tee", "/etc/pf.conf"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .spawn()?;

        child
            .stdin
            .take()
            .unwrap()
            .write_all(new_conf.as_bytes())?;

        let status = child.wait()?;
        if !status.success() {
            return Err("Failed to update /etc/pf.conf".into());
        }
    }

    let status = Command::new("sudo")
        .args(["pfctl", "-ef", "/etc/pf.conf"])
        .stderr(Stdio::null())
        .status()?;

    if !status.success() {
        return Err("Failed to enable pfctl".into());
    }

    Ok(())
}

#[cfg(target_os = "linux")]
fn setup_port_forwarding_platform() -> Result<(), Box<dyn std::error::Error>> {
    for (from, to) in [(443, 10443), (80, 10080)] {
        let status = Command::new("sudo")
            .args([
                "iptables",
                "-t", "nat",
                "-C", "OUTPUT",
                "-o", "lo",
                "-p", "tcp",
                "--dport", &from.to_string(),
                "-j", "REDIRECT",
                "--to-port", &to.to_string(),
            ])
            .stderr(Stdio::null())
            .status()?;

        if !status.success() {
            let status = Command::new("sudo")
                .args([
                    "iptables",
                    "-t", "nat",
                    "-A", "OUTPUT",
                    "-o", "lo",
                    "-p", "tcp",
                    "--dport", &from.to_string(),
                    "-j", "REDIRECT",
                    "--to-port", &to.to_string(),
                ])
                .status()?;

            if !status.success() {
                return Err(format!("Failed to add iptables rule for port {from}").into());
            }
        }
    }

    Ok(())
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn setup_port_forwarding_platform() -> Result<(), Box<dyn std::error::Error>> {
    println!("     Port forwarding not supported on this platform");
    println!("     Proxy will listen on :10443 and :10080 directly");
    Ok(())
}
