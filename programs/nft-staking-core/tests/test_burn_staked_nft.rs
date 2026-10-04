//! Task 2: `burn_staked_nft` destroys the staked asset for accrued rewards plus a bonus.

mod common;

use {
    common::{Env, BURN_BONUS, REWARD_RATE},
    solana_signer::Signer,
};

#[test]
fn burn_pays_accrued_rewards_plus_bonus_and_destroys_the_nft() {
    let mut env = Env::new();
    let user = env.new_user();
    let asset = env.mint_asset(&user, "burn-asset");

    env.stake(&user, &asset).expect("stake");
    env.advance_seconds(100);
    env.burn_staked_nft(&user, &asset).expect("burn_staked_nft");

    assert_eq!(
        env.reward_balance(&user.pubkey()),
        100 * REWARD_RATE + BURN_BONUS,
        "payout is accrued rewards plus the one-time burn bonus"
    );

    assert!(!env.asset_exists(&asset), "the NFT must be gone");
    assert!(
        env.stake_state(&asset).is_none(),
        "stake account must be closed"
    );
    assert_eq!(
        env.total_staked_attribute(),
        0,
        "burning removes the asset from the staked count"
    );
}

#[test]
fn burn_credits_the_bonus_on_top_of_an_earlier_claim() {
    let mut env = Env::new();
    let user = env.new_user();
    let asset = env.mint_asset(&user, "claim-then-burn");

    env.stake(&user, &asset).expect("stake");
    env.advance_seconds(40);
    env.claim_rewards(&user, &asset).expect("claim");
    assert_eq!(env.reward_balance(&user.pubkey()), 40 * REWARD_RATE);

    env.advance_seconds(60);
    env.burn_staked_nft(&user, &asset).expect("burn");

    // Already-claimed time is not paid twice: 100s of accrual, bonus once.
    assert_eq!(
        env.reward_balance(&user.pubkey()),
        100 * REWARD_RATE + BURN_BONUS
    );
}

#[test]
fn burning_with_a_zero_bonus_still_pays_accrued_rewards() {
    let mut env = Env::with_params(REWARD_RATE, 0);
    let user = env.new_user();
    let asset = env.mint_asset(&user, "no-bonus");

    env.stake(&user, &asset).expect("stake");
    env.advance_seconds(25);
    env.burn_staked_nft(&user, &asset).expect("burn");

    assert_eq!(env.reward_balance(&user.pubkey()), 25 * REWARD_RATE);
    assert!(!env.asset_exists(&asset));
}

#[test]
fn a_burned_nft_cannot_be_unstaked_or_claimed_afterwards() {
    let mut env = Env::new();
    let user = env.new_user();
    let asset = env.mint_asset(&user, "gone-for-good");

    env.stake(&user, &asset).expect("stake");
    env.advance_seconds(10);
    env.burn_staked_nft(&user, &asset).expect("burn");

    let balance_after_burn = env.reward_balance(&user.pubkey());

    assert!(
        env.claim_rewards(&user, &asset).is_err(),
        "no stake account remains to claim against"
    );
    assert!(
        env.unstake(&user, &asset).is_err(),
        "a burned asset cannot be unstaked"
    );
    assert!(
        env.burn_staked_nft(&user, &asset).is_err(),
        "the bonus cannot be farmed by burning twice"
    );
    assert_eq!(
        env.reward_balance(&user.pubkey()),
        balance_after_burn,
        "no further tokens were minted"
    );
}

#[test]
fn burn_rejects_a_signer_who_does_not_own_the_stake() {
    let mut env = Env::new();
    let owner = env.new_user();
    let attacker = env.new_user();
    let asset = env.mint_asset(&owner, "attacker-target");

    env.stake(&owner, &asset).expect("stake");
    env.advance_seconds(100);

    assert!(
        env.burn_staked_nft(&attacker, &asset).is_err(),
        "only the staker may burn"
    );
    assert!(env.asset_exists(&asset), "the NFT must survive");
    assert_eq!(env.reward_balance(&attacker.pubkey()), 0);
    env.assert_staked_and_frozen(&asset);
}

#[test]
fn burn_rejects_an_asset_that_was_never_staked() {
    let mut env = Env::new();
    let user = env.new_user();
    let asset = env.mint_asset(&user, "unstaked");

    assert!(
        env.burn_staked_nft(&user, &asset).is_err(),
        "burn-to-earn only applies to staked assets"
    );
    assert!(env.asset_exists(&asset));
    assert_eq!(env.reward_balance(&user.pubkey()), 0);
}
