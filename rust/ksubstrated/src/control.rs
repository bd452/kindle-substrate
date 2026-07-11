//! A deliberately tiny line-oriented control protocol.  Each request and
//! response is one line, so neither peer waits for EOF from the other.
use crate::layout::StateTmpfs;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};

pub const SOCKET: &str = "control.sock";

pub fn socket_path(state: &StateTmpfs) -> std::path::PathBuf {
    state.path().join(SOCKET)
}

pub fn listen(state: &StateTmpfs) -> Result<UnixListener, String> {
    let path = socket_path(state);
    let _ = fs::remove_file(&path);
    let listener = UnixListener::bind(&path).map_err(|e| format!("bind control socket: {e}"))?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600))
        .map_err(|e| format!("protect control socket: {e}"))?;
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    Ok(listener)
}

pub fn request(state: &StateTmpfs, command: &str) -> Result<String, String> {
    let mut stream = UnixStream::connect(socket_path(state))
        .map_err(|e| format!("control socket unavailable: {e}"))?;
    writeln!(stream, "{command}").map_err(|e| format!("send control command: {e}"))?;
    let mut response = String::new();
    BufReader::new(stream)
        .read_line(&mut response)
        .map_err(|e| format!("read control response: {e}"))?;
    let response = response.trim_end();
    if let Some(message) = response.strip_prefix("OK ") {
        Ok(message.to_owned())
    } else if let Some(message) = response.strip_prefix("ERR ") {
        Err(message.to_owned())
    } else {
        Err("malformed control response".to_owned())
    }
}

pub fn receive(stream: &UnixStream) -> Result<String, String> {
    let mut command = String::new();
    BufReader::new(stream)
        .read_line(&mut command)
        .map_err(|e| format!("read control command: {e}"))?;
    let command = command.trim();
    if command.is_empty() || command.len() > 64 || command.bytes().any(|byte| !byte.is_ascii_lowercase() && byte != b'-') {
        return Err("invalid control command".to_owned());
    }
    Ok(command.to_owned())
}

pub fn respond(stream: &mut UnixStream, result: Result<&str, String>) -> Result<(), String> {
    match result {
        Ok(message) => writeln!(stream, "OK {message}"),
        Err(message) => writeln!(stream, "ERR {message}"),
    }
    .map_err(|e| format!("write control response: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixStream;

    #[test]
    fn receives_one_line_without_waiting_for_eof() {
        let (mut client, server) = UnixStream::pair().unwrap();
        writeln!(client, "status").unwrap();
        assert_eq!(receive(&server).unwrap(), "status");
    }
}
