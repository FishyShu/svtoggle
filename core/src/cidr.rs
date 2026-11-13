use std::net::Ipv4Addr;

/// IPv4 prefix. IPv6 isn't needed: every source list we use is v4.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cidr {
    pub net: u32,
    pub len: u8,
}

impl Cidr {
    pub fn parse(s: &str) -> Result<Self, String> {
        let s = s.trim();
        let (ip, len) = s.split_once('/').unwrap_or((s, "32"));
        let ip: Ipv4Addr = ip.parse().map_err(|_| format!("bad ip in '{s}'"))?;
        let len: u8 = len.parse().map_err(|_| format!("bad prefix in '{s}'"))?;
        if len > 32 {
            return Err(format!("prefix > 32 in '{s}'"));
        }
        let mask = Self::mask(len);
        Ok(Cidr { net: u32::from(ip) & mask, len })
    }

    fn mask(len: u8) -> u32 {
        if len == 0 { 0 } else { u32::MAX << (32 - len) }
    }

    /// True if the two prefixes share any address (one contains the other).
    pub fn overlaps(&self, o: &Cidr) -> bool {
        let m = Self::mask(self.len.min(o.len));
        self.net & m == o.net & m
    }
}

impl std::fmt::Display for Cidr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", Ipv4Addr::from(self.net), self.len)
    }
}

/// Parse a list, rejecting the whole thing on the first bad entry (never silently drop a range).
pub fn parse_all(items: &[String]) -> Result<Vec<Cidr>, String> {
    items.iter().map(|s| Cidr::parse(s)).collect()
}

/// Every (blocked, protected) pair that overlaps. Empty means safe.
pub fn violations(block: &[Cidr], protect: &[Cidr]) -> Vec<(Cidr, Cidr)> {
    let mut v = Vec::new();
    for b in block {
        for p in protect {
            if b.overlaps(p) {
                v.push((*b, *p));
            }
        }
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    fn c(s: &str) -> Cidr { Cidr::parse(s).unwrap() }

    #[test]
    fn parse_masks_host_bits() {
        assert_eq!(c("100.70.1.7/16").to_string(), "100.70.0.0/16");
        assert_eq!(c("1.2.3.4").len, 32);
        assert!(Cidr::parse("1.2.3.4/33").is_err());
        assert!(Cidr::parse("nope").is_err());
    }

    #[test]
    fn adjacent_is_not_overlap_but_contained_is() {
        // the real footgun: blocked 100.64.32.0/19 sits next to protected 100.64.0.0/19
        assert!(!c("100.64.32.0/19").overlaps(&c("100.64.0.0/19")));
        assert!(c("100.64.0.0/16").overlaps(&c("100.64.0.0/19"))); // roll-up swallows the protected range
        assert!(c("100.64.5.0/24").overlaps(&c("100.64.0.0/19")));
    }

    #[test]
    fn violations_reports_pairs() {
        let v = violations(&[c("100.70.0.0/15")], &[c("100.70.0.0/16"), c("8.8.8.0/24")]);
        assert_eq!(v.len(), 1);
    }
}
