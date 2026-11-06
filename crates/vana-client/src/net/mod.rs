pub mod action;
pub mod auth;
pub mod lobby;
pub mod map;
pub mod pol;
pub mod profile;
pub mod search;
pub mod tls;

/// A TCP connection to `host:port` within 10 s in all, so an unreachable server fails in seconds rather than after the
/// system's two-minute retries. Each address gets an even share of the time left (an IPv6 address that hangs leaves
/// the IPv4 one its turn).
pub fn tcp(host: &str, port: u16) -> std::io::Result<std::net::TcpStream> {
    use std::net::ToSocketAddrs;
    use std::time::{Duration, Instant};
    let addrs: Vec<_> = (host, port).to_socket_addrs()?.collect();
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut last = std::io::Error::new(std::io::ErrorKind::NotFound, format!("{host} has no address"));
    for (k, a) in addrs.iter().enumerate() {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            break;
        }
        match std::net::TcpStream::connect_timeout(a, (left / (addrs.len() - k) as u32).max(Duration::from_millis(1))) {
            Ok(s) => return Ok(s),
            Err(e) => last = e,
        }
    }
    Err(last)
}

#[cfg(test)]
mod tests {
    /// A name's addresses are tried in turn until one answers (localhost: ::1 first where it resolves to it).
    #[test]
    fn tcp_connects() {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let s = super::tcp("localhost", port).unwrap();
        assert_eq!(s.peer_addr().unwrap().port(), port);
    }
}
