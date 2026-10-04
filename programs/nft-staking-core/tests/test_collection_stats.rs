//! Task 3: the `total_staked` counter lives as an Attribute on the Core Collection account.

mod common;

use {
    common::Env,
    mpl_core::{types::PluginAuthority, BaseAuthority, Collection},
    solana_signer::Signer,
};

#[test]
fn total_staked_tracks_stakes_and_unstakes_across_users() {
    let mut env = Env::new();
    let alice = env.new_user();
    let bob = env.new_user();
    let alice_asset = env.mint_asset(&alice, "alice-nft");
    let bob_asset = env.mint_asset(&bob, "bob-nft");

    assert_eq!(env.total_staked_attribute(), 0, "starts at zero");

    env.stake(&alice, &alice_asset).expect("alice stakes");
    assert_eq!(env.total_staked_attribute(), 1);

    env.stake(&bob, &bob_asset).expect("bob stakes");
    assert_eq!(env.total_staked_attribute(), 2);

    env.unstake(&alice, &alice_asset).expect("alice unstakes");
    assert_eq!(env.total_staked_attribute(), 1);

    env.unstake(&bob, &bob_asset).expect("bob unstakes");
    assert_eq!(env.total_staked_attribute(), 0);
}

#[test]
fn burning_also_decrements_the_collection_counter() {
    let mut env = Env::new();
    let user = env.new_user();
    let kept = env.mint_asset(&user, "kept");
    let doomed = env.mint_asset(&user, "doomed");

    env.stake(&user, &kept).expect("stake kept");
    env.stake(&user, &doomed).expect("stake doomed");
    assert_eq!(env.total_staked_attribute(), 2);

    env.advance_seconds(10);
    env.burn_staked_nft(&user, &doomed).expect("burn doomed");
    assert_eq!(env.total_staked_attribute(), 1);

    env.unstake(&user, &kept).expect("unstake kept");
    assert_eq!(env.total_staked_attribute(), 0);
}

#[test]
fn the_counter_crosses_a_decimal_width_boundary_cleanly() {
    // The value is a string, so 9 -> 10 -> 9 reallocs the collection account, exercising both
    // the grow and shrink paths of mpl-core's plugin update.
    let mut env = Env::new();
    let user = env.new_user();

    let assets: Vec<_> = (0..11)
        .map(|i| env.mint_asset(&user, &format!("asset-{i}")))
        .collect();

    for (index, asset) in assets.iter().enumerate() {
        env.stake(&user, asset).expect("stake");
        assert_eq!(env.total_staked_attribute(), index as u64 + 1);
    }
    assert_eq!(env.total_staked_attribute(), 11);

    for (index, asset) in assets.iter().enumerate() {
        env.unstake(&user, asset).expect("unstake");
        assert_eq!(env.total_staked_attribute(), 10 - index as u64);
    }
    assert_eq!(env.total_staked_attribute(), 0);
}

#[test]
fn the_config_mirror_matches_the_collection_attribute() {
    let mut env = Env::new();
    let user = env.new_user();
    let a = env.mint_asset(&user, "mirror-a");
    let b = env.mint_asset(&user, "mirror-b");

    env.stake(&user, &a).expect("stake a");
    env.stake(&user, &b).expect("stake b");
    assert_eq!(env.config_state().total_staked, 2);
    assert_eq!(
        env.config_state().total_staked,
        env.total_staked_attribute()
    );

    env.unstake(&user, &a).expect("unstake a");
    assert_eq!(env.config_state().total_staked, 1);
    assert_eq!(
        env.config_state().total_staked,
        env.total_staked_attribute()
    );
}

#[test]
fn staking_the_same_asset_twice_fails_and_does_not_double_count() {
    let mut env = Env::new();
    let user = env.new_user();
    let asset = env.mint_asset(&user, "double-stake");

    env.stake(&user, &asset).expect("first stake");
    assert_eq!(env.total_staked_attribute(), 1);

    assert!(
        env.stake(&user, &asset).is_err(),
        "the stake PDA already exists"
    );
    assert_eq!(
        env.total_staked_attribute(),
        1,
        "the failed stake must not inflate the counter"
    );
}

#[test]
fn unstaking_an_unstaked_asset_fails_rather_than_underflowing() {
    let mut env = Env::new();
    let user = env.new_user();
    let asset = env.mint_asset(&user, "never-staked");

    assert_eq!(env.total_staked_attribute(), 0);
    assert!(env.unstake(&user, &asset).is_err());
    assert_eq!(
        env.total_staked_attribute(),
        0,
        "the counter must never go negative"
    );
}

#[test]
fn a_foreign_asset_cannot_be_staked_into_this_collection() {
    let mut env = Env::new();
    let user = env.new_user();

    // A Core asset with no collection at all.
    let outsider = {
        use mpl_core::{instructions::CreateV2Builder, types::DataState};
        let asset_kp = solana_keypair::Keypair::new();
        let asset = asset_kp.pubkey();
        let instruction = CreateV2Builder::new()
            .asset(asset)
            .payer(user.pubkey())
            .owner(Some(user.pubkey()))
            .data_state(DataState::AccountState)
            .name("outsider".to_string())
            .uri("https://example.com/outsider.json".to_string())
            .instruction();
        let user = user.insecure_clone();
        env.send(&[instruction], &user, &[&asset_kp])
            .expect("create standalone asset");
        asset
    };

    assert!(
        env.stake(&user, &outsider).is_err(),
        "assets outside the configured collection must be rejected"
    );
    assert_eq!(env.total_staked_attribute(), 0);
}

#[test]
fn the_attribute_is_stored_on_the_collection_account_itself() {
    let mut env = Env::new();
    let user = env.new_user();
    let asset = env.mint_asset(&user, "on-chain-proof");
    env.stake(&user, &asset).expect("stake");

    // Walk the raw Collection account's plugin registry, with no help from the program's own
    // state, to prove where the counter actually lives.
    let account = env
        .svm
        .get_account(&env.collection)
        .expect("collection account");
    assert_eq!(account.owner, mpl_core::ID, "owned by mpl-core");

    let collection = Collection::from_bytes(&account.data).expect("deserialize collection");
    let attributes = collection
        .plugin_list
        .attributes
        .expect("Attributes plugin present on the collection");

    assert_eq!(
        attributes.base.authority,
        BaseAuthority::from(PluginAuthority::Address {
            address: env.config
        })
    );
    assert_eq!(attributes.attributes.attribute_list.len(), 1);
    assert_eq!(attributes.attributes.attribute_list[0].key, "total_staked");
    assert_eq!(attributes.attributes.attribute_list[0].value, "1");
}
