//! The edge's half of signing the local wire (#249).
//!
//! The edge holds the `command_key` of every module it coordinates, fetched
//! from `GET /api/toolguard/module-keys`, and one [`Peer`] per module: the
//! nonces and sequence numbers that make a sealed message fresh. It seals what
//! it sends every device it holds a key for, on every resource, and checks
//! every sealed message from one, because a device with a key believes nothing
//! unsealed. On a resource an administrator has set to **Require signed
//! messages** it also demands it: nothing sent to a device without a key, and
//! nothing about the resource acted on unless sealed by a device bound to it.
//!
//! The scheme itself, and why it needs no clock, is in `css_wire::seal`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, RwLock};

use css_lib::wire::ModuleKeysPayload;
use css_wire::seal::Sealed;
use css_wire_sign::{Peer, Refused};
use rand::RngCore;

/// Why a message about a signed resource was not acted on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotAdmitted {
    /// It names no device, so there is no key to check it with.
    Unnamed,
    /// It names a device this edge holds no key for.
    NoKey,
    /// It names a device not bound to the resource it is about.
    NotBound,
    /// The seal itself: absent, wrong, or stale.
    Seal(Refused),
}

/// This edge's keys and peers.
#[derive(Default)]
pub struct LocalSeals {
    keys: RwLock<HashMap<String, Vec<u8>>>,
    /// Where the keys are kept between runs, if anywhere. A device with a key
    /// believes nothing unsealed, so an edge that restarted while the server
    /// was out of reach would otherwise speak to none of them until it was
    /// back.
    cache: Option<PathBuf>,
    peers: Mutex<HashMap<String, Peer>>,
    /// When each device was last sent a hello, so a stale message replayed
    /// over and over gets one a second at most.
    hellos: Mutex<HashMap<String, std::time::Instant>>,
}

/// The least time between two hellos to one device.
pub const HELLO_EVERY: std::time::Duration = std::time::Duration::from_secs(1);

/// A fresh nonce: 16 random bytes as hex, as `css_wire::seal` asks.
fn nonce() -> String {
    let mut b = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut b);
    b.iter().map(|x| format!("{x:02x}")).collect()
}

impl LocalSeals {
    pub fn new() -> Self {
        Self::default()
    }

    /// Keys kept at `path` between runs: those it holds now, if it can be
    /// read, and every later [`set_keys`](Self::set_keys) written back to it,
    /// owner-only. A file that is missing or unreadable is no keys, said so,
    /// and never a reason not to start: the next poll fills them in.
    pub fn cached_at(path: PathBuf) -> Self {
        let seals = LocalSeals {
            cache: Some(path.clone()),
            ..Self::default()
        };
        match load_keys(&path) {
            Ok(Some(payload)) => {
                seals.install(payload);
                tracing::info!(
                    "Holding {} module key(s) from {} until the server is asked",
                    seals.key_count(),
                    path.display()
                );
            }
            Ok(None) => {}
            Err(e) => tracing::warn!(
                "Module keys at {} could not be read ({e:#}); none held until the server is asked",
                path.display()
            ),
        }
        seals
    }

    /// Replace the keys with the server's current answer. A device no longer
    /// listed loses its key and its peer, so nothing it sealed is believed
    /// any more, and a device given a new key starts afresh.
    pub fn set_keys(&self, payload: ModuleKeysPayload) {
        if let Some(path) = &self.cache {
            if let Err(e) = save_keys(path, &payload) {
                tracing::warn!(
                    "Module keys could not be kept at {} ({e:#}); a restart without the server will hold none",
                    path.display()
                );
            }
        }
        self.install(payload);
    }

    fn install(&self, payload: ModuleKeysPayload) {
        let fresh: HashMap<String, Vec<u8>> = payload
            .keys
            .into_iter()
            // The key is the string's own bytes, never decoded from hex.
            .map(|k| (k.device_id, k.command_key.into_bytes()))
            .collect();
        let mut keys = self.keys.write().expect("seal keys lock");
        let mut peers = self.peers.lock().expect("seal peers lock");
        peers.retain(|d, _| fresh.get(d).is_some_and(|k| keys.get(d) == Some(k)));
        *keys = fresh;
    }

    pub fn has_key(&self, device: &str) -> bool {
        self.keys
            .read()
            .expect("seal keys lock")
            .contains_key(device)
    }

    pub fn key_count(&self) -> usize {
        self.keys.read().expect("seal keys lock").len()
    }

    /// Seal `msg` for `device`. `false`, and `msg` untouched, when this edge
    /// holds no key for it.
    pub fn seal<M: Sealed>(&self, device: &str, msg: &mut M) -> bool {
        let Some(key) = self
            .keys
            .read()
            .expect("seal keys lock")
            .get(device)
            .cloned()
        else {
            return false;
        };
        let mut peers = self.peers.lock().expect("seal peers lock");
        peers
            .entry(device.to_string())
            .or_insert_with(|| Peer::new(nonce()))
            .seal(&key, msg);
        true
    }

    /// A [`LocalHello`](css_lib::wire::LocalHello) sealed for `device`, when
    /// one is due: it holds a key, and was not sent one in the last
    /// [`HELLO_EVERY`]. For a device whose signed message was stale, so it
    /// learns this edge's nonce; a reader or a sensor hears nothing else.
    pub fn hello_for(
        &self,
        device: &str,
        now: std::time::Instant,
    ) -> Option<css_lib::wire::LocalHello> {
        {
            let mut hellos = self.hellos.lock().expect("seal hellos lock");
            if hellos
                .get(device)
                .is_some_and(|at| now.duration_since(*at) < HELLO_EVERY)
            {
                return None;
            }
            hellos.insert(device.to_string(), now);
        }
        let mut hello = css_lib::wire::LocalHello {
            device_id: device.to_string(),
            seal: None,
        };
        self.seal(device, &mut hello).then_some(hello)
    }

    /// Whether to act on `msg`, a message about a signed resource from the
    /// device it names. `bound` says whether that device is bound to the
    /// resource; the caller knows the wiring, this does not.
    pub fn admit<M: Sealed>(
        &self,
        msg: &M,
        bound: impl Fn(&str) -> bool,
    ) -> Result<(), NotAdmitted> {
        let device = msg.device().ok_or(NotAdmitted::Unnamed)?;
        if !bound(device) {
            return Err(NotAdmitted::NotBound);
        }
        let key = self
            .keys
            .read()
            .expect("seal keys lock")
            .get(device)
            .cloned()
            .ok_or(NotAdmitted::NoKey)?;
        let mut peers = self.peers.lock().expect("seal peers lock");
        peers
            .entry(device.to_string())
            .or_insert_with(|| Peer::new(nonce()))
            .open(&key, msg, nonce)
            .map_err(NotAdmitted::Seal)
    }
}

/// The keys kept at `path`; `None` when there is no such file.
pub fn load_keys(path: &Path) -> anyhow::Result<Option<ModuleKeysPayload>> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(serde_json::from_slice(&bytes)?)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Keep `payload` at `path`, owner-only from the moment it exists: these are
/// the keys that make a lease believed, as secret as the edge's own config.
/// Written beside and renamed over, so a crash leaves the old keys or the new
/// ones, never half of either.
pub fn save_keys(path: &Path, payload: &ModuleKeysPayload) -> anyhow::Result<()> {
    use std::io::Write;
    let bytes = serde_json::to_vec(payload)?;
    let tmp = path.with_extension("tmp");
    let mut open = std::fs::OpenOptions::new();
    open.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        open.mode(0o600);
    }
    let mut f = open.open(&tmp)?;
    // `mode` applies only to a file it creates; a tmp left over from before
    // is narrowed here too.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        f.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    f.write_all(&bytes)?;
    f.sync_all()?;
    drop(f);
    std::fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use css_lib::wire::{LocalAliveRequest, ModuleKey, ResourceLeasePayload};
    use css_wire_sign::seal_with;

    const KEY: &str = "0a0b0c0d";

    fn keyed(devices: &[(&str, &str)]) -> LocalSeals {
        let s = LocalSeals::new();
        s.set_keys(ModuleKeysPayload {
            keys: devices
                .iter()
                .map(|(d, k)| ModuleKey {
                    device_id: d.to_string(),
                    command_key: k.to_string(),
                })
                .collect(),
        });
        s
    }

    fn alive(device: &str) -> LocalAliveRequest {
        LocalAliveRequest {
            device_id: device.to_string(),
            seal: None,
        }
    }

    /// A module, as firmware would be: its own peer, sealing with its key.
    fn module_says(peer: &mut Peer, key: &str, device: &str) -> LocalAliveRequest {
        let mut m = alive(device);
        peer.seal(key.as_bytes(), &mut m);
        m
    }

    fn lease_for(device: &str) -> ResourceLeasePayload {
        ResourceLeasePayload {
            resource: "r".into(),
            device_id: device.into(),
            grant: true,
            ttl_ms: 3000,
            reason: None,
            member: None,
            seal: None,
        }
    }

    #[test]
    fn a_module_and_the_edge_meet_and_then_believe_each_other() {
        let edge = keyed(&[("dev-1", KEY)]);
        let mut module = Peer::new("n-mod".into());
        // The first message is stale: it teaches the edge the module's nonce.
        let first = module_says(&mut module, KEY, "dev-1");
        assert_eq!(
            edge.admit(&first, |_| true),
            Err(NotAdmitted::Seal(Refused::Stale))
        );
        // The edge's lease names it; the module believes it and learns the
        // edge's nonce; the module's next message is then believed.
        let mut lease = lease_for("dev-1");
        assert!(edge.seal("dev-1", &mut lease));
        assert_eq!(module.open(KEY.as_bytes(), &lease, || "x".into()), Ok(()));
        let second = module_says(&mut module, KEY, "dev-1");
        assert_eq!(edge.admit(&second, |_| true), Ok(()));
        // And not twice.
        assert_eq!(
            edge.admit(&second, |_| true),
            Err(NotAdmitted::Seal(Refused::Stale))
        );
    }

    #[test]
    fn a_device_that_only_sends_is_told_the_nonce_and_is_then_believed() {
        // A reader: the edge sends it no lease, so without a hello it would
        // never learn the nonce to name, and every message would stay stale.
        let edge = keyed(&[("reader", KEY)]);
        let mut reader = Peer::new("n-reader".into());
        let first = module_says(&mut reader, KEY, "reader");
        assert_eq!(
            edge.admit(&first, |_| true),
            Err(NotAdmitted::Seal(Refused::Stale))
        );
        let now = std::time::Instant::now();
        let hello = edge.hello_for("reader", now).expect("a hello is due");
        assert_eq!(reader.open(KEY.as_bytes(), &hello, || "x".into()), Ok(()));
        let again = module_says(&mut reader, KEY, "reader");
        assert_eq!(edge.admit(&again, |_| true), Ok(()));
        // At most one a second, however often it is asked for.
        assert_eq!(
            edge.hello_for("reader", now + std::time::Duration::from_millis(500)),
            None
        );
        assert!(edge.hello_for("reader", now + HELLO_EVERY).is_some());
        // And none for a device with no key.
        assert_eq!(edge.hello_for("stranger", now), None);
    }

    #[test]
    fn a_message_is_refused_for_each_thing_that_is_wrong_with_it() {
        let edge = keyed(&[("dev-1", KEY), ("dev-2", "another key")]);
        assert_eq!(
            edge.admit(&alive("dev-1"), |_| true),
            Err(NotAdmitted::Seal(Refused::Unsigned))
        );
        assert_eq!(
            edge.admit(&alive("dev-9"), |_| true),
            Err(NotAdmitted::NoKey)
        );
        assert_eq!(
            edge.admit(&alive("dev-1"), |_| false),
            Err(NotAdmitted::NotBound)
        );
        // dev-2's key on a message claiming to be dev-1.
        let mut forged = alive("dev-1");
        seal_with(b"another key", &mut forged, "f", "t", 1);
        assert_eq!(
            edge.admit(&forged, |_| true),
            Err(NotAdmitted::Seal(Refused::BadSignature))
        );
        let unnamed = css_lib::wire::LocalScan {
            resource: "r".into(),
            card: "c".into(),
            device_id: None,
            seal: None,
        };
        assert_eq!(edge.admit(&unnamed, |_| true), Err(NotAdmitted::Unnamed));
    }

    #[test]
    fn the_edge_seals_with_the_keys_string_bytes_and_only_for_a_keyed_device() {
        let edge = keyed(&[("dev-1", KEY)]);
        let mut l = lease_for("dev-1");
        assert!(edge.seal("dev-1", &mut l));
        let seal = l.seal.clone().unwrap();
        // Checked independently of the edge's peer: the key's own bytes.
        let mut again = lease_for("dev-1");
        seal_with(KEY.as_bytes(), &mut again, &seal.from, &seal.to, seal.seq);
        assert_eq!(again.seal.unwrap().sig, seal.sig);
        let mut other = lease_for("dev-2");
        assert!(!edge.seal("dev-2", &mut other));
        assert_eq!(other.seal, None);
    }

    #[test]
    fn a_key_withdrawn_or_changed_takes_its_peer_with_it() {
        let edge = keyed(&[("dev-1", KEY)]);
        let mut module = Peer::new("n-mod".into());
        let _ = edge.admit(&module_says(&mut module, KEY, "dev-1"), |_| true);
        let mut lease = lease_for("dev-1");
        edge.seal("dev-1", &mut lease);
        assert_eq!(module.open(KEY.as_bytes(), &lease, || "x".into()), Ok(()));
        // Re-keyed: the old peer goes, so the module must meet the edge again.
        edge.set_keys(ModuleKeysPayload {
            keys: vec![ModuleKey {
                device_id: "dev-1".into(),
                command_key: "a new key".into(),
            }],
        });
        let msg = module_says(&mut module, "a new key", "dev-1");
        assert_eq!(
            edge.admit(&msg, |_| true),
            Err(NotAdmitted::Seal(Refused::Stale))
        );
        // Withdrawn: no key, nothing believed.
        edge.set_keys(ModuleKeysPayload { keys: vec![] });
        assert!(!edge.has_key("dev-1"));
        let msg = module_says(&mut module, "a new key", "dev-1");
        assert_eq!(edge.admit(&msg, |_| true), Err(NotAdmitted::NoKey));
    }

    fn payload(devices: &[(&str, &str)]) -> ModuleKeysPayload {
        ModuleKeysPayload {
            keys: devices
                .iter()
                .map(|(d, k)| ModuleKey {
                    device_id: d.to_string(),
                    command_key: k.to_string(),
                })
                .collect(),
        }
    }

    /// The claim the cache exists for: keys given before a restart are held
    /// after it, with no server to ask, and a message sealed under one is
    /// believed by the restarted edge.
    #[test]
    fn keys_given_before_a_restart_are_held_after_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("module-keys.json");
        LocalSeals::cached_at(path.clone()).set_keys(payload(&[("dev-1", KEY)]));

        let restarted = LocalSeals::cached_at(path);
        assert!(restarted.has_key("dev-1"));
        assert_eq!(restarted.key_count(), 1);
        let mut module = Peer::new("n-mod".into());
        let first = module_says(&mut module, KEY, "dev-1");
        // The first is stale, as after any restart, and teaches the edge's
        // nonce: what proves the key is that it is stale and not BadSignature.
        assert_eq!(
            restarted.admit(&first, |_| true),
            Err(NotAdmitted::Seal(Refused::Stale))
        );
    }

    /// A later answer replaces the file, so a device whose key the server
    /// withdrew is not believed after a restart either.
    #[test]
    fn a_withdrawn_key_stays_withdrawn_across_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("module-keys.json");
        let seals = LocalSeals::cached_at(path.clone());
        seals.set_keys(payload(&[("dev-1", KEY), ("dev-2", KEY)]));
        seals.set_keys(payload(&[("dev-2", KEY)]));
        let restarted = LocalSeals::cached_at(path);
        assert!(!restarted.has_key("dev-1"));
        assert!(restarted.has_key("dev-2"));
    }

    #[test]
    fn no_file_is_no_keys_and_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("module-keys.json");
        assert!(load_keys(&path).unwrap().is_none());
        assert_eq!(LocalSeals::cached_at(path).key_count(), 0);
    }

    /// An unreadable file is no keys, and the edge still starts.
    #[test]
    fn a_corrupt_file_is_no_keys() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("module-keys.json");
        std::fs::write(&path, b"{not json").unwrap();
        assert!(load_keys(&path).is_err());
        assert_eq!(LocalSeals::cached_at(path).key_count(), 0);
    }

    /// The keys are what make a lease believed: owner-only, even over a file
    /// that was world-readable before.
    #[cfg(unix)]
    #[test]
    fn the_cache_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("module-keys.json");
        std::fs::write(path.with_extension("tmp"), b"").unwrap();
        std::fs::set_permissions(
            path.with_extension("tmp"),
            std::fs::Permissions::from_mode(0o644),
        )
        .unwrap();
        save_keys(&path, &payload(&[("dev-1", KEY)])).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "mode {mode:o}");
        assert_eq!(load_keys(&path).unwrap(), Some(payload(&[("dev-1", KEY)])));
    }
}
