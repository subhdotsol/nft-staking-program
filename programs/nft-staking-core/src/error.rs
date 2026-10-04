use anchor_lang::prelude::*;

#[error_code]
pub enum StakingError {
    #[msg("Arithmetic overflow while computing rewards")]
    MathOverflow,
    #[msg("Collection total_staked would underflow below zero")]
    TotalStakedUnderflow,
    #[msg("Account does not belong to this staking collection")]
    InvalidCollection,
    #[msg("Signer is not the owner of this Core asset")]
    NotAssetOwner,
    #[msg("Core asset is not a member of the configured collection")]
    AssetNotInCollection,
    #[msg("Account could not be deserialized as a Metaplex Core account")]
    InvalidMplCoreAccount,
    #[msg("Collection is missing the Attributes plugin")]
    MissingAttributesPlugin,
    #[msg("Collection Attributes plugin has no total_staked entry")]
    MissingTotalStakedAttribute,
    #[msg("Collection total_staked attribute is not a valid u64")]
    InvalidTotalStakedAttribute,
    #[msg("Asset is not frozen by this stake account, so it is not staked")]
    AssetNotStaked,
}
