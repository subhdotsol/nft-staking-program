//! Metaplex Core NFT staking.
//!
//! Assets are frozen in place rather than escrowed: `stake` adds a `FreezeDelegate` plugin
//! whose authority is the per-asset stake PDA, plus a `BurnDelegate` for burn-to-earn. Rewards
//! are SPL tokens minted by the per-collection config PDA. The staked count is a `total_staked`
//! Attribute on the Core Collection account.

pub mod asset_checks;
pub mod collection_stats;
pub mod constants;
pub mod error;
pub mod instructions;
pub mod state;

use anchor_lang::prelude::*;

pub use constants::*;
pub use instructions::*;
pub use state::*;

declare_id!("CchzcHWcfb7VrRQ3VDg3aRdfaf5CWeiHeYYV5GrwoeZf");

#[program]
pub mod nft_staking_core {
    use super::*;

    /// Must be signed by the collection's Core update authority.
    pub fn initialize_config(
        ctx: Context<InitializeConfig>,
        reward_rate: u64,
        burn_bonus: u64,
    ) -> Result<()> {
        instructions::initialize_config::handle_initialize_config(ctx, reward_rate, burn_bonus)
    }

    pub fn stake(ctx: Context<Stake>) -> Result<()> {
        instructions::stake::handle_stake(ctx)
    }

    /// The NFT stays staked and frozen.
    pub fn claim_rewards(ctx: Context<ClaimRewards>) -> Result<()> {
        instructions::claim_rewards::handle_claim_rewards(ctx)
    }

    pub fn unstake(ctx: Context<Unstake>) -> Result<()> {
        instructions::unstake::handle_unstake(ctx)
    }

    /// Burn-to-earn: accrued rewards plus `config.burn_bonus`, and the asset is destroyed.
    pub fn burn_staked_nft(ctx: Context<BurnStakedNft>) -> Result<()> {
        instructions::burn_staked_nft::handle_burn_staked_nft(ctx)
    }
}
