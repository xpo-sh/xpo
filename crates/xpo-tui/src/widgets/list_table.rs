use std::io;

pub struct ListRow {
    pub kind: String,
    pub domain: String,
    pub target: String,
    pub status: String,
}

fn color(r: u8, g: u8, b: u8) -> String {
    format!("\x1b[38;2;{r};{g};{b}m")
}

pub fn render_list_table(rows: &[ListRow]) -> io::Result<()> {
    let r = "\x1b[0m";
    let b = "\x1b[1m";
    let bd = color(72, 79, 88);
    let tx = color(230, 237, 243);
    let dm = color(139, 148, 158);
    let ac = color(88, 166, 255);
    let ok = color(126, 231, 135);
    let er = color(248, 81, 73);
    let wn = color(224, 175, 104);

    let tw = rows.iter().map(|r| r.kind.len()).max().unwrap_or(4).max(4);
    let dw = rows
        .iter()
        .map(|r| r.domain.len())
        .max()
        .unwrap_or(6)
        .max(6);
    let gw = rows
        .iter()
        .map(|r| r.target.len())
        .max()
        .unwrap_or(6)
        .max(6);

    let content_width = 1 + tw + 2 + dw + 2 + gw + 2 + 8 + 1;
    let hr = "\u{2500}".repeat(content_width);

    println!();
    println!("  {bd}\u{256d}{hr}\u{256e}{r}");

    let title = "xpo list";
    let title_pad = content_width - 1 - title.len();
    println!(
        "  {bd}\u{2502}{r} {ac}{b}{title}{r}{}{bd}\u{2502}{r}",
        " ".repeat(title_pad)
    );

    println!("  {bd}\u{251c}{hr}\u{2524}{r}");

    let hdr_pad = content_width - 1 - tw - 2 - dw - 2 - gw - 2 - 8;
    println!(
        "  {bd}\u{2502}{r} {dm}{:<tw$}  {:<dw$}  {:<gw$}  {:<8}{r}{}{bd}\u{2502}{r}",
        "TYPE",
        "DOMAIN",
        "TARGET",
        "STATUS",
        " ".repeat(hdr_pad),
    );

    for row in rows {
        let (icon, ic) = match row.status.as_str() {
            "active" => ("\u{25cf} active", &ok),
            "inactive" => ("\u{25cb} inact.", &er),
            _ => ("? unkn.", &wn),
        };
        let row_pad = content_width - 1 - tw - 2 - dw - 2 - gw - 2 - 8;
        let kind_color = match row.kind.as_str() {
            "share" => &ac,
            "dev" => &wn,
            _ => &tx,
        };
        println!(
            "  {bd}\u{2502}{r} {kind_color}{:<tw$}{r}  {tx}{:<dw$}{r}  {dm}{:<gw$}{r}  {ic}{:<8}{r}{}{bd}\u{2502}{r}",
            row.kind,
            row.domain,
            row.target,
            icon,
            " ".repeat(row_pad),
        );
    }

    let summary = format!(
        "{} tunnel(s), {} dev domain(s)",
        rows.iter().filter(|r| r.kind == "share").count(),
        rows.iter().filter(|r| r.kind == "dev").count(),
    );
    println!("  {bd}\u{251c}{hr}\u{2524}{r}");
    let sum_pad = content_width - 1 - summary.len();
    println!(
        "  {bd}\u{2502}{r} {dm}{summary}{r}{}{bd}\u{2502}{r}",
        " ".repeat(sum_pad),
    );

    println!("  {bd}\u{2570}{hr}\u{256f}{r}");
    println!();

    Ok(())
}

pub fn render_empty() {
    let r = "\x1b[0m";
    let b = "\x1b[1m";
    let bd = color(72, 79, 88);
    let ac = color(88, 166, 255);
    let dm = color(139, 148, 158);

    let content_width = 42;
    let hr = "\u{2500}".repeat(content_width);

    println!();
    println!("  {bd}\u{256d}{hr}\u{256e}{r}");

    let title = "xpo list";
    let title_pad = content_width - 1 - title.len();
    println!(
        "  {bd}\u{2502}{r} {ac}{b}{title}{r}{}{bd}\u{2502}{r}",
        " ".repeat(title_pad)
    );

    println!("  {bd}\u{251c}{hr}\u{2524}{r}");

    let msg = "No active tunnels or dev domains";
    let msg_pad = content_width - 1 - msg.len();
    println!(
        "  {bd}\u{2502}{r} {dm}{msg}{r}{}{bd}\u{2502}{r}",
        " ".repeat(msg_pad),
    );

    println!("  {bd}\u{2570}{hr}\u{256f}{r}");
    println!();
}
