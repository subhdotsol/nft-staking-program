use anchor_lang::prelude::*;

/// One per Core collection. The PDA is both the reward mint authority and the authority of the
/// collection's `Attributes` plugin.
#[account]
#[derive(InitSpace)]
pub struct StakeConfig {
    pub authority: Pubkey,
    pub collection: Pubkey,
    pub reward_mint: Pubkey,
    /// Reward base units accrued per staked asset per second.
    pub reward_rate: u64,
    /// One-time bonus paid out by `burn_staked_nft`.
    pub burn_bonus: u64,
    /// Mirror of the collection's `total_staked` Attribute, which is the source of truth.
    pub total_staked: u64,
    pub bump: u8,
}

impl StakeConfig {
    pub fn signer_seeds<'a>(&'a self, bump: &'a [u8; 1]) -> [&'a [u8]; 3] {
        [
            crate::constants::CONFIG_SEED,
            self.collection.as_ref(),
            bump,
        ]
    }
}
