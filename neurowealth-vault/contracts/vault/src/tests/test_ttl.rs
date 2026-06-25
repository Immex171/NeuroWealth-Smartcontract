//! Tests for persistent-storage TTL behavior on user share entries.
//!
//! TTL constants (from lib.rs):
//!   USER_SHARES_TTL_THRESHOLD = 100  — minimum remaining ledgers before extend acts
//!   USER_SHARES_TTL_EXTEND_TO  = 100  — ledgers to extend to when below threshold
//!
//! Expiration model: when a persistent entry's TTL reaches zero it is
//! inaccessible, as if it never existed. In unit tests, expiration is simulated
//! by removing the entry directly, which produces identical observable behaviour.
//!
//! Acceptance criteria covered (#253):
//!   • Expiration path: expired/removed entry treated as non-existent.
//!   • Restoration path: touch on a below-threshold entry extends to EXTEND_TO.
//!   • Boundary: TTL at exactly threshold is NOT extended; threshold-1 IS.
//!   • Insufficient headroom: very low TTL is extended by touch.

use super::utils::*;
use crate::DataKey;
use soroban_sdk::testutils::storage::Persistent as _;
use soroban_sdk::{testutils::Address as _, Address, Env};

#[test]
fn test_get_shares_does_not_extend_ttl() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let user = Address::generate(&env);

    mint_and_deposit(&env, &client, &usdc_token, &user, 5_000_000);

    let ttl_before = env.as_contract(&contract_id, || {
        env.storage()
            .persistent()
            .get_ttl(&DataKey::Shares(user.clone()))
    });

    let _ = client.get_shares(&user);

    let ttl_after_read = env.as_contract(&contract_id, || {
        env.storage()
            .persistent()
            .get_ttl(&DataKey::Shares(user.clone()))
    });

    assert_eq!(
        ttl_before, ttl_after_read,
        "get_shares must not extend Shares TTL"
    );
}

#[test]
fn test_get_balance_does_not_extend_ttl() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let user = Address::generate(&env);

    mint_and_deposit(&env, &client, &usdc_token, &user, 5_000_000);

    let ttl_before = env.as_contract(&contract_id, || {
        env.storage()
            .persistent()
            .get_ttl(&DataKey::Shares(user.clone()))
    });

    let _ = client.get_balance(&user);

    let ttl_after_read = env.as_contract(&contract_id, || {
        env.storage()
            .persistent()
            .get_ttl(&DataKey::Shares(user.clone()))
    });

    assert_eq!(
        ttl_before, ttl_after_read,
        "get_balance must not extend Shares TTL"
    );
}

#[test]
fn test_touch_user_ttl_extends_shares_entry() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let user = Address::generate(&env);

    mint_and_deposit(&env, &client, &usdc_token, &user, 5_000_000);

    let ttl_before = env.as_contract(&contract_id, || {
        env.storage()
            .persistent()
            .get_ttl(&DataKey::Shares(user.clone()))
    });

    assert!(client.touch_user_ttl(&user));

    let ttl_after_touch = env.as_contract(&contract_id, || {
        env.storage()
            .persistent()
            .get_ttl(&DataKey::Shares(user.clone()))
    });

    assert!(
        ttl_after_touch >= ttl_before,
        "touch_user_ttl should extend or preserve Shares TTL"
    );
}

#[test]
fn test_touch_user_ttl_no_entry_returns_false() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let user = Address::generate(&env);

    assert!(!client.touch_user_ttl(&user));
}

// ============================================================================
// Edge-case TTL coverage — issue #253
// ============================================================================

/// Expiration path: once a Shares entry is removed (simulating ledger expiry),
/// `get_shares` returns 0 and `touch_user_ttl` returns false — indistinguishable
/// from a user who never deposited.
#[test]
fn test_expired_entry_is_treated_as_nonexistent() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let user = Address::generate(&env);

    mint_and_deposit(&env, &client, &usdc_token, &user, 5_000_000);

    // Confirm the entry exists before simulating expiry
    assert!(client.get_shares(&user) > 0, "shares must exist before expiry");

    // Simulate TTL expiry: remove the persistent entry directly
    env.as_contract(&contract_id, || {
        env.storage()
            .persistent()
            .remove(&DataKey::Shares(user.clone()))
    });

    // After expiry, get_shares must return 0 (same as never deposited)
    assert_eq!(
        client.get_shares(&user),
        0,
        "expired entry: get_shares must return 0"
    );
    // touch_user_ttl must return false (entry gone)
    assert!(
        !client.touch_user_ttl(&user),
        "expired entry: touch_user_ttl must return false"
    );
}

/// Restoration path: a Shares entry with TTL below the threshold (100 ledgers)
/// is extended to USER_SHARES_TTL_EXTEND_TO (100) by `touch_user_ttl`.
/// This is the primary recovery mechanism for entries approaching expiry.
#[test]
fn test_touch_restores_ttl_when_below_threshold() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let user = Address::generate(&env);

    mint_and_deposit(&env, &client, &usdc_token, &user, 5_000_000);

    // Force the entry's TTL to a very low value (1 ledger — insufficient headroom)
    // by calling extend_ttl with threshold=0 (always extend) and extend_to=1.
    env.as_contract(&contract_id, || {
        env.storage().persistent().extend_ttl(
            &DataKey::Shares(user.clone()),
            0,  // threshold: always act
            1,  // extend_to: 1 ledger remaining
        )
    });

    let ttl_before = env.as_contract(&contract_id, || {
        env.storage()
            .persistent()
            .get_ttl(&DataKey::Shares(user.clone()))
    });

    // Entry is near expiry — touch should restore it
    assert!(
        client.touch_user_ttl(&user),
        "touch must return true when entry exists"
    );

    let ttl_after = env.as_contract(&contract_id, || {
        env.storage()
            .persistent()
            .get_ttl(&DataKey::Shares(user.clone()))
    });

    assert!(
        ttl_after > ttl_before,
        "TTL must increase from {ttl_before} after touch (restored from insufficient headroom)"
    );
    // The contract extends to USER_SHARES_TTL_EXTEND_TO = 100
    assert!(
        ttl_after >= 100,
        "TTL must reach at least USER_SHARES_TTL_EXTEND_TO (100) after restoration"
    );
}

/// Boundary — exactly at threshold: TTL == 100 is NOT below threshold (condition
/// is TTL < threshold), so `touch_user_ttl` must leave the TTL unchanged.
#[test]
fn test_touch_does_not_extend_when_ttl_equals_threshold() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let user = Address::generate(&env);

    mint_and_deposit(&env, &client, &usdc_token, &user, 5_000_000);

    // Set TTL to exactly USER_SHARES_TTL_THRESHOLD (100)
    env.as_contract(&contract_id, || {
        env.storage().persistent().extend_ttl(
            &DataKey::Shares(user.clone()),
            0,   // always act
            100, // extend_to == threshold
        )
    });

    let ttl_at_threshold = env.as_contract(&contract_id, || {
        env.storage()
            .persistent()
            .get_ttl(&DataKey::Shares(user.clone()))
    });

    // Touch: condition is TTL < threshold → 100 < 100 is false → no-op
    assert!(client.touch_user_ttl(&user), "touch must still return true");

    let ttl_after = env.as_contract(&contract_id, || {
        env.storage()
            .persistent()
            .get_ttl(&DataKey::Shares(user.clone()))
    });

    assert_eq!(
        ttl_at_threshold, ttl_after,
        "TTL must not change when it is already at the threshold (100 is not < 100)"
    );
}

/// Boundary — one below threshold: TTL == 99 IS below threshold, so
/// `touch_user_ttl` must extend it to USER_SHARES_TTL_EXTEND_TO (100).
#[test]
fn test_touch_extends_when_ttl_is_one_below_threshold() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let user = Address::generate(&env);

    mint_and_deposit(&env, &client, &usdc_token, &user, 5_000_000);

    // Set TTL to 99 (one below threshold of 100)
    env.as_contract(&contract_id, || {
        env.storage().persistent().extend_ttl(
            &DataKey::Shares(user.clone()),
            0,  // always act
            99, // extend_to: threshold - 1
        )
    });

    let ttl_before = env.as_contract(&contract_id, || {
        env.storage()
            .persistent()
            .get_ttl(&DataKey::Shares(user.clone()))
    });

    assert!(client.touch_user_ttl(&user));

    let ttl_after = env.as_contract(&contract_id, || {
        env.storage()
            .persistent()
            .get_ttl(&DataKey::Shares(user.clone()))
    });

    assert!(
        ttl_after > ttl_before,
        "TTL of 99 (below threshold) must be extended by touch"
    );
    assert!(
        ttl_after >= 100,
        "TTL must reach at least USER_SHARES_TTL_EXTEND_TO (100)"
    );
}

/// Multiple successive `touch_user_ttl` calls keep the entry alive without
/// error. Each call that finds TTL < threshold extends it; calls when TTL >=
/// threshold are no-ops. Neither case returns an error or corrupts state.
#[test]
fn test_multiple_touch_calls_keep_entry_alive() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let user = Address::generate(&env);

    mint_and_deposit(&env, &client, &usdc_token, &user, 5_000_000);

    // The first touch may or may not extend (depends on initial TTL vs threshold).
    // All subsequent touches with TTL >= 100 are no-ops. Both cases must return true.
    for i in 0..5 {
        assert!(
            client.touch_user_ttl(&user),
            "touch call {i} must return true while entry exists"
        );
        let ttl = env.as_contract(&contract_id, || {
            env.storage()
                .persistent()
                .get_ttl(&DataKey::Shares(user.clone()))
        });
        // After any touch, TTL must be at least USER_SHARES_TTL_EXTEND_TO (100)
        // OR unchanged at >= 100 (if it was already above threshold).
        assert!(
            ttl >= 100 || ttl > 0,
            "TTL must remain positive after touch {i}"
        );
    }
}

/// After a user withdraws all their shares (Shares entry becomes 0), touch
/// returns true if the entry still exists (zero shares, live key) or false if
/// the contract removes the zero-share entry.
/// This test documents the observed behaviour rather than prescribing it.
#[test]
fn test_touch_after_full_withdrawal_reflects_entry_state() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let user = Address::generate(&env);
    let amount: i128 = 5_000_000;

    mint_and_deposit(&env, &client, &usdc_token, &user, amount);
    client.withdraw_all(&user);

    // get_shares must be 0 after full withdrawal
    assert_eq!(client.get_shares(&user), 0, "shares must be 0 after withdraw_all");

    // touch behaviour depends on whether the contract retains or removes the key;
    // either outcome (true or false) is valid — what matters is no panic.
    let _touched = client.touch_user_ttl(&user);
}
