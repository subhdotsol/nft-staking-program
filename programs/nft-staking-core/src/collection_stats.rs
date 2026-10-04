//! The `total_staked` counter lives as an `Attributes` entry on the Core Collection account.
//! Its plugin authority is the `StakeConfig` PDA, so stake/unstake can update it without the
//! collection authority co-signing.

use anchor_lang::prelude::*;
use mpl_core::{
    fetch_collection_plugin,
    instructions::UpdateCollectionPluginV1CpiBuilder,
    types::{Attribute, Attributes, Plugin, PluginType},
};

use crate::{constants::TOTAL_STAKED_KEY, error::StakingError};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StakeDelta {
    Increment,
    Decrement,
}

/// `fetch_collection_plugin` rather than `Collection::from_bytes`: the latter builds the full
/// `PluginsList`, whose stack frame exceeds the SBF 4KB per-frame limit.
fn attribute_list(collection: &AccountInfo) -> Result<Vec<Attribute>> {
    let (_authority, attributes, _offset) =
        fetch_collection_plugin::<Attributes>(collection, PluginType::Attributes)
            .map_err(|_| error!(StakingError::MissingAttributesPlugin))?;
    Ok(attributes.attribute_list)
}

fn parse_total_staked(list: &[Attribute]) -> Result<(usize, u64)> {
    let index = list
        .iter()
        .position(|attribute| attribute.key == TOTAL_STAKED_KEY)
        .ok_or_else(|| error!(StakingError::MissingTotalStakedAttribute))?;
    let value = list[index]
        .value
        .parse::<u64>()
        .map_err(|_| error!(StakingError::InvalidTotalStakedAttribute))?;
    Ok((index, value))
}

fn next_total(current: u64, delta: StakeDelta) -> Result<u64> {
    match delta {
        StakeDelta::Increment => current
            .checked_add(1)
            .ok_or_else(|| error!(StakingError::MathOverflow)),
        StakeDelta::Decrement => current
            .checked_sub(1)
            .ok_or_else(|| error!(StakingError::TotalStakedUnderflow)),
    }
}

/// Returns the new value. `UpdateCollectionPluginV1` replaces the whole plugin, so the list is
/// read, edited and written back in full to preserve any other attributes.
#[allow(clippy::too_many_arguments)]
pub fn apply_total_staked_delta<'info>(
    mpl_core_program: &AccountInfo<'info>,
    collection: &AccountInfo<'info>,
    payer: &AccountInfo<'info>,
    config: &AccountInfo<'info>,
    system_program: &AccountInfo<'info>,
    config_signer_seeds: &[&[u8]],
    delta: StakeDelta,
) -> Result<u64> {
    let mut list = attribute_list(collection)?;
    let (index, current) = parse_total_staked(&list)?;
    let next = next_total(current, delta)?;
    list[index].value = next.to_string();

    UpdateCollectionPluginV1CpiBuilder::new(mpl_core_program)
        .collection(collection)
        .payer(payer)
        .authority(Some(config))
        .system_program(system_program)
        .plugin(Plugin::Attributes(Attributes {
            attribute_list: list,
        }))
        .invoke_signed(&[config_signer_seeds])?;

    msg!("collection total_staked: {} -> {}", current, next);
    Ok(next)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constants::TOTAL_STAKED_KEY;
    use anchor_lang::error::ERROR_CODE_OFFSET;

    fn attr(key: &str, value: &str) -> Attribute {
        Attribute {
            key: key.to_string(),
            value: value.to_string(),
        }
    }

    fn code(error: anchor_lang::error::Error) -> u32 {
        match error {
            anchor_lang::error::Error::AnchorError(inner) => inner.error_code_number,
            other => panic!("expected an AnchorError, got {other:?}"),
        }
    }

    #[test]
    fn decrement_at_zero_is_rejected_instead_of_wrapping() {
        let err = next_total(0, StakeDelta::Decrement).unwrap_err();
        assert_eq!(
            code(err),
            StakingError::TotalStakedUnderflow as u32 + ERROR_CODE_OFFSET
        );
    }

    #[test]
    fn increment_at_u64_max_is_rejected_instead_of_wrapping() {
        let err = next_total(u64::MAX, StakeDelta::Increment).unwrap_err();
        assert_eq!(
            code(err),
            StakingError::MathOverflow as u32 + ERROR_CODE_OFFSET
        );
    }

    #[test]
    fn deltas_move_by_one_in_each_direction() {
        assert_eq!(next_total(0, StakeDelta::Increment).unwrap(), 1);
        assert_eq!(next_total(9, StakeDelta::Increment).unwrap(), 10);
        assert_eq!(next_total(10, StakeDelta::Decrement).unwrap(), 9);
        assert_eq!(next_total(1, StakeDelta::Decrement).unwrap(), 0);
    }

    #[test]
    fn total_staked_is_located_by_key_not_by_position() {
        let list = vec![
            attr("season", "1"),
            attr(TOTAL_STAKED_KEY, "42"),
            attr("tier", "gold"),
        ];
        assert_eq!(parse_total_staked(&list).unwrap(), (1, 42));
    }

    #[test]
    fn a_missing_or_malformed_counter_is_an_error() {
        assert!(parse_total_staked(&[attr("season", "1")]).is_err());
        assert!(parse_total_staked(&[attr(TOTAL_STAKED_KEY, "many")]).is_err());
        assert!(parse_total_staked(&[attr(TOTAL_STAKED_KEY, "-1")]).is_err());
    }
}
