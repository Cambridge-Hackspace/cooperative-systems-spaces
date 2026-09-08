//! The door path must resolve access cards the same status-aware way the tool
//! path does, and raise the same revoked-card fraud signal (#33).
//!
//! `devices_inbound.rs` records door-access events reported over MQTT/WS and
//! attributes the scanned card to a member. Before #33 it resolved the card with
//! `find_user_by_profile_field`, which cannot see a first-class `user_cards`
//! credential and has no notion of a disabled or released card -- so a revoked
//! card presented at a door would resolve to nobody and pass without a trace.
//!
//! ## Why this is a source check rather than a behavioral one
//!
//! Door events arrive only over MQTT or the device WebSocket; the e2e drivers
//! have no publisher (the one publish path is shell, in `stack.sh`). The runtime
//! behaviour of card resolution -- active grants, disabled/released deny AND
//! raise `revoked_card_presented`, unknown stays quiet -- is exercised against a
//! real stack in the `cards` e2e stage, which drives the *tool* path. Both paths
//! call the same `DatabaseManager::resolve_card`, so that stage covers the
//! resolver itself.
//!
//! What this stage cannot see is the DOOR wiring: that `devices_inbound` went
//! through `resolve_card` and handles its `Revoked` arm by emitting the event.
//! That is what this file pins. It proves the branch exists and is wired; it does
//! not prove the emitted row's contents (the `cards` stage does that for the
//! shared event type).

use css_checks::read;

/// The door handler resolves cards status-aware, not via the pre-#33 lookup that
/// could not see `user_cards` or a card's status.
#[test]
fn the_door_path_resolves_cards_status_aware() {
    let src = read("server/src/devices_inbound.rs");

    // Anti-vacuity: this is still the door-event handler.
    assert!(
        src.contains("doors/event"),
        "devices_inbound.rs no longer looks like the door-event handler; \
         re-target this check"
    );
    assert!(
        src.contains("resolve_card"),
        "the door path must resolve the scanned card via resolve_card \
         (status-aware), so a first-class user_cards credential is seen and a \
         revoked card is recognised"
    );
    assert!(
        !src.contains("find_user_by_profile_field"),
        "the door path calls find_user_by_profile_field directly again -- that \
         lookup cannot see user_cards or card status, so a revoked card would \
         resolve to nobody and pass silently. Go through resolve_card."
    );
}

/// A revoked card presented at a door raises the same distinct fraud event the
/// tool path raises.
#[test]
fn the_door_path_raises_the_revoked_card_signal() {
    let src = read("server/src/devices_inbound.rs");

    assert!(
        src.contains("CardResolution::Revoked"),
        "the door path must handle the CardResolution::Revoked arm -- the branch \
         a disabled/released card takes"
    );
    assert!(
        src.contains("RevokedCardPresented"),
        "a disabled/released card presented at a door must raise the \
         RevokedCardPresented audit event, as it does at a tool"
    );
}
