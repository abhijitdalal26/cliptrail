//! LAN discovery: lets a client find its hub again after the hub's address
//! changed (another Wi-Fi, a new DHCP lease, a hotspot). Without it the
//! client only knows the addresses it learned at pairing and from `hello`.
//!
//! Over UDP on the sync port:
//! - probe, client → broadcast: `CTDISC1 <tag>`
//! - reply, hub → client (unicast): `CTHUB1 <tag> <tcp port>`
//!
//! `tag` names the space without broadcasting its id. A reply is only a
//! hint: the client still checks the space id in `/v1/hello` before using
//! the address.

use std::net::{Ipv4Addr, SocketAddr};
use std::time::Duration;

use sha2::{Digest, Sha256};
use tokio::net::UdpSocket;

const PROBE: &str = "CTDISC1";
const REPLY: &str = "CTHUB1";
/// Datagrams are tiny; anything bigger isn't ours.
const MAX_DATAGRAM: usize = 128;

/// Short public name of a space for probes and replies.
pub fn tag(space_id: &str) -> String {
    let h = Sha256::new()
        .chain_update(b"cliptrail-discovery\0")
        .chain_update(space_id.as_bytes())
        .finalize();
    h[..8].iter().map(|b| format!("{b:02x}")).collect()
}

/// Limited broadcast plus each IPv4 interface's directed broadcast (some
/// networks drop 255.255.255.255).
fn broadcast_targets() -> Vec<Ipv4Addr> {
    let mut v = vec![Ipv4Addr::BROADCAST];
    for i in if_addrs::get_if_addrs().unwrap_or_default() {
        if let if_addrs::IfAddr::V4(a) = i.addr
            && !a.ip.is_loopback()
            && let Some(b) = a.broadcast
            && !v.contains(&b)
        {
            v.push(b);
        }
    }
    v
}

/// Ask the LAN where the hub of `space_id` is. `ports`: where it may
/// listen (from the addresses we knew). Returns `host:port` candidates
/// that answered within `wait`; empty when none did or the network has no
/// broadcast (mobile data).
pub async fn find_hub(space_id: &str, ports: &[u16], wait: Duration) -> Vec<String> {
    let sock = match UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).await {
        Ok(s) => s,
        Err(e) => {
            tracing::debug!("discovery socket: {e}");
            return Vec::new();
        }
    };
    if let Err(e) = sock.set_broadcast(true) {
        tracing::debug!("discovery broadcast: {e}");
        return Vec::new();
    }
    let tag = tag(space_id);
    let probe = format!("{PROBE} {tag}");
    for ip in broadcast_targets() {
        for port in ports {
            // Fails without a broadcast-capable network; that's fine.
            let _ = sock.send_to(probe.as_bytes(), (ip, *port)).await;
        }
    }
    let mut found = Vec::new();
    let mut buf = [0u8; MAX_DATAGRAM];
    let deadline = tokio::time::Instant::now() + wait;
    loop {
        let res = tokio::time::timeout_at(deadline, sock.recv_from(&mut buf)).await;
        let Ok(Ok((n, from))) = res else {
            break;
        };
        if let Some(port) = parse_reply(&buf[..n], &tag) {
            let host = SocketAddr::new(from.ip(), port).to_string();
            if !found.contains(&host) {
                found.push(host);
            }
        }
    }
    found
}

fn parse_reply(b: &[u8], tag: &str) -> Option<u16> {
    let s = std::str::from_utf8(b).ok()?;
    let mut it = s.split(' ');
    (it.next()? == REPLY && it.next()? == tag).then_some(())?;
    let port = it.next()?.parse().ok()?;
    it.next().is_none().then_some(port)
}

#[cfg(any(feature = "hub", test))]
fn is_probe(b: &[u8], tag: &str) -> bool {
    std::str::from_utf8(b).is_ok_and(|s| s.split_once(' ') == Some((PROBE, tag)))
}

/// Hub side: answer probes for `space_id` from LAN addresses until `stop`.
#[cfg(feature = "hub")]
pub async fn serve(
    sock: UdpSocket,
    space_id: String,
    tcp_port: u16,
    mut stop: tokio::sync::watch::Receiver<bool>,
) {
    use std::collections::HashSet;
    use std::net::IpAddr;

    let tag = tag(&space_id);
    let reply = format!("{REPLY} {tag} {tcp_port}");
    let mut buf = [0u8; MAX_DATAGRAM];
    // Who we answered recently, so a flood of probes can't use the hub
    // as an amplifier.
    let mut answered: HashSet<IpAddr> = HashSet::new();
    let mut reset = tokio::time::interval(Duration::from_secs(1));
    loop {
        tokio::select! {
            r = sock.recv_from(&mut buf) => {
                let Ok((n, from)) = r else { continue };
                if !crate::hub::is_lan(from.ip()) || !is_probe(&buf[..n], &tag) {
                    continue;
                }
                if answered.insert(from.ip()) {
                    let _ = sock.send_to(reply.as_bytes(), from).await;
                }
            }
            _ = reset.tick() => answered.clear(),
            _ = crate::engine::stopped(&mut stop) => break,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tag_is_stable_and_short() {
        assert_eq!(tag("S1"), tag("S1"));
        assert_ne!(tag("S1"), tag("S2"));
        assert_eq!(tag("S1").len(), 16);
        assert!(!tag("S1").contains("S1"));
    }

    #[test]
    fn messages_parse() {
        let t = tag("S1");
        assert!(is_probe(format!("CTDISC1 {t}").as_bytes(), &t));
        assert!(!is_probe(format!("CTDISC1 {}", tag("S2")).as_bytes(), &t));
        assert!(!is_probe(b"CTDISC1", &t));
        assert_eq!(
            parse_reply(format!("CTHUB1 {t} 47821").as_bytes(), &t),
            Some(47821)
        );
        assert_eq!(
            parse_reply(format!("CTHUB1 {t} 47821 x").as_bytes(), &t),
            None
        );
        assert_eq!(
            parse_reply(format!("CTHUB1 {} 47821", tag("S2")).as_bytes(), &t),
            None
        );
        assert_eq!(parse_reply(b"CTHUB1 zz notaport", &t), None);
    }

    #[cfg(feature = "hub")]
    #[tokio::test]
    async fn finds_a_hub_on_loopback() {
        // Broadcast isn't available everywhere (CI containers), so probe
        // the serve loop directly over loopback.
        let hub = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let addr = hub.local_addr().unwrap();
        let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);
        let task = tokio::spawn(serve(hub, "S1".into(), 5555, stop_rx));
        let c = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let t = tag("S1");
        c.send_to(format!("CTDISC2 {t}").as_bytes(), addr)
            .await
            .unwrap();
        c.send_to(format!("CTDISC1 {}", tag("other")).as_bytes(), addr)
            .await
            .unwrap();
        c.send_to(format!("CTDISC1 {t}").as_bytes(), addr)
            .await
            .unwrap();
        let mut buf = [0u8; MAX_DATAGRAM];
        let (n, _) = tokio::time::timeout(Duration::from_secs(2), c.recv_from(&mut buf))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(parse_reply(&buf[..n], &t), Some(5555));
        let _ = stop_tx.send(true);
        task.await.unwrap();
    }
}
