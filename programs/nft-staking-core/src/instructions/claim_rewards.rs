use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token::{Mint, Token, TokenAccount},
};

use crate::{
    asset_checks::{assert_frozen_by, mint_rewards},
    constants::*,
    error::StakingError,
    state::{StakeAccount, StakeConfig},
};

/// Takes no mpl-core program account and no mutable `asset`, so no path through this
/// instruction can thaw, transfer or burn the NFT.
#[derive(Accounts)]
pub struct ClaimRewards<'info> {
    #[account(mut)]
    pub owner: Signer<'info>,

    #[account(
        seeds = [CONFIG_SEED, config.collection.as_ref()],
        bump = config.bump,
    )]
    pub config: Account<'info, StakeConfig>,

    #[account(
        mut,
        seeds = [STAKE_SEED, asset.key().as_ref()],
        bump = stake_account.bump,
        has_one = owner @ StakingError::NotAssetOwner,
        has_one = asset,
        constraint = stake_account.collection == config.collection @ StakingError::InvalidCollection,
    )]
    pub stake_account: Account<'info, StakeAccount>,

    /// CHECK: read-only, asserted frozen by `stake_account`.
    #[account(owner = mpl_core::ID)]
    pub asset: UncheckedAccount<'info>,

    #[account(mut, address = config.reward_mint)]
    pub reward_mint: Account<'info, Mint>,

    #[account(
        init_if_needed,
        payer = owner,
        associated_token::mint = reward_mint,
        associated_token::authority = owner,
    )]
    pub reward_ata: Account<'info, TokenAccount>,

    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

pub fn handle_claim_rewards(ctx: Context<ClaimRewards>) -> Result<()> {
    let now = Clock::get()?.unix_timestamp;

    assert_frozen_by(&ctx.accounts.asset, &ctx.accounts.stake_account.key())?;

    let reward_rate = ctx.accounts.config.reward_rate;
    let amount = ctx.accounts.stake_account.settle(now, reward_rate)?;

    let bump = [ctx.accounts.config.bump];
    let seeds = ctx.accounts.config.signer_seeds(&bump);
    mint_rewards(
        &ctx.accounts.token_program.key(),
        &ctx.accounts.reward_mint.to_account_info(),
        &ctx.accounts.reward_ata.to_account_info(),
        &ctx.accounts.config.to_account_info(),
        &seeds,
        amount,
    )?;

    msg!(
        "claimed {} (lifetime {}); NFT stays staked and frozen",
        amount,
        ctx.accounts.stake_account.total_claimed
    );
    Ok(())
}
