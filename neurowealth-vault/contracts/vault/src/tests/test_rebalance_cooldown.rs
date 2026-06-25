//! Tests for rebalance cooldown (Issues #59, #250)
//!
//! Acceptance criteria:
//!   1. Configurable minimum ledgers between rebalances (owner-set).
//!   2. Agent call within cooldown panics with clear error (Error(Contract, #43)).
//!   3. Boundary timing is fully covered (exact interval, one short, zero).
//!   4. Concurrent/repeated failed attempts don't advance LastRebalanceLedger.
//!   5. Pause/unpause interaction leaves cooldown state intact.

use super::utils::*;
use soroban_sdk::{symbol_short, testutils::Ledger, Env};

// ============================================================================
// AC-1: Configurable minimum ledgers between rebalances (owner-set)
// ============================================================================

/// Default state: no cooldown is configured; `get_rebalance_cooldown` returns 0.
#[test]
fn test_no_cooldown_by_default() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    assert_eq!(
        client.get_rebalance_cooldown(),
        0,
        "Default cooldown should be 0 (disabled)"
    );
}

/// Owner can set a non-zero interval; getter reflects the new value.
#[test]
fn test_owner_can_set_rebalance_cooldown() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.set_rebalance_cooldown(&100_u32);

    assert_eq!(
        client.get_rebalance_cooldown(),
        100,
        "Cooldown should be 100 ledgers after owner sets it"
    );
}

/// Owner can update the interval to a different value.
#[test]
fn test_owner_can_update_rebalance_cooldown() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.set_rebalance_cooldown(&50_u32);
    assert_eq!(client.get_rebalance_cooldown(), 50);

    client.set_rebalance_cooldown(&200_u32);
    assert_eq!(
        client.get_rebalance_cooldown(),
        200,
        "Cooldown should update to 200 after second set"
    );
}

/// Setting cooldown to 0 disables it; subsequent rebalances are unrestricted.
#[test]
fn test_set_cooldown_zero_disables_it() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    // First enable a cooldown
    client.set_rebalance_cooldown(&50_u32);
    assert_eq!(client.get_rebalance_cooldown(), 50);

    // Then disable it
    client.set_rebalance_cooldown(&0_u32);
    assert_eq!(
        client.get_rebalance_cooldown(),
        0,
        "Setting interval to 0 should disable the cooldown"
    );
}

/// Rebalance succeeds when no cooldown is configured (default state).
#[test]
fn test_rebalance_succeeds_without_cooldown() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    // Should succeed: no cooldown set
    client.rebalance(&symbol_short!("none"), &0_i128, &0_i128);
    client.rebalance(&symbol_short!("none"), &0_i128, &0_i128);
}

/// LastRebalanceLedger is 0 before the first call.
#[test]
fn test_last_rebalance_ledger_is_zero_before_first_call() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    assert_eq!(
        client.get_last_rebalance_ledger(),
        0,
        "LastRebalanceLedger should be 0 before any rebalance"
    );
}

/// After a successful rebalance, LastRebalanceLedger is updated to the current ledger.
#[test]
fn test_last_rebalance_ledger_updated_after_rebalance() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let before = env.ledger().sequence();
    client.rebalance(&symbol_short!("none"), &0_i128, &0_i128);

    let stored = client.get_last_rebalance_ledger();
    assert!(
        stored >= before,
        "LastRebalanceLedger ({stored}) should be >= ledger at call time ({before})"
    );
}

/// Owner can set cooldown to any positive value (e.g. 1, which is the minimum
/// meaningful throttle: two consecutive rebalances in the same ledger are blocked).
#[test]
fn test_owner_can_set_cooldown_to_one_ledger() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.set_rebalance_cooldown(&1_u32);
    assert_eq!(client.get_rebalance_cooldown(), 1);
}

// ============================================================================
// AC-2: Agent call within cooldown panics with clear error (Error #43)
// ============================================================================

/// Calling rebalance twice in the same ledger when cooldown == 1 panics with
/// `RebalanceCooldownActive` (Error(Contract, #43)).
#[test]
#[should_panic(expected = "Error(Contract, #43)")]
fn test_rebalance_within_cooldown_panics() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    // Set cooldown to 10 ledgers
    client.set_rebalance_cooldown(&10_u32);

    // First rebalance succeeds and stores the current ledger
    client.rebalance(&symbol_short!("none"), &0_i128, &0_i128);

    // Immediately attempt a second rebalance in the same ledger — must panic
    client.rebalance(&symbol_short!("none"), &0_i128, &0_i128);
}

/// `try_rebalance` returns the cooldown error without unwinding the test.
#[test]
fn test_rebalance_within_cooldown_returns_error() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.set_rebalance_cooldown(&10_u32);

    // First call: must succeed
    client.rebalance(&symbol_short!("none"), &0_i128, &0_i128);

    // Second call: must fail with RebalanceCooldownActive (#43)
    let result = client.try_rebalance(&symbol_short!("none"), &0_i128, &0_i128);
    assert!(
        result.is_err(),
        "Second rebalance within cooldown should fail"
    );
}

/// After cooldown elapses (ledger advances past the interval), rebalance
/// succeeds again.
#[test]
fn test_rebalance_succeeds_after_cooldown_elapses() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    // Set cooldown to 5 ledgers
    let interval = 5_u32;
    client.set_rebalance_cooldown(&interval);

    // First rebalance succeeds
    client.rebalance(&symbol_short!("none"), &0_i128, &0_i128);
    let last_ledger = client.get_last_rebalance_ledger();

    // Advance ledger past the cooldown window
    env.ledger().with_mut(|li| {
        li.sequence_number = last_ledger + interval;
    });

    // Second rebalance: cooldown has elapsed, should succeed
    client.rebalance(&symbol_short!("none"), &0_i128, &0_i128);

    // LastRebalanceLedger should be updated again
    let new_last = client.get_last_rebalance_ledger();
    assert!(
        new_last >= last_ledger + interval,
        "LastRebalanceLedger should advance after second successful rebalance"
    );
}

/// Rebalance that fails the cooldown check does NOT update `LastRebalanceLedger`.
#[test]
fn test_failed_cooldown_does_not_update_last_rebalance_ledger() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.set_rebalance_cooldown(&10_u32);

    // First rebalance succeeds
    client.rebalance(&symbol_short!("none"), &0_i128, &0_i128);
    let ledger_after_first = client.get_last_rebalance_ledger();

    // Second call fails (cooldown still active)
    let _ = client.try_rebalance(&symbol_short!("none"), &0_i128, &0_i128);

    // LastRebalanceLedger must NOT have changed
    assert_eq!(
        client.get_last_rebalance_ledger(),
        ledger_after_first,
        "LastRebalanceLedger must not change when cooldown check fails"
    );
}

/// Cooldown is enforced even when the same ledger as the initial one is used.
#[test]
#[should_panic(expected = "Error(Contract, #43)")]
fn test_cooldown_enforced_on_same_ledger() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    // Set cooldown to 1 ledger — two calls in the same ledger should fail
    client.set_rebalance_cooldown(&1_u32);

    client.rebalance(&symbol_short!("none"), &0_i128, &0_i128);
    // elapsed == 0, which is < 1 → panic
    client.rebalance(&symbol_short!("none"), &0_i128, &0_i128);
}

/// Disabling cooldown (interval = 0) after a previous rebalance lets the agent
/// call again immediately.
#[test]
fn test_disabled_cooldown_allows_immediate_rebalance() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    // Enable cooldown
    client.set_rebalance_cooldown(&100_u32);
    client.rebalance(&symbol_short!("none"), &0_i128, &0_i128);

    // Verify it's blocked
    let result = client.try_rebalance(&symbol_short!("none"), &0_i128, &0_i128);
    assert!(result.is_err(), "Should be blocked by cooldown");

    // Disable cooldown
    client.set_rebalance_cooldown(&0_u32);

    // Now should succeed immediately
    client.rebalance(&symbol_short!("none"), &0_i128, &0_i128);
}

/// Exactly-at-boundary: elapsed == interval is allowed (>= means pass).
#[test]
fn test_rebalance_allowed_at_exact_interval_boundary() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let interval = 5_u32;
    client.set_rebalance_cooldown(&interval);

    client.rebalance(&symbol_short!("none"), &0_i128, &0_i128);
    let last = client.get_last_rebalance_ledger();

    // Advance ledger to exactly `last + interval`
    env.ledger().with_mut(|li| {
        li.sequence_number = last + interval;
    });

    // elapsed == interval, which is NOT < interval → should pass
    client.rebalance(&symbol_short!("none"), &0_i128, &0_i128);
}

/// One ledger before the boundary: elapsed == interval - 1 is still blocked.
#[test]
#[should_panic(expected = "Error(Contract, #43)")]
fn test_rebalance_blocked_one_ledger_before_boundary() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let interval = 5_u32;
    client.set_rebalance_cooldown(&interval);

    client.rebalance(&symbol_short!("none"), &0_i128, &0_i128);
    let last = client.get_last_rebalance_ledger();

    // Advance to one ledger short of the interval
    env.ledger().with_mut(|li| {
        li.sequence_number = last + interval - 1;
    });

    // elapsed == interval - 1 < interval → must panic
    client.rebalance(&symbol_short!("none"), &0_i128, &0_i128);
}

// ============================================================================
// AC-3: Boundary timing — additional edge cases (#250)
// ============================================================================

/// Multiple repeated failed attempts within cooldown must ALL be rejected and
/// must NEVER advance LastRebalanceLedger beyond the first successful call.
#[test]
fn test_multiple_repeated_failures_preserve_last_rebalance_ledger() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.set_rebalance_cooldown(&50_u32);

    // First rebalance succeeds; record the ledger it stamps
    client.rebalance(&symbol_short!("none"), &0_i128, &0_i128);
    let ledger_after_first = client.get_last_rebalance_ledger();

    // Three repeated failed attempts — none should change LastRebalanceLedger
    for _ in 0..3 {
        let _ = client.try_rebalance(&symbol_short!("none"), &0_i128, &0_i128);
        assert_eq!(
            client.get_last_rebalance_ledger(),
            ledger_after_first,
            "LastRebalanceLedger must not change on cooldown rejection"
        );
    }
}

/// Advancing the ledger exactly to the interval boundary (elapsed == interval)
/// then failing on the next rebalance (one past boundary — new cooldown starts)
/// demonstrates the sliding window behaviour.
#[test]
fn test_sliding_cooldown_window_after_successful_second_call() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let interval = 10_u32;
    client.set_rebalance_cooldown(&interval);

    // First rebalance
    client.rebalance(&symbol_short!("none"), &0_i128, &0_i128);
    let first_ledger = client.get_last_rebalance_ledger();

    // Advance to exactly the boundary for the second call
    env.ledger().with_mut(|li| {
        li.sequence_number = first_ledger + interval;
    });

    // Second rebalance succeeds; stamps a new LastRebalanceLedger
    client.rebalance(&symbol_short!("none"), &0_i128, &0_i128);
    let second_ledger = client.get_last_rebalance_ledger();
    assert!(
        second_ledger >= first_ledger + interval,
        "second LastRebalanceLedger must advance to the new window"
    );

    // Immediately after the second call the cooldown resets — third call blocked
    let result = client.try_rebalance(&symbol_short!("none"), &0_i128, &0_i128);
    assert!(
        result.is_err(),
        "third call immediately after second should be blocked by new cooldown window"
    );
}

// ============================================================================
// AC-4: Concurrent / repeated attempts (#250)
// ============================================================================

/// Enabling a large cooldown (u32::MAX ledgers) permanently blocks rebalances
/// until the owner disables it. This tests an extreme configuration.
#[test]
fn test_very_large_cooldown_blocks_rebalance() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    // Use a very large but not max value to avoid ledger arithmetic overflow
    let huge_interval: u32 = 1_000_000;
    client.set_rebalance_cooldown(&huge_interval);

    // First call records LastRebalanceLedger
    client.rebalance(&symbol_short!("none"), &0_i128, &0_i128);

    // Advance ledger by a lot — but still less than huge_interval
    env.ledger().with_mut(|li| {
        li.sequence_number += 500_000;
    });

    // Still blocked
    let result = client.try_rebalance(&symbol_short!("none"), &0_i128, &0_i128);
    assert!(
        result.is_err(),
        "rebalance must be blocked when elapsed < huge_interval"
    );
}

/// Owner can update the cooldown while the previous window is still active.
/// The NEW interval is applied to the NEXT call, not retroactively.
#[test]
fn test_changing_cooldown_mid_window_applies_to_next_call() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.set_rebalance_cooldown(&100_u32);

    client.rebalance(&symbol_short!("none"), &0_i128, &0_i128);
    let last = client.get_last_rebalance_ledger();

    // Lower the cooldown to 5 while we're still inside the original 100-ledger window
    client.set_rebalance_cooldown(&5_u32);
    assert_eq!(client.get_rebalance_cooldown(), 5);

    // Advance past the new (shorter) interval
    env.ledger().with_mut(|li| {
        li.sequence_number = last + 5;
    });

    // Should now succeed because elapsed (5) >= new_interval (5)
    client.rebalance(&symbol_short!("none"), &0_i128, &0_i128);
}

// ============================================================================
// AC-5: Pause / unpause interaction (#250)
// ============================================================================

/// Pausing the vault does not reset or invalidate the rebalance cooldown.
/// After unpausing, the same cooldown window continues from LastRebalanceLedger.
#[test]
fn test_cooldown_survives_pause_unpause_cycle() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let interval = 20_u32;
    client.set_rebalance_cooldown(&interval);

    // First rebalance stamps LastRebalanceLedger
    client.rebalance(&symbol_short!("none"), &0_i128, &0_i128);
    let last_before_pause = client.get_last_rebalance_ledger();

    // Pause the vault
    client.pause(&owner);

    // LastRebalanceLedger and cooldown config must be unchanged during pause
    assert_eq!(
        client.get_last_rebalance_ledger(),
        last_before_pause,
        "pause must not alter LastRebalanceLedger"
    );
    assert_eq!(
        client.get_rebalance_cooldown(),
        interval,
        "pause must not alter cooldown interval"
    );

    // Unpause; the cooldown window from the first rebalance is still in effect
    client.unpause(&owner);

    // Still within the original cooldown window — must be blocked
    let result = client.try_rebalance(&symbol_short!("none"), &0_i128, &0_i128);
    assert!(
        result.is_err(),
        "rebalance must still be blocked after unpause when cooldown has not elapsed"
    );
}

/// After pause + unpause the rebalance succeeds once the cooldown finally elapses.
#[test]
fn test_rebalance_allowed_after_unpause_and_cooldown_elapsed() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let interval = 15_u32;
    client.set_rebalance_cooldown(&interval);

    client.rebalance(&symbol_short!("none"), &0_i128, &0_i128);
    let last = client.get_last_rebalance_ledger();

    client.pause(&owner);
    client.unpause(&owner);

    // Advance past the cooldown
    env.ledger().with_mut(|li| {
        li.sequence_number = last + interval;
    });

    // Must succeed now that cooldown has elapsed after unpause
    client.rebalance(&symbol_short!("none"), &0_i128, &0_i128);
    assert!(
        client.get_last_rebalance_ledger() >= last + interval,
        "LastRebalanceLedger should advance after cooldown-elapsed rebalance post-unpause"
    );
}
