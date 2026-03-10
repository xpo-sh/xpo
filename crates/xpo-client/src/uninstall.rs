pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    println!(
        "\n  {} This will remove all xpo data from your system.\n",
        console::style("⚠").yellow().bold()
    );

    let items = vec!["Yes, uninstall xpo", "No, cancel"];
    let selection = dialoguer::Select::with_theme(&dialoguer::theme::ColorfulTheme::default())
        .with_prompt("  Are you sure?")
        .items(&items)
        .default(1)
        .interact()
        .unwrap_or(1);

    if selection != 0 {
        println!("  {} Cancelled.", console::style("→").dim());
        return Ok(());
    }

    println!();

    if crate::dev::ca::ca_exists() {
        print_step("Removing dev setup (CA, trust, port forwarding)...");
        match crate::dev::uninstall::run() {
            Ok(()) => print_done("Dev setup removed"),
            Err(e) => print_warn(&format!("Dev cleanup partial: {e}")),
        }
    } else {
        print_skip("No dev setup found");
    }

    let xpo_dir = xpo_core::config::Config::dir();
    if xpo_dir.exists() {
        print_step("Removing ~/.xpo/ ...");
        std::fs::remove_dir_all(&xpo_dir)?;
        print_done("~/.xpo/ removed");
    } else {
        print_skip("~/.xpo/ not found");
    }

    let exe = std::env::current_exe().ok();
    if let Some(ref exe_path) = exe {
        if is_direct_install(exe_path) {
            print_step(&format!("Removing binary ({})...", exe_path.display()));
            if remove_binary(exe_path) {
                print_done(&format!("Binary removed ({})", exe_path.display()));
            } else {
                print_warn(&format!(
                    "Could not remove binary. Run manually:\n    sudo rm {}",
                    exe_path.display()
                ));
            }
        } else {
            print_skip("Binary installed via package manager, skipping removal");
            println!(
                "    {}",
                console::style("Run: brew uninstall xpo / cargo uninstall xpo / npm uninstall -g @xposh/cli").dim()
            );
        }
    }

    println!(
        "\n  {} {}\n",
        console::style("✓").green().bold(),
        console::style("xpo has been uninstalled.").bold()
    );

    Ok(())
}

fn print_step(msg: &str) {
    println!("  {} {msg}", console::style("○").dim());
}

fn print_done(msg: &str) {
    println!("  {} {msg}", console::style("✓").green().bold());
}

fn print_warn(msg: &str) {
    println!("  {} {msg}", console::style("⚠").yellow().bold());
}

fn print_skip(msg: &str) {
    println!("  {} {msg}", console::style("–").dim());
}

fn is_direct_install(exe_path: &std::path::Path) -> bool {
    let resolved = exe_path.canonicalize().unwrap_or_else(|_| exe_path.to_path_buf());
    let path_str = resolved.to_string_lossy();

    let pkg_manager_paths = [
        "/homebrew/",
        "/Homebrew/",
        "/Cellar/",
        "/.cargo/bin/",
        "/lib/node_modules/",
    ];
    !pkg_manager_paths.iter().any(|p| path_str.contains(p))
}

fn remove_binary(exe_path: &std::path::Path) -> bool {
    if std::fs::remove_file(exe_path).is_ok() {
        return true;
    }

    std::process::Command::new("sudo")
        .args(["rm", "-f"])
        .arg(exe_path)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}
