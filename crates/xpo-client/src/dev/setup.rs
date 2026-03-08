use crate::dev::ca;
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
    if is_ca_trusted() {
        println!("  2. CA already trusted");
    } else {
        println!("  2. Trusting CA in system keychain...");
        println!("     sudo required");
        trust_ca_platform()?;
        println!("     CA trusted");
    }
    Ok(())
}

fn step_port_forwarding() -> Result<(), Box<dyn std::error::Error>> {
    if is_port_forwarding_active() {
        println!("  3. Port forwarding already active");
    } else {
        println!("  3. Setting up port forwarding (443 -> 10443, 80 -> 10080)...");
        println!("     sudo required");
        setup_port_forwarding_platform()?;
        println!("     Port forwarding active");
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
        "     Manual trust required: add {} to your trust store",
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
    println!("     Port forwarding not supported on this platform");
    println!("     Proxy will listen on :10443 and :10080 directly");
    Ok(())
}
