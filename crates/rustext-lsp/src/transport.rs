//! JSON-RPC over a child process's stdio, framed with `Content-Length` headers.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, channel};

use serde_json::{Value, json};

/// Reads one framed message; `None` at the end of the stream.
pub fn read_message(reader: &mut impl BufRead) -> Option<Value> {
    let mut content_length = None;
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header).ok()? == 0 {
            return None;
        }
        let header = header.trim_end();
        if header.is_empty() {
            break;
        }
        if let Some(length) = header.strip_prefix("Content-Length:") {
            content_length = length.trim().parse::<usize>().ok();
        }
    }
    let mut body = vec![0; content_length?];
    reader.read_exact(&mut body).ok()?;
    serde_json::from_slice(&body).ok()
}

pub fn write_message(writer: &mut impl Write, message: &Value) -> std::io::Result<()> {
    let body = message.to_string();
    write!(writer, "Content-Length: {}\r\n\r\n{body}", body.len())?;
    writer.flush()
}

/// A language server process: messages to it, and a channel of messages from it.
pub struct Connection {
    child: Child,
    stdin: ChildStdin,
    pub incoming: Receiver<Value>,
    next_request_id: u64,
}

impl Connection {
    pub fn spawn(program: &str, arguments: &[&str]) -> std::io::Result<Self> {
        let mut child = Command::new(program)
            .args(arguments)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let stdin = child.stdin.take().expect("piped stdin");
        let stdout = child.stdout.take().expect("piped stdout");
        let (sender, incoming) = channel();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            while let Some(message) = read_message(&mut reader) {
                if sender.send(message).is_err() {
                    break;
                }
            }
        });
        Ok(Self { child, stdin, incoming, next_request_id: 1 })
    }

    /// Sends a request; returns its id.
    pub fn request(&mut self, method: &str, params: Value) -> std::io::Result<u64> {
        let id = self.next_request_id;
        self.next_request_id += 1;
        write_message(&mut self.stdin, &json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }))?;
        Ok(id)
    }

    /// Answers a request the server sent.
    pub fn respond(&mut self, id: Value, result: Value) -> std::io::Result<()> {
        write_message(&mut self.stdin, &json!({ "jsonrpc": "2.0", "id": id, "result": result }))
    }

    pub fn notify(&mut self, method: &str, params: Value) -> std::io::Result<()> {
        write_message(&mut self.stdin, &json!({ "jsonrpc": "2.0", "method": method, "params": params }))
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        let _ = self.request("shutdown", Value::Null);
        let _ = self.notify("exit", Value::Null);
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_round_trip() {
        let mut buffer = Vec::new();
        write_message(&mut buffer, &json!({ "id": 1, "result": "é" })).unwrap();
        write_message(&mut buffer, &json!({ "id": 2 })).unwrap();

        let mut reader = BufReader::new(buffer.as_slice());
        assert_eq!(read_message(&mut reader), Some(json!({ "id": 1, "result": "é" })));
        assert_eq!(read_message(&mut reader), Some(json!({ "id": 2 })));
        assert_eq!(read_message(&mut reader), None);
    }
}
