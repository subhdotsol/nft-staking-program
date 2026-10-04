use anchor_lang::prelude::*;

use crate::error::StakingError;

/// One per staked Core asset. Its existence is the staked flag, and the PDA holds the asset's
/// `FreezeDelegate` and `BurnDelegate` authority for as long as it lives.
#[account]
#[derive(InitSpace)]
pub struct StakeAccount {
    pub owner: Pubkey,
    pub asset: Pubkey,
    pub collection: Pubkey,
    pub staked_at: i64,
    /// Accrual clock: when rewards were last settled.
    pub last_claim_at: i64,
    pub total_claimed: u64,
    pub bump: u8,
}

impl StakeAccount {
    pub fn pending_rewards(&self, now: i64, reward_rate: u64) -> Result<u64> {
        let elapsed = now.saturating_sub(self.last_claim_at).max(0) as u64;
        elapsed
            .checked_mul(reward_rate)
            .ok_or_else(|| error!(StakingError::MathOverflow))
    }

    /// Settle up to `now` and return the amount owed. The only place the clock advances.
    pub fn settle(&mut self, now: i64, reward_rate: u64) -> Result<u64> {
        let pending = self.pending_rewards(now, reward_rate)?;
        self.last_claim_at = now;
        self.total_claimed = self
            .total_claimed
            .checked_add(pending)
            .ok_or_else(|| error!(StakingError::MathOverflow))?;
        Ok(pending)
    }
}
