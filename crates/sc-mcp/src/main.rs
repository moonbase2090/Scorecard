use std::io::{self, BufRead, Write};

fn main() {
    if std::env::args().nth(1).as_deref() == Some("--version") {
        println!("{}", env!("CARGO_PKG_VERSION"));
        return;
    }
    let stdin = io::stdin();
    let mut stdin = stdin.lock();
    let mut stdout = io::stdout().lock();
    loop {
        let Some((frame, message)) = read_message(&mut stdin) else {
            break;
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&message) else {
            continue;
        };
        let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        if let Some(response) = sc_mcp::handle(&value, &cwd) {
            let body = response.to_string();
            write_message(&mut stdout, frame, &body);
            let _ = stdout.flush();
        }
    }
}

fn write_message(stdout: &mut impl Write, frame: Frame, body: &str) {
    match frame {
        Frame::Line => {
            let _ = writeln!(stdout, "{body}");
        }
        Frame::ContentLength => {
            let _ = write!(stdout, "Content-Length: {}\r\n\r\n{body}", body.len());
        }
    }
}

enum Frame {
    Line,
    ContentLength,
}

fn read_message(stdin: &mut impl BufRead) -> Option<(Frame, String)> {
    let mut header = String::new();
    stdin.read_line(&mut header).ok()?;
    if header.is_empty() {
        return None;
    }
    if header.to_ascii_lowercase().starts_with("content-length:") {
        let length: usize = header.split(':').nth(1)?.trim().parse().ok()?;
        let mut blank = String::new();
        stdin.read_line(&mut blank).ok()?;
        let mut body = vec![0; length];
        stdin.read_exact(&mut body).ok()?;
        return Some((Frame::ContentLength, String::from_utf8(body).ok()?));
    }
    let line = header.trim();
    if line.is_empty() {
        return read_message(stdin);
    }
    Some((Frame::Line, line.to_string()))
}
