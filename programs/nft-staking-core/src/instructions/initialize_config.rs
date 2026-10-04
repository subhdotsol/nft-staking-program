use anchor_lang::prelude::*;
use anchor_spl::token::{Mint, Token};
use mpl_core::{
    instructions::AddCollectionPluginV1CpiBuilder,
    types::{Attribute, Attributes, Plugin, PluginAuthority},
};

use crate::{constants::*, state::StakeConfig};

#[derive(Accounts)]
pub struct InitializeConfig<'info> {
    /// Must be the collection's Core update authority, or mpl-core rejects the plugin add.
    #[account(mut)]
    pub authority: Signer<'info>,

    /// CHECK: owner-checked; mpl-core validates it is a `CollectionV1` when the plugin is added.
    #[account(mut, owner = mpl_core::ID)]
    pub collection: UncheckedAccount<'info>,

    #[account(
        init,
        payer = authority,
        space = 8 + StakeConfig::INIT_SPACE,
        seeds = [CONFIG_SEED, collection.key().as_ref()],
        bump,
    )]
    pub config: Account<'info, StakeConfig>,

    #[account(
        init,
        payer = authority,
        seeds = [REWARD_MINT_SEED, collection.key().as_ref()],
        bump,
        mint::decimals = REWARD_DECIMALS,
        mint::authority = config,
    )]
    pub reward_mint: Account<'info, Mint>,

    /// CHECK: address-pinned.
    #[account(address = mpl_core::ID)]
    pub mpl_core_program: UncheckedAccount<'info>,
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}

pub fn handle_initialize_config(
    ctx: Context<InitializeConfig>,
    reward_rate: u64,
    burn_bonus: u64,
) -> Result<()> {
    let config_key = ctx.accounts.config.key();

    ctx.accounts.config.set_inner(StakeConfig {
        authority: ctx.accounts.authority.key(),
        collection: ctx.accounts.collection.key(),
        reward_mint: ctx.accounts.reward_mint.key(),
        reward_rate,
        burn_bonus,
        total_staked: 0,
        bump: ctx.bumps.config,
    });

    // Seed `total_staked = 0` on the Collection account. The config PDA takes the plugin
    // authority so stake/unstake can update it later without this signer.
    AddCollectionPluginV1CpiBuilder::new(&ctx.accounts.mpl_core_program)
        .collection(&ctx.accounts.collection)
        .payer(&ctx.accounts.authority)
        .authority(Some(&ctx.accounts.authority))
        .system_program(&ctx.accounts.system_program)
        .plugin(Plugin::Attributes(Attributes {
            attribute_list: vec![Attribute {
                key: TOTAL_STAKED_KEY.to_string(),
                value: "0".to_string(),
            }],
        }))
        .init_authority(PluginAuthority::Address {
            address: config_key,
        })
        .invoke()?;

    msg!(
        "staking config ready: reward_rate={}/s, burn_bonus={}, total_staked=0",
        reward_rate,
        burn_bonus
    );
    Ok(())
}
