//! Time-based one-time passwords (RFC 6238), for two-step log-in.
//!
//! HMAC-SHA1, 30-second steps, six digits: the parameters every authenticator
//! app assumes when an `otpauth://` URI leaves them out, so the URI names them
//! anyway and nothing is left to an app's defaults.
//!
//! A code is accepted for the step before and after the current one, so a phone
//! clock a few seconds out still works. The step a code matched is returned, and
//! the account store refuses any step at or before the last one used, so a code
//! read over somebody's shoulder cannot be replayed inside its window.

use aws_lc_rs::hmac;

/// Seconds per step.
pub const STEP_SECONDS: u64 = 30;

/// Digits per code.
pub const DIGITS: u32 = 6;

/// Bytes of secret. 160 bits, the size RFC 4226 recommends for HMAC-SHA1.
pub const SECRET_BYTES: usize = 20;

const ALPHABET: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

/// Base32 without padding, as authenticator apps expect.
#[must_use]
pub fn base32_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(5) * 8);
    let mut buffer: u32 = 0;
    let mut bits = 0u32;
    for &b in bytes {
        buffer = (buffer << 8) | u32::from(b);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(ALPHABET[((buffer >> bits) & 31) as usize] as char);
        }
    }
    if bits > 0 {
        out.push(ALPHABET[((buffer << (5 - bits)) & 31) as usize] as char);
    }
    out
}

/// Decode base32, ignoring case, spaces and padding. `None` on any other
/// character.
#[must_use]
pub fn base32_decode(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() * 5 / 8);
    let mut buffer: u32 = 0;
    let mut bits = 0u32;
    for c in text.chars().filter(|c| !c.is_whitespace() && *c != '=') {
        let up = c.to_ascii_uppercase() as u8;
        let v = ALPHABET.iter().position(|&a| a == up)? as u32;
        buffer = (buffer << 5) | v;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push(((buffer >> bits) & 0xff) as u8);
        }
    }
    Some(out)
}

/// A fresh random secret, in base32.
#[must_use]
pub fn new_secret() -> String {
    let bytes: [u8; SECRET_BYTES] = rand::random();
    base32_encode(&bytes)
}

/// The code for one step.
#[must_use]
pub fn code_at(secret: &[u8], step: u64) -> u32 {
    let key = hmac::Key::new(hmac::HMAC_SHA1_FOR_LEGACY_USE_ONLY, secret);
    let tag = hmac::sign(&key, &step.to_be_bytes());
    let mac = tag.as_ref();
    let offset = (mac[mac.len() - 1] & 0x0f) as usize;
    let bin = (u32::from(mac[offset]) & 0x7f) << 24
        | u32::from(mac[offset + 1]) << 16
        | u32::from(mac[offset + 2]) << 8
        | u32::from(mac[offset + 3]);
    bin % 10u32.pow(DIGITS)
}

/// The step a Unix time falls in.
#[must_use]
pub const fn step_of(unix_seconds: u64) -> u64 {
    unix_seconds / STEP_SECONDS
}

/// The step `code` matches, within one step either side of `now`, or `None`.
///
/// Every candidate is computed and compared, so the time taken does not say
/// which of the three steps matched or whether any did.
#[must_use]
pub fn verify(secret_b32: &str, code: &str, unix_seconds: u64) -> Option<u64> {
    let code = code.trim().replace(' ', "");
    if code.len() != DIGITS as usize || !code.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let wanted: u32 = code.parse().ok()?;
    let secret = base32_decode(secret_b32)?;
    let now = step_of(unix_seconds);
    let mut found = None;
    for step in [now.saturating_sub(1), now, now + 1] {
        let matched = constant_eq_u32(code_at(&secret, step), wanted);
        if matched && found.is_none() {
            found = Some(step);
        }
    }
    found
}

fn constant_eq_u32(a: u32, b: u32) -> bool {
    let diff = a ^ b;
    // Fold every bit into the lowest, so no branch depends on where they differ.
    let mut x = diff;
    x |= x >> 16;
    x |= x >> 8;
    x |= x >> 4;
    x |= x >> 2;
    x |= x >> 1;
    (x & 1) == 0
}

/// The `otpauth://` URI an authenticator app scans.
#[must_use]
pub fn otpauth_uri(issuer: &str, account: &str, secret_b32: &str) -> String {
    let enc = |s: &str| {
        url::form_urlencoded::byte_serialize(s.as_bytes()).collect::<String>().replace('+', "%20")
    };
    format!(
        "otpauth://totp/{}:{}?secret={secret_b32}&issuer={}&algorithm=SHA1&digits={DIGITS}&period={STEP_SECONDS}",
        enc(issuer),
        enc(account),
        enc(issuer)
    )
}

/// The URI as a QR code, an SVG document.
#[must_use]
pub fn qr_svg(uri: &str) -> String {
    match qrcode::QrCode::new(uri.as_bytes()) {
        Ok(code) => code
            .render::<qrcode::render::svg::Color<'_>>()
            .min_dimensions(200, 200)
            .quiet_zone(true)
            .build(),
        Err(_) => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base32_round_trips_and_matches_rfc_4648() {
        assert_eq!(base32_encode(b"foobar"), "MZXW6YTBOI");
        assert_eq!(base32_decode("mzxw6ytboi").unwrap(), b"foobar");
        assert_eq!(base32_decode("MZXW 6YTB OI======").unwrap(), b"foobar");
        assert!(base32_decode("not base32!").is_none());
        let s = new_secret();
        assert_eq!(base32_decode(&s).unwrap().len(), SECRET_BYTES);
    }

    // The SHA-1 vectors from RFC 6238 appendix B, cut to six digits.
    #[test]
    fn codes_match_the_rfc_6238_vectors() {
        let secret = b"12345678901234567890";
        for (t, want) in [
            (59u64, 287_082u32),
            (1_111_111_109, 81_804),
            (1_111_111_111, 50_471),
            (1_234_567_890, 5_924),
            (2_000_000_000, 279_037),
        ] {
            assert_eq!(code_at(secret, step_of(t)), want, "at {t}");
        }
    }

    #[test]
    fn a_code_is_accepted_one_step_either_side_and_not_two() {
        let b32 = base32_encode(b"12345678901234567890");
        let t = 1_111_111_111;
        let code = format!("{:06}", code_at(b"12345678901234567890", step_of(t)));
        assert_eq!(verify(&b32, &code, t), Some(step_of(t)));
        assert_eq!(verify(&b32, &code, t + 30), Some(step_of(t)));
        assert_eq!(verify(&b32, &code, t - 30), Some(step_of(t)));
        assert_eq!(verify(&b32, &code, t + 90), None);
        assert_eq!(verify(&b32, "12345", t), None);
        assert_eq!(verify(&b32, "abcdef", t), None);
    }

    #[test]
    fn the_uri_names_every_parameter() {
        let u = otpauth_uri("mcpd", "ana@example.com", "ABC");
        assert!(u.starts_with("otpauth://totp/mcpd:ana%40example.com?secret=ABC"), "{u}");
        assert!(u.contains("digits=6") && u.contains("period=30") && u.contains("algorithm=SHA1"));
        assert!(qr_svg(&u).starts_with("<?xml"));
    }
}
