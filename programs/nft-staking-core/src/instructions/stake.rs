use anchor_lang::prelude::*;
use mpl_core::{
    instructions::AddPluginV1CpiBuilder,
    types::{BurnDelegate, FreezeDelegate, Plugin, PluginAuthority},
};

use crate::{
    asset_checks::assert_owned_member,
    collection_stats::{apply_total_staked_delta, StakeDelta},
    constants::*,
    error::StakingError,
    state::{StakeAccount, StakeConfig},
};

#[derive(Accounts)]
pub struct Stake<'info> {
    #[account(mut)]
    pub owner: Signer<'info>,

    #[account(
        mut,
        seeds = [CONFIG_SEED, collection.key().as_ref()],
        bump = config.bump,
        has_one = collection @ StakingError::InvalidCollection,
    )]
    pub config: Account<'info, StakeConfig>,

    /// CHECK: owner-checked; `has_one` on `config` pins the exact collection.
    #[account(mut, owner = mpl_core::ID)]
    pub collection: UncheckedAccount<'info>,

    /// CHECK: owner-checked; membership and ownership verified in the handler.
    #[account(mut, owner = mpl_core::ID)]
    pub asset: UncheckedAccount<'info>,

    /// `init` makes a second stake of the same asset impossible.
    #[account(
        init,
        payer = owner,
        space = 8 + StakeAccount::INIT_SPACE,
        seeds = [STAKE_SEED, asset.key().as_ref()],
        bump,
    )]
    pub stake_account: Account<'info, StakeAccount>,

    /// CHECK: address-pinned.
    #[account(address = mpl_core::ID)]
    pub mpl_core_program: UncheckedAccount<'info>,
    pub system_program: Program<'info, System>,
}

pub fn handle_stake(ctx: Context<Stake>) -> Result<()> {
    let now = Clock::get()?.unix_timestamp;
    let collection_key = ctx.accounts.collection.key();
    let stake_account_key = ctx.accounts.stake_account.key();

    assert_owned_member(
        &ctx.accounts.asset,
        &ctx.accounts.owner.key(),
        &collection_key,
    )?;

    ctx.accounts.stake_account.set_inner(StakeAccount {
        owner: ctx.accounts.owner.key(),
        asset: ctx.accounts.asset.key(),
        collection: collection_key,
        staked_at: now,
        last_claim_at: now,
        total_claimed: 0,
        bump: ctx.bumps.stake_account,
    });

    // mpl-core refuses to revoke or remove a frozen FreezeDelegate, so handing the authority to
    // the stake PDA locks the NFT until this program thaws it.
    AddPluginV1CpiBuilder::new(&ctx.accounts.mpl_core_program)
        .asset(&ctx.accounts.asset)
        .collection(Some(&ctx.accounts.collection))
        .payer(&ctx.accounts.owner)
        .authority(Some(&ctx.accounts.owner))
        .system_program(&ctx.accounts.system_program)
        .plugin(Plugin::FreezeDelegate(FreezeDelegate { frozen: true }))
        .init_authority(PluginAuthority::Address {
            address: stake_account_key,
        })
        .invoke()?;

    // Burn rights to the same PDA, for `burn_staked_nft`.
    AddPluginV1CpiBuilder::new(&ctx.accounts.mpl_core_program)
        .asset(&ctx.accounts.asset)
        .collection(Some(&ctx.accounts.collection))
        .payer(&ctx.accounts.owner)
        .authority(Some(&ctx.accounts.owner))
        .system_program(&ctx.accounts.system_program)
        .plugin(Plugin::BurnDelegate(BurnDelegate {}))
        .init_authority(PluginAuthority::Address {
            address: stake_account_key,
        })
        .invoke()?;

    let bump = [ctx.accounts.config.bump];
    let seeds = ctx.accounts.config.signer_seeds(&bump);
    let total_staked = apply_total_staked_delta(
        &ctx.accounts.mpl_core_program,
        &ctx.accounts.collection,
        &ctx.accounts.owner,
        &ctx.accounts.config.to_account_info(),
        &ctx.accounts.system_program.to_account_info(),
        &seeds,
        StakeDelta::Increment,
    )?;
    ctx.accounts.config.total_staked = total_staked;

    Ok(())
}
