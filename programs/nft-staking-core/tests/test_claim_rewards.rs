//! Task 1: `claim_rewards` pays out accrued rewards without unstaking.

mod common;

use {
    common::{Env, BURN_BONUS, REWARD_RATE},
    mpl_core::{types::PluginAuthority, BaseAuthority},
    solana_signer::Signer,
};

#[test]
fn claim_pays_accrued_rewards_and_leaves_nft_staked() {
    let mut env = Env::new();
    let user = env.new_user();
    let asset = env.mint_asset(&user, "claim-asset");

    env.stake(&user, &asset).expect("stake");
    env.assert_staked_and_frozen(&asset);
    assert_eq!(env.reward_balance(&user.pubkey()), 0);

    let staked_at = env.stake_state(&asset).expect("stake account").staked_at;

    env.advance_seconds(100);
    env.claim_rewards(&user, &asset).expect("claim_rewards");

    let stake = env.stake_state(&asset).expect("stake account still exists");
    let expected = (stake.last_claim_at - staked_at) as u64 * REWARD_RATE;
    assert_eq!(expected, 100 * REWARD_RATE, "100s should have elapsed");
    assert_eq!(env.reward_balance(&user.pubkey()), expected);
    assert_eq!(stake.total_claimed, expected);

    // The point of the instruction: the NFT is untouched.
    assert!(env.asset_exists(&asset), "NFT must not be burned");
    assert_eq!(env.asset_owner(&asset), user.pubkey(), "owner unchanged");
    env.assert_staked_and_frozen(&asset);
    assert_eq!(
        env.total_staked_attribute(),
        1,
        "claiming must not change the collection counter"
    );
}

#[test]
fn claiming_twice_only_pays_for_newly_elapsed_time() {
    let mut env = Env::new();
    let user = env.new_user();
    let asset = env.mint_asset(&user, "double-claim");

    env.stake(&user, &asset).expect("stake");

    env.advance_seconds(60);
    env.claim_rewards(&user, &asset).expect("first claim");
    let after_first = env.reward_balance(&user.pubkey());
    assert_eq!(after_first, 60 * REWARD_RATE);

    // No time passes, so this must be a no-op rather than a double payout.
    env.claim_rewards(&user, &asset).expect("immediate reclaim");
    assert_eq!(
        env.reward_balance(&user.pubkey()),
        after_first,
        "claiming with zero elapsed time must not mint"
    );

    env.advance_seconds(40);
    env.claim_rewards(&user, &asset).expect("second claim");
    assert_eq!(
        env.reward_balance(&user.pubkey()),
        100 * REWARD_RATE,
        "second claim pays only the newly elapsed 40s"
    );
    env.assert_staked_and_frozen(&asset);
}

#[test]
fn claim_rejects_a_signer_who_does_not_own_the_stake() {
    let mut env = Env::new();
    let owner = env.new_user();
    let attacker = env.new_user();
    let asset = env.mint_asset(&owner, "not-yours");

    env.stake(&owner, &asset).expect("stake");
    env.advance_seconds(100);

    assert!(
        env.claim_rewards(&attacker, &asset).is_err(),
        "only the staker may claim"
    );
    assert_eq!(env.reward_balance(&attacker.pubkey()), 0);
}

#[test]
fn claim_rejects_an_unstaked_asset() {
    let mut env = Env::new();
    let user = env.new_user();
    let asset = env.mint_asset(&user, "never-staked");

    assert!(
        env.claim_rewards(&user, &asset).is_err(),
        "an asset that was never staked has no stake account"
    );
}

#[test]
fn unstake_settles_remaining_rewards_and_releases_the_nft() {
    let mut env = Env::with_params(REWARD_RATE, BURN_BONUS);
    let user = env.new_user();
    let asset = env.mint_asset(&user, "unstake-asset");

    env.stake(&user, &asset).expect("stake");
    env.advance_seconds(50);
    env.claim_rewards(&user, &asset).expect("claim");
    env.advance_seconds(30);
    env.unstake(&user, &asset).expect("unstake");

    assert_eq!(
        env.reward_balance(&user.pubkey()),
        80 * REWARD_RATE,
        "unstake pays out the 30s accrued since the claim"
    );
    assert!(
        env.stake_state(&asset).is_none(),
        "stake account should be closed"
    );

    assert!(env.asset_exists(&asset));
    assert_eq!(env.asset_owner(&asset), user.pubkey());
    assert!(
        !env.has_freeze_delegate(&asset),
        "FreezeDelegate should be removed on unstake"
    );
    assert!(
        !env.has_burn_delegate(&asset),
        "BurnDelegate should be removed on unstake"
    );
}

#[test]
fn an_unstaked_asset_can_be_staked_again() {
    let mut env = Env::new();
    let user = env.new_user();
    let asset = env.mint_asset(&user, "restake");

    env.stake(&user, &asset).expect("first stake");
    env.advance_seconds(10);
    env.unstake(&user, &asset).expect("unstake");
    assert_eq!(env.total_staked_attribute(), 0);

    env.stake(&user, &asset).expect("re-stake");
    assert_eq!(env.total_staked_attribute(), 1);
    env.assert_staked_and_frozen(&asset);

    // Accrual restarts rather than resuming the old clock.
    let stake = env.stake_state(&asset).expect("stake account");
    assert_eq!(stake.total_claimed, 0);
    assert_eq!(stake.staked_at, stake.last_claim_at);
    assert_eq!(stake.staked_at, env.now());
}

#[test]
fn the_config_pda_holds_the_collection_attributes_authority() {
    let env = Env::new();
    assert_eq!(
        env.attributes_plugin_authority(),
        BaseAuthority::from(PluginAuthority::Address {
            address: env.config
        }),
        "the config PDA must own the Attributes plugin so stake/unstake can update it"
    );
}
