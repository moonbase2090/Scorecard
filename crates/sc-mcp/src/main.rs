// SPDX-License-Identifier: MPL-2.0
use std::io::{self, BufRead, Write};

fn main() {
    if std::env::args().nth(1).as_deref() == Some("--version") {
        println!("{}", env!("CARGO_PKG_VERSION"));
        return;
    }
    let stdin = io::stdin();
    let mut stdin = stdin.lock();
    let mut stdout = io::stdout().lock();
    while let Some((frame, message)) = read_message(&mut stdin) {
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
    loop {
        let mut header = String::new();
        stdin.read_line(&mut header).ok()?;
        if header.is_empty() {
            return None;
        }
        if header.to_ascii_lowercase().starts_with("content-length:") {
            return Some((Frame::ContentLength, read_body(stdin, &header)?));
        }
        let line = header.trim();
        if !line.is_empty() {
            return Some((Frame::Line, line.to_string()));
        }
    }
}

fn parse_content_length(header: &str) -> Option<usize> {
    header.split(':').nth(1)?.trim().parse().ok()
}

fn read_body(stdin: &mut impl BufRead, header: &str) -> Option<String> {
    let length = parse_content_length(header)?;
    let mut blank = String::new();
    stdin.read_line(&mut blank).ok()?;
    let mut body = vec![0; length];
    stdin.read_exact(&mut body).ok()?;
    String::from_utf8(body).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn reads_a_line_delimited_message() {
        let mut input = Cursor::new(b"{\"jsonrpc\":\"2.0\"}\n");
        let (frame, message) = read_message(&mut input).unwrap();
        assert!(matches!(frame, Frame::Line));
        assert_eq!(message, "{\"jsonrpc\":\"2.0\"}");
    }

    #[test]
    fn reads_a_content_length_message() {
        let mut input = Cursor::new(b"Content-Length: 7\r\n\r\n{\"a\":1}");
        let (frame, message) = read_message(&mut input).unwrap();
        assert!(matches!(frame, Frame::ContentLength));
        assert_eq!(message, "{\"a\":1}");
    }

    #[test]
    fn skips_blank_lines_and_hits_eof() {
        let mut blanks = Cursor::new(b"\n\n");
        assert!(read_message(&mut blanks).is_none());
        let mut empty = Cursor::new(b"");
        assert!(read_message(&mut empty).is_none());
    }

    #[test]
    fn rejects_a_bad_content_length() {
        let mut input = Cursor::new(b"Content-Length: nope\r\n\r\n{}");
        assert!(read_message(&mut input).is_none());
        assert_eq!(parse_content_length("Content-Length: 12"), Some(12));
        assert_eq!(parse_content_length("no colon here"), None);
    }

    #[test]
    fn rejects_non_utf8_bodies() {
        let mut input = Cursor::new(b"Content-Length: 2\r\n\r\n\xff\xfe");
        assert!(read_message(&mut input).is_none());
    }

    #[test]
    fn writes_both_frames() {
        let mut out = Vec::new();
        write_message(&mut out, Frame::Line, "{}");
        write_message(&mut out, Frame::ContentLength, "{}");
        let text = String::from_utf8(out).unwrap();
        assert!(text.starts_with("{}\n"));
        assert!(text.contains("Content-Length: 2"));
    }
}
