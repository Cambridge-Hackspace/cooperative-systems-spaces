//! Signing the local wire (#249).
//!
//! Until #249 nothing on the local broker was signed. #121 gave every device a
//! `command_key` and signed the two door messages on the *site* broker; the
//! edge then re-published an unlock to the latch as an ordinary scan reply, and
//! a lease, a condition report or a usage report could be forged by anyone who
//! could publish on the local broker. This module is how a message on the local
//! wire proves who sent it, and that it is not a replay.
//!
//! This crate holds the seal's shape and its canonical bytes, which need
//! nothing but serde. Signing and checking, which need hmac and sha2, are in
//! `css_wire_sign` beside it, so firmware that does not sign builds as light as
//! before (checks/tests/the_wire_crate_stays_light.rs).
//!
//! # The seal
//!
//! A signed message carries one extra field, `seal`:
//!
//! ```json
//! "seal": { "from": "…sender's nonce…", "to": "…receiver's nonce…", "seq": 7, "sig": "…64 hex…" }
//! ```
//!
//! - `sig` is HMAC-SHA256, under the **module's** `command_key`, of the
//!   message's [canonical bytes](#the-canonical-bytes). The edge holds the
//!   keys of the modules it serves; a module holds only its own. A key shared
//!   between modules would let any of them forge for the others.
//! - `from` is the sender's own nonce, drawn at random when it boots.
//! - `to` is the receiver's nonce, as the sender last heard it: empty when it
//!   has not heard one yet.
//! - `seq` rises by one with every message the sender seals for that receiver,
//!   from 1 at boot.
//!
//! # Freshness, without clocks
//!
//! A receiver acts on a message only when the signature checks, `to` is its
//! **own current nonce**, and `seq` is higher than any it has accepted from that
//! `from` since it booted. So:
//!
//! - a message recorded before the receiver last booted names a nonce the
//!   receiver no longer has, and is refused;
//! - a message replayed within the receiver's boot repeats a `seq` already
//!   accepted from that sender, and is refused;
//! - a sender that reboots starts `seq` again at 1 under a new `from`, and is
//!   believed, because it can only name the receiver's nonce by having heard it
//!   since.
//!
//! A message that is signed but stale is still read for one thing: its `from`,
//! so the receiver can name the sender correctly in what it sends next. That is
//! how two ends that have just met, or just rebooted, find each other: the
//! first message each way is refused as stale, and teaches the other its
//! nonce. A forged stale message can at worst make a receiver name the wrong
//! nonce until the real sender's next message, which is the denial of service
//! anyone who can publish on the broker has anyway.
//!
//! What this does **not** stop: a message withheld and delivered late, so long
//! as nothing newer from that sender arrived first. The lease's own bound is
//! what limits that: a withheld grant, delivered late, energizes for one
//! `ttl_ms` at most.
//!
//! # The canonical bytes
//!
//! The MAC is over bytes built from the message's fields, never over its JSON,
//! so no difference in how two implementations print JSON can break it. Each
//! value is written as its length in bytes, in decimal, a colon, and the value:
//! `5:hello`. An absent optional value is the single byte `~`, which a length
//! never begins with, so absent and empty differ. The bytes are:
//!
//! ```text
//! "css-local/1" topic device_id from to seq field…
//! ```
//!
//! every item written that way, the first being the version, and each message's own
//! fields in the order its [`Sealed::canon`] lists them. A boolean is `true` or
//! `false`; an integer is decimal; a floating-point number is the hex of its
//! IEEE-754 bits, lower case, 8 digits for 32-bit and 16 for 64-bit, taken from
//! the value **as it parses from the JSON sent**.

use serde::{Deserialize, Serialize};

/// The `seal` on a signed local message (#249). See the [module](self).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Seal {
    pub from: String,
    pub to: String,
    pub seq: u64,
    pub sig: String,
}

/// Builds a message's canonical bytes (#249): each value length-prefixed, an
/// absent one `~`. See the [module](self).
#[derive(Debug, Default)]
pub struct Canon(Vec<u8>);

impl Canon {
    fn item(&mut self, bytes: &[u8]) {
        self.0.extend_from_slice(bytes.len().to_string().as_bytes());
        self.0.push(b':');
        self.0.extend_from_slice(bytes);
    }

    pub fn str(&mut self, v: &str) -> &mut Self {
        self.item(v.as_bytes());
        self
    }

    pub fn opt_str(&mut self, v: Option<&str>) -> &mut Self {
        match v {
            Some(v) => self.str(v),
            None => self.absent(),
        }
    }

    pub fn bool(&mut self, v: bool) -> &mut Self {
        self.str(if v { "true" } else { "false" })
    }

    pub fn opt_bool(&mut self, v: Option<bool>) -> &mut Self {
        match v {
            Some(v) => self.bool(v),
            None => self.absent(),
        }
    }

    pub fn int(&mut self, v: i64) -> &mut Self {
        self.str(&v.to_string())
    }

    pub fn opt_int(&mut self, v: Option<i64>) -> &mut Self {
        match v {
            Some(v) => self.int(v),
            None => self.absent(),
        }
    }

    pub fn opt_f32(&mut self, v: Option<f32>) -> &mut Self {
        match v {
            Some(v) => self.str(&format!("{:08x}", v.to_bits())),
            None => self.absent(),
        }
    }

    pub fn opt_f64(&mut self, v: Option<f64>) -> &mut Self {
        match v {
            Some(v) => self.str(&format!("{:016x}", v.to_bits())),
            None => self.absent(),
        }
    }

    pub fn absent(&mut self) -> &mut Self {
        self.0.push(b'~');
        self
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.0
    }
}

/// A local message that can carry a [`Seal`] (#249).
pub trait Sealed {
    /// The topic it travels on: signed into the bytes, so a message cannot be
    /// replayed as a different kind of message.
    const TOPIC: &'static str;
    /// The device it is to or from. Signed into the bytes, so a message cannot
    /// be redirected to, or claimed by, another device.
    fn device(&self) -> Option<&str>;
    /// The message's own fields, in their canonical order. Everything that
    /// decides what the receiver does, or shows a person, is in it.
    fn canon(&self, c: &mut Canon);
    fn seal(&self) -> Option<&Seal>;
    fn set_seal(&mut self, seal: Option<Seal>);
}

/// The bytes the MAC covers: everything but `sig` itself.
pub fn canonical_bytes<M: Sealed>(msg: &M, from: &str, to: &str, seq: u64) -> Vec<u8> {
    let mut c = Canon::default();
    c.str("css-local/1")
        .str(M::TOPIC)
        .opt_str(msg.device())
        .str(from)
        .str(to)
        .str(&seq.to_string());
    msg.canon(&mut c);
    c.into_bytes()
}
