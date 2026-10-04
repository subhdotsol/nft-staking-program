use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token::{Mint, Token, TokenAccount},
};
use mpl_core::{
    instructions::{BurnV1CpiBuilder, UpdatePluginV1CpiBuilder},
    types::{FreezeDelegate, Plugin},
};

use crate::{
    asset_checks::{assert_frozen_by, mint_rewards},
    collection_stats::{apply_total_staked_delta, StakeDelta},
    constants::*,
    error::StakingError,
    state::{StakeAccount, StakeConfig},
};

#[derive(Accounts)]
pub struct BurnStakedNft<'info> {
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

    /// CHECK: owner-checked, asserted frozen by `stake_account`.
    #[account(mut, owner = mpl_core::ID)]
    pub asset: UncheckedAccount<'info>,

    #[account(
        mut,
        close = owner,
        seeds = [STAKE_SEED, asset.key().as_ref()],
        bump = stake_account.bump,
        has_one = owner @ StakingError::NotAssetOwner,
        has_one = asset,
        has_one = collection @ StakingError::InvalidCollection,
    )]
    pub stake_account: Account<'info, StakeAccount>,

    #[account(mut, address = config.reward_mint)]
    pub reward_mint: Account<'info, Mint>,

    #[account(
        init_if_needed,
        payer = owner,
        associated_token::mint = reward_mint,
        associated_token::authority = owner,
    )]
    pub reward_ata: Account<'info, TokenAccount>,

    /// CHECK: address-pinned.
    #[account(address = mpl_core::ID)]
    pub mpl_core_program: UncheckedAccount<'info>,
    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

pub fn handle_burn_staked_nft(ctx: Context<BurnStakedNft>) -> Result<()> {
    let now = Clock::get()?.unix_timestamp;
    let stake_account_key = ctx.accounts.stake_account.key();
    let asset_key = ctx.accounts.asset.key();

    assert_frozen_by(&ctx.accounts.asset, &stake_account_key)?;

    let reward_rate = ctx.accounts.config.reward_rate;
    let accrued = ctx.accounts.stake_account.settle(now, reward_rate)?;
    let bonus = ctx.accounts.config.burn_bonus;
    let total = accrued
        .checked_add(bonus)
        .ok_or_else(|| error!(StakingError::MathOverflow))?;

    let config_bump = [ctx.accounts.config.bump];
    let config_seeds = ctx.accounts.config.signer_seeds(&config_bump);
    mint_rewards(
        &ctx.accounts.token_program.key(),
        &ctx.accounts.reward_mint.to_account_info(),
        &ctx.accounts.reward_ata.to_account_info(),
        &ctx.accounts.config.to_account_info(),
        &config_seeds,
        total,
    )?;

    let stake_bump = [ctx.accounts.stake_account.bump];
    let stake_seeds: [&[u8]; 3] = [STAKE_SEED, asset_key.as_ref(), &stake_bump];
    let stake_account_info = ctx.accounts.stake_account.to_account_info();

    // A frozen FreezeDelegate rejects burns, so thaw first. The stake PDA holds both the
    // freeze authority and the burn delegation.
    UpdatePluginV1CpiBuilder::new(&ctx.accounts.mpl_core_program)
        .asset(&ctx.accounts.asset)
        .collection(Some(&ctx.accounts.collection))
        .payer(&ctx.accounts.owner)
        .authority(Some(&stake_account_info))
        .system_program(&ctx.accounts.system_program.to_account_info())
        .plugin(Plugin::FreezeDelegate(FreezeDelegate { frozen: false }))
        .invoke_signed(&[&stake_seeds])?;

    BurnV1CpiBuilder::new(&ctx.accounts.mpl_core_program)
        .asset(&ctx.accounts.asset)
        .collection(Some(&ctx.accounts.collection))
        .payer(&ctx.accounts.owner)
        .authority(Some(&stake_account_info))
        .system_program(Some(&ctx.accounts.system_program.to_account_info()))
        .invoke_signed(&[&stake_seeds])?;

    let total_staked = apply_total_staked_delta(
        &ctx.accounts.mpl_core_program,
        &ctx.accounts.collection,
        &ctx.accounts.owner,
        &ctx.accounts.config.to_account_info(),
        &ctx.accounts.system_program.to_account_info(),
        &config_seeds,
        StakeDelta::Decrement,
    )?;
    ctx.accounts.config.total_staked = total_staked;

    msg!(
        "burned asset {} for {} accrued + {} bonus = {} total",
        asset_key,
        accrued,
        bonus,
        total
    );
    Ok(())
}
