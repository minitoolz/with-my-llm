use anyhow::{bail, Context, Result};
use std::{
    collections::BTreeSet,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, TcpListener},
};

pub const DEFAULT_CANDIDATES: &str = "1024-65535";

pub fn default_candidates() -> String {
    DEFAULT_CANDIDATES.into()
}

pub fn parse_candidates(input: &str) -> Result<Vec<u16>> {
    fn port(input: &str) -> Result<u16> {
        let input = input.trim_matches([' ', '\t']);
        anyhow::ensure!(
            !input.is_empty() && input.bytes().all(|b| b.is_ascii_digit()),
            "Ports must contain only digits"
        );
        let port: u16 = input.parse().context("Ports must be between 1 and 65535")?;
        anyhow::ensure!(port != 0, "Ports must be between 1 and 65535");
        Ok(port)
    }

    let mut ports = BTreeSet::new();
    for item in input.split(',') {
        if let Some((start, end)) = item.split_once('-') {
            let (start, end) = (port(start)?, port(end)?);
            anyhow::ensure!(start <= end, "Port range start must not exceed its end");
            ports.extend(start..=end);
        } else {
            ports.insert(port(item)?);
        }
    }
    Ok(ports.into_iter().collect())
}

pub fn choose(candidates: &str) -> Result<u16> {
    let mut ports = parse_candidates(candidates)?;
    while !ports.is_empty() {
        // The final four UUID v4 bytes contain only random bits. Rejection
        // sampling keeps each remaining candidate equally likely.
        let count = ports.len() as u32;
        let limit = u32::MAX - u32::MAX % count;
        let index = loop {
            let bytes = *uuid::Uuid::new_v4().as_bytes();
            let random = u32::from_be_bytes(bytes[12..].try_into().unwrap());
            if random < limit {
                break (random % count) as usize;
            }
        };
        let port = ports.swap_remove(index);
        if available(port) {
            // Commands need to bind the port themselves; this is an availability
            // check, not a reservation across process startup.
            return Ok(port);
        }
    }
    bail!("Unable to assign a port in the port range: {candidates} (no candidate can be opened and listened on)")
}

fn available(port: u16) -> bool {
    // On macOS a wildcard bind can succeed while a loopback-specific listener
    // owns the port. Check both, releasing each probe before the next bind.
    for address in [
        IpAddr::V4(Ipv4Addr::LOCALHOST),
        IpAddr::V4(Ipv4Addr::UNSPECIFIED),
        IpAddr::V6(Ipv6Addr::LOCALHOST),
        IpAddr::V6(Ipv6Addr::UNSPECIFIED),
    ] {
        // TcpListener::bind opens the socket, binds it, and calls listen; a
        // successful probe verifies more than just whether connect fails.
        match TcpListener::bind((address, port)) {
            Ok(listener) => drop(listener),
            Err(error)
                if address.is_ipv6()
                    && (error.kind() == std::io::ErrorKind::AddrNotAvailable
                        || error.raw_os_error() == Some(libc::EAFNOSUPPORT)) => {}
            Err(_) => return false,
        }
    }
    true
}

pub fn has_placeholder(value: &str) -> bool {
    value
        .as_bytes()
        .windows(6)
        .any(|s| s.eq_ignore_ascii_case(b"{PORT}"))
}

pub fn substitute(value: &str, port: u16) -> String {
    let mut result = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(index) = rest.find('{') {
        result.push_str(&rest[..index]);
        rest = &rest[index..];
        if rest
            .get(..6)
            .is_some_and(|s| s.eq_ignore_ascii_case("{PORT}"))
        {
            result.push_str(&port.to_string());
            rest = &rest[6..];
        } else {
            result.push('{');
            rest = &rest[1..];
        }
    }
    result.push_str(rest);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_unions_ranges_whitespace_and_duplicates() {
        assert_eq!(
            parse_candidates("80, 8000\t  -\t8002,8001,65535").unwrap(),
            [80, 8000, 8001, 8002, 65535]
        );
        assert_eq!(parse_candidates(DEFAULT_CANDIDATES).unwrap().len(), 64512);
        assert_eq!(parse_candidates("00080-80").unwrap(), [80]);
        for invalid in [
            "", "0", "65536", "3-2", "1,", ",1", "1,,2", "1-2-3", "-1", "+1", "1 2", "1\n-2", "a",
        ] {
            assert!(parse_candidates(invalid).is_err(), "{invalid:?}");
        }
    }

    #[test]
    fn substitutes_only_case_insensitive_port_tokens() {
        for value in ["{PORT}", "prefix{pOrT}suffix", "é{port}"] {
            assert!(has_placeholder(value));
        }
        for value in ["", "{POR}", "{PORTS}", "{ PORT }", "fixed"] {
            assert!(!has_placeholder(value));
        }
        assert_eq!(
            substitute("é{PORT}/{port}/{PoRt}/{POR}/{other}/{한글}", 8123),
            "é8123/8123/8123/{POR}/{other}/{한글}"
        );
    }

    #[test]
    fn skips_occupied_ports_and_reports_exhaustion() {
        let occupied = TcpListener::bind("127.0.0.1:0").unwrap();
        let blocked = occupied.local_addr().unwrap().port();
        assert!(choose(&blocked.to_string()).is_err());
        let free = TcpListener::bind("127.0.0.1:0").unwrap();
        let available = free.local_addr().unwrap().port();
        drop(free);
        assert_eq!(
            choose(&format!("{blocked},{available}")).unwrap(),
            available
        );
    }
}
