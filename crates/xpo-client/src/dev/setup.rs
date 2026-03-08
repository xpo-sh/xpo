use crate::dev::ca;
use console::style;
use indicatif::{ProgressBar, ProgressStyle};
use std::process::{Command, Stdio};

fn spinner(msg: &str) -> ProgressBar {
    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::default_spinner()
            .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"])
            .template("  {spinner:.cyan} {msg}")
            .unwrap(),
    );
    pb.enable_steady_tick(std::time::Duration::from_millis(80));
    pb.set_message(msg.to_string());
    pb
}

fn done(msg: &str) {
    println!("  {} {msg}", style("✓").green().bold());
}

fn done_dim(msg: &str, detail: &str) {
    println!(
        "  {} {msg} {}",
        style("✓").green().bold(),
        style(detail).dim()
    );
}

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    println!();
    println!("  {}", style("xpo dev setup").bold());
    println!();

    step_generate_ca()?;
    step_trust_ca()?;
    step_port_forwarding()?;

    println!();
    println!(
        "  {} Run: {}",
        style("Setup complete!").green().bold(),
        style("xpo dev 3000 -n myapp").cyan()
    );
    println!();
    Ok(())
}

fn step_generate_ca() -> Result<(), Box<dyn std::error::Error>> {
    if ca::ca_exists() {
        done_dim("Root CA exists", &ca::ca_cert_path().display().to_string());
    } else {
        let sp = spinner("Generating root CA...");
        ca::generate_ca()?;
        sp.finish_and_clear();
        done_dim(
            "Root CA created (P-256 ECDSA, 10yr)",
            &ca::ca_cert_path().display().to_string(),
        );
    }
    Ok(())
}

fn step_trust_ca() -> Result<(), Box<dyn std::error::Error>> {
    if is_ca_trusted() {
        done("CA trusted in system keychain");
    } else {
        let sp = spinner("Trusting CA in system keychain (sudo)...");
        trust_ca_platform()?;
        sp.finish_and_clear();
        done("CA trusted in system keychain");
    }
    Ok(())
}

fn step_port_forwarding() -> Result<(), Box<dyn std::error::Error>> {
    if is_port_forwarding_active() {
        done_dim("Port forwarding active", "443→10443, 80→10080");
    } else {
        let sp = spinner("Setting up port forwarding (sudo)...");
        setup_port_forwarding_platform()?;
        sp.finish_and_clear();
        done_dim("Port forwarding active", "443→10443, 80→10080");
    }
    Ok(())
}

// --- Check functions (no sudo needed) ---

#[cfg(target_os = "macos")]
fn is_ca_trusted() -> bool {
    Command::new("security")
        .args([
            "find-certificate",
            "-c",
            "xpo.sh Development CA",
            "/Library/Keychains/System.keychain",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[cfg(target_os = "linux")]
fn is_ca_trusted() -> bool {
    std::path::Path::new("/usr/local/share/ca-certificates/xpo-ca.crt").exists()
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn is_ca_trusted() -> bool {
    false
}

#[cfg(target_os = "macos")]
fn is_port_forwarding_active() -> bool {
    let output = Command::new("pfctl")
        .args(["-a", "com.apple/xpo", "-s", "nat"])
        .stderr(Stdio::null())
        .output();

    match output {
        Ok(o) => {
            let stdout = String::from_utf8_lossy(&o.stdout);
            stdout.contains("port = 443") && stdout.contains("port = 80")
        }
        Err(_) => false,
    }
}

#[cfg(target_os = "linux")]
fn is_port_forwarding_active() -> bool {
    Command::new("sudo")
        .args([
            "iptables",
            "-t",
            "nat",
            "-C",
            "OUTPUT",
            "-o",
            "lo",
            "-p",
            "tcp",
            "--dport",
            "443",
            "-j",
            "REDIRECT",
            "--to-port",
            "10443",
        ])
        .stderr(Stdio::null())
        .stdout(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn is_port_forwarding_active() -> bool {
    false
}

// --- Setup functions (sudo needed) ---

#[cfg(target_os = "macos")]
fn trust_ca_platform() -> Result<(), Box<dyn std::error::Error>> {
    let output = Command::new("sudo")
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
        .output()?;

    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(format!("Failed to trust CA: {err}").into());
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn trust_ca_platform() -> Result<(), Box<dyn std::error::Error>> {
    let output = Command::new("sudo")
        .args([
            "cp",
            &ca::ca_cert_path().to_string_lossy(),
            "/usr/local/share/ca-certificates/xpo-ca.crt",
        ])
        .output()?;

    if !output.status.success() {
        return Err("Failed to copy CA cert".into());
    }

    let output = Command::new("sudo")
        .args(["update-ca-certificates"])
        .output()?;

    if !output.status.success() {
        return Err("Failed to update CA certificates".into());
    }
    Ok(())
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn trust_ca_platform() -> Result<(), Box<dyn std::error::Error>> {
    println!(
        "  {} Manual trust required: add {} to your trust store",
        style("!").yellow().bold(),
        ca::ca_cert_path().display()
    );
    Ok(())
}

#[cfg(target_os = "macos")]
fn setup_port_forwarding_platform() -> Result<(), Box<dyn std::error::Error>> {
    use std::io::Write;

    let rules = b"rdr pass on lo0 inet proto tcp from any to any port 443 -> 127.0.0.1 port 10443\nrdr pass on lo0 inet proto tcp from any to any port 80 -> 127.0.0.1 port 10080\n";

    let mut child = Command::new("sudo")
        .args(["pfctl", "-a", "com.apple/xpo", "-f", "-"])
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()?;

    child.stdin.take().unwrap().write_all(rules)?;

    let output = child.wait_with_output()?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(format!("Failed to load pf rules: {err}").into());
    }

    Ok(())
}

#[cfg(target_os = "linux")]
fn setup_port_forwarding_platform() -> Result<(), Box<dyn std::error::Error>> {
    for (from, to) in [(443, 10443), (80, 10080)] {
        let check = Command::new("sudo")
            .args([
                "iptables",
                "-t",
                "nat",
                "-C",
                "OUTPUT",
                "-o",
                "lo",
                "-p",
                "tcp",
                "--dport",
                &from.to_string(),
                "-j",
                "REDIRECT",
                "--to-port",
                &to.to_string(),
            ])
            .stderr(Stdio::null())
            .status()?;

        if !check.success() {
            let output = Command::new("sudo")
                .args([
                    "iptables",
                    "-t",
                    "nat",
                    "-A",
                    "OUTPUT",
                    "-o",
                    "lo",
                    "-p",
                    "tcp",
                    "--dport",
                    &from.to_string(),
                    "-j",
                    "REDIRECT",
                    "--to-port",
                    &to.to_string(),
                ])
                .output()?;

            if !output.status.success() {
                let err = String::from_utf8_lossy(&output.stderr);
                return Err(format!("Failed to add iptables rule for port {from}: {err}").into());
            }
        }
    }

    Ok(())
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn setup_port_forwarding_platform() -> Result<(), Box<dyn std::error::Error>> {
    println!(
        "  {} Port forwarding not supported on this platform",
        style("!").yellow().bold()
    );
    println!("     Proxy will listen on :10443 and :10080 directly");
    Ok(())
}
