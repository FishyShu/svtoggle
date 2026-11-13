use std::net::{Ipv4Addr, TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant};
use windows::Win32::NetworkManagement::IpHelper::{IcmpCloseHandle, IcmpCreateFile, IcmpSendEcho, ICMP_ECHO_REPLY};

/// Latency in ms, best of 2. None = unreachable / blocked / timeout.
/// Spec: "icmp:1.2.3.4" for an echo, anything else is "host:port" TCP connect.
pub fn ping(spec: &str) -> Option<u32> {
    match spec.strip_prefix("icmp:") {
        Some(ip) => icmp(ip.parse().ok()?),
        None => tcp(spec),
    }
}

// TCP for cloud proxies: AWS/GCP API endpoints often drop ICMP.
fn tcp(target: &str) -> Option<u32> {
    let addr = target.to_socket_addrs().ok()?.next()?;
    (0..2)
        .filter_map(|_| {
            let t = Instant::now();
            TcpStream::connect_timeout(&addr, Duration::from_millis(1500)).ok()?;
            Some(t.elapsed().as_millis() as u32)
        })
        .min()
}

fn icmp(ip: Ipv4Addr) -> Option<u32> {
    unsafe {
        let h = IcmpCreateFile().ok()?;
        let data = [0u8; 32];
        // reply buffer: one reply + payload + 8 bytes for an ICMP error, per the IcmpSendEcho docs
        let mut buf = vec![0u8; size_of::<ICMP_ECHO_REPLY>() + data.len() + 8];
        let mut best = None;
        for _ in 0..2 {
            let n = IcmpSendEcho(h, u32::from_ne_bytes(ip.octets()), data.as_ptr().cast(), data.len() as u16,
                                 None, buf.as_mut_ptr().cast(), buf.len() as u32, 1500);
            let r = &*(buf.as_ptr() as *const ICMP_ECHO_REPLY);
            if n > 0 && r.Status == 0 {
                best = Some(best.map_or(r.RoundTripTime, |b: u32| b.min(r.RoundTripTime)));
            }
        }
        let _ = IcmpCloseHandle(h);
        best
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn loopback_answers() {
        assert!(super::ping("icmp:127.0.0.1").is_some());
        assert!(super::ping("icmp:not-an-ip").is_none());
    }
}
