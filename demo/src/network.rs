// Copyright © The Daybrite Project
// SPDX-License-Identifier: MPL-2.0
//! Deliberately separate from discovery: applications choose their transport and authentication.
use day_part_zeroconf::{Error, Service, TxtRecord};
use std::{
    io::{self, Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};
pub(crate) fn parse_txt(text: &str) -> Result<TxtRecord, Error> {
    let txt = TxtRecord(
        text.lines()
            .filter(|l| !l.is_empty())
            .map(|line| match line.split_once('=') {
                Some((k, v)) => (k.to_owned(), Some(v.as_bytes().to_vec())),
                None => (line.to_owned(), None),
            })
            .collect(),
    );
    txt.encode()?;
    Ok(txt)
}
pub(crate) struct Server {
    pub port: u16,
    stop: Arc<AtomicBool>,
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
    }
}
impl Server {
    pub fn start(body: String) -> io::Result<Self> {
        let listener = TcpListener::bind(("0.0.0.0", 0))?;
        listener.set_nonblocking(true)?;
        let port = listener.local_addr()?.port();
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        thread::Builder::new()
            .name("zeroconf-demo-http".into())
            .spawn(move || {
                while !stopping.load(Ordering::Acquire) {
                    match listener.accept() {
                        Ok((mut stream, _)) => {
                            let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
                            let _ = stream.set_write_timeout(Some(Duration::from_millis(500)));
                            let mut bytes = [0; 4096];
                            if let Ok(n) = stream.read(&mut bytes)
                                && n > 0
                                && bytes[..n].starts_with(b"GET ")
                            {
                                let _ = stream.write_all(response.as_bytes());
                            }
                        }
                        Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(20))
                        }
                        Err(_) => break,
                    }
                }
            })?;
        Ok(Self { port, stop })
    }
}
pub(crate) enum Probe {
    Tcp(SocketAddr),
    Http(String),
}
pub(crate) fn can_http(service: &Service) -> bool {
    matches!(
        service.id.service_type.as_str(),
        "_http._tcp" | "_calibre._tcp" | "_dayzc._tcp"
    )
}
pub(crate) fn probe(service: &Service, http: bool) -> io::Result<Probe> {
    if !service.id.service_type.ends_with("._tcp") || http && !can_http(service) {
        return Err(io::ErrorKind::Unsupported.into());
    }
    let deadline = Instant::now() + Duration::from_secs(8);
    let mut last = io::Error::from(io::ErrorKind::NotFound);
    for address in &service.addresses {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        match TcpStream::connect_timeout(address, remaining.min(Duration::from_secs(2))) {
            Ok(mut stream) => {
                if !http {
                    return Ok(Probe::Tcp(*address));
                }
                stream.set_read_timeout(Some(Duration::from_secs(2)))?;
                stream.set_write_timeout(Some(Duration::from_secs(2)))?;
                let path = service
                    .txt
                    .get("path")
                    .and_then(|b| std::str::from_utf8(b).ok())
                    .filter(|s| {
                        s.starts_with('/')
                            && !s.starts_with("//")
                            && s.bytes().all(|c| c > 32 && c < 127)
                    })
                    .unwrap_or("/");
                // The Host header uses the connected numeric endpoint; untrusted DNS/TXT cannot inject headers.
                write!(
                    stream,
                    "GET {path} HTTP/1.1\r\nHost: {address}\r\nAccept: */*\r\nConnection: close\r\n\r\n"
                )?;
                let mut bytes = vec![];
                let mut buf = [0; 2048];
                while bytes.len() < 16384 && Instant::now() < deadline {
                    match stream.read(&mut buf[..(16384 - bytes.len()).min(2048)]) {
                        Ok(0) => break,
                        Ok(n) => bytes.extend_from_slice(&buf[..n]),
                        Err(e)
                            if !bytes.is_empty()
                                && (e.kind() == io::ErrorKind::TimedOut
                                    || e.kind() == io::ErrorKind::WouldBlock) =>
                        {
                            break;
                        }
                        Err(e) => return Err(e),
                    }
                }
                if !bytes.starts_with(b"HTTP/") {
                    return Err(io::ErrorKind::InvalidData.into());
                }
                return Ok(Probe::Http(String::from_utf8_lossy(&bytes).into_owned()));
            }
            Err(e) => last = e,
        }
    }
    Err(last)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn txt_editor_preserves_flags() {
        let t = parse_txt("flag\nempty=\npath=/opds").unwrap();
        assert_eq!(t.0[0].1, None);
        assert_eq!(t.get("empty"), Some(&b""[..]));
        assert!(parse_txt("a=1\nA=2").is_err());
    }
    #[test]
    fn real_http_and_tcp_roundtrip() {
        let server = Server::start("Synthetic test fixture".into()).unwrap();
        let s = Service {
            id: day_part_zeroconf::ServiceId {
                name: "fixture".into(),
                service_type: "_dayzc._tcp".into(),
                domain: "local.".into(),
                interface: 0,
            },
            hostname: "localhost".into(),
            port: server.port,
            addresses: vec![SocketAddr::from(([127, 0, 0, 1], server.port))],
            txt: TxtRecord::default(),
        };
        assert!(matches!(probe(&s, false).unwrap(), Probe::Tcp(_)));
        match probe(&s, true).unwrap() {
            Probe::Http(s) => assert!(s.contains("Synthetic test fixture")),
            _ => panic!("HTTP response expected"),
        }
    }
}
