//! RADPACK1 — the deterministic transport envelope (deliverable D1).
//!
//! Wire format: `RADPACK1:<tag> <blake3-of-body> <base64(deflate(body))>`
//!
//! Design constraints, learned by dogfooding RADTRACK and the arena:
//!
//! - **String-safe.** Payloads travel as rad `str` values through `\n`/`\t`
//!   line protocols and are dissected with `split(s, " ")` in user code, so
//!   the envelope is base64 text, never raw bytes.
//! - **`split(s, " ")[1]` is the digest — in every format.** User code
//!   (`digest_of` in lib_track/lib_combat) names worlds by the second
//!   space-separated token. RADPACK keeps the digest there.
//! - **Content-addressed.** The digest covers the *uncompressed canonical
//!   body*, so a world has the same name whether it shipped packed or plain,
//!   and decoders verify integrity after inflation.
//! - **Self-selecting.** Below [`PACK_THRESHOLD`] (or when DEFLATE+base64
//!   doesn't pay) the encoder emits the plain current format: small payloads
//!   stay human-readable, debuggable, and grep-able.
//! - **Deterministic.** miniz_oxide at a fixed level is a pure function of
//!   the input for a given build — the same world packs to the same bytes
//!   on every machine running the same binary, which the base-by-digest
//!   bookkeeping in sync servers relies on.

use base64::engine::general_purpose::STANDARD_NO_PAD as B64;
use base64::Engine as _;
use std::borrow::Cow;

/// Bodies smaller than this ship plain: the envelope's fixed overhead
/// (~90 B of header + 4/3 base64 expansion) eats most of the win, and
/// keeping small payloads readable is worth more than a few dozen bytes.
pub const PACK_THRESHOLD: usize = 4096;

/// DEFLATE level (0-10), fixed within this format domain so encoding remains
/// deterministic.
const LEVEL: u8 = 10;

/// Inflate ceiling — network input must not be a decompression bomb.
const MAX_BODY: usize = 1 << 28; // 256 MiB

/// The plain current form carries the digest between header and body
/// (`<tag> <digest> <body>`). `RADTRACE` is raw JSONL with no header.
fn plain_form(tag: &str, digest: &str, body: &str) -> String {
    match tag {
        "RADTRACE" => body.to_string(),
        _ => format!("{} {} {}", tag, digest, body),
    }
}

/// Encode `body` under `tag`, choosing whichever of packed/plain is
/// smaller. The digest is always blake3 of `body`.
pub fn seal(tag: &str, body: &str) -> String {
    let digest = blake3::hash(body.as_bytes()).to_hex();
    let plain = plain_form(tag, digest.as_str(), body);
    if body.len() < PACK_THRESHOLD {
        return plain;
    }
    let compressed = miniz_oxide::deflate::compress_to_vec(body.as_bytes(), LEVEL);
    let payload = B64.encode(&compressed);
    let packed = format!("RADPACK1:{} {} {}", tag, digest.as_str(), payload);
    if packed.len() < plain.len() {
        packed
    } else {
        plain
    }
}

/// First `max` bytes of `s`, backed off to a char boundary. Digest-mismatch
/// errors quote the *claimed* digest — attacker-controlled bytes — and
/// slicing those at a fixed byte index panics mid-codepoint (fuzzer
/// finding: a multi-byte char at offset 12 turned a tamper report into an
/// abort).
pub fn preview(s: &str, max: usize) -> &str {
    let mut end = s.len().min(max);
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

/// Normalize a wire string to its plain form: RADPACK1 envelopes are opened
/// (base64 -> inflate -> digest check), while plain current inputs pass
/// through untouched.
pub fn open(text: &str) -> Result<Cow<'_, str>, String> {
    let Some(rest) = text.strip_prefix("RADPACK1:") else {
        return Ok(Cow::Borrowed(text));
    };
    let (tag, rest) = rest
        .split_once(' ')
        .ok_or("radpack: malformed envelope (missing tag separator)")?;
    let (claimed, payload) = rest
        .split_once(' ')
        .ok_or("radpack: malformed envelope (missing digest separator)")?;
    let compressed = B64
        .decode(payload.trim_end())
        .map_err(|e| format!("radpack: invalid base64 payload: {}", e))?;
    let bytes = miniz_oxide::inflate::decompress_to_vec_with_limit(&compressed, MAX_BODY)
        .map_err(|e| format!("radpack: inflate failed: {}", e))?;
    let body =
        String::from_utf8(bytes).map_err(|_| "radpack: body is not valid UTF-8".to_string())?;
    let actual = blake3::hash(body.as_bytes()).to_hex();
    if claimed != actual.as_str() {
        return Err(format!(
            "radpack: integrity digest mismatch (claimed {}…, computed {}…) — \
             payload corrupted or tampered",
            preview(claimed, 12),
            &actual.as_str()[..12]
        ));
    }
    Ok(Cow::Owned(plain_form(tag, claimed, &body)))
}

/// File variant: `RADPACKZ:<tag> <digest> ` (zstd, native) or
/// `RADPACKB:<tag> <digest> ` (DEFLATE, wasm) header followed by **raw**
/// compressed bytes. Files don't traverse line protocols, so base64 would
/// be a 33% tax for nothing; and tapes are digest-dense JSONL where zstd
/// beats DEFLATE by ~25%. Small bodies stay plain text.
pub fn seal_file(tag: &str, body: &str) -> Vec<u8> {
    let digest = blake3::hash(body.as_bytes()).to_hex();
    if body.len() < PACK_THRESHOLD {
        return plain_form(tag, digest.as_str(), body).into_bytes();
    }
    #[cfg(not(target_arch = "wasm32"))]
    let (magic, compressed) = (
        "RADPACKZ",
        zstd::stream::encode_all(body.as_bytes(), ZSTD_LEVEL).unwrap_or_default(),
    );
    #[cfg(target_arch = "wasm32")]
    let (magic, compressed) = (
        "RADPACKB",
        miniz_oxide::deflate::compress_to_vec(body.as_bytes(), LEVEL),
    );
    let mut out = format!("{}:{} {} ", magic, tag, digest.as_str()).into_bytes();
    if compressed.is_empty() || compressed.len() + out.len() >= body.len() {
        return plain_form(tag, digest.as_str(), body).into_bytes();
    }
    out.extend_from_slice(&compressed);
    out
}

/// zstd level for file envelopes. Fixed: part of the deterministic encode.
#[cfg(not(target_arch = "wasm32"))]
const ZSTD_LEVEL: i32 = 19;

/// Open a file payload: RADPACKZ (zstd), RADPACKB (DEFLATE), RADPACK1
/// (text envelope), or plain text — all normalize to the same plain string
/// form. Both platform encodings decode on every target that can.
pub fn open_file(bytes: &[u8]) -> Result<String, String> {
    for magic in ["RADPACKZ:", "RADPACKB:"] {
        let Some(rest) = bytes.strip_prefix(magic.as_bytes()) else {
            continue;
        };
        let header_end = rest
            .iter()
            .enumerate()
            .filter(|(_, &b)| b == b' ')
            .map(|(i, _)| i)
            .nth(1)
            .ok_or("radpack: malformed binary envelope header")?;
        let header = std::str::from_utf8(&rest[..header_end])
            .map_err(|_| "radpack: malformed binary envelope header".to_string())?;
        let (tag, claimed) = header
            .split_once(' ')
            .ok_or("radpack: malformed binary envelope header")?;
        let compressed = &rest[header_end + 1..];
        let body_bytes = if magic == "RADPACKZ:" {
            #[cfg(not(target_arch = "wasm32"))]
            {
                use std::io::Read as _;
                let mut out = Vec::new();
                let dec = zstd::stream::Decoder::new(compressed)
                    .map_err(|e| format!("radpack: zstd decode failed: {}", e))?;
                // bounded read: decompression bombs are refused, not OOMed
                dec.take(MAX_BODY as u64 + 1)
                    .read_to_end(&mut out)
                    .map_err(|e| format!("radpack: zstd decode failed: {}", e))?;
                if out.len() > MAX_BODY {
                    return Err("radpack: body exceeds inflate ceiling".into());
                }
                out
            }
            #[cfg(target_arch = "wasm32")]
            {
                return Err("radpack: RADPACKZ tapes need a native build".into());
            }
        } else {
            miniz_oxide::inflate::decompress_to_vec_with_limit(compressed, MAX_BODY)
                .map_err(|e| format!("radpack: inflate failed: {}", e))?
        };
        let body = String::from_utf8(body_bytes)
            .map_err(|_| "radpack: body is not valid UTF-8".to_string())?;
        let actual = blake3::hash(body.as_bytes()).to_hex();
        if claimed != actual.as_str() {
            return Err(format!(
                "radpack: integrity digest mismatch (claimed {}…, computed {}…) — \
                 file corrupted or tampered",
                preview(claimed, 12),
                &actual.as_str()[..12]
            ));
        }
        return Ok(plain_form(tag, claimed, &body));
    }
    let text = std::str::from_utf8(bytes)
        .map_err(|_| "radpack: file is neither RADPACK binary nor text".to_string())?;
    open(text).map(|c| c.into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tiny deterministic generator — enough entropy to stress the codec
    /// without pulling a proptest dependency.
    struct XorShift(u64);
    impl XorShift {
        fn next(&mut self) -> u64 {
            let mut x = self.0;
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            self.0 = x;
            x
        }
    }

    fn gen_body(rng: &mut XorShift, len: usize, repetitive: bool) -> String {
        let mut s = String::with_capacity(len);
        while s.len() < len {
            if repetitive {
                s.push_str("{\"entities\":[[7,\"T-1\",[[\"Ticket\",[1,\"open\"]]]]],");
            } else {
                let v = rng.next();
                // mix ascii, digits, escapes-in-waiting, and multibyte
                match v % 5 {
                    0 => s.push_str(&format!("{}", v)),
                    1 => s.push((b'a' + (v % 26) as u8) as char),
                    2 => s.push('"'),
                    3 => s.push('\\'),
                    _ => s.push('é'),
                }
            }
        }
        s.truncate(len);
        s
    }

    /// The property: for every tag and any body, open(seal(body)) yields
    /// exactly the plain form — packed or not.
    #[test]
    fn seal_open_round_trips_for_arbitrary_bodies() {
        let mut rng = XorShift(0x5eed_cafe_f00d_0001);
        for tag in ["RADFORK2", "RADDELTA1", "RADWORLD3", "RADTRACE"] {
            for &(len, repetitive) in &[
                (0usize, false),
                (1, false),
                (100, false),
                (PACK_THRESHOLD - 1, true),
                (PACK_THRESHOLD + 1, true),
                (20_000, true),
                (20_000, false), // high-entropy: packing may not pay; must still round-trip
                (300_000, true),
            ] {
                let body = gen_body(&mut rng, len, repetitive);
                let digest = blake3::hash(body.as_bytes()).to_hex();
                let sealed = seal(tag, &body);
                let opened = open(&sealed).expect("open must succeed on sealed output");
                assert_eq!(
                    opened.as_ref(),
                    plain_form(tag, digest.as_str(), &body),
                    "tag={} len={} repetitive={}",
                    tag,
                    len,
                    repetitive
                );
            }
        }
    }

    /// Big repetitive bodies (which is what world saves and tapes are) must
    /// actually shrink — that's the whole point of D1.
    #[test]
    fn repetitive_bodies_pack_substantially_smaller() {
        let mut rng = XorShift(0x5eed_cafe_f00d_0002);
        let body = gen_body(&mut rng, 100_000, true);
        let sealed = seal("RADFORK2", &body);
        assert!(sealed.starts_with("RADPACK1:RADFORK2 "));
        assert!(
            sealed.len() * 4 < body.len(),
            "expected ≥4x: body {} B, sealed {} B",
            body.len(),
            sealed.len()
        );
        // digest stays at split(s, " ")[1] — user-space digest_of() relies on it
        let digest = blake3::hash(body.as_bytes()).to_hex();
        assert_eq!(sealed.split(' ').nth(1), Some(digest.as_str()));
    }

    /// Sub-threshold bodies stay plain and readable.
    #[test]
    fn small_bodies_stay_plain() {
        let body = "{\"entities\":[]}";
        assert_eq!(
            seal("RADDELTA1", body),
            format!(
                "RADDELTA1 {} {}",
                blake3::hash(body.as_bytes()).to_hex(),
                body
            )
        );
        assert_eq!(seal("RADTRACE", body), body);
    }

    /// Tampered payloads must refuse loudly, never inflate garbage.
    #[test]
    fn tampering_is_detected() {
        let mut rng = XorShift(0x5eed_cafe_f00d_0003);
        let body = gen_body(&mut rng, 50_000, true);
        let sealed = seal("RADFORK2", &body);
        assert!(sealed.starts_with("RADPACK1:"));

        // flip a character inside the base64 payload
        let mut bytes: Vec<u8> = sealed.clone().into_bytes();
        let pos = bytes.len() - 10;
        bytes[pos] = if bytes[pos] == b'A' { b'B' } else { b'A' };
        let tampered = String::from_utf8(bytes).unwrap();
        assert!(open(&tampered).is_err(), "tampered payload must not open");

        // and a wrong claimed digest is caught even when inflate succeeds
        let with_bad_digest =
            sealed.replacen(sealed.split(' ').nth(1).unwrap(), &"0".repeat(64), 1);
        assert!(open(&with_bad_digest).is_err());
    }

    /// File envelope: binary round trip, tamper detection, plain text
    /// passthrough.
    #[test]
    fn file_envelope_round_trips_and_detects_corruption() {
        let mut rng = XorShift(0x5eed_cafe_f00d_0004);
        let body = gen_body(&mut rng, 60_000, true);
        let sealed = seal_file("RADTRACE", &body);
        #[cfg(not(target_arch = "wasm32"))]
        assert!(sealed.starts_with(b"RADPACKZ:RADTRACE "));
        #[cfg(target_arch = "wasm32")]
        assert!(sealed.starts_with(b"RADPACKB:RADTRACE "));
        assert!(
            sealed.len() * 4 < body.len(),
            "file envelope must beat 4x on repetitive bodies"
        );
        assert_eq!(open_file(&sealed).expect("open_file"), body);

        let mut corrupt = sealed.clone();
        let last = corrupt.len() - 1;
        corrupt[last] ^= 0xFF;
        assert!(open_file(&corrupt).is_err(), "corrupted tail must refuse");

        // Plain raw trace text passes through.
        assert_eq!(
            open_file(b"{\"t\":\"header\"}\n").expect("plain trace"),
            "{\"t\":\"header\"}\n"
        );
        // and the text envelope is also accepted from a file
        let text_sealed = seal("RADTRACE", &body);
        assert_eq!(
            open_file(text_sealed.as_bytes()).expect("text envelope"),
            body
        );

        // The WASM DEFLATE envelope decodes on native targets too.
        let digest = blake3::hash(body.as_bytes()).to_hex();
        let mut wasm_form = format!("RADPACKB:RADTRACE {} ", digest.as_str()).into_bytes();
        wasm_form.extend_from_slice(&miniz_oxide::deflate::compress_to_vec(
            body.as_bytes(),
            LEVEL,
        ));
        assert_eq!(open_file(&wasm_form).expect("wasm deflate form"), body);
    }

    /// Plain current strings pass through without allocation.
    #[test]
    fn plain_passthrough_is_borrowing_identity() {
        for s in [
            "RADFORK2 abc {\"entities\":[]}",
            "RADDELTA1 def {}",
            "RADWORLD3 abc {\"entities\":[]}",
            "{\"t\":\"header\"}\n{\"t\":\"end\"}\n",
            "",
        ] {
            match open(s).expect("plain input must pass through") {
                Cow::Borrowed(out) => assert_eq!(out, s),
                Cow::Owned(_) => panic!("plain input must not be copied"),
            }
        }
    }
}
