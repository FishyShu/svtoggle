//! "Now connected": which game server region the user is in a match on.
//!
//! One background thread. Every 2 s it snapshots processes; while a game runs it reads a raw
//! IPv4 socket in `SIO_RCVALL` mode on the default-route interface and counts UDP packets per
//! remote IP, keeping only packets on a local port owned by the game process (so Discord voice
//! on the same GCP/AWS ranges can't be mistaken for a match). A remote that sustains
//! >= `MIN_PPS` over the last `WINDOW` seconds and falls in a target range is the current match.
//!
//! Direction: Microsoft documents SIO_RCVALL as "all IPv4 packets passing through the
//! interface", i.e. in + out. Windows Firewall can still drop the *inbound* copy for a raw
//! socket whose app has no allow rule, leaving only outbound. Both carry the same remote, and
//! games send ~30-60 pps upstream too, so either direction alone is enough.
use crate::cidr::Cidr;
use std::collections::{HashMap, HashSet, VecDeque};
use std::net::{Ipv4Addr, UdpSocket};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::NetworkManagement::IpHelper::{GetExtendedUdpTable, MIB_UDPTABLE_OWNER_PID, UDP_TABLE_OWNER_PID};
use windows::Win32::Networking::WinSock::{
    AF_INET, IN_ADDR, IN_ADDR_0, IPPROTO_IP, RCVALL_ON, SEND_RECV_FLAGS, SIO_RCVALL, SOCK_RAW, SOCKADDR, SOCKADDR_IN,
    SOCKET, SOL_SOCKET, SO_RCVTIMEO, WSADATA, WSAEACCES, WSAETIMEDOUT, WSAGetLastError, WSAIoctl, WSAStartup, bind,
    closesocket, recv, setsockopt, socket,
};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};

const GAMES: [(&str, &str); 2] = [("overwatch", "overwatch.exe"), ("deadlock", "deadlock.exe")];
const WINDOW: u64 = 3;
const MIN_PPS: u32 = 20;
const TICK: Duration = Duration::from_secs(2);

#[derive(Clone, Debug)]
pub struct Target {
    pub cidr: Cidr,
    pub game: String,
    pub region: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Conn {
    pub game: String,
    pub region: String,
    pub remote: Ipv4Addr,
    pub pps: u32,
}

#[derive(Default)]
struct State {
    targets: Vec<Target>,
    current: Option<Conn>,
    running: Vec<String>,
    error: Option<String>,
}

pub struct Monitor {
    st: Arc<Mutex<State>>,
}

impl Monitor {
    /// Starts a background thread. Never panics; on failure (not admin, no interface) it records an error.
    pub fn start(targets: Vec<Target>, on_change: impl Fn() + Send + 'static) -> Monitor {
        let st = Arc::new(Mutex::new(State { targets, ..Default::default() }));
        // probe once so error() is known before any game starts
        st.lock().unwrap().error = Capture::open().err();
        let s = st.clone();
        std::thread::spawn(move || run(s, on_change));
        Monitor { st }
    }
    pub fn set_targets(&self, targets: Vec<Target>) {
        self.st.lock().unwrap().targets = targets;
    }
    /// None = not in a match.
    pub fn current(&self) -> Option<Conn> {
        self.st.lock().unwrap().current.clone()
    }
    /// "overwatch" / "deadlock" if their process is running.
    pub fn running_games(&self) -> Vec<String> {
        self.st.lock().unwrap().running.clone()
    }
    pub fn error(&self) -> Option<String> {
        self.st.lock().unwrap().error.clone()
    }
}

fn run(st: Arc<Mutex<State>>, on_change: impl Fn()) {
    let t0 = Instant::now();
    let mut cap: Option<Capture> = None;
    let mut win = Window::default();
    let mut last_sec = 0;
    let mut buf = vec![0u8; 65536];
    while Arc::strong_count(&st) > 1 {
        let (running, pids) = games_running();
        let ports = game_ports(&pids);
        st.lock().unwrap().running = running.clone();
        if running.is_empty() {
            cap = None; // close the socket: idle CPU ~0
            win = Window::default();
            set_current(&st, None, &on_change);
            std::thread::sleep(TICK);
            continue;
        }
        if cap.is_none() {
            match Capture::open() {
                Ok(c) => {
                    cap = Some(c);
                    st.lock().unwrap().error = None;
                }
                Err(e) => {
                    st.lock().unwrap().error = Some(e);
                    std::thread::sleep(TICK);
                    continue;
                }
            }
        }
        let c = cap.as_ref().unwrap();
        let until = Instant::now() + TICK;
        while Instant::now() < until {
            match c.recv(&mut buf) {
                Ok(Some(n)) => {
                    if let Some((remote, port)) = parse(&buf[..n], c.local) {
                        // fail closed: no ownership info = count nothing (Discord voice must not look like a match)
                        if ports.as_ref().is_some_and(|p| p.contains(&port)) {
                            win.add(t0.elapsed().as_secs(), remote);
                        }
                    }
                }
                Ok(None) => {} // timeout, just tick
                Err(e) => {
                    st.lock().unwrap().error = Some(e);
                    cap = None; // reopen next round (interface may have changed)
                    break;
                }
            }
            let sec = t0.elapsed().as_secs();
            if sec != last_sec {
                last_sec = sec;
                let rates = win.rates(sec);
                let conn = pick(&st.lock().unwrap().targets, &rates, &running);
                set_current(&st, conn, &on_change);
            }
        }
    }
}

/// Fires on_change only when the match itself changes; a pps update alone is silent.
fn set_current(st: &Mutex<State>, conn: Option<Conn>, on_change: &impl Fn()) {
    let key = |c: &Option<Conn>| c.as_ref().map(|c| (c.game.clone(), c.region.clone(), c.remote));
    let changed = {
        let mut s = st.lock().unwrap();
        let changed = key(&s.current) != key(&conn);
        s.current = conn;
        changed
    };
    if changed {
        on_change();
    }
}

// ---------- pure logic ----------

/// (remote ip, local port) of an IPv4 UDP packet; None for anything else or a packet not involving `local`.
fn parse(p: &[u8], local: u32) -> Option<(u32, u16)> {
    if p.len() < 20 || p[0] >> 4 != 4 || p[9] != 17 {
        return None;
    }
    let ihl = (p[0] & 0x0f) as usize * 4;
    let u = p.get(ihl..ihl + 4)?;
    let be = |i: usize| u32::from_be_bytes(p[i..i + 4].try_into().unwrap());
    let (src, dst) = (be(12), be(16));
    let (sport, dport) = (u16::from_be_bytes([u[0], u[1]]), u16::from_be_bytes([u[2], u[3]]));
    if dst == local {
        Some((src, dport))
    } else if src == local {
        Some((dst, sport))
    } else {
        None
    }
}

/// Longest-prefix match.
fn lookup(targets: &[Target], ip: u32) -> Option<&Target> {
    targets
        .iter()
        .filter(|t| t.cidr.overlaps(&Cidr { net: ip, len: 32 }))
        .max_by_key(|t| t.cidr.len)
}

/// Per-second packet counts per remote, last WINDOW full seconds.
#[derive(Default)]
struct Window {
    buckets: VecDeque<(u64, HashMap<u32, u32>)>,
}

impl Window {
    fn add(&mut self, sec: u64, remote: u32) {
        if self.buckets.back().is_none_or(|b| b.0 != sec) {
            self.buckets.push_back((sec, HashMap::new()));
            while self.buckets.front().is_some_and(|b| b.0 + WINDOW < sec) {
                self.buckets.pop_front();
            }
        }
        *self.buckets.back_mut().unwrap().1.entry(remote).or_default() += 1;
    }

    /// Average pps per remote over the WINDOW completed seconds before `now` (the current second is partial).
    fn rates(&self, now: u64) -> HashMap<u32, u32> {
        let mut sum = HashMap::new();
        for (s, m) in &self.buckets {
            if *s < now && s + WINDOW >= now {
                for (ip, n) in m {
                    *sum.entry(*ip).or_default() += n;
                }
            }
        }
        sum.values_mut().for_each(|v| *v /= WINDOW as u32);
        sum
    }
}

/// Busiest remote that is >= MIN_PPS and in a target range of a running game.
fn pick(targets: &[Target], rates: &HashMap<u32, u32>, running: &[String]) -> Option<Conn> {
    rates
        .iter()
        .filter(|&(_, &pps)| pps >= MIN_PPS)
        .filter_map(|(&ip, &pps)| {
            let t = lookup(targets, ip)?;
            running.contains(&t.game).then(|| Conn {
                game: t.game.clone(),
                region: t.region.clone(),
                remote: Ipv4Addr::from(ip),
                pps,
            })
        })
        .max_by_key(|c| c.pps)
}

// ---------- OS glue ----------

/// Running games and their pids.
fn games_running() -> (Vec<String>, Vec<u32>) {
    let (mut names, mut pids) = (Vec::new(), Vec::new());
    unsafe {
        let Ok(h) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else { return (names, pids) };
        let mut e = PROCESSENTRY32W { dwSize: size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        let mut ok = Process32FirstW(h, &mut e).is_ok();
        while ok {
            let n = e.szExeFile.iter().position(|&c| c == 0).unwrap_or(260);
            let exe = String::from_utf16_lossy(&e.szExeFile[..n]).to_ascii_lowercase();
            if let Some((g, _)) = GAMES.iter().find(|(_, x)| *x == exe) {
                pids.push(e.th32ProcessID);
                if !names.iter().any(|x| x == g) {
                    names.push(g.to_string());
                }
            }
            ok = Process32NextW(h, &mut e).is_ok();
        }
        let _ = CloseHandle(h);
    }
    (names, pids)
}

/// Local UDP ports owned by `pids`. None = table unavailable; the caller then counts nothing.
fn game_ports(pids: &[u32]) -> Option<HashSet<u16>> {
    if pids.is_empty() {
        return Some(HashSet::new());
    }
    unsafe {
        let mut size = 0u32;
        let _ = GetExtendedUdpTable(None, &mut size, false, AF_INET.0 as u32, UDP_TABLE_OWNER_PID, 0);
        let mut buf;
        let mut tries = 0;
        loop {
            // headroom + retry: the table can grow between the size query and the read
            size += 4096;
            buf = vec![0u32; size as usize / 4 + 1]; // u32 storage keeps the rows aligned
            match GetExtendedUdpTable(Some(buf.as_mut_ptr().cast()), &mut size, false, AF_INET.0 as u32, UDP_TABLE_OWNER_PID, 0) {
                0 => break,
                122 if tries < 3 => tries += 1, // ERROR_INSUFFICIENT_BUFFER: size was updated, try again
                _ => return None,
            }
        }
        let t = &*(buf.as_ptr() as *const MIB_UDPTABLE_OWNER_PID);
        let rows = std::slice::from_raw_parts(t.table.as_ptr(), t.dwNumEntries as usize);
        Some(
            rows.iter()
                .filter(|r| pids.contains(&r.dwOwningPid))
                .map(|r| u16::from_be(r.dwLocalPort as u16))
                .collect(),
        )
    }
}

/// Default-route local IPv4: connect() a UDP socket picks the route without sending anything.
fn local_ipv4() -> Option<Ipv4Addr> {
    let s = UdpSocket::bind("0.0.0.0:0").ok()?;
    s.connect("8.8.8.8:53").ok()?;
    match s.local_addr().ok()?.ip() {
        std::net::IpAddr::V4(ip) if !ip.is_unspecified() => Some(ip),
        _ => None,
    }
}

struct Capture {
    s: SOCKET,
    local: u32,
}

impl Capture {
    fn open() -> Result<Capture, String> {
        let ip = local_ipv4().ok_or("no network interface with an IPv4 default route")?;
        unsafe {
            let mut wsa = WSADATA::default();
            WSAStartup(0x0202, &mut wsa); // refcounted; std::net already started it, harmless
            let s = socket(AF_INET.0 as i32, SOCK_RAW, IPPROTO_IP.0).map_err(|e| {
                if e.code() == windows::core::HRESULT::from_win32(WSAEACCES.0 as u32) {
                    "needs administrator".to_string()
                } else {
                    format!("raw socket: {e}")
                }
            })?;
            let c = Capture { s, local: u32::from(ip) };
            let addr = SOCKADDR_IN {
                sin_family: AF_INET,
                sin_addr: IN_ADDR { S_un: IN_ADDR_0 { S_addr: u32::from_ne_bytes(ip.octets()) } },
                ..Default::default()
            };
            if bind(s, &addr as *const _ as *const SOCKADDR, size_of::<SOCKADDR_IN>() as i32) != 0 {
                return Err(format!("bind {ip}: error {}", WSAGetLastError().0));
            }
            let on = RCVALL_ON.0 as u32;
            let mut ret = 0u32;
            if WSAIoctl(s, SIO_RCVALL, Some(&on as *const u32 as _), 4, None, 0, &mut ret, None, None) != 0 {
                return Err(format!("SIO_RCVALL: error {}", WSAGetLastError().0));
            }
            let ms = 500u32.to_ne_bytes(); // recv wakes up so the loop can tick with no traffic
            setsockopt(s, SOL_SOCKET, SO_RCVTIMEO, Some(&ms));
            Ok(c)
        }
    }

    /// Ok(None) = timeout.
    fn recv(&self, buf: &mut [u8]) -> Result<Option<usize>, String> {
        let n = unsafe { recv(self.s, buf, SEND_RECV_FLAGS(0)) };
        if n >= 0 {
            return Ok(Some(n as usize));
        }
        match unsafe { WSAGetLastError() } {
            e if e == WSAETIMEDOUT => Ok(None),
            e => Err(format!("capture recv: error {}", e.0)),
        }
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        unsafe { closesocket(self.s) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(s: &str) -> u32 {
        u32::from(s.parse::<Ipv4Addr>().unwrap())
    }
    fn t(c: &str, g: &str, r: &str) -> Target {
        Target { cidr: Cidr::parse(c).unwrap(), game: g.into(), region: r.into() }
    }
    fn pkt(proto: u8, src: &str, dst: &str, sport: u16, dport: u16) -> Vec<u8> {
        let mut p = vec![0x45, 0, 0, 28, 0, 0, 0, 0, 64, proto, 0, 0];
        p.extend(ip(src).to_be_bytes());
        p.extend(ip(dst).to_be_bytes());
        p.extend(sport.to_be_bytes());
        p.extend(dport.to_be_bytes());
        p.extend([0, 8, 0, 0]);
        p
    }

    #[test]
    fn parse_picks_remote_side() {
        let me = ip("192.168.1.5");
        assert_eq!(parse(&pkt(17, "34.1.2.3", "192.168.1.5", 3478, 50000), me), Some((ip("34.1.2.3"), 50000)));
        assert_eq!(parse(&pkt(17, "192.168.1.5", "34.1.2.3", 50000, 3478), me), Some((ip("34.1.2.3"), 50000)));
        assert_eq!(parse(&pkt(6, "34.1.2.3", "192.168.1.5", 443, 50000), me), None); // TCP
        assert_eq!(parse(&pkt(17, "34.1.2.3", "10.0.0.9", 1, 2), me), None); // not ours
        assert_eq!(parse(&pkt(17, "34.1.2.3", "192.168.1.5", 1, 2)[..22], me), None); // truncated
        let mut v6 = pkt(17, "34.1.2.3", "192.168.1.5", 1, 2);
        v6[0] = 0x65;
        assert_eq!(parse(&v6, me), None);
    }

    #[test]
    fn longest_prefix_wins() {
        let ts = [t("100.0.0.0/8", "overwatch", "wide"), t("100.70.0.0/16", "overwatch", "me"), t("100.70.9.0/24", "overwatch", "narrow")];
        assert_eq!(lookup(&ts, ip("100.70.9.1")).unwrap().region, "narrow");
        assert_eq!(lookup(&ts, ip("100.70.1.1")).unwrap().region, "me");
        assert_eq!(lookup(&ts, ip("100.1.1.1")).unwrap().region, "wide");
        assert!(lookup(&ts, ip("8.8.8.8")).is_none());
    }

    fn feed(w: &mut Window, sec: u64, remote: u32, n: u32) {
        (0..n).for_each(|_| w.add(sec, remote));
    }

    #[test]
    fn threshold_and_pick() {
        let ts = [t("100.70.0.0/16", "overwatch", "me"), t("1.2.3.4/32", "deadlock", "fra")];
        let running = vec!["overwatch".to_string()];
        let mut w = Window::default();
        for s in 0..3 {
            feed(&mut w, s, ip("100.70.1.1"), 60);
            feed(&mut w, s, ip("1.2.3.4"), 200); // deadlock not running: ignored
            feed(&mut w, s, ip("9.9.9.9"), 500); // not a target
        }
        let c = pick(&ts, &w.rates(3), &running).unwrap();
        assert_eq!((c.region.as_str(), c.pps), ("me", 60));

        let mut w = Window::default();
        (0..3).for_each(|s| feed(&mut w, s, ip("100.70.1.1"), MIN_PPS - 1));
        assert_eq!(pick(&ts, &w.rates(3), &running), None); // below threshold
    }

    #[test]
    fn window_expires_and_ignores_partial_second() {
        let mut w = Window::default();
        (0..3).for_each(|s| feed(&mut w, s, 7, 30));
        feed(&mut w, 3, 7, 1000); // current, partial second: not counted yet
        assert_eq!(w.rates(3)[&7], 30);
        assert_eq!(w.rates(5)[&7], (30 + 1000) / 3); // seconds 2..=4
        assert!(w.rates(7).is_empty()); // all out of the window
        feed(&mut w, 20, 7, 1);
        assert!(w.buckets.len() == 1); // old buckets dropped
    }
}
