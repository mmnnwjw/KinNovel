//! Host name resolution with a fallback for musl.
//!
//! The device binary is statically linked against musl. musl's resolver gives
//! up (EAI_AGAIN after 5 s) as soon as `/etc/resolv.conf` lists a link-local
//! IPv6 name server without an interface, e.g. `nameserver fe80::1` that
//! odhcp6c writes for routers announcing DNS over DHCPv6 — even when an IPv4
//! server is listed too. The Kindle's own glibc programs resolve fine there.
//! Verified on a KPW5 (2026-10-14): getaddrinfo failed, while the same A query
//! sent by hand to the IPv4 server answered in ~10 ms.
//!
//! [`resolve`] therefore asks the name servers itself when getaddrinfo fails,
//! and goes straight to that path when resolv.conf contains such an entry.
//! resolv.conf can end up listing ONLY `fe80::1` (seen 2026-10-14: every
//! lookup failed, images and comics stopped loading), so besides its IPv4 and
//! global IPv6 entries we also ask the default IPv4 gateway (home routers
//! answer DNS) and the link-local servers with the interface's scope id
//! (`fe80::1%wlan0`) -- both answered on the KPW5 where musl gave up.

use std::io;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV6, ToSocketAddrs, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

const RESOLV_CONF: &str = "/etc/resolv.conf";
const ROUTES: &str = "/proc/net/route";
const QUERY_TIMEOUT: Duration = Duration::from_secs(3);

/// Set once the system resolver failed and ours succeeded: later lookups skip
/// the system resolver's 5 s timeout.
static PREFER_OWN: AtomicBool = AtomicBool::new(false);

pub fn resolve(host: &str, port: u16) -> io::Result<Vec<SocketAddr>> {
    if let Ok(ip) = host.trim_start_matches('[').trim_end_matches(']').parse::<IpAddr>() {
        return Ok(vec![SocketAddr::new(ip, port)]);
    }
    let conf = std::fs::read_to_string(RESOLV_CONF).unwrap_or_default();
    let routes = std::fs::read_to_string(ROUTES).unwrap_or_default();
    let servers = name_servers(&conf, &routes, interface_index);
    let own_first = !servers.is_empty() && (PREFER_OWN.load(Ordering::Relaxed) || musl_unfriendly(&conf));
    if own_first {
        if let Ok(addrs) = query_servers(&servers, host, port) {
            return Ok(addrs);
        }
    }
    match (host, port).to_socket_addrs() {
        Ok(addrs) => Ok(addrs.collect()),
        Err(e) if servers.is_empty() || own_first => Err(e),
        Err(e) => {
            let addrs = query_servers(&servers, host, port).map_err(|_| e)?;
            PREFER_OWN.store(true, Ordering::Relaxed);
            Ok(addrs)
        }
    }
}

/// A `nameserver fe80::...` line without `%iface`.
fn musl_unfriendly(conf: &str) -> bool {
    conf.lines().filter_map(nameserver_value).any(|v| {
        !v.contains('%') && v.parse::<Ipv6Addr>().map(|ip| (ip.segments()[0] & 0xffc0) == 0xfe80).unwrap_or(false)
    })
}

fn nameserver_value(line: &str) -> Option<&str> {
    let mut words = line.split_whitespace();
    (words.next()? == "nameserver").then_some(())?;
    words.next()
}

fn is_link_local(ip: &Ipv6Addr) -> bool {
    (ip.segments()[0] & 0xffc0) == 0xfe80
}

/// Servers to ask ourselves, in order: resolv.conf's IPv4 / global IPv6
/// entries, the default IPv4 gateway, then link-local entries with a scope
/// id (from `%iface`, the odhcp6c comment `#odhcp6c:wlan0:`, or the default
/// route's interface). `ifindex` maps an interface name to its index.
fn name_servers(conf: &str, routes: &str, ifindex: impl Fn(&str) -> Option<u32>) -> Vec<SocketAddr> {
    let gateway = default_gateway(routes);
    let mut plain = Vec::new();
    let mut scoped = Vec::new();
    for line in conf.lines() {
        let Some(value) = nameserver_value(line) else { continue };
        let (addr, iface) = match value.split_once('%') {
            Some((a, i)) => (a, Some(i)),
            None => (value, None),
        };
        match addr.parse::<IpAddr>() {
            Ok(IpAddr::V6(v6)) if is_link_local(&v6) => {
                let iface = iface
                    .or_else(|| line.split("#odhcp6c:").nth(1).and_then(|r| r.split(':').next()))
                    .filter(|i| !i.is_empty())
                    .or(gateway.map(|(i, _)| i));
                if let Some(index) = iface.and_then(&ifindex) {
                    scoped.push(SocketAddr::V6(SocketAddrV6::new(v6, 53, 0, index)));
                }
            }
            Ok(ip) => plain.push(SocketAddr::new(ip, 53)),
            Err(_) => {}
        }
    }
    if let Some((_, gw)) = gateway {
        let gw = SocketAddr::new(IpAddr::V4(gw), 53);
        if !plain.contains(&gw) {
            plain.push(gw);
        }
    }
    plain.extend(scoped);
    plain
}

/// (interface, gateway) of the default IPv4 route in /proc/net/route.
fn default_gateway(routes: &str) -> Option<(&str, Ipv4Addr)> {
    routes.lines().skip(1).find_map(|line| {
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.len() < 3 || f[1] != "00000000" {
            return None;
        }
        let gw = u32::from_str_radix(f[2], 16).ok().filter(|&g| g != 0)?;
        Some((f[0], Ipv4Addr::from(gw.to_le_bytes())))
    })
}

fn interface_index(name: &str) -> Option<u32> {
    std::fs::read_to_string(format!("/sys/class/net/{name}/ifindex")).ok()?.trim().parse().ok()
}

fn query_servers(servers: &[SocketAddr], host: &str, port: u16) -> io::Result<Vec<SocketAddr>> {
    let mut last = io::Error::new(io::ErrorKind::NotFound, "no name server");
    for server in servers {
        match query_a(*server, host) {
            Ok(ips) if !ips.is_empty() => return Ok(ips.into_iter().map(|ip| SocketAddr::new(IpAddr::V4(ip), port)).collect()),
            Ok(_) => last = io::Error::new(io::ErrorKind::NotFound, format!("{host}: no A record")),
            Err(e) => last = e,
        }
    }
    Err(last)
}

fn query_a(server: SocketAddr, host: &str) -> io::Result<Vec<Ipv4Addr>> {
    let bind: SocketAddr = if server.is_ipv4() { "0.0.0.0:0".parse().unwrap() } else { "[::]:0".parse().unwrap() };
    let socket = UdpSocket::bind(bind)?;
    socket.connect(server)?;
    socket.set_read_timeout(Some(QUERY_TIMEOUT))?;
    let id = (std::process::id() as u16) ^ (std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos() as u16).unwrap_or(0));
    let query = build_query(id, host)?;
    let mut buf = [0u8; 1500];
    for _ in 0..2 {
        socket.send(&query)?;
        loop {
            match socket.recv(&mut buf) {
                Ok(n) => {
                    if let Some(ips) = parse_response(&buf[..n], id) {
                        return ips;
                    }
                }
                Err(e) if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut) => break,
                Err(e) => return Err(e),
            }
        }
    }
    Err(io::Error::new(io::ErrorKind::TimedOut, format!("DNS {server} timed out")))
}

fn build_query(id: u16, host: &str) -> io::Result<Vec<u8>> {
    let mut q = Vec::with_capacity(18 + host.len());
    q.extend_from_slice(&id.to_be_bytes());
    q.extend_from_slice(&[0x01, 0x00, 0, 1, 0, 0, 0, 0, 0, 0]); // RD, QDCOUNT 1
    for label in host.trim_end_matches('.').split('.') {
        if label.is_empty() || label.len() > 63 {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, format!("bad host name {host}")));
        }
        q.push(label.len() as u8);
        q.extend_from_slice(label.as_bytes());
    }
    q.extend_from_slice(&[0, 0, 1, 0, 1]); // root, QTYPE A, QCLASS IN
    Ok(q)
}

/// `None` = not our answer (wrong id / not a response / garbled), keep waiting.
fn parse_response(msg: &[u8], id: u16) -> Option<io::Result<Vec<Ipv4Addr>>> {
    if msg.len() < 12 || u16::from_be_bytes([msg[0], msg[1]]) != id || msg[2] & 0x80 == 0 {
        return None;
    }
    let rcode = msg[3] & 0x0f;
    if rcode != 0 {
        return Some(Err(io::Error::new(io::ErrorKind::NotFound, format!("DNS rcode {rcode}"))));
    }
    let qd = u16::from_be_bytes([msg[4], msg[5]]);
    let an = u16::from_be_bytes([msg[6], msg[7]]);
    let mut pos = 12;
    for _ in 0..qd {
        pos = skip_name(msg, pos)? + 4;
    }
    let mut ips = Vec::new();
    for _ in 0..an {
        pos = skip_name(msg, pos)?;
        let rr = msg.get(pos..pos + 10)?;
        let (rtype, class, len) = (u16::from_be_bytes([rr[0], rr[1]]), u16::from_be_bytes([rr[2], rr[3]]), u16::from_be_bytes([rr[8], rr[9]]) as usize);
        let data = msg.get(pos + 10..pos + 10 + len)?;
        if rtype == 1 && class == 1 && len == 4 {
            ips.push(Ipv4Addr::new(data[0], data[1], data[2], data[3]));
        }
        pos += 10 + len;
    }
    Some(Ok(ips))
}

fn skip_name(msg: &[u8], mut pos: usize) -> Option<usize> {
    loop {
        let len = *msg.get(pos)?;
        match len {
            0 => return Some(pos + 1),
            l if l & 0xc0 == 0xc0 => return Some(pos + 2),
            l => pos += 1 + l as usize,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ODHCP6C: &str = "nameserver 192.168.1.1\nnameserver fe80::1 #odhcp6c:wlan0:\n";

    #[test]
    fn link_local_without_scope_is_detected() {
        assert!(musl_unfriendly(ODHCP6C));
        assert!(!musl_unfriendly("nameserver 192.168.1.1\nnameserver fe80::1%wlan0\n"));
        assert!(!musl_unfriendly("nameserver 2001:db8::53\n# nameserver fe80::1\n"));
        assert_eq!(name_servers(ODHCP6C, "", |_| Some(20)), vec![
            "192.168.1.1:53".parse().unwrap(),
            SocketAddr::V6(SocketAddrV6::new("fe80::1".parse().unwrap(), 53, 0, 20)),
        ]);
    }

    const ROUTE: &str = "Iface	Destination	Gateway 	Flags	RefCnt	Use	Metric	Mask
                         wlan0	0001A8C0	00000000	0001	0	0	0	00FFFFFF
                         wlan0	00000000	0101A8C0	0003	0	0	0	00000000
";

    #[test]
    fn only_link_local_falls_back_to_gateway_and_scoped_server() {
        // Seen on the KPW5: resolv.conf had nothing but this line.
        let conf = "nameserver fe80::1 #odhcp6c:wlan0:
";
        assert_eq!(default_gateway(ROUTE), Some(("wlan0", Ipv4Addr::new(192, 168, 1, 1))));
        let servers = name_servers(conf, ROUTE, |i| (i == "wlan0").then_some(20));
        assert_eq!(servers, vec![
            "192.168.1.1:53".parse().unwrap(),
            SocketAddr::V6(SocketAddrV6::new("fe80::1".parse().unwrap(), 53, 0, 20)),
        ]);
        // Explicit scope wins; no comment -> the default route's interface.
        let named = name_servers("nameserver fe80::2%eth0
nameserver fe80::3
", ROUTE, |i| Some(if i == "eth0" { 7 } else { 20 }));
        assert_eq!(named[1..], [
            SocketAddr::V6(SocketAddrV6::new("fe80::2".parse().unwrap(), 53, 0, 7)),
            SocketAddr::V6(SocketAddrV6::new("fe80::3".parse().unwrap(), 53, 0, 20)),
        ]);
        // The gateway isn't listed twice.
        assert_eq!(name_servers("nameserver 192.168.1.1
", ROUTE, |_| None).len(), 1);
    }

    #[test]
    fn parses_a_records_after_a_cname() {
        let mut msg = build_query(0x1234, "api.example.com").unwrap();
        msg[2] = 0x81;
        msg[3] = 0x80;
        msg[7] = 3; // ANCOUNT
        // CNAME api.example.com -> c.example.com, then two A records for it
        msg.extend_from_slice(&[0xc0, 12, 0, 5, 0, 1, 0, 0, 0, 60, 0, 4, 1, b'c', 0xc0, 16]);
        msg.extend_from_slice(&[0xc0, 45, 0, 1, 0, 1, 0, 0, 0, 60, 0, 4, 10, 0, 0, 1]);
        msg.extend_from_slice(&[0xc0, 45, 0, 1, 0, 1, 0, 0, 0, 60, 0, 4, 10, 0, 0, 2]);
        let ips = parse_response(&msg, 0x1234).unwrap().unwrap();
        assert_eq!(ips, vec![Ipv4Addr::new(10, 0, 0, 1), Ipv4Addr::new(10, 0, 0, 2)]);
        assert!(parse_response(&msg, 0x4321).is_none());
    }

    #[test]
    fn ip_literals_skip_dns() {
        assert_eq!(resolve("127.0.0.1", 80).unwrap(), vec!["127.0.0.1:80".parse().unwrap()]);
        assert_eq!(resolve("[::1]", 80).unwrap(), vec!["[::1]:80".parse().unwrap()]);
    }
}
