//! The local wire's seal (#249): signature, freshness, and what each message
//! signs. Every message here crosses a real JSON round trip, as it would the
//! broker, so a field that does not survive one is caught too.
//!
use css_wire::seal::Sealed;
use css_wire::*;
use css_wire_sign::{Peer, Refused, seal_with};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

const KEY: &[u8] = &[0x0a, 0x0b, 0x0c, 0x0d];

fn deliver<M: Serialize + DeserializeOwned>(m: &M) -> M {
    serde_json::from_str(&serde_json::to_string(m).unwrap()).unwrap()
}

fn alex() -> LocalMember {
    LocalMember {
        id: "6f1c0b8e-0000-4000-8000-000000000001".to_string(),
        name: "Alex Doe".to_string(),
    }
}

fn lease(grant: bool) -> ResourceLeasePayload {
    ResourceLeasePayload {
        resource: "7c9e6679-7425-40de-944b-e07fc1f90ae7".to_string(),
        device_id: "dev-1".to_string(),
        grant,
        ttl_ms: 3000,
        reason: None,
        member: None,
        seal: None,
    }
    .with_member(Some(alex()))
}

fn nonces() -> impl FnMut() -> String {
    let mut n = 0;
    move || {
        n += 1;
        format!("rotated-{n}")
    }
}

/// An edge and a module that have each heard the other once: the first
/// message each way is refused as stale, and teaches the other its nonce.
fn met() -> (Peer, Peer) {
    let mut edge = Peer::new("n-edge".to_string());
    let mut module = Peer::new("n-mod".to_string());
    let mut hello = LocalAliveRequest {
        device_id: "dev-1".to_string(),
        seal: None,
    };
    module.seal(KEY, &mut hello);
    assert_eq!(
        edge.open(KEY, &deliver(&hello), nonces()),
        Err(Refused::Stale)
    );
    let mut first = lease(true);
    edge.seal(KEY, &mut first);
    assert_eq!(module.open(KEY, &deliver(&first), nonces()), Ok(()));
    (edge, module)
}

#[test]
fn two_ends_that_have_met_believe_each_other_both_ways() {
    let (mut edge, mut module) = met();
    let mut l = lease(true);
    edge.seal(KEY, &mut l);
    assert_eq!(module.open(KEY, &deliver(&l), nonces()), Ok(()));
    let mut usage = LocalUsage {
        resource: "laser".to_string(),
        seconds: Some(1.1),
        device_id: Some("dev-1".to_string()),
        ..Default::default()
    };
    module.seal(KEY, &mut usage);
    assert_eq!(edge.open(KEY, &deliver(&usage), nonces()), Ok(()));
}

#[test]
fn a_first_message_is_stale_and_teaches_the_senders_nonce() {
    let mut edge = Peer::new("n-edge".to_string());
    let mut module = Peer::new("n-mod".to_string());
    assert_eq!(edge.theirs(), None);
    let mut hello = LocalAliveRequest {
        device_id: "dev-1".to_string(),
        seal: None,
    };
    module.seal(KEY, &mut hello);
    assert_eq!(
        hello.seal.as_ref().unwrap().to,
        "",
        "it has heard no nonce yet"
    );
    assert_eq!(
        edge.open(KEY, &deliver(&hello), nonces()),
        Err(Refused::Stale)
    );
    assert_eq!(edge.theirs(), Some("n-mod"));
}

#[test]
fn a_message_replayed_is_refused() {
    let (mut edge, mut module) = met();
    let mut l = lease(true);
    edge.seal(KEY, &mut l);
    let wire = deliver(&l);
    assert_eq!(module.open(KEY, &wire, nonces()), Ok(()));
    assert_eq!(module.open(KEY, &wire, nonces()), Err(Refused::Stale));
}

#[test]
fn an_older_message_after_a_newer_one_is_refused() {
    let (mut edge, mut module) = met();
    let (mut older, mut newer) = (lease(true), lease(false));
    edge.seal(KEY, &mut older);
    edge.seal(KEY, &mut newer);
    assert_eq!(module.open(KEY, &deliver(&newer), nonces()), Ok(()));
    assert_eq!(
        module.open(KEY, &deliver(&older), nonces()),
        Err(Refused::Stale)
    );
}

#[test]
fn a_message_recorded_before_the_receiver_rebooted_is_refused() {
    let (mut edge, _module) = met();
    let mut l = lease(true);
    edge.seal(KEY, &mut l);
    let recorded = deliver(&l);
    let mut rebooted = Peer::new("n-mod-2".to_string());
    assert_eq!(rebooted.open(KEY, &recorded, nonces()), Err(Refused::Stale));
}

#[test]
fn a_sender_that_reboots_is_believed_once_it_has_heard_the_receiver() {
    let (mut edge, mut module) = met();
    for _ in 0..3 {
        // Several leases from the old edge, so the module's high-water mark
        // is well above where the new one starts.
        let mut l = lease(true);
        edge.seal(KEY, &mut l);
        assert_eq!(module.open(KEY, &deliver(&l), nonces()), Ok(()));
    }
    let mut edge2 = Peer::new("n-edge-2".to_string());
    let mut hello = LocalAliveRequest {
        device_id: "dev-1".to_string(),
        seal: None,
    };
    module.seal(KEY, &mut hello);
    assert_eq!(
        edge2.open(KEY, &deliver(&hello), nonces()),
        Err(Refused::Stale)
    );
    let mut l = lease(true);
    edge2.seal(KEY, &mut l);
    assert_eq!(
        l.seal.as_ref().unwrap().seq,
        1,
        "a rebooted sender starts again"
    );
    assert_eq!(module.open(KEY, &deliver(&l), nonces()), Ok(()));
}

#[test]
fn a_sender_that_reboots_does_not_reopen_its_old_messages() {
    let (mut edge, mut module) = met();
    let mut withheld = lease(true);
    edge.seal(KEY, &mut withheld);
    let mut later = lease(true);
    edge.seal(KEY, &mut later);
    assert_eq!(module.open(KEY, &deliver(&later), nonces()), Ok(()));
    // The edge reboots and they meet again.
    let mut edge2 = Peer::new("n-edge-2".to_string());
    let mut hello = LocalAliveRequest {
        device_id: "dev-1".to_string(),
        seal: None,
    };
    module.seal(KEY, &mut hello);
    let _ = edge2.open(KEY, &deliver(&hello), nonces());
    let mut l = lease(true);
    edge2.seal(KEY, &mut l);
    assert_eq!(module.open(KEY, &deliver(&l), nonces()), Ok(()));
    // An older message from the old edge, with a lower seq than was accepted
    // from it, stays refused.
    assert_eq!(
        module.open(KEY, &deliver(&withheld), nonces()),
        Err(Refused::Stale)
    );
}

#[test]
fn too_many_senders_draw_a_new_nonce_and_everything_old_goes_stale() {
    let mut module = Peer::new("n-mod".to_string());
    let mut accepted = Vec::new();
    for i in 0..css_wire_sign::SENDERS_KEPT {
        let mut l = lease(true);
        seal_with(KEY, &mut l, &format!("edge-boot-{i}"), "n-mod", 1);
        assert_eq!(module.open(KEY, &deliver(&l), nonces()), Ok(()));
        accepted.push(l);
    }
    let mut one_more = lease(true);
    seal_with(KEY, &mut one_more, "edge-boot-new", "n-mod", 1);
    let mut draw = nonces();
    assert_eq!(
        module.open(KEY, &deliver(&one_more), &mut draw),
        Err(Refused::Stale)
    );
    assert_eq!(module.nonce(), "rotated-1");
    // An unseen message to the old nonce, from a sender it had forgotten:
    // stale, because the nonce it names is gone.
    let mut late = lease(true);
    seal_with(KEY, &mut late, "edge-boot-0", "n-mod", 99);
    assert_eq!(
        module.open(KEY, &deliver(&late), nonces()),
        Err(Refused::Stale)
    );
    // And it starts afresh: a new sender that has heard the new nonce is
    // believed, rather than drawing yet another.
    let mut fresh = lease(true);
    seal_with(KEY, &mut fresh, "edge-boot-next", "rotated-1", 1);
    assert_eq!(module.open(KEY, &deliver(&fresh), nonces()), Ok(()));
    assert_eq!(module.nonce(), "rotated-1");
}

#[test]
fn an_unsigned_message_is_refused_as_unsigned() {
    let mut module = Peer::new("n-mod".to_string());
    assert_eq!(
        module.open(KEY, &deliver(&lease(true)), nonces()),
        Err(Refused::Unsigned)
    );
}

#[test]
fn the_wrong_key_is_a_bad_signature_and_teaches_nothing() {
    let mut edge = Peer::new("n-edge".to_string());
    let mut module = Peer::new("n-mod".to_string());
    let mut hello = LocalAliveRequest {
        device_id: "dev-1".to_string(),
        seal: None,
    };
    module.seal(b"another device's key", &mut hello);
    assert_eq!(
        edge.open(KEY, &deliver(&hello), nonces()),
        Err(Refused::BadSignature)
    );
    assert_eq!(
        edge.theirs(),
        None,
        "a forgery must not set the nonce it names"
    );
}

#[test]
fn a_garbage_signature_is_refused_not_panicked() {
    let (mut edge, mut module) = met();
    let mut l = lease(true);
    edge.seal(KEY, &mut l);
    let real = l.seal.as_ref().unwrap().sig.clone();
    // The real signature with a NUL after it: JSON can carry one, and a
    // comparison that pads the shorter side with zeros would accept it.
    let padded = format!("{real}\u{0}");
    let short = real[..63].to_string();
    for sig in ["", "zz", &"0".repeat(64), &"0".repeat(200), &padded, &short] {
        let mut forged = l.clone();
        forged.seal.as_mut().unwrap().sig = sig.to_string();
        assert_eq!(
            module.open(KEY, &deliver(&forged), nonces()),
            Err(Refused::BadSignature)
        );
    }
}

#[test]
fn a_refused_lease_never_names_a_member() {
    assert_eq!(lease(false).member, None);
    assert_eq!(lease(true).member, Some(alex()));
}

#[test]
fn a_lease_with_no_member_or_seal_is_the_lease_from_before_249() {
    let l = ResourceLeasePayload {
        resource: "r".to_string(),
        device_id: "d".to_string(),
        grant: true,
        ttl_ms: 3000,
        reason: None,
        member: None,
        seal: None,
    };
    assert_eq!(
        serde_json::to_string(&l).unwrap(),
        r#"{"resource":"r","device_id":"d","grant":true,"ttl_ms":3000,"reason":null}"#
    );
}

// ── every field is signed ────────────────────────────────────────────────────

/// Seal `m`, then change each field the JSON carries, one at a time, and
/// require the signature to break -- except for `unsigned`, which must not.
/// The fields come from the message as serialized, not from the canonical
/// list, so a field the canonical bytes forget is caught here.
fn every_field_is_signed<M>(mut m: M, unsigned: &[&str])
where
    M: Sealed + Serialize + DeserializeOwned,
{
    seal_with(KEY, &mut m, "from", "to", 5);
    let sealed = serde_json::to_value(&m).unwrap();
    let Value::Object(fields) = &sealed else {
        panic!("a message is an object")
    };
    let mut changed = 0;
    for path in leaves(&Value::Object(fields.clone()), "") {
        if path.starts_with("/seal/sig") {
            continue;
        }
        let mut v = sealed.clone();
        let leaf = v.pointer_mut(&path).unwrap();
        *leaf = tamper(leaf);
        let forged: M = match serde_json::from_value(v) {
            Ok(forged) => forged,
            // A sign-in's kind names which field carries its value, so a
            // changed kind does not parse as the same message: it cannot be
            // changed under the signature at all. Only that path may fail.
            Err(_) if path == "/sign_in/kind" => {
                changed += 1;
                continue;
            }
            Err(e) => panic!("{path}: {e}"),
        };
        let ok = css_wire::seal::canonical_bytes(&forged, "from", "to", 5)
            == css_wire::seal::canonical_bytes(&m, "from", "to", 5)
            && forged.seal().map(|s| (&s.from, &s.to, s.seq))
                == m.seal().map(|s| (&s.from, &s.to, s.seq));
        if unsigned.contains(&path.as_str()) {
            assert!(ok, "{path} is meant to be outside the signature");
        } else {
            assert!(
                !ok,
                "{} {path} can be changed under the same signature",
                M::TOPIC
            );
            changed += 1;
        }
    }
    assert!(changed > 0);
}

fn leaves(v: &Value, at: &str) -> Vec<String> {
    match v {
        Value::Object(m) => m
            .iter()
            .flat_map(|(k, v)| leaves(v, &format!("{at}/{k}")))
            .collect(),
        _ => vec![at.to_string()],
    }
}

fn tamper(v: &Value) -> Value {
    match v {
        Value::Bool(b) => Value::Bool(!b),
        Value::Number(n) if n.is_u64() || n.is_i64() => {
            Value::from(n.as_i64().unwrap_or(0).wrapping_add(1))
        }
        Value::Number(n) => Value::from(n.as_f64().unwrap() + 0.5),
        Value::String(s) if s == "code" || s == "totp" || s == "refused" => {
            Value::String(if s == "code" { "refused" } else { "code" }.to_string())
        }
        Value::String(s) => Value::String(format!("{s}x")),
        Value::Null => Value::String("x".to_string()),
        other => panic!("no tamper for {other}"),
    }
}

fn dev() -> Option<String> {
    Some("dev-1".to_string())
}

#[test]
fn a_lease_signs_everything_but_its_reason() {
    let mut l = lease(true);
    l.reason = Some("for logs".to_string());
    every_field_is_signed(l, &["/reason"]);
}

#[test]
fn a_scan_reply_signs_everything() {
    for sign_in in [
        LocalSignIn::Code { code: "c".into() },
        LocalSignIn::Totp { ticket: "t".into() },
        LocalSignIn::Refused { reason: "r".into() },
    ] {
        let mut r = LocalScanReply::new("door", true, "authorized", Some(4200))
            .with_member(Some(alex()))
            .with_sign_in(Some(sign_in));
        r.device_id = dev();
        every_field_is_signed(r, &[]);
    }
}

#[test]
fn every_request_signs_everything() {
    every_field_is_signed(
        LocalAliveRequest {
            device_id: "dev-1".into(),
            seal: None,
        },
        &[],
    );
    every_field_is_signed(
        LocalCondition {
            device_id: "dev-1".into(),
            resource: "laser".into(),
            condition: "door_open".into(),
            asserted: true,
            seal: None,
        },
        &[],
    );
    every_field_is_signed(
        LocalScan {
            resource: "laser".into(),
            card: "04a1b2".into(),
            device_id: dev(),
            seal: None,
        },
        &[],
    );
    every_field_is_signed(
        LocalStop {
            resource: "laser".into(),
            card: Some("04a1b2".into()),
            device_id: dev(),
            seal: None,
        },
        &[],
    );
    every_field_is_signed(
        LocalUsage {
            resource: "laser".into(),
            card: Some("04a1b2".into()),
            seconds: Some(1.1),
            active_seconds: Some(0.7),
            temperature: Some(21.5),
            device_id: dev(),
            seal: None,
        },
        &[],
    );
    every_field_is_signed(
        LocalPower {
            resource: "laser".into(),
            device_id: dev(),
            relay_on: Some(true),
            draw_now: Some(1.25),
            voltage_now: Some(230.1),
            max_voltage: Some(240.0),
            amperage_limit: Some(13.0),
            self_tripped: Some(false),
            seal: None,
        },
        &[],
    );
}

#[test]
fn absent_and_empty_are_different_messages() {
    let stop = |card: Option<&str>| LocalStop {
        resource: "laser".into(),
        card: card.map(str::to_string),
        device_id: dev(),
        seal: None,
    };
    assert_ne!(
        css_wire::seal::canonical_bytes(&stop(None), "f", "t", 1),
        css_wire::seal::canonical_bytes(&stop(Some("")), "f", "t", 1)
    );
}

// ── the bytes, as FIRMWARE.md gives them ────────────────────────────────────
//
// Computed independently of this crate, with Python's hmac:
//
//   F = lambda s: f"{len(s.encode())}:{s}"
//   msg = "".join(F(x) for x in [...]) (+ "~" for an absent value)
//   hmac.new(bytes.fromhex("0a0b0c0d"), msg.encode(), "sha256").hexdigest()

#[test]
fn a_lease_signs_the_documented_bytes() {
    let mut l = ResourceLeasePayload {
        resource: "res-1".into(),
        device_id: "dev-1".into(),
        grant: true,
        ttl_ms: 3000,
        reason: None,
        member: None,
        seal: None,
    }
    .with_member(Some(LocalMember {
        id: "u-1".into(),
        name: "Cal".into(),
    }));
    assert_eq!(
        css_wire::seal::canonical_bytes(&l, "n-edge", "n-mod", 7),
        b"11:css-local/114:resource/lease5:dev-16:n-edge5:n-mod1:75:res-14:true4:30003:u-13:Cal"
    );
    seal_with(KEY, &mut l, "n-edge", "n-mod", 7);
    assert_eq!(
        l.seal.unwrap().sig,
        "72e3a622991a939f980a6780ab7818e76e4cfdf08ce6d0fc4de571d738fdcd00"
    );
}

#[test]
fn an_absent_value_signs_as_a_tilde() {
    let mut stop = LocalStop {
        resource: "res-1".into(),
        card: None,
        device_id: dev(),
        seal: None,
    };
    assert_eq!(
        css_wire::seal::canonical_bytes(&stop, "n-mod", "n-edge", 2),
        b"11:css-local/121:resource/request/stop5:dev-15:n-mod6:n-edge1:25:res-1~"
    );
    seal_with(KEY, &mut stop, "n-mod", "n-edge", 2);
    assert_eq!(
        stop.seal.unwrap().sig,
        "afc98d6b5e92d602ad2f2fb1e2b6dd578342e3a306e3a76bab029fd5d1854ad7"
    );
}

#[test]
fn a_float_signs_as_its_bits_as_parsed_from_the_json() {
    let usage = deliver(&LocalUsage {
        resource: "laser".into(),
        seconds: Some(1.1),
        active_seconds: Some(0.7),
        device_id: dev(),
        ..Default::default()
    });
    let mut usage = usage;
    assert_eq!(
        css_wire::seal::canonical_bytes(&usage, "n-mod", "n-edge", 3),
        b"11:css-local/122:resource/request/usage5:dev-15:n-mod6:n-edge1:35:laser~8:3f8ccccd8:3f333333~"
    );
    seal_with(KEY, &mut usage, "n-mod", "n-edge", 3);
    assert_eq!(
        usage.seal.unwrap().sig,
        "63041bc69198566f46d907be93a33400116bd8838feaea8aa7aea3385b2313a7"
    );
    let mut power = deliver(&LocalPower {
        resource: "laser".into(),
        device_id: dev(),
        relay_on: Some(true),
        draw_now: Some(1.25),
        ..Default::default()
    });
    assert_eq!(
        css_wire::seal::canonical_bytes(&power, "n-mod", "n-edge", 4),
        b"11:css-local/122:resource/request/power5:dev-15:n-mod6:n-edge1:45:laser4:true16:3ff4000000000000~~~~"
    );
    seal_with(KEY, &mut power, "n-mod", "n-edge", 4);
    assert_eq!(
        power.seal.unwrap().sig,
        "30db447392b41d3a0acd780a40f59d5ceb1850a98de897fb2b6df8f353a9532b"
    );
}
