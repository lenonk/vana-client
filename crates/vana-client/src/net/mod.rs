pub mod action;
pub mod auth;
pub mod lobby;
pub mod map;
pub mod pol;
pub mod profile;
pub mod search;
pub mod tls;

/// A TCP connection to `host:port`, each address tried for 10 s, so an unreachable server fails in seconds rather than
/// after the system's two-minute retries.
pub fn tcp(host: &str, port: u16) -> std::io::Result<std::net::TcpStream> {
    use std::net::ToSocketAddrs;
    let mut last = std::io::Error::new(std::io::ErrorKind::NotFound, format!("{host} has no address"));
    for a in (host, port).to_socket_addrs()? {
        match std::net::TcpStream::connect_timeout(&a, std::time::Duration::from_secs(10)) {
            Ok(s) => return Ok(s),
            Err(e) => last = e,
        }
    }
    Err(last)
}
