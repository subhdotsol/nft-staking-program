use anchor_lang::prelude::*;

#[constant]
pub const CONFIG_SEED: &[u8] = b"config";

#[constant]
pub const STAKE_SEED: &[u8] = b"stake";

#[constant]
pub const REWARD_MINT_SEED: &[u8] = b"reward_mint";

/// Key of the `Attributes` entry on the Core Collection account holding the staked count.
#[constant]
pub const TOTAL_STAKED_KEY: &str = "total_staked";

pub const REWARD_DECIMALS: u8 = 9;
