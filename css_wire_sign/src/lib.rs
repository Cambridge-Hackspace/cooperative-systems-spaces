//! Signing and checking the local wire (#249): the half of `css_wire::seal`
//! that needs cryptography.
//!
//! `css_wire` holds the seal's shape and canonical bytes and depends on serde
//! alone, because firmware builds it for an ESP32 and a crypto dependency there
//! broke a build in another repository once already
//! (checks/tests/the_wire_crate_stays_light.rs). This crate adds hmac and sha2,
//! pure Rust, which build for an ESP32 too; firmware that signs depends on it
//! as well, and firmware that does not never builds it.
//!
//! [`Peer`] is the whole of it for a real end: [`Peer::seal`] what you send,
//! [`Peer::open`] what you receive. See [`css_wire::seal`] for the scheme.
use css_wire::seal::{Seal, Sealed, canonical_bytes};
use hmac::{Hmac, Mac};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

/// How many senders' sequence numbers a receiver keeps within one boot
/// before it draws a new nonce and starts afresh. A sender that reboots
/// is a new entry; four is generous for an edge or a module, and drawing a
/// new nonce costs one stale message each way.
pub const SENDERS_KEPT: usize = 4;

fn hmac_hex(key: &[u8], bytes: &[u8]) -> String {
    let mut mac =
        <HmacSha256 as Mac>::new_from_slice(key).expect("HMAC accepts a key of any length");
    mac.update(bytes);
    let out = mac.finalize().into_bytes();
    let mut hex = String::with_capacity(out.len() * 2);
    for b in out {
        hex.push(char::from(b"0123456789abcdef"[usize::from(b >> 4)]));
        hex.push(char::from(b"0123456789abcdef"[usize::from(b & 0xf)]));
    }
    hex
}

/// Constant-time over the full length of both, so a guess learns nothing
/// from how long the comparison took.
fn same(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let mut diff = a.len() ^ b.len();
    for i in 0..a.len().max(b.len()) {
        let x = a.get(i).copied().unwrap_or(0);
        let y = b.get(i).copied().unwrap_or(0);
        diff |= usize::from(x ^ y);
    }
    diff == 0
}

/// Why a message was not acted on (#249).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refused {
    /// No seal, from a device that holds a key. Never fall back to
    /// acting on it.
    Unsigned,
    /// The seal's signature does not check out under the key.
    BadSignature,
    /// Signed, but not fresh: it names another nonce than this end's, or
    /// repeats a sequence number already accepted.
    Stale,
}

/// One end's view of one counterparty on the local wire (#249): the edge
/// keeps one per module it serves, a module one for its edge. See the
/// [seal module](css_wire::seal).
#[derive(Debug, Clone)]
pub struct Peer {
    mine: String,
    theirs: Option<String>,
    sent: u64,
    heard: Vec<(String, u64)>,
}

impl Peer {
    /// `nonce` is this end's own, drawn at random when it boots: 16 random
    /// bytes as hex is plenty. The caller draws it, since this crate has no
    /// source of randomness on a microcontroller.
    pub fn new(nonce: String) -> Self {
        Peer {
            mine: nonce,
            theirs: None,
            sent: 0,
            heard: Vec::new(),
        }
    }

    /// This end's current nonce.
    pub fn nonce(&self) -> &str {
        &self.mine
    }

    /// The counterparty's nonce as last heard, if any.
    pub fn theirs(&self) -> Option<&str> {
        self.theirs.as_deref()
    }

    /// Sign `msg` for the counterparty, under `key`, and put the seal on it.
    pub fn seal<M: Sealed>(&mut self, key: &[u8], msg: &mut M) {
        self.sent += 1;
        let to = self.theirs.clone().unwrap_or_default();
        let bytes = canonical_bytes(msg, &self.mine, &to, self.sent);
        let sig = hmac_hex(key, &bytes);
        msg.set_seal(Some(Seal {
            from: self.mine.clone(),
            to,
            seq: self.sent,
            sig,
        }));
    }

    /// Whether to act on `msg`, received from the counterparty, whose key
    /// is `key`. `Ok` only for a message that is signed, fresh, and not
    /// seen before; see the [seal module](css_wire::seal). A signed message that is
    /// refused as stale still teaches this end the sender's nonce.
    /// `new_nonce` is called only when this end must draw a new nonce of
    /// its own, which happens after [`SENDERS_KEPT`] senders.
    pub fn open<M: Sealed>(
        &mut self,
        key: &[u8],
        msg: &M,
        new_nonce: impl FnOnce() -> String,
    ) -> Result<(), Refused> {
        let seal = msg.seal().ok_or(Refused::Unsigned)?;
        let bytes = canonical_bytes(msg, &seal.from, &seal.to, seal.seq);
        if !same(&hmac_hex(key, &bytes), &seal.sig) {
            return Err(Refused::BadSignature);
        }
        if seal.to != self.mine {
            self.theirs = Some(seal.from.clone());
            return Err(Refused::Stale);
        }
        let known = self.heard.len();
        match self.heard.iter_mut().find(|(from, _)| *from == seal.from) {
            Some((_, last)) if seal.seq <= *last => return Err(Refused::Stale),
            Some((_, last)) => *last = seal.seq,
            None if known >= SENDERS_KEPT => {
                // A new nonce: everything sealed to the old one goes stale
                // at once, so forgetting the old senders opens nothing.
                self.mine = new_nonce();
                self.heard.clear();
                self.theirs = Some(seal.from.clone());
                return Err(Refused::Stale);
            }
            None => self.heard.push((seal.from.clone(), seal.seq)),
        }
        self.theirs = Some(seal.from.clone());
        Ok(())
    }
}

/// Sign `msg` statelessly, for a test or a tool that seals one message by
/// hand. Real ends use [`Peer::seal`].
pub fn seal_with<M: Sealed>(key: &[u8], msg: &mut M, from: &str, to: &str, seq: u64) {
    let sig = hmac_hex(key, &canonical_bytes(msg, from, to, seq));
    msg.set_seal(Some(Seal {
        from: from.to_string(),
        to: to.to_string(),
        seq,
        sig,
    }));
}
