use crate::dev::ca;
use console::style;
use std::process::Command;

fn done(msg: &str) {
    println!("  {} {msg}", style("✓").green().bold());
}

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    println!();
    println!("  {}", style("xpo dev uninstall").bold());
    println!();

    step_remove_port_forwarding()?;
    step_untrust_ca()?;
    step_remove_ca()?;

    println!();
    println!("  {} Uninstalled.", style("✓").green().bold());
    println!();
    Ok(())
}

fn step_remove_port_forwarding() -> Result<(), Box<dyn std::error::Error>> {
    println!("  {} Removing port forwarding...", style("○").dim());
    remove_port_forwarding_platform()?;
    done("Port forwarding removed");
    Ok(())
}

fn step_untrust_ca() -> Result<(), Box<dyn std::error::Error>> {
    if !is_ca_trusted() {
        done("CA not in trust store");
        return Ok(());
    }

    println!("  {} Removing CA from trust store...", style("○").dim());
    untrust_ca_platform()?;
    done("CA removed from trust store");
    Ok(())
}

fn step_remove_ca() -> Result<(), Box<dyn std::error::Error>> {
    let dir = ca::ca_dir();
    if !dir.exists() {
        done("CA files not found");
        return Ok(());
    }

    std::fs::remove_dir_all(&dir)?;
    done("CA files deleted");
    Ok(())
}

// --- Check functions ---

#[cfg(target_os = "macos")]
fn is_ca_trusted() -> bool {
    use std::process::Stdio;
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

#[cfg(not(target_os = "macos"))]
fn is_ca_trusted() -> bool {
    std::path::Path::new("/usr/local/share/ca-certificates/xpo-ca.crt").exists()
        || std::path::Path::new("/etc/pki/ca-trust/source/anchors/xpo-ca.pem").exists()
}

// --- Remove functions ---

#[cfg(target_os = "macos")]
fn untrust_ca_platform() -> Result<(), Box<dyn std::error::Error>> {
    let status = Command::new("sudo")
        .args([
            "security",
            "delete-certificate",
            "-c",
            "xpo.sh Development CA",
            "/Library/Keychains/System.keychain",
        ])
        .status()?;

    if !status.success() {
        return Err("Failed to untrust CA".into());
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn untrust_ca_platform() -> Result<(), Box<dyn std::error::Error>> {
    use crate::dev::setup::linux_ca_paths;
    let (cert_path, update_cmd) = linux_ca_paths();

    let _ = Command::new("sudo")
        .args(["rm", "-f", cert_path])
        .output()?;

    let update_arg = if update_cmd == "update-ca-trust" {
        "extract"
    } else {
        "--fresh"
    };
    let _ = Command::new("sudo")
        .args([update_cmd, update_arg])
        .output()?;

    Ok(())
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn untrust_ca_platform() -> Result<(), Box<dyn std::error::Error>> {
    println!(
        "  {} Remove CA manually from your trust store",
        style("!").yellow().bold()
    );
    Ok(())
}

#[cfg(target_os = "macos")]
fn remove_port_forwarding_platform() -> Result<(), Box<dyn std::error::Error>> {
    use crate::dev::setup::pf_token_path;
    use std::process::Stdio;

    let token_path = pf_token_path();
    if let Ok(token_str) = std::fs::read_to_string(&token_path) {
        if let Ok(token) = token_str.trim().parse::<u64>() {
            let _ = Command::new("sudo")
                .args(["pfctl", "-X", &token.to_string()])
                .stderr(Stdio::null())
                .output();
        }
        let _ = std::fs::remove_file(&token_path);
    }

    let _ = Command::new("sudo")
        .args(["rm", "-f", "/etc/pf.anchors/com.xpo"])
        .stderr(Stdio::null())
        .output();

    let pf_conf = std::fs::read_to_string("/etc/pf.conf").unwrap_or_default();
    let needs_update = pf_conf.contains("com.xpo") || pf_conf.contains("# xpo-start");

    if !needs_update {
        return Ok(());
    }

    let new_conf = strip_xpo_from_pf_conf(&pf_conf);
    let tmp = std::env::temp_dir().join("xpo-pf.conf");
    std::fs::write(&tmp, &new_conf)?;

    let _ = Command::new("sudo")
        .args(["cp", &tmp.to_string_lossy(), "/etc/pf.conf"])
        .stderr(Stdio::null())
        .output();
    let _ = std::fs::remove_file(&tmp);

    let _ = Command::new("sudo")
        .args(["pfctl", "-f", "/etc/pf.conf"])
        .stderr(Stdio::null())
        .output();

    Ok(())
}

#[cfg(any(target_os = "macos", test))]
fn strip_xpo_from_pf_conf(pf_conf: &str) -> String {
    let mut new_lines = Vec::new();
    let mut skip = false;
    for line in pf_conf.lines() {
        if line.contains("# xpo-start") {
            skip = true;
            continue;
        }
        if line.contains("# xpo-end") {
            skip = false;
            continue;
        }
        if skip {
            continue;
        }
        if line.contains("rdr-anchor \"com.xpo\"") {
            continue;
        }
        if line.contains("load anchor \"com.xpo\"") {
            continue;
        }
        new_lines.push(line);
    }
    new_lines.join("\n") + "\n"
}

#[cfg(target_os = "linux")]
fn remove_port_forwarding_platform() -> Result<(), Box<dyn std::error::Error>> {
    for (from, to) in [(443, 10443), (80, 10080)] {
        let _ = Command::new("sudo")
            .args([
                "iptables",
                "-t",
                "nat",
                "-D",
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
            .output();
    }
    Ok(())
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn remove_port_forwarding_platform() -> Result<(), Box<dyn std::error::Error>> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_old_xpo_markers() {
        let pf = "\
rdr-anchor \"com.apple/*\"
# xpo-start
rdr pass on lo0 inet proto tcp from any to any port 443 -> 127.0.0.1 port 10443
# xpo-end
anchor \"com.apple/*\"";

        let result = strip_xpo_from_pf_conf(pf);
        assert!(!result.contains("# xpo-start"));
        assert!(!result.contains("# xpo-end"));
        assert!(!result.contains("port 10443"));
        assert!(result.contains("rdr-anchor \"com.apple/*\""));
        assert!(result.contains("anchor \"com.apple/*\""));
    }

    #[test]
    fn strip_anchor_lines() {
        let pf = "\
rdr-anchor \"com.apple/*\"
rdr-anchor \"com.xpo\"
anchor \"com.apple/*\"
load anchor \"com.xpo\" from \"/etc/pf.anchors/com.xpo\"
load anchor \"com.apple\" from \"/etc/pf.anchors/com.apple\"";

        let result = strip_xpo_from_pf_conf(pf);
        assert!(
            !result.contains("com.xpo"),
            "all com.xpo refs should be removed"
        );
        assert!(result.contains("com.apple"), "com.apple should remain");
    }

    #[test]
    fn strip_both_old_and_new() {
        let pf = "\
rdr-anchor \"com.apple/*\"
rdr-anchor \"com.xpo\"
# xpo-start
rdr pass on lo0 inet proto tcp from any to any port 443 -> 127.0.0.1 port 10443
# xpo-end
load anchor \"com.xpo\" from \"/etc/pf.anchors/com.xpo\"";

        let result = strip_xpo_from_pf_conf(pf);
        assert!(!result.contains("com.xpo"));
        assert!(!result.contains("# xpo-start"));
        assert!(!result.contains("port 10443"));
    }

    #[test]
    fn clean_pf_conf_unchanged() {
        let pf = "\
rdr-anchor \"com.apple/*\"
anchor \"com.apple/*\"
load anchor \"com.apple\" from \"/etc/pf.anchors/com.apple\"";

        let result = strip_xpo_from_pf_conf(pf);
        assert_eq!(result.trim(), pf.trim());
    }
}
