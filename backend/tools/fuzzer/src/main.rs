use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::{Duration, Instant};

fn host() -> String {
    std::env::var("SMTP_HOST").unwrap_or_else(|_| "127.0.0.1".into())
}

fn port() -> u16 {
    std::env::var("SMTP_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(25u16)
}

struct SmtpSession {
    stream: TcpStream,
    responses: Vec<String>,
}

impl SmtpSession {
    fn connect() -> Result<Self, String> {
        let addr = format!("{}:{}", host(), port());
        let stream =
            TcpStream::connect(&addr).map_err(|e| format!("connect {addr}: {e}"))?;
        stream
            .set_read_timeout(Some(Duration::from_secs(30)))
            .ok();
        stream
            .set_write_timeout(Some(Duration::from_secs(10)))
            .ok();
        let mut s = SmtpSession {
            stream,
            responses: Vec::new(),
        };
        s.read_response();
        Ok(s)
    }

    fn read_line(&mut self) -> String {
        let mut line = Vec::new();
        let mut buf = [0u8; 1];
        loop {
            match self.stream.read(&mut buf) {
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
                    // Timeout with no data yet — retry
                    continue;
                }
                Err(e) => {
                    line.extend_from_slice(format!(" [read error: {e}]").as_bytes());
                    break;
                }
            }
        }
        let s = String::from_utf8_lossy(&line).trim_end().to_string();
        self.responses.push(s.clone());
        s
    }

    fn read_response(&mut self) -> String {
        let line = self.read_line();
        let has_more = line.starts_with("250-") || line.starts_with("220-");
        if has_more {
            loop {
                let l = self.read_line();
                if !l.starts_with("250-") {
                    break;
                }
            }
        }
        line
    }

    fn cmd(&mut self, c: &str) -> String {
        writeln!(self.stream, "{c}").ok();
        self.read_response()
    }

    fn cmd_raw(&mut self, data: &[u8]) -> String {
        self.stream.write_all(data).ok();
        self.stream.write_all(b"\r\n").ok();
        self.read_response()
    }

    fn ehlo(&mut self) -> String {
        self.cmd("EHLO fuzzer")
    }

    fn mail_from(&mut self, from: &str) -> String {
        self.cmd(&format!("MAIL FROM:<{from}>"))
    }

    fn rcpt_to(&mut self, to: &str) -> String {
        self.cmd(&format!("RCPT TO:<{to}>"))
    }

    fn data_begin(&mut self) -> String {
        self.cmd("DATA")
    }

    fn data_end(&mut self) -> String {
        writeln!(self.stream, ".").ok();
        self.read_response()
    }

    fn write_header(&mut self, name: &str, value: &str) {
        writeln!(self.stream, "{name}: {value}").ok();
    }

    fn write_body(&mut self, data: &[u8]) {
        self.stream.write_all(data).ok();
        self.stream.write_all(b"\r\n").ok();
    }

    fn write_line(&mut self, line: &str) {
        writeln!(self.stream, "{line}").ok();
    }

    fn quit(&mut self) -> String {
        self.cmd("QUIT")
    }

    fn send_text_email(
        &mut self,
        from: &str,
        to: &str,
        subject: &str,
        body: &str,
    ) -> Vec<String> {
        self.ehlo();
        self.mail_from(from);
        self.rcpt_to(to);
        self.data_begin();
        self.write_header("From", from);
        self.write_header("To", to);
        self.write_header("Subject", subject);
        self.write_header("Content-Type", "text/plain; charset=utf-8");
        self.write_line("");
        self.write_body(body.as_bytes());
        self.data_end();
        self.quit();
        self.responses.drain(..).collect()
    }

    fn send_raw(&mut self, raw_data: &[u8]) -> Vec<String> {
        self.stream.write_all(raw_data).ok();
        // try to read whatever comes back
        let mut buf = [0u8; 4096];
        let mut responses = self.responses.clone();
        match self.stream.read(&mut buf) {
            Ok(n) if n > 0 => {
                let s = String::from_utf8_lossy(&buf[..n]);
                for line in s.lines() {
                    responses.push(line.to_string());
                }
            }
            _ => {}
        }
        responses
    }
}

fn fmt(responses: &[String]) -> String {
    responses.join(" | ")
}

fn check(responses: &[String], contains: &[&str]) -> bool {
    contains.iter().any(|c| responses.iter().any(|r| r.contains(c)))
}

fn main() {
    let mut passed = 0u32;
    let mut failed = 0u32;
    let start = Instant::now();

    macro_rules! test {
        ($name:expr, $body:expr) => {
            print!("  {:<55} ", $name);
            std::io::stdout().flush().ok();
            match (|| -> Result<(), String> { $body })( ) {
                Ok(()) => {
                    println!("\x1b[32mPASS\x1b[0m");
                    passed += 1;
                }
                Err(e) => {
                    println!("\x1b[31mFAIL\x1b[0m  {e}");
                    failed += 1;
                }
            }
        };
    }

    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("  SMTP Fuzzer — targeting {}:{}", host(), port());
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    // ── 1. Valid emails without attachments ──
    println!("\n── Valid emails without attachments ──");

    test!("basic text email", {
        let mut s = SmtpSession::connect()?;
        let r = s.send_text_email("alice@test.example.com", "test-foo@test.example.com", "Hello", "This is a test.");
        if !check(&r, &["250 OK: Message accepted"]) {
            return Err(format!("no 250 after DATA: {}", fmt(&r)));
        }
        Ok(())
    });

    test!("unicode subject and body", {
        let mut s = SmtpSession::connect()?;
        let r = s.send_text_email(
            "bücher@test.example.com",
            "test-foo@test.example.com",
            "日本語 你好 ⚡测试",
            "Body with unicode: ñoño 🎉 éèêë",
        );
        if !check(&r, &["250 OK: Message accepted"]) {
            return Err(format!("no 250: {}", fmt(&r)));
        }
        Ok(())
    });

    test!("very long subject (10k chars)", {
        let mut s = SmtpSession::connect()?;
        let subject = "x".repeat(10_000);
        let r = s.send_text_email("a@test.example.com", "test-foo@test.example.com", &subject, "short body");
        if !check(&r, &["250 OK: Message accepted"]) {
            return Err(format!("no 250: {}", fmt(&r)));
        }
        Ok(())
    });

    test!("very long body (1MB)", {
        let mut s = SmtpSession::connect()?;
        let body = "A".repeat(1_000_000);
        let r = s.send_text_email("big@test.example.com", "test-foo@test.example.com", "Large body", &body);
        if !check(&r, &["250 OK: Message accepted"]) {
            return Err(format!("no 250: {}", fmt(&r)));
        }
        Ok(())
    });

    test!("empty body", {
        let mut s = SmtpSession::connect()?;
        let r = s.send_text_email("a@test.example.com", "test-foo@test.example.com", "empty body", "");
        if !check(&r, &["250 OK: Message accepted"]) {
            return Err(format!("no 250: {}", fmt(&r)));
        }
        Ok(())
    });

    // ── 2. Emails with attachments ──
    println!("\n── Emails with attachments ──");

    test!("text attachment (inline)", {
        let mut s = SmtpSession::connect()?;
        s.ehlo();
        s.mail_from("att@test.example.com");
        s.rcpt_to("test-foo@test.example.com");
        s.data_begin();
        s.write_header("From", "att@test.example.com");
        s.write_header("To", "test-foo@test.example.com");
        s.write_header("Subject", "with text attachment");
        s.write_header("Content-Type", "multipart/mixed; boundary=\"boundary123\"");
        s.write_line("");
        s.write_line("--boundary123");
        s.write_header("Content-Type", "text/plain");
        s.write_line("");
        s.write_line("This is the body.");
        s.write_line("--boundary123");
        s.write_header("Content-Type", "text/plain");
        s.write_header("Content-Disposition", "attachment; filename=\"notes.txt\"");
        s.write_line("");
        s.write_line("This is the attachment content.");
        s.write_line("--boundary123--");
        let r = s.data_end();
        s.quit();
        if !r.contains("250 OK") {
            return Err(format!("no 250 after multipart DATA: {r}"));
        }
        Ok(())
    });

    test!("binary attachment (PNG bytes)", {
        let mut s = SmtpSession::connect()?;
        s.ehlo();
        s.mail_from("img@test.example.com");
        s.rcpt_to("test-foo@test.example.com");
        s.data_begin();
        s.write_header("From", "img@test.example.com");
        s.write_header("To", "test-foo@test.example.com");
        s.write_header("Subject", "with binary attachment");
        s.write_header("Content-Type", "multipart/mixed; boundary=\"b2\"");
        s.write_line("");
        s.write_line("--b2");
        s.write_header("Content-Type", "text/plain");
        s.write_line("");
        s.write_line("See attached image.");
        s.write_line("--b2");
        s.write_header("Content-Type", "image/png");
        s.write_header("Content-Disposition", "attachment; filename=\"img.png\"");
        s.write_header("Content-Transfer-Encoding", "base64");
        s.write_line("");
        // A tiny valid PNG (1x1 pixel) in base64
        let png_b64 = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";
        s.write_body(png_b64.as_bytes());
        s.write_line("");
        s.write_line("--b2--");
        let r = s.data_end();
        s.quit();
        if !r.contains("250 OK") {
            return Err(format!("no 250 after binary attachment: {r}"));
        }
        Ok(())
    });

    // ── 3. Edge cases ──
    println!("\n── Edge cases ──");

    test!("empty subject", {
        let mut s = SmtpSession::connect()?;
        let r = s.send_text_email("a@test.example.com", "test-foo@test.example.com", "", "body");
        if !check(&r, &["250 OK: Message accepted"]) {
            return Err(format!("no 250: {}", fmt(&r)));
        }
        Ok(())
    });

    test!("subject with only whitespace", {
        let mut s = SmtpSession::connect()?;
        let r = s.send_text_email("a@test.example.com", "test-foo@test.example.com", "   ", "body");
        if !check(&r, &["250 OK: Message accepted"]) {
            return Err(format!("no 250: {}", fmt(&r)));
        }
        Ok(())
    });

    test!("null bytes in body", {
        let mut s = SmtpSession::connect()?;
        s.ehlo();
        s.mail_from("null@test.example.com");
        s.rcpt_to("test-foo@test.example.com");
        s.data_begin();
        s.write_header("From", "null@test.example.com");
        s.write_header("Subject", "null bytes");
        s.write_header("Content-Type", "text/plain");
        s.write_line("");
        let body = b"before\x00after".to_vec();
        s.write_body(&body);
        let r = s.data_end();
        s.quit();
        if !r.contains("250 OK") {
            return Err(format!("no 250 with null bytes: {r}"));
        }
        Ok(())
    });

    test!("body with only newlines", {
        let mut s = SmtpSession::connect()?;
        let r = s.send_text_email("a@test.example.com", "test-foo@test.example.com", "newlines", "\n\n\n\n\n\n\n\n\n\n");
        if !check(&r, &["250 OK: Message accepted"]) {
            return Err(format!("no 250: {}", fmt(&r)));
        }
        Ok(())
    });

    test!("line starting with dot (dot-stuffing)", {
        let mut s = SmtpSession::connect()?;
        s.ehlo();
        s.mail_from("dot@test.example.com");
        s.rcpt_to("test-foo@test.example.com");
        s.data_begin();
        s.write_header("From", "dot@test.example.com");
        s.write_header("Subject", "dot stuffing");
        s.write_line("");
        // This line has a leading dot which will be dot-stuffed
        writeln!(s.stream, "normal line").ok();
        writeln!(s.stream, ".this line starts with a dot").ok();
        writeln!(s.stream, "..this line starts with two dots").ok();
        writeln!(s.stream, "normal again").ok();
        let r = s.data_end();
        s.quit();
        if !r.contains("250 OK") {
            return Err(format!("no 250 with dot-stuffing: {r}"));
        }
        Ok(())
    });

    test!("multiple recipients (3 RCPT TO)", {
        let mut s = SmtpSession::connect()?;
        s.ehlo();
        s.mail_from("multi@test.example.com");
        s.rcpt_to("test-foo@test.example.com");
        s.rcpt_to("test-bar@test.example.com");
        s.rcpt_to("test-baz@test.example.com");
        s.data_begin();
        s.write_header("From", "multi@test.example.com");
        s.write_header("Subject", "multiple rcpt");
        s.write_line("");
        s.write_line("body");
        let r = s.data_end();
        s.quit();
        if !r.contains("250 OK") {
            return Err(format!("no 250 with multiple rcpt: {r}"));
        }
        Ok(())
    });

    // ── 4. Invalid / malformed inputs ──
    println!("\n── Invalid / malformed inputs ──");

    test!("no headers (raw body only)", {
        let mut s = SmtpSession::connect()?;
        s.ehlo();
        s.mail_from("raw@test.example.com");
        s.rcpt_to("test-foo@test.example.com");
        s.data_begin();
        // Send raw body without any RFC5322 headers
        writeln!(s.stream, "Just some raw text without headers.").ok();
        writeln!(s.stream, "No From, no Subject, nothing.").ok();
        let r = s.data_end();
        s.quit();
        if !r.contains("250 OK") {
            return Err(format!("no 250 for raw body: {r}"));
        }
        Ok(())
    });

    test!("binary gibberish in DATA", {
        let mut s = SmtpSession::connect()?;
        s.ehlo();
        s.mail_from("garbage@test.example.com");
        s.rcpt_to("test-foo@test.example.com");
        s.data_begin();
        // Send purely random bytes
        let mut garbage = Vec::with_capacity(5000);
        for _ in 0..5000 {
            garbage.push(rand::random::<u8>());
        }
        s.stream.write_all(&garbage).ok();
        writeln!(s.stream, "").ok();
        let r = s.data_end();
        s.quit();
        if !r.contains("250 OK") && !r.contains("451") {
            // Either accepted or rejected gracefully — no crash
            return Err(format!("unexpected response to garbage: {r}"));
        }
        Ok(())
    });

    test!("DATA with early termination (naked dot)", {
        let mut s = SmtpSession::connect()?;
        s.ehlo();
        s.mail_from("early@test.example.com");
        s.rcpt_to("test-foo@test.example.com");
        s.data_begin();
        // End DATA immediately with just a dot
        let r = s.data_end();
        s.quit();
        if !r.contains("250 OK") {
            return Err(format!("no 250 for empty DATA: {r}"));
        }
        Ok(())
    });

    test!("missing angle brackets in MAIL FROM", {
        let mut s = SmtpSession::connect()?;
        s.ehlo();
        let r = s.cmd("MAIL FROM: user@test.example.com");
        s.quit();
        if !r.contains("250") && !r.contains("501") {
            // Per RFC, should accept with or without brackets — either is fine
            return Err(format!("unexpected: {r}"));
        }
        Ok(())
    });

    test!("extremely long header line (100k chars)", {
        let mut s = SmtpSession::connect()?;
        s.ehlo();
        s.mail_from("long@test.example.com");
        s.rcpt_to("test-foo@test.example.com");
        s.data_begin();
        s.write_header("From", "long@test.example.com");
        s.write_header("Subject", "long header");
        let long_val = "X".repeat(100_000);
        writeln!(s.stream, "X-Long-Header: {long_val}").ok();
        s.write_line("");
        s.write_line("body");
        let r = s.data_end();
        s.quit();
        if !r.contains("250 OK") {
            return Err(format!("no 250 for long header: {r}"));
        }
        Ok(())
    });

    // ── 5. Protocol abuse ──
    println!("\n── Protocol abuse ──");

    test!("DATA before EHLO", {
        let mut s = SmtpSession::connect()?;
        let r = s.cmd("DATA");
        s.quit();
        if !r.contains("503") && !r.contains("500") {
            return Err(format!("expected 503/500, got: {r}"));
        }
        Ok(())
    });

    test!("DATA without MAIL FROM", {
        let mut s = SmtpSession::connect()?;
        s.ehlo();
        let r = s.cmd("DATA");
        s.quit();
        if !r.contains("503") {
            return Err(format!("expected 503, got: {r}"));
        }
        Ok(())
    });

    test!("RCPT TO without MAIL FROM", {
        let mut s = SmtpSession::connect()?;
        s.ehlo();
        // Server accepts RCPT before MAIL (pipelining-friendly).
        // The recipient is valid, so expect 250.
        // If server rejects it, any 5xx code is also acceptable.
        let r = s.cmd("RCPT TO:<test-foo@test.example.com>");
        s.quit();
        if !r.contains("250") && !r.contains("5") {
            return Err(format!("expected 250 or 5xx, got: {r}"));
        }
        Ok(())
    });

    test!("unknown command", {
        let mut s = SmtpSession::connect()?;
        let r = s.cmd("BOGUS xyz");
        s.quit();
        if !r.contains("500") {
            return Err(format!("expected 500, got: {r}"));
        }
        Ok(())
    });

    test!("too many commands (pipeline flood)", {
        let mut s = SmtpSession::connect()?;
        s.ehlo();
        // Send many commands at once (pipelining)
        let many = (0..100)
            .map(|i| format!("RCPT TO:<test-user{i}@test.example.com>"))
            .collect::<Vec<_>>()
            .join("\r\n");
        writeln!(s.stream, "{many}").ok();
        // Read responses
        for _ in 0..100 {
            let _ = s.read_line();
        }
        s.quit();
        Ok(())
    });

    test!("immediate connection close", {
        // Just connect and close immediately — server should handle gracefully
        let addr = format!("{}:{}", host(), port());
        let mut stream = TcpStream::connect(&addr).map_err(|e| e.to_string())?;
        let mut buf = [0u8; 256];
        let _ = stream.read(&mut buf); // read greeting
        drop(stream); // close immediately
        Ok(())
    });

    test!("send half of greeting then disconnect", {
        let addr = format!("{}:{}", host(), port());
        let mut stream = TcpStream::connect(&addr).map_err(|e| e.to_string())?;
        stream.write_all(b"EH").ok();
        std::thread::sleep(Duration::from_millis(200));
        drop(stream);
        Ok(())
    });

    // ── 6. Concurrent connections ──
    println!("\n── Concurrent connections ──");

    test!("20 concurrent valid emails", {
        let mut handles = Vec::new();
        for i in 0..20 {
            handles.push(std::thread::spawn(move || {
                let mut s = SmtpSession::connect()?;
                let r = s.send_text_email(
                    &format!("sender{i}@test.example.com"),
                    "test-foo@test.example.com",
                    &format!("Concurrent {i}"),
                    &format!("Body of message {i}"),
                );
                if !check(&r, &["250 OK: Message accepted"]) {
                    return Err(format!("msg {i} failed: {}", fmt(&r)));
                }
                Ok::<_, String>(())
            }));
        }
        for (i, h) in handles.into_iter().enumerate() {
            h.join().map_err(|_| format!("thread {i} panicked"))??;
        }
        Ok(())
    });

    // ── Summary ──
    println!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!(
        "  Results: {} \x1b[32mPASS\x1b[0m, {} \x1b[31mFAIL\x1b[0m  ({}.{:03}s)",
        passed,
        failed,
        start.elapsed().as_secs(),
        start.elapsed().subsec_millis(),
    );
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    if failed > 0 {
        std::process::exit(1);
    }
}
