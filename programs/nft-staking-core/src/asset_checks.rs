//! Shared Core asset assertions and reward minting.
//!
//! Plugins are read with mpl-core's single-plugin fetchers, not `Asset::from_bytes`, which
//! builds the full `PluginsList` and overruns the SBF 4KB per-frame stack limit.

use anchor_lang::prelude::*;
use anchor_spl::token::{self, MintTo};
use mpl_core::{
    accounts::BaseAssetV1,
    fetch_asset_plugin,
    types::{FreezeDelegate, PluginAuthority, PluginType, UpdateAuthority},
};

use crate::error::StakingError;

pub fn assert_owned_member(asset: &AccountInfo, owner: &Pubkey, collection: &Pubkey) -> Result<()> {
    let base = {
        let data = asset.try_borrow_data()?;
        BaseAssetV1::from_bytes(&data).map_err(|_| error!(StakingError::InvalidMplCoreAccount))?
    };

    require_keys_eq!(base.owner, *owner, StakingError::NotAssetOwner);
    require!(
        base.update_authority == UpdateAuthority::Collection(*collection),
        StakingError::AssetNotInCollection
    );
    Ok(())
}

/// Frozen *by this stake account*, which proves both that the asset is staked and that the
/// program still holds the authority needed to thaw it.
pub fn assert_frozen_by(asset: &AccountInfo, stake_account: &Pubkey) -> Result<()> {
    let (authority, freeze, _offset) =
        fetch_asset_plugin::<FreezeDelegate>(asset, PluginType::FreezeDelegate)
            .map_err(|_| error!(StakingError::AssetNotStaked))?;

    require!(freeze.frozen, StakingError::AssetNotStaked);
    require!(
        authority
            == PluginAuthority::Address {
                address: *stake_account,
            },
        StakingError::AssetNotStaked
    );
    Ok(())
}

/// Signed by the config PDA. A zero amount is a no-op so callers need not special-case it.
pub fn mint_rewards<'info>(
    token_program: &Pubkey,
    reward_mint: &AccountInfo<'info>,
    reward_ata: &AccountInfo<'info>,
    config: &AccountInfo<'info>,
    config_signer_seeds: &[&[u8]],
    amount: u64,
) -> Result<()> {
    if amount == 0 {
        msg!("no rewards accrued yet; nothing to mint");
        return Ok(());
    }

    token::mint_to(
        CpiContext::new_with_signer(
            *token_program,
            MintTo {
                mint: reward_mint.clone(),
                to: reward_ata.clone(),
                authority: config.clone(),
            },
            &[config_signer_seeds],
        ),
        amount,
    )?;

    msg!("minted {} reward base units", amount);
    Ok(())
}
