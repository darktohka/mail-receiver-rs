use std::fs;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

fn host() -> String {
    std::env::var("SMTP_HOST").unwrap_or_else(|_| "127.0.0.1".into())
}
fn port() -> u16 {
    std::env::var("SMTP_PORT").ok().and_then(|s| s.parse().ok()).unwrap_or(25)
}

fn collect_raw_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    if !dir.is_dir() {
        return files;
    }
    let mut stack = vec![dir.to_path_buf()];
    while let Some(cur) = stack.pop() {
        let Ok(entries) = fs::read_dir(&cur) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if let Some(ext) = path.extension() {
                if ext == "raw" {
                    files.push(path);
                }
            }
        }
    }
    files.sort();
    files
}

fn read_line(stream: &mut TcpStream) -> Result<String, String> {
    let mut line = Vec::new();
    let mut buf = [0u8; 1];
    loop {
        match stream.read(&mut buf) {
            Ok(0) => break,
            Ok(_) => {
                line.push(buf[0]);
                if buf[0] == b'\n' {
                    break;
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock
                || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                continue;
            }
            Err(e) => return Err(format!("read error: {e}")),
        }
    }
    Ok(String::from_utf8_lossy(&line).trim_end().to_string())
}

macro_rules! read_resp {
    ($s:expr) => { read_line(&mut $s)? };
}

macro_rules! cmd {
    ($s:expr, $fmt:expr $(, $a:expr)*) => {{
        writeln!($s, $fmt $(, $a)*).map_err(|e| e.to_string())?;
        read_line(&mut $s)?
    }};
}

fn extract_header_value<'a>(lines: &[&'a str], name: &str) -> Option<String> {
    let search = format!("{name}:");
    let mut i = 0;
    while i < lines.len() {
        if lines[i].to_uppercase().starts_with(&search.to_uppercase()) {
            let mut value = lines[i][search.len()..].trim().to_string();
            i += 1;
            while i < lines.len() && (lines[i].starts_with(' ') || lines[i].starts_with('\t')) {
                value.push(' ');
                value.push_str(lines[i].trim());
                i += 1;
            }
            return Some(value);
        }
        i += 1;
    }
    None
}

fn extract_addresses(value: &str) -> Vec<String> {
    let mut addrs = Vec::new();
    for part in value.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let email = if let Some(start) = part.find('<') {
            let after_open = &part[start + 1..];
            if let Some(end) = after_open.find('>') {
                after_open[..end].trim().to_string()
            } else {
                continue;
            }
        } else if part.contains('@') {
            part.to_string()
        } else {
            continue;
        };
        if !email.is_empty() {
            addrs.push(email);
        }
    }
    addrs
}

fn send_raw(path: &Path, raw: &[u8]) -> Result<String, String> {
    let text = String::from_utf8_lossy(raw);
    let lines: Vec<&str> = text.lines().collect();

    let from = extract_header_value(&lines, "From")
        .and_then(|v| extract_addresses(&v).into_iter().next())
        .unwrap_or_else(|| "unknown@localhost".to_string());

    let to_addrs = extract_header_value(&lines, "To")
        .map(|v| extract_addresses(&v))
        .unwrap_or_default();

    if to_addrs.is_empty() {
        return Err("no To: header found".to_string());
    }

    let addr = format!("{}:{}", host(), port());
    let mut stream = TcpStream::connect(&addr)
        .map_err(|e| format!("connect: {e}"))?;
    stream.set_read_timeout(Some(Duration::from_secs(30))).ok();
    stream.set_write_timeout(Some(Duration::from_secs(15))).ok();

    read_resp!(stream); // greeting

    // EHLO
    writeln!(stream, "EHLO raw-sender").map_err(|e| e.to_string())?;
    loop {
        let l = read_resp!(stream);
        if !l.starts_with("250-") {
            break;
        }
    }

    // MAIL FROM
    let r = cmd!(stream, "MAIL FROM:<{from}>");
    if !r.starts_with("250") {
        let _ = writeln!(stream, "RSET");
        return Err(format!("MAIL FROM rejected ({from}): {r}"));
    }

    // RCPT TO
    let mut accepted = false;
    for to in &to_addrs {
        let r = cmd!(stream, "RCPT TO:<{to}>");
        if r.starts_with("250") {
            accepted = true;
        }
    }
    if !accepted {
        let _ = writeln!(stream, "RSET");
        return Err(format!("all RCPT TO rejected: {to_addrs:?}"));
    }

    // DATA
    let r = cmd!(stream, "DATA");
    if !r.starts_with("354") {
        let _ = writeln!(stream, "RSET");
        return Err(format!("DATA not accepted: {r}"));
    }

    // Send raw content with dot-stuffing and \r\n line endings
    for line in text.lines() {
        if line == "." {
            // Bare dot: must be dot-stuffed
            write!(stream, "..\r\n").map_err(|e| e.to_string())?;
        } else if line.starts_with('.') {
            write!(stream, ".{line}\r\n").map_err(|e| e.to_string())?;
        } else {
            write!(stream, "{line}\r\n").map_err(|e| e.to_string())?;
        }
    }
    // DATA terminator
    write!(stream, ".\r\n").map_err(|e| e.to_string())?;

    let r = read_resp!(stream);
    let _ = writeln!(stream, "QUIT");
    let _ = read_resp!(stream);

    if r.starts_with("250") {
        Ok(r)
    } else {
        Err(format!("DATA rejected: {r}"))
    }
}

fn report(display: &Path, result: &Result<String, String>, ok: &mut usize, fail: &mut usize) {
    match result {
        Ok(msg) => {
            println!("  \x1b[32mOK\x1b[0m  {}  ({})", display.display(), msg.trim());
            *ok += 1;
        }
        Err(e) => {
            println!("  \x1b[31mFAIL\x1b[0m {}  {e}", display.display());
            *fail += 1;
        }
    }
}

fn main() {
    let arg = std::env::args().nth(1).unwrap_or_else(|| "realmail".to_string());
    let dir = Path::new(&arg);

    if !dir.is_dir() {
        eprintln!("error: {} is not a directory", dir.display());
        std::process::exit(1);
    }

    let files = collect_raw_files(dir);
    if files.is_empty() {
        eprintln!("No .raw files found in {}", dir.display());
        return;
    }

    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("  Raw Mail Sender — {}:{}", host(), port());
    println!("  Scanning: {}", dir.display());
    println!("  Found {} .raw file(s)", files.len());
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    let mut ok = 0usize;
    let mut fail = 0usize;
    let start = Instant::now();

    for file in &files {
        match fs::read(file) {
            Ok(raw) => {
                let result = send_raw(file, &raw);
                let display = file.strip_prefix("realmail").unwrap_or(file);
                report(display, &result, &mut ok, &mut fail);
            }
            Err(e) => {
                let display = file.strip_prefix("realmail").unwrap_or(file);
                println!("  \x1b[31mSKIP\x1b[0m {}  {e}", display.display());
                fail += 1;
            }
        }
    }

    let elapsed = start.elapsed();
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!(
        "  {} \x1b[32mOK\x1b[0m, {} \x1b[31mFAIL\x1b[0m  ({}.{:03}s)",
        ok,
        fail,
        elapsed.as_secs(),
        elapsed.subsec_millis(),
    );
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    if fail > 0 {
        std::process::exit(1);
    }
}
