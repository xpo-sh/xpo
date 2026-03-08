use std::process::{Command, Stdio};

pub fn add(domain: &str) -> Result<(), Box<dyn std::error::Error>> {
    let hosts = std::fs::read_to_string("/etc/hosts")?;
    let entry = format!("127.0.0.1 {domain}");

    if hosts.lines().any(|l| l.contains(&entry)) {
        return Ok(());
    }

    let line = format!("127.0.0.1 {domain} # xpo\n");

    use std::io::Write;
    let mut child = Command::new("sudo")
        .args(["tee", "-a", "/etc/hosts"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()?;

    child.stdin.take().unwrap().write_all(line.as_bytes())?;

    let status = child.wait()?;
    if !status.success() {
        return Err("Failed to add /etc/hosts entry".into());
    }

    Ok(())
}

pub fn remove(domain: &str) -> Result<(), Box<dyn std::error::Error>> {
    let hosts = std::fs::read_to_string("/etc/hosts")?;
    let entry = format!("127.0.0.1 {domain}");

    if !hosts.lines().any(|l| l.contains(&entry)) {
        return Ok(());
    }

    let filtered: Vec<&str> = hosts.lines().filter(|l| !l.contains(&entry)).collect();

    let new_hosts = filtered.join("\n") + "\n";

    use std::io::Write;
    let mut child = Command::new("sudo")
        .args(["tee", "/etc/hosts"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()?;

    child
        .stdin
        .take()
        .unwrap()
        .write_all(new_hosts.as_bytes())?;

    let status = child.wait()?;
    if !status.success() {
        return Err("Failed to update /etc/hosts".into());
    }

    Ok(())
}
