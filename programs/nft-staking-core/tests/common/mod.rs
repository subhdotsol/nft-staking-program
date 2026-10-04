//! LiteSVM harness. Runs the staking program against the real `mpl_core` binary in
//! `tests/fixtures/`, so Core CPIs, plugins and lifecycle checks all execute for real.

#![allow(dead_code)]
#![allow(clippy::result_large_err)]

use {
    anchor_lang::{
        prelude::Pubkey,
        solana_program::{instruction::Instruction, program_pack::Pack, system_program},
        AccountDeserialize, InstructionData, ToAccountMetas,
    },
    anchor_spl::{associated_token::get_associated_token_address, token::spl_token},
    litesvm::{types::TransactionResult, LiteSVM},
    mpl_core::{
        accounts::BaseAssetV1,
        instructions::{CreateCollectionV2Builder, CreateV2Builder},
        types::{DataState, Key, PluginAuthority},
        Asset, BaseAuthority, Collection, PluginsList,
    },
    nft_staking_core::{
        constants::{CONFIG_SEED, REWARD_MINT_SEED, STAKE_SEED, TOTAL_STAKED_KEY},
        state::{StakeAccount, StakeConfig},
    },
    solana_clock::Clock,
    solana_keypair::Keypair,
    solana_message::{Message, VersionedMessage},
    solana_signer::Signer,
    solana_transaction::versioned::VersionedTransaction,
};

pub const ONE_SOL: u64 = 1_000_000_000;

/// Chosen so elapsed seconds are easy to eyeball in assertions.
pub const REWARD_RATE: u64 = 10;
pub const BURN_BONUS: u64 = 1_000_000;

pub struct Env {
    pub svm: LiteSVM,
    pub program_id: Pubkey,
    /// Collection update authority, and the staking config authority.
    pub authority: Keypair,
    pub collection: Pubkey,
    pub config: Pubkey,
    pub reward_mint: Pubkey,
}

impl Env {
    pub fn new() -> Self {
        Self::with_params(REWARD_RATE, BURN_BONUS)
    }

    pub fn with_params(reward_rate: u64, burn_bonus: u64) -> Self {
        let program_id = nft_staking_core::id();
        let mut svm = LiteSVM::new();

        svm.add_program(
            program_id,
            include_bytes!(concat!(
                env!("CARGO_TARGET_TMPDIR"),
                "/../deploy/nft_staking_core.so"
            )),
        )
        .unwrap();
        svm.add_program(mpl_core::ID, include_bytes!("../fixtures/mpl_core.so"))
            .unwrap();

        let authority = Keypair::new();
        svm.airdrop(&authority.pubkey(), 100 * ONE_SOL).unwrap();

        let collection_kp = Keypair::new();
        let collection = collection_kp.pubkey();

        let (config, _) =
            Pubkey::find_program_address(&[CONFIG_SEED, collection.as_ref()], &program_id);
        let (reward_mint, _) =
            Pubkey::find_program_address(&[REWARD_MINT_SEED, collection.as_ref()], &program_id);

        let mut env = Self {
            svm,
            program_id,
            authority,
            collection,
            config,
            reward_mint,
        };

        let create_collection = CreateCollectionV2Builder::new()
            .collection(collection)
            .update_authority(Some(env.authority.pubkey()))
            .payer(env.authority.pubkey())
            .name("Staking Collection".to_string())
            .uri("https://example.com/collection.json".to_string())
            .instruction();
        let authority = env.authority.insecure_clone();
        env.send(&[create_collection], &authority, &[&collection_kp])
            .expect("create collection");

        let initialize = Instruction::new_with_bytes(
            program_id,
            &nft_staking_core::instruction::InitializeConfig {
                reward_rate,
                burn_bonus,
            }
            .data(),
            nft_staking_core::accounts::InitializeConfig {
                authority: env.authority.pubkey(),
                collection,
                config,
                reward_mint,
                mpl_core_program: mpl_core::ID,
                token_program: spl_token::ID,
                system_program: system_program::ID,
            }
            .to_account_metas(None),
        );
        env.send(&[initialize], &authority, &[])
            .expect("initialize_config");

        env
    }

    /// Expires the previous blockhash each call so repeated identical instructions are not
    /// rejected as duplicates.
    pub fn send(
        &mut self,
        instructions: &[Instruction],
        payer: &Keypair,
        extra_signers: &[&Keypair],
    ) -> TransactionResult {
        self.svm.expire_blockhash();
        let blockhash = self.svm.latest_blockhash();
        let message = Message::new_with_blockhash(instructions, Some(&payer.pubkey()), &blockhash);

        let mut signers: Vec<&Keypair> = vec![payer];
        signers.extend_from_slice(extra_signers);

        let tx =
            VersionedTransaction::try_new(VersionedMessage::Legacy(message), &signers).unwrap();
        self.svm.send_transaction(tx)
    }

    pub fn new_user(&mut self) -> Keypair {
        let user = Keypair::new();
        self.svm.airdrop(&user.pubkey(), 100 * ONE_SOL).unwrap();
        user
    }

    pub fn advance_seconds(&mut self, seconds: i64) {
        let mut clock: Clock = self.svm.get_sysvar();
        clock.unix_timestamp += seconds;
        clock.slot += seconds as u64 * 2;
        self.svm.set_sysvar(&clock);
    }

    pub fn now(&self) -> i64 {
        self.svm.get_sysvar::<Clock>().unix_timestamp
    }

    pub fn mint_asset(&mut self, owner: &Keypair, name: &str) -> Pubkey {
        let asset_kp = Keypair::new();
        let asset = asset_kp.pubkey();

        let instruction = CreateV2Builder::new()
            .asset(asset)
            .collection(Some(self.collection))
            // Adding an asset to a collection needs the collection's update authority.
            .authority(Some(self.authority.pubkey()))
            .payer(self.authority.pubkey())
            .owner(Some(owner.pubkey()))
            .data_state(DataState::AccountState)
            .name(name.to_string())
            .uri(format!("https://example.com/{name}.json"))
            .instruction();

        let authority = self.authority.insecure_clone();
        self.send(&[instruction], &authority, &[&asset_kp])
            .unwrap_or_else(|e| panic!("mint asset {name}: {e:?}"));

        asset
    }

    pub fn stake_pda(&self, asset: &Pubkey) -> Pubkey {
        Pubkey::find_program_address(&[STAKE_SEED, asset.as_ref()], &self.program_id).0
    }

    pub fn stake(&mut self, owner: &Keypair, asset: &Pubkey) -> TransactionResult {
        let instruction = Instruction::new_with_bytes(
            self.program_id,
            &nft_staking_core::instruction::Stake {}.data(),
            nft_staking_core::accounts::Stake {
                owner: owner.pubkey(),
                config: self.config,
                collection: self.collection,
                asset: *asset,
                stake_account: self.stake_pda(asset),
                mpl_core_program: mpl_core::ID,
                system_program: system_program::ID,
            }
            .to_account_metas(None),
        );
        let owner = owner.insecure_clone();
        self.send(&[instruction], &owner, &[])
    }

    pub fn claim_rewards(&mut self, owner: &Keypair, asset: &Pubkey) -> TransactionResult {
        let instruction = Instruction::new_with_bytes(
            self.program_id,
            &nft_staking_core::instruction::ClaimRewards {}.data(),
            nft_staking_core::accounts::ClaimRewards {
                owner: owner.pubkey(),
                config: self.config,
                stake_account: self.stake_pda(asset),
                asset: *asset,
                reward_mint: self.reward_mint,
                reward_ata: self.reward_ata(&owner.pubkey()),
                token_program: spl_token::ID,
                associated_token_program: anchor_spl::associated_token::ID,
                system_program: system_program::ID,
            }
            .to_account_metas(None),
        );
        let owner = owner.insecure_clone();
        self.send(&[instruction], &owner, &[])
    }

    pub fn unstake(&mut self, owner: &Keypair, asset: &Pubkey) -> TransactionResult {
        let instruction = Instruction::new_with_bytes(
            self.program_id,
            &nft_staking_core::instruction::Unstake {}.data(),
            nft_staking_core::accounts::Unstake {
                owner: owner.pubkey(),
                config: self.config,
                collection: self.collection,
                asset: *asset,
                stake_account: self.stake_pda(asset),
                reward_mint: self.reward_mint,
                reward_ata: self.reward_ata(&owner.pubkey()),
                mpl_core_program: mpl_core::ID,
                token_program: spl_token::ID,
                associated_token_program: anchor_spl::associated_token::ID,
                system_program: system_program::ID,
            }
            .to_account_metas(None),
        );
        let owner = owner.insecure_clone();
        self.send(&[instruction], &owner, &[])
    }

    pub fn burn_staked_nft(&mut self, owner: &Keypair, asset: &Pubkey) -> TransactionResult {
        let instruction = Instruction::new_with_bytes(
            self.program_id,
            &nft_staking_core::instruction::BurnStakedNft {}.data(),
            nft_staking_core::accounts::BurnStakedNft {
                owner: owner.pubkey(),
                config: self.config,
                collection: self.collection,
                asset: *asset,
                stake_account: self.stake_pda(asset),
                reward_mint: self.reward_mint,
                reward_ata: self.reward_ata(&owner.pubkey()),
                mpl_core_program: mpl_core::ID,
                token_program: spl_token::ID,
                associated_token_program: anchor_spl::associated_token::ID,
                system_program: system_program::ID,
            }
            .to_account_metas(None),
        );
        let owner = owner.insecure_clone();
        self.send(&[instruction], &owner, &[])
    }

    pub fn reward_ata(&self, owner: &Pubkey) -> Pubkey {
        get_associated_token_address(owner, &self.reward_mint)
    }

    pub fn reward_balance(&self, owner: &Pubkey) -> u64 {
        match self.svm.get_account(&self.reward_ata(owner)) {
            Some(account) if !account.data.is_empty() => {
                spl_token::state::Account::unpack(&account.data)
                    .expect("unpack reward ATA")
                    .amount
            }
            _ => 0,
        }
    }

    pub fn config_state(&self) -> StakeConfig {
        let account = self.svm.get_account(&self.config).expect("config exists");
        StakeConfig::try_deserialize(&mut account.data.as_slice()).expect("deserialize config")
    }

    pub fn stake_state(&self, asset: &Pubkey) -> Option<StakeAccount> {
        let account = self.svm.get_account(&self.stake_pda(asset))?;
        if account.data.is_empty() {
            return None;
        }
        Some(
            StakeAccount::try_deserialize(&mut account.data.as_slice())
                .expect("deserialize stake account"),
        )
    }

    /// The `total_staked` Attribute on the Core Collection, which is the source of truth.
    pub fn total_staked_attribute(&self) -> u64 {
        let account = self
            .svm
            .get_account(&self.collection)
            .expect("collection exists");
        let collection = Collection::from_bytes(&account.data).expect("deserialize collection");
        let attributes = collection
            .plugin_list
            .attributes
            .as_ref()
            .expect("collection has an Attributes plugin");

        attributes
            .attributes
            .attribute_list
            .iter()
            .find(|attribute| attribute.key == TOTAL_STAKED_KEY)
            .expect("collection has a total_staked attribute")
            .value
            .parse()
            .expect("total_staked is a u64")
    }

    pub fn attributes_plugin_authority(&self) -> BaseAuthority {
        let account = self
            .svm
            .get_account(&self.collection)
            .expect("collection exists");
        Collection::from_bytes(&account.data)
            .expect("deserialize collection")
            .plugin_list
            .attributes
            .as_ref()
            .expect("collection has an Attributes plugin")
            .base
            .authority
    }

    /// mpl-core burns by shrinking the account to a single `Key::Uninitialized` byte rather
    /// than deallocating it, so the discriminator is what says whether an asset is live.
    pub fn asset_exists(&self, asset: &Pubkey) -> bool {
        self.svm.get_account(asset).is_some_and(|account| {
            account.owner == mpl_core::ID && account.data.first() == Some(&(Key::AssetV1 as u8))
        })
    }

    pub fn asset_owner(&self, asset: &Pubkey) -> Pubkey {
        let account = self.svm.get_account(asset).expect("asset exists");
        BaseAssetV1::from_bytes(&account.data)
            .expect("deserialize asset")
            .owner
    }

    fn asset_plugins(&self, asset: &Pubkey) -> Option<PluginsList> {
        if !self.asset_exists(asset) {
            return None;
        }
        let account = self.svm.get_account(asset)?;
        Some(
            Asset::from_bytes(&account.data)
                .expect("deserialize asset")
                .plugin_list,
        )
    }

    /// `None` if the asset has no `FreezeDelegate` plugin.
    pub fn freeze_state(&self, asset: &Pubkey) -> Option<(bool, BaseAuthority)> {
        let plugins = self.asset_plugins(asset)?;
        let freeze = plugins.freeze_delegate.as_ref()?;
        Some((freeze.freeze_delegate.frozen, freeze.base.authority))
    }

    pub fn has_burn_delegate(&self, asset: &Pubkey) -> bool {
        self.asset_plugins(asset)
            .is_some_and(|plugins| plugins.burn_delegate.is_some())
    }

    pub fn has_freeze_delegate(&self, asset: &Pubkey) -> bool {
        self.asset_plugins(asset)
            .is_some_and(|plugins| plugins.freeze_delegate.is_some())
    }

    pub fn assert_staked_and_frozen(&self, asset: &Pubkey) {
        let (frozen, authority) = self
            .freeze_state(asset)
            .expect("staked asset must carry a FreezeDelegate plugin");
        assert!(frozen, "staked asset must be frozen");
        assert_eq!(
            authority,
            BaseAuthority::from(PluginAuthority::Address {
                address: self.stake_pda(asset)
            }),
            "freeze authority must be the stake PDA"
        );
    }
}
