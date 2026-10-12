//! End-user identity, asserted by a proxy you have said to trust.
//!
//! mcpd authenticates keys, not people. A key answers "which credential acted",
//! which is what the audit log's `principal` records as a truncated digest. It
//! does not answer "who", and on a shared deployment that is the question the
//! audit log gets read to answer: not "an admin key deleted this run" but "which
//! person deleted this run".
//!
//! mcpd has no identity provider. A person's identity is what your ingress
//! asserts through the trusted header, and authorisation is a key's scope and
//! its project list. The ingress has already run the SSO flow and already knows
//! whose session this is, so the honest bridge is to let it tell mcpd, in one
//! header, and to record what it says as the audit log's `subject`.
//!
//! ## Why the address list exists
//!
//! A header is a string anybody who can open a socket to the port can send. If
//! mcpd believed the header on its own, every caller could name themselves, and
//! the `subject` column would be worth less than the empty string it replaced,
//! because an empty column is honestly empty while a forged one reads as fact.
//!
//! So the header is read only when the connection came from an address the
//! operator named. Both halves are required and neither has a default: with no
//! header name configured the feature is off and this module always answers
//! `None`.
//!
//! ## What is refused, and when
//!
//! Configuration is checked at startup, because every mistake here fails
//! quietly at runtime: a range mcpd cannot parse, a header set with no range to
//! read it from, a range set with no header to read. Each of those produces a
//! daemon that runs, serves traffic, and writes an empty subject on every row,
//! which is indistinguishable from the feature not being turned on. An operator
//! finds that out weeks later, while reading an audit log during an incident.
//!
//! At runtime the answer is `None` for an untrusted peer, an unknown peer, an
//! absent header, a duplicated header, and a value that could corrupt the line
//! it is about to be written into. `None` becomes an empty subject, which is the
//! behaviour before this module existed. Failing closed costs an attribution;
//! failing open records a lie.

use axum::http::{HeaderMap, HeaderName};
use std::fmt;
use std::net::IpAddr;

/// Longest subject accepted, in bytes after trimming.
///
/// Generous against every real identity: an email address is capped at 254
/// bytes by RFC 5321, and the OIDC subject identifiers issued in practice are
/// shorter. A value past this is a bug or an attempt to pad a log line, and
/// neither is worth storing.
pub const MAX_SUBJECT_BYTES: usize = 256;

/// A configuration mistake that stops the daemon starting.
///
/// Each message names the flag, what was given, and the edit that fixes it,
/// because these are read once, by somebody who is now blocked.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ConfigError {
    /// A range with no `/`.
    #[error(
        "--trusted-proxy-cidr wants an address and a prefix length, got {got:?}; \
             write it as 10.0.0.0/8 or 2001:db8::/32"
    )]
    NoPrefix {
        /// What was configured.
        got: String,
    },

    /// The part before the `/` is not an IP address.
    #[error("--trusted-proxy-cidr got {got:?}, and {address:?} is not an IPv4 or IPv6 address")]
    BadAddress {
        /// What was configured.
        got: String,
        /// The part before the slash.
        address: String,
    },

    /// The part after the `/` is not a number.
    #[error("--trusted-proxy-cidr got {got:?}, and the prefix length {prefix:?} is not a number")]
    BadPrefix {
        /// What was configured.
        got: String,
        /// The part after the slash.
        prefix: String,
    },

    /// The prefix length is longer than the address family has bits.
    #[error("--trusted-proxy-cidr got {got:?}, and an {family} prefix length is at most {max}")]
    PrefixTooLong {
        /// What was configured.
        got: String,
        /// `IPv4` or `IPv6`.
        family: &'static str,
        /// The longest prefix that family accepts.
        max: u8,
    },

    /// Bits are set below the prefix, so the range does not mean what it looks
    /// like it means.
    #[error(
        "--trusted-proxy-cidr got {got:?}, which sets bits below its prefix; \
             write {network} for the whole subnet, or {host} for that one host"
    )]
    HostBitsSet {
        /// What was configured.
        got: String,
        /// The range they probably meant, with the host bits cleared.
        network: String,
        /// The single address they may have meant instead.
        host: String,
    },

    /// The header name is not a legal HTTP header name.
    #[error("--trusted-identity-header got {got:?}, which is not a valid HTTP header name")]
    BadHeaderName {
        /// What was configured.
        got: String,
    },

    /// A header to read, and nowhere it is believed from.
    #[error(
        "--trusted-identity-header is set with no --trusted-proxy-cidr, so no request \
             could ever carry a subject; name the address range your proxy connects from"
    )]
    HeaderWithoutRanges,

    /// The client address header is not a legal HTTP header name.
    #[error("--client-ip-header got {got:?}, which is not a valid HTTP header name")]
    BadClientIpHeader {
        /// What was configured.
        got: String,
    },

    /// A client address header, and nowhere it is believed from.
    #[error(
        "--client-ip-header is set with no --trusted-proxy-cidr, so the header would never \
             be believed; name the address range your proxy connects from"
    )]
    ClientIpWithoutRanges,

    /// Addresses to trust, and nothing to read from them.
    #[error(
        "--trusted-proxy-cidr is set with no --trusted-identity-header and no \
             --client-ip-header, so the ranges are never consulted; name the header your \
             proxy sets, or drop the ranges"
    )]
    RangesWithoutHeader,
}

/// One CIDR range: a base address, and how many of its leading bits are fixed.
///
/// Parsed here rather than with a crate. `std::net` already parses both address
/// families, masking a prefix is a dozen lines, and the workspace carries no
/// CIDR dependency of its own. Adding one would put a crate into the released
/// binary's dependency graph and into `deny.toml`'s licence and advisory
/// surface, which is a larger change than the code it saves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cidr {
    base: IpAddr,
    prefix: u8,
}

impl Cidr {
    /// Parse `10.0.0.0/8`, `192.0.2.7/32` or `2001:db8::/32`.
    ///
    /// # Errors
    /// Returns the [`ConfigError`] describing what is wrong. Every one of them
    /// stops startup, because a range mcpd cannot read is a range whose reach
    /// nobody knows, and carrying on would leave the operator unable to see
    /// whether their proxy is trusted or not.
    pub fn parse(text: &str) -> Result<Self, ConfigError> {
        let got = text.trim();
        let Some((address, prefix)) = got.split_once('/') else {
            return Err(ConfigError::NoPrefix { got: got.to_owned() });
        };
        let base: IpAddr = address.parse().map_err(|_| ConfigError::BadAddress {
            got: got.to_owned(),
            address: address.to_owned(),
        })?;
        let (family, max) = match base {
            IpAddr::V4(_) => ("IPv4", 32),
            IpAddr::V6(_) => ("IPv6", 128),
        };
        let prefix: u8 = prefix.parse().map_err(|_| ConfigError::BadPrefix {
            got: got.to_owned(),
            prefix: prefix.to_owned(),
        })?;
        if prefix > max {
            return Err(ConfigError::PrefixTooLong { got: got.to_owned(), family, max });
        }

        // `192.168.1.5/24` is accepted by most tools as "the /24 containing
        // that host". It is refused here, because in a trust list it is the
        // exact shape of a typo for `/32`, and the two differ by 254 addresses
        // that are now allowed to name whoever they like.
        let network = mask(base, prefix);
        if network != base {
            return Err(ConfigError::HostBitsSet {
                got: got.to_owned(),
                network: format!("{network}/{prefix}"),
                host: format!("{base}/{max}"),
            });
        }
        Ok(Self { base, prefix })
    }

    /// Whether `addr` falls inside this range.
    ///
    /// An address of the other family never matches. There is no ordering
    /// between the two, so a v4 range says nothing about a v6 peer.
    #[must_use]
    pub fn contains(&self, addr: IpAddr) -> bool {
        match (self.base, addr) {
            (IpAddr::V4(_), IpAddr::V4(_)) | (IpAddr::V6(_), IpAddr::V6(_)) => {
                mask(addr, self.prefix) == self.base
            }
            _ => false,
        }
    }
}

impl fmt::Display for Cidr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.base, self.prefix)
    }
}

/// Clear every bit below `prefix`.
fn mask(addr: IpAddr, prefix: u8) -> IpAddr {
    match addr {
        IpAddr::V4(a) => {
            let mut o = a.octets();
            mask_octets(&mut o, prefix);
            IpAddr::from(o)
        }
        IpAddr::V6(a) => {
            let mut o = a.octets();
            mask_octets(&mut o, prefix);
            IpAddr::from(o)
        }
    }
}

fn mask_octets(octets: &mut [u8], prefix: u8) {
    let prefix = usize::from(prefix);
    for (i, byte) in octets.iter_mut().enumerate() {
        let above = i * 8;
        *byte &= if prefix >= above + 8 {
            0xff
        } else if prefix <= above {
            0x00
        } else {
            // One to seven bits of this byte are inside the prefix.
            0xffu8 << (8 - (prefix - above))
        };
    }
}

/// An IPv4 peer that arrived over a dual-stack socket, as plain IPv4.
///
/// A listener on `[::]` reports a v4 client as `::ffff:10.0.0.7`, so an
/// operator who wrote `10.0.0.0/8` would find their proxy untrusted for a
/// reason nothing in the configuration shows. Only the v4-mapped form is
/// unwrapped: the deprecated v4-compatible form (`::10.0.0.7`) is left alone,
/// because it is not a way anybody should reach this port.
fn unmapped(addr: IpAddr) -> Option<IpAddr> {
    match addr {
        IpAddr::V6(a) => a.to_ipv4_mapped().map(IpAddr::V4),
        IpAddr::V4(_) => None,
    }
}

/// Which header carries an asserted identity, and who is allowed to set it.
///
/// The default is off: no header name, no ranges, and [`Self::subject`] answers
/// `None` for every request. That is what a deployment behind no proxy wants,
/// and it is what every deployment gets until an operator names both halves.
#[derive(Debug, Clone, Default)]
pub struct TrustedIdentity {
    /// `None` turns the whole feature off.
    header: Option<HeaderName>,
    /// Addresses permitted to set that header. Never empty when `header` is
    /// set: [`Self::new`] refuses the combination.
    trusted: Vec<Cidr>,
}

impl TrustedIdentity {
    /// Build from what the operator configured.
    ///
    /// # Errors
    /// Returns a [`ConfigError`] for an unparseable range, an illegal header
    /// name, or either half configured without the other. All three are
    /// startup failures: see the module documentation for why none of them can
    /// be left to fail at runtime.
    pub fn new(header: Option<&str>, ranges: &[String]) -> Result<Self, ConfigError> {
        let header = header.map(str::trim).filter(|h| !h.is_empty());
        let trusted =
            ranges.iter().map(|r| Cidr::parse(r)).collect::<Result<Vec<_>, ConfigError>>()?;

        match (header, trusted.is_empty()) {
            (None, true) => Ok(Self::default()),
            (None, false) => Err(ConfigError::RangesWithoutHeader),
            (Some(_), true) => Err(ConfigError::HeaderWithoutRanges),
            (Some(name), false) => {
                let header = HeaderName::try_from(name)
                    .map_err(|_| ConfigError::BadHeaderName { got: name.to_owned() })?;
                Ok(Self { header: Some(header), trusted })
            }
        }
    }

    /// Whether an identity header is configured at all.
    #[must_use]
    pub fn is_enabled(&self) -> bool {
        self.header.is_some()
    }

    /// The header read, for `mcpd config` to print.
    #[must_use]
    pub fn header(&self) -> Option<&HeaderName> {
        self.header.as_ref()
    }

    /// The ranges permitted to set it, for `mcpd config` to print.
    #[must_use]
    pub fn ranges(&self) -> &[Cidr] {
        &self.trusted
    }

    /// Whether `peer` is inside any trusted range.
    #[must_use]
    pub fn trusts(&self, peer: IpAddr) -> bool {
        let unwrapped = unmapped(peer);
        self.trusted.iter().any(|c| c.contains(peer) || unwrapped.is_some_and(|u| c.contains(u)))
    }

    /// The identity this request may claim, or `None`.
    ///
    /// `peer` is optional because the address is not always knowable: a router
    /// built without connection info, or a future transport that has no peer,
    /// both produce `None` rather than a panic, and an unknown peer is not a
    /// trusted one.
    ///
    /// `None` means "record no subject". It is returned when the feature is
    /// off, the peer is unknown, the peer is outside every trusted range, the
    /// header is absent, the header is present more than once, or the value
    /// cannot be written safely into a log line.
    #[must_use]
    pub fn subject(&self, headers: &HeaderMap, peer: Option<IpAddr>) -> Option<String> {
        let name = self.header.as_ref()?;
        if !self.trusts(peer?) {
            return None;
        }
        // More than one copy of the header is ambiguous, and the ambiguity is
        // the attack: a proxy that appends rather than overwrites leaves the
        // client's value sitting beside the real one, and picking either is a
        // guess. Two identities is no identity.
        let mut values = headers.get_all(name).iter();
        let first = values.next()?;
        if values.next().is_some() {
            return None;
        }
        sanitise(first.as_bytes())
    }
}

/// The subject a header value carries, or `None` if it cannot be trusted in a
/// log line.
///
/// The value is attacker influenced even when the proxy is honest, because the
/// proxy is asserting something about whoever is talking to it. A subject is
/// written to storage and read back by people and by log tooling, so anything
/// that can end a line, start a new record, or reorder what a reader sees is
/// refused outright rather than stripped. Stripping would turn two distinct
/// identities into one string, which is worse in an audit log than no identity.
fn sanitise(raw: &[u8]) -> Option<String> {
    let trimmed = raw.trim_ascii();
    // Refused rather than truncated, for the same reason. Two long identities
    // sharing a prefix would truncate to the same subject, and the log would
    // then attribute one person's action to another with no sign that it had.
    if trimmed.is_empty() || trimmed.len() > MAX_SUBJECT_BYTES {
        return None;
    }
    let text = std::str::from_utf8(trimmed).ok()?;
    if text.chars().any(corrupts_a_line) {
        return None;
    }
    Some(text.to_owned())
}

/// Whether a character can change how the line it sits in is read.
fn corrupts_a_line(c: char) -> bool {
    // Covers C0 (newline, carriage return, tab, NUL) and C1.
    c.is_control()
        // Rust does not class these as control characters, but a log reader
        // that splits on Unicode line breaks does end a record on them.
        || matches!(c, '\u{2028}' | '\u{2029}')
        // Bidirectional overrides reorder what a human sees without changing
        // the bytes, so a subject can be made to render as somebody else.
        || matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
}

/// Where a request came from, for the per-address limits on sign-up and log-in.
///
/// Behind a proxy every connection comes from the proxy, so the peer address
/// alone would put every client in one bucket and one busy client would lock
/// everybody else out. The proxy names the client in a header, and that header
/// is believed only from an address in `--trusted-proxy-cidr`, for the same
/// reason the identity header is: anybody who can open a socket to the port
/// can send it.
///
/// Off by default. With no header configured the answer is the peer address,
/// which is right for a daemon nobody put a proxy in front of.
#[derive(Debug, Clone, Default)]
pub struct ClientIp {
    /// The header the proxy sets, such as `cf-connecting-ip`. `None` reads the
    /// peer address only.
    header: Option<HeaderName>,
    /// Addresses permitted to set it.
    trusted: Vec<Cidr>,
}

impl ClientIp {
    /// Build from what the operator configured.
    ///
    /// # Errors
    /// Returns a [`ConfigError`] for an unparseable range, an illegal header
    /// name, or a header with no range to believe it from.
    pub fn new(header: Option<&str>, ranges: &[String]) -> Result<Self, ConfigError> {
        let header = header.map(str::trim).filter(|h| !h.is_empty());
        let Some(name) = header else { return Ok(Self::default()) };
        let header = HeaderName::try_from(name)
            .map_err(|_| ConfigError::BadClientIpHeader { got: name.to_owned() })?;
        let trusted =
            ranges.iter().map(|r| Cidr::parse(r)).collect::<Result<Vec<_>, ConfigError>>()?;
        if trusted.is_empty() {
            return Err(ConfigError::ClientIpWithoutRanges);
        }
        Ok(Self { header: Some(header), trusted })
    }

    /// The header read, for `mcpd config` to print.
    #[must_use]
    pub fn header(&self) -> Option<&HeaderName> {
        self.header.as_ref()
    }

    /// The ranges permitted to set it, for `mcpd config` to print.
    #[must_use]
    pub fn ranges(&self) -> &[Cidr] {
        &self.trusted
    }

    /// The client's address, or `None` when the peer is unknown.
    ///
    /// From a trusted peer: the configured header, then the left-most
    /// `X-Forwarded-For` entry, then the peer itself. From any other peer: the
    /// peer, whatever the headers say.
    #[must_use]
    pub fn resolve(&self, headers: &HeaderMap, peer: Option<IpAddr>) -> Option<IpAddr> {
        let peer = peer?;
        let Some(name) = &self.header else { return Some(peer) };
        let unwrapped = unmapped(peer);
        let trusted = self
            .trusted
            .iter()
            .any(|c| c.contains(peer) || unwrapped.is_some_and(|u| c.contains(u)));
        if !trusted {
            return Some(peer);
        }
        let parse = |v: &str| v.trim().parse::<IpAddr>().ok();
        // One copy only. Two copies of the client header is the shape of a
        // proxy that appends, and choosing either is a guess.
        let mut values = headers.get_all(name).iter();
        if let (Some(first), None) = (values.next(), values.next()) {
            if let Some(ip) = first.to_str().ok().and_then(parse) {
                return Some(ip);
            }
        }
        let forwarded = headers
            .get("x-forwarded-for")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.split(',').next())
            .and_then(parse);
        Some(forwarded.unwrap_or(peer))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    const HEADER: &str = "x-forwarded-user";

    fn ip(s: &str) -> IpAddr {
        s.parse().expect("the test address should parse")
    }

    fn ranges(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    fn configured(list: &[&str]) -> TrustedIdentity {
        TrustedIdentity::new(Some(HEADER), &ranges(list))
            .expect("the test configuration should parse")
    }

    fn header_value(value: &[u8]) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(HEADER, HeaderValue::from_bytes(value).expect("the test value should be legal"));
        h
    }

    // ------------------------------------------------------------- the switch ---

    #[test]
    fn off_by_default_even_when_the_header_is_present() {
        // The whole feature is opt in. A deployment that never names a header
        // must behave exactly as it did before this module existed, however
        // convincing the request looks.
        let off = TrustedIdentity::default();
        assert!(!off.is_enabled());
        assert_eq!(off.subject(&header_value(b"root@example.com"), Some(ip("127.0.0.1"))), None);
        assert_eq!(off.subject(&header_value(b"root@example.com"), Some(ip("::1"))), None);
    }

    #[test]
    fn a_trusted_peer_with_the_header_yields_the_subject() {
        let id = configured(&["10.4.0.0/16"]);
        assert_eq!(
            id.subject(&header_value(b"ana@example.com"), Some(ip("10.4.9.2"))),
            Some("ana@example.com".to_owned())
        );
    }

    #[test]
    fn an_untrusted_peer_is_never_believed() {
        // THE security property. The header is identical, the value is
        // identical, and the only difference is which address sent it. If this
        // test ever passes a subject through, anyone who can reach the port can
        // write any name into the audit log, and the column becomes a forgery
        // rather than a gap.
        let id = configured(&["10.4.0.0/16"]);
        let headers = header_value(b"ana@example.com");
        for attacker in ["10.5.0.1", "192.0.2.9", "127.0.0.1", "::1"] {
            assert_eq!(
                id.subject(&headers, Some(ip(attacker))),
                None,
                "{attacker} is outside 10.4.0.0/16 and must not be able to name itself"
            );
        }
        // And the trusted address still works, so the test is not passing
        // because the feature is broken and returns None for everybody.
        assert_eq!(id.subject(&headers, Some(ip("10.4.0.1"))), Some("ana@example.com".to_owned()));
    }

    #[test]
    fn an_unknown_peer_is_not_a_trusted_one() {
        // A router built without connection info yields no address. Failing
        // closed here is what keeps that wiring mistake from opening the header
        // to everybody.
        let id = configured(&["10.0.0.0/8"]);
        assert_eq!(id.subject(&header_value(b"ana@example.com"), None), None);
    }

    #[test]
    fn an_absent_header_yields_nothing() {
        let id = configured(&["10.0.0.0/8"]);
        assert_eq!(id.subject(&HeaderMap::new(), Some(ip("10.0.0.1"))), None);

        // A different header from the same trusted peer is still not it.
        let mut other = HeaderMap::new();
        other.insert("x-forwarded-for", HeaderValue::from_static("10.0.0.1"));
        assert_eq!(id.subject(&other, Some(ip("10.0.0.1"))), None);
    }

    #[test]
    fn a_duplicated_header_is_refused() {
        // A proxy that appends leaves the client's copy beside the real one.
        let id = configured(&["10.0.0.0/8"]);
        let mut h = HeaderMap::new();
        h.append(HEADER, HeaderValue::from_static("ana@example.com"));
        h.append(HEADER, HeaderValue::from_static("root@example.com"));
        assert_eq!(id.subject(&h, Some(ip("10.0.0.1"))), None);
    }

    #[test]
    fn the_header_name_is_matched_case_insensitively() {
        let id = TrustedIdentity::new(Some("X-Forwarded-User"), &ranges(&["10.0.0.0/8"]))
            .expect("the header name should parse");
        assert_eq!(
            id.subject(&header_value(b"ana@example.com"), Some(ip("10.0.0.1"))),
            Some("ana@example.com".to_owned())
        );
    }

    // -------------------------------------------------------------- the ranges ---

    #[test]
    fn both_ends_of_a_range_are_inside_it_and_the_neighbours_are_not() {
        let id = configured(&["10.1.2.0/24"]);
        let headers = header_value(b"ana");
        let inside = |a: &str| id.subject(&headers, Some(ip(a))).is_some();

        assert!(inside("10.1.2.0"), "the network address is in the range");
        assert!(inside("10.1.2.255"), "the broadcast address is in the range");
        assert!(!inside("10.1.1.255"), "one below the range is outside it");
        assert!(!inside("10.1.3.0"), "one above the range is outside it");
    }

    #[test]
    fn a_slash_32_is_exactly_one_address() {
        let id = configured(&["192.0.2.7/32"]);
        assert!(id.trusts(ip("192.0.2.7")));
        assert!(!id.trusts(ip("192.0.2.6")));
        assert!(!id.trusts(ip("192.0.2.8")));
    }

    #[test]
    fn a_slash_128_is_exactly_one_address() {
        let id = configured(&["2001:db8::7/128"]);
        assert!(id.trusts(ip("2001:db8::7")));
        assert!(!id.trusts(ip("2001:db8::6")));
        assert!(!id.trusts(ip("2001:db8::8")));
    }

    #[test]
    fn ipv6_ranges_work_at_both_ends() {
        let id = configured(&["2001:db8:abcd::/48"]);
        let headers = header_value(b"ana");
        let inside = |a: &str| id.subject(&headers, Some(ip(a))).is_some();

        assert!(inside("2001:db8:abcd::"), "the first address is in the range");
        assert!(
            inside("2001:db8:abcd:ffff:ffff:ffff:ffff:ffff"),
            "the last address is in the range"
        );
        assert!(!inside("2001:db8:abcc:ffff:ffff:ffff:ffff:ffff"), "one below is outside");
        assert!(!inside("2001:db8:abce::"), "one above is outside");
    }

    #[test]
    fn a_prefix_that_is_not_a_whole_number_of_bytes_masks_correctly() {
        // /12 and /67 both cut a byte in half, which is where an off-by-one in
        // the mask would hide.
        let v4 = configured(&["172.16.0.0/12"]);
        assert!(v4.trusts(ip("172.16.0.0")));
        assert!(v4.trusts(ip("172.31.255.255")));
        assert!(!v4.trusts(ip("172.15.255.255")));
        assert!(!v4.trusts(ip("172.32.0.0")));

        let v6 = configured(&["2001:db8:0:0:2000::/67"]);
        assert!(v6.trusts(ip("2001:db8:0:0:2000::")));
        assert!(v6.trusts(ip("2001:db8:0:0:3fff:ffff:ffff:ffff")));
        assert!(!v6.trusts(ip("2001:db8:0:0:1fff:ffff:ffff:ffff")));
        assert!(!v6.trusts(ip("2001:db8:0:0:4000::")));
    }

    #[test]
    fn the_two_address_families_do_not_reach_into_each_other() {
        let v4 = configured(&["0.0.0.0/0"]);
        assert!(v4.trusts(ip("203.0.113.1")), "0.0.0.0/0 covers every IPv4 address");
        assert!(!v4.trusts(ip("2001:db8::1")), "an IPv4 range must not match an IPv6 peer");

        let v6 = configured(&["::/0"]);
        assert!(v6.trusts(ip("2001:db8::1")), "::/0 covers every IPv6 address");
        assert!(!v6.trusts(ip("203.0.113.1")), "an IPv6 range must not match an IPv4 peer");
    }

    #[test]
    fn a_dual_stack_listener_reports_an_ipv4_proxy_as_ipv4_mapped() {
        // A listener on [::] hands back ::ffff:10.0.0.7 for a v4 client. The
        // operator wrote 10.0.0.0/8 and is entitled to have it work.
        let id = configured(&["10.0.0.0/8"]);
        assert!(id.trusts(ip("::ffff:10.0.0.7")));
        assert!(!id.trusts(ip("::ffff:11.0.0.7")), "unmapping must not widen the range");
        // The deprecated v4-compatible form is a different address and is not
        // unwrapped, so it matches only a v6 range that really contains it.
        assert!(!id.trusts(ip("::10.0.0.7")));
    }

    // ------------------------------------------------------- the configuration ---

    #[test]
    fn a_malformed_range_stops_the_daemon_starting() {
        // Each of these has to fail. Skipping an unreadable range would leave a
        // daemon whose trust list is not the one the operator wrote, with
        // nothing on screen to say so.
        let cases: &[(&str, ConfigError)] = &[
            ("10.0.0.0", ConfigError::NoPrefix { got: "10.0.0.0".into() }),
            (
                "10.0.0.256/8",
                ConfigError::BadAddress {
                    got: "10.0.0.256/8".into(),
                    address: "10.0.0.256".into(),
                },
            ),
            (
                "10.0.0.0/eight",
                ConfigError::BadPrefix { got: "10.0.0.0/eight".into(), prefix: "eight".into() },
            ),
            (
                "10.0.0.0/33",
                ConfigError::PrefixTooLong { got: "10.0.0.0/33".into(), family: "IPv4", max: 32 },
            ),
            (
                "2001:db8::/129",
                ConfigError::PrefixTooLong {
                    got: "2001:db8::/129".into(),
                    family: "IPv6",
                    max: 128,
                },
            ),
            ("", ConfigError::NoPrefix { got: String::new() }),
        ];
        for (text, expected) in cases {
            assert_eq!(Cidr::parse(text).unwrap_err(), *expected, "parsing {text:?}");
            assert_eq!(
                TrustedIdentity::new(Some(HEADER), &ranges(&[text])).unwrap_err(),
                *expected,
                "configuring {text:?}"
            );
        }
    }

    #[test]
    fn a_range_with_host_bits_set_is_refused_and_the_error_offers_both_readings() {
        // 192.168.1.5/24 is the typo for /32, and it trusts 254 addresses the
        // operator did not mean to trust.
        let e = Cidr::parse("192.168.1.5/24").unwrap_err();
        assert_eq!(
            e,
            ConfigError::HostBitsSet {
                got: "192.168.1.5/24".into(),
                network: "192.168.1.0/24".into(),
                host: "192.168.1.5/32".into(),
            }
        );
        let message = e.to_string();
        assert!(message.contains("192.168.1.0/24"), "the message should offer the subnet");
        assert!(message.contains("192.168.1.5/32"), "the message should offer the host");

        assert!(Cidr::parse("2001:db8::1/64").is_err());
        // The same addresses written as their own networks are fine.
        assert!(Cidr::parse("192.168.1.0/24").is_ok());
        assert!(Cidr::parse("192.168.1.5/32").is_ok());
        assert!(Cidr::parse("2001:db8::1/128").is_ok());
    }

    #[test]
    fn half_a_configuration_is_refused() {
        // Both of these start, serve traffic, and write an empty subject on
        // every row, which looks exactly like the feature being off.
        assert_eq!(
            TrustedIdentity::new(Some(HEADER), &[]).unwrap_err(),
            ConfigError::HeaderWithoutRanges
        );
        assert_eq!(
            TrustedIdentity::new(None, &ranges(&["10.0.0.0/8"])).unwrap_err(),
            ConfigError::RangesWithoutHeader
        );
        // Neither half is the off switch, and it is not an error.
        assert!(TrustedIdentity::new(None, &[]).is_ok());
        // An empty header name is the same as not setting one.
        assert!(TrustedIdentity::new(Some("  "), &[]).is_ok());
    }

    #[test]
    fn an_illegal_header_name_is_refused() {
        let e =
            TrustedIdentity::new(Some("x forwarded user"), &ranges(&["10.0.0.0/8"])).unwrap_err();
        assert_eq!(e, ConfigError::BadHeaderName { got: "x forwarded user".into() });
    }

    #[test]
    fn a_range_renders_back_as_it_was_written() {
        // `mcpd config` prints these, and an operator compares them against
        // what they set.
        assert_eq!(Cidr::parse("10.0.0.0/8").unwrap().to_string(), "10.0.0.0/8");
        assert_eq!(Cidr::parse("2001:db8::/32").unwrap().to_string(), "2001:db8::/32");
    }

    // ---------------------------------------------------------------- the value ---

    #[test]
    fn a_control_character_in_the_value_is_refused() {
        let id = configured(&["10.0.0.0/8"]);
        let peer = Some(ip("10.0.0.1"));

        // U+0085 NEXT LINE, as UTF-8. Both bytes are above 0x7f, so this is a
        // legal HTTP header value and it reaches the sanitiser intact.
        assert_eq!(id.subject(&header_value("ana\u{85}root".as_bytes()), peer), None);
        // A Unicode line separator, which several log readers treat as the end
        // of a record.
        assert_eq!(id.subject(&header_value("ana\u{2028}root".as_bytes()), peer), None);
        // A bidirectional override, which changes what a human reads without
        // changing a byte.
        assert_eq!(id.subject(&header_value("ana\u{202e}root".as_bytes()), peer), None);
        // Bytes that are not UTF-8 at all.
        assert_eq!(id.subject(&header_value(&[0xff, 0xfe]), peer), None);

        // The plain C0 characters cannot be put in a `HeaderValue` at all, so
        // they are checked against the sanitiser directly. The guard is there
        // for any future path that does not build one.
        assert_eq!(sanitise(b"ana\nroot"), None);
        assert_eq!(sanitise(b"ana\rroot"), None);
        assert_eq!(sanitise(b"ana\troot"), None);
        assert_eq!(sanitise(b"ana\0root"), None);
        assert!(HeaderValue::from_bytes(b"ana\nroot").is_err());
    }

    #[test]
    fn an_overlong_value_is_refused_rather_than_truncated() {
        let id = configured(&["10.0.0.0/8"]);
        let peer = Some(ip("10.0.0.1"));

        let at_cap = "a".repeat(MAX_SUBJECT_BYTES);
        assert_eq!(id.subject(&header_value(at_cap.as_bytes()), peer), Some(at_cap));

        // Truncating would map two different long identities onto one subject,
        // and the audit log would then name the wrong person with no sign of it.
        let over = "a".repeat(MAX_SUBJECT_BYTES + 1);
        assert_eq!(id.subject(&header_value(over.as_bytes()), peer), None);
    }

    #[test]
    fn surrounding_whitespace_is_trimmed_and_an_empty_value_is_nothing() {
        let id = configured(&["10.0.0.0/8"]);
        let peer = Some(ip("10.0.0.1"));
        assert_eq!(
            id.subject(&header_value(b"  ana@example.com  "), peer),
            Some("ana@example.com".to_owned())
        );
        assert_eq!(id.subject(&header_value(b""), peer), None);
        assert_eq!(id.subject(&header_value(b"   "), peer), None);
    }

    #[test]
    fn an_ordinary_identity_survives_unchanged() {
        // The shapes an ingress actually asserts: an email, an OIDC subject, a
        // Windows principal name, and one with non-ASCII in it.
        let id = configured(&["10.0.0.0/8"]);
        let peer = Some(ip("10.0.0.1"));
        for subject in [
            "ana@example.com",
            "auth0|65f0c1d2e3b4a5c6d7e8f901",
            "EXAMPLE\\ana",
            "ana.garcía@example.com",
        ] {
            assert_eq!(
                id.subject(&header_value(subject.as_bytes()), peer),
                Some(subject.to_owned()),
                "{subject} should pass through unchanged"
            );
        }
    }
}
