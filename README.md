# NFT Staking Core

Metaplex Core NFT staking built on Anchor 1.x. Users stake Core assets to earn SPL reward
tokens, can claim rewards without unstaking, can burn a staked asset for a one-time bonus, and
the number of currently staked assets is tracked on the Core Collection account itself.

Staking does not escrow the asset. The NFT stays in the owner's wallet and is frozen in place
using a `FreezeDelegate` plugin whose authority is the per-asset stake PDA. The owner keeps
custody but cannot transfer or unfreeze the NFT while it is staked.

## Layout

```
programs/nft-staking-core/
├── src/
│   ├── lib.rs                 # program entrypoints
│   ├── constants.rs           # PDA seeds, the "total_staked" attribute key
│   ├── error.rs
│   ├── asset_checks.rs        # Core asset reads/assertions, reward minting
│   ├── collection_stats.rs    # read/write of the collection's total_staked Attribute
│   ├── state/
│   │   ├── stake_config.rs    # per-collection config
│   │   └── stake_account.rs   # per-asset stake and reward accrual
│   └── instructions/
│       ├── initialize_config.rs
│       ├── stake.rs
│       ├── claim_rewards.rs
│       ├── unstake.rs
│       └── burn_staked_nft.rs
└── tests/
    ├── common/mod.rs          # LiteSVM harness
    ├── fixtures/mpl_core.so   # mpl-core program binary, dumped from mainnet
    ├── test_claim_rewards.rs
    ├── test_burn_staked_nft.rs
    └── test_collection_stats.rs
```

## Build and test

```bash
anchor build
cargo test        # or: anchor test
```

Tests run on LiteSVM against the actual `mpl_core` program binary in `tests/fixtures/`, so all
plugin behavior, delegations and lifecycle validations execute for real instead of being mocked.
The fixture was produced with:

```bash
solana program dump -u m CoREENxT6tW1HoK8ypY1SxRMZTcVPm7R94rH4PZNhX7d mpl_core.so
```

## Accounts

| Account | Seeds | Role |
| --- | --- | --- |
| `StakeConfig` | `["config", collection]` | One per collection. Mint authority of the reward mint and plugin authority of the collection's `Attributes` plugin. |
| reward mint | `["reward_mint", collection]` | SPL mint, 9 decimals, mintable only by the config PDA. |
| `StakeAccount` | `["stake", asset]` | One per staked asset. Its existence is the staked flag. Holds the asset's `FreezeDelegate` and `BurnDelegate` authority. |

## Instructions

### `initialize_config(reward_rate, burn_bonus)`

Signed by the collection's Core update authority. Creates the config PDA and the reward mint,
then adds an `Attributes` plugin to the Collection account seeded with `total_staked = "0"`.
The plugin's authority is set to the config PDA.

Setting the plugin authority to the config PDA is what makes Task 3 work without friction. Since
the PDA owns the `Attributes` plugin, later stake and unstake calls update the counter by signing
as the PDA, so the collection authority only has to sign once at initialization rather than
co-signing every user's transaction.

### `stake()`

1. Verify the signer owns the asset and the asset belongs to the configured collection.
2. Create the `StakeAccount` PDA. Anchor's `init` makes a second stake of the same asset fail.
3. CPI `AddPluginV1` with `FreezeDelegate { frozen: true }`, authority set to the stake PDA.
4. CPI `AddPluginV1` with `BurnDelegate {}`, authority set to the stake PDA.
5. Increment the collection's `total_staked` Attribute.

mpl-core refuses to remove or revoke a `FreezeDelegate` while it is frozen, so step 3 locks the
NFT until this program thaws it.

### `claim_rewards()`

Task 1. Mints `elapsed_seconds * reward_rate` to the owner's reward ATA, then advances
`last_claim_at` and `total_claimed`.

The instruction takes no `mpl_core_program` account and no mutable `asset`, so no code path
through it can thaw, transfer or burn the NFT. It asserts up front that the asset is still frozen
with the stake PDA as freeze authority, and leaves it in that state.

### `unstake()`

Settles outstanding rewards, thaws the asset while signing as the freeze authority, removes both
delegations so the asset is returned unencumbered, decrements `total_staked`, and closes the
stake account. An unstaked asset can be staked again from a clean plugin registry.

### `burn_staked_nft()`

Task 2. Settles accrued rewards, adds `config.burn_bonus`, mints the total, then thaws the asset
and calls `BurnV1` as the `BurnDelegate` authority.

The thaw has to come first because `FreezeDelegate::validate_burn` rejects a burn while the asset
is frozen. The same stake PDA holds both the freeze authority and the burn delegation, so one
instruction can do both. The stake account is closed and `total_staked` is decremented.

```
accrued rewards (100) + burn bonus (1,000) = 1,100 tokens minted, NFT destroyed
```

### Collection-level `total_staked`

Task 3. The counter is not a field on an Anchor account. It is an `Attribute` on the Core
Collection account:

```
Collection account (owned by mpl-core)
└── Attributes plugin  (authority: StakeConfig PDA)
    └── { key: "total_staked", value: "2" }
```

`collection_stats.rs` reads the plugin, edits only the `total_staked` entry, and writes the list
back. The full list is rewritten because `UpdateCollectionPluginV1` replaces the whole plugin, so
any other attributes on the collection are preserved rather than dropped. Decrementing at zero
fails with `TotalStakedUnderflow` instead of wrapping.

Because the attribute value is a string, the counter growing from `"9"` to `"10"` changes the
plugin's serialized length and reallocates the collection account. Both the grow and shrink paths
are covered in the tests.

## Notes on the implementation

Two details are worth recording for anyone extending this.

**SBF stack limit.** On-chain reads use mpl-core's single-plugin fetchers
(`fetch_collection_plugin`, `fetch_asset_plugin`) rather than `Collection::from_bytes` or
`Asset::from_bytes`. The latter build the full 19-variant `PluginsList`, whose stack frame is
4224 to 4480 bytes against the SBF limit of 4096 bytes per frame. Using the targeted fetchers
keeps those functions out of the linked program. The tests still use the full deserializers,
which is fine because there is no stack frame limit off-chain.

**Reward accounting.** `StakeAccount::settle` is the single place rewards are computed and the
accrual clock is advanced. `claim_rewards`, `unstake` and `burn_staked_nft` all go through it, so
there is no way to be paid twice for the same elapsed time.

## Assignment checklist

Baseline

- [x] Program builds (`anchor build`)
- [x] Tests pass (`cargo test`), 26 tests

Task 1, claim rewards

- [x] `claim_rewards` instruction
- [x] Finds the user's staking account (PDA derived from the asset, `has_one` on owner)
- [x] Calculates accumulated rewards as `elapsed * reward_rate`
- [x] Creates or finds the user's reward ATA (`init_if_needed`)
- [x] Mints reward tokens to the ATA
- [x] Updates reward state (`last_claim_at`, `total_claimed`)
- [x] Does not unstake or burn, NFT stays frozen
- [x] Tests

Task 2, burn to earn

- [x] `burn_staked_nft` instruction
- [x] Verifies the NFT is currently staked and frozen under the stake PDA's delegation
- [x] Verifies the signer owns the stake
- [x] Calculates accumulated rewards
- [x] Adds the burn bonus
- [x] Mints the total to the user's reward ATA
- [x] Burns the NFT through `BurnDelegate`
- [x] Closes the staking state
- [x] NFT cannot be unstaked, claimed or re-burned afterwards
- [x] Tests

Task 3, collection stats

- [x] `"total_staked"` Attribute on the Collection account
- [x] Initialized to 0
- [x] Incremented on stake
- [x] Decremented on unstake and on burn
- [x] Cannot underflow (`checked_sub`, returns `TotalStakedUnderflow`)
- [x] Verified by reading the raw Core Collection account in tests

Optional Oracle challenge is out of scope and not implemented.

## Test coverage

| Suite | Tests |
| --- | --- |
| `test_claim_rewards` | 7 |
| `test_burn_staked_nft` | 6 |
| `test_collection_stats` | 8 |
| `collection_stats` unit tests | 5 |

`test_claim_rewards` covers payout math, the NFT remaining frozen under the stake PDA after a
claim, repeat claims paying only newly elapsed time, rejection of a signer who does not own the
stake, rejection of an asset that was never staked, unstake settling the remainder and releasing
both plugins, re-staking resetting the accrual clock, and the config PDA holding the Attributes
plugin authority.

`test_burn_staked_nft` covers the accrued plus bonus payout, no double payment after an earlier
claim, the zero-bonus case, the NFT being destroyed, rejection of claim, unstake and re-burn after
a burn, rejection of a signer who does not own the stake, and rejection of an asset that was never
staked.

`test_collection_stats` covers the counter moving 0, 1, 2, 1, 0 across two users, burn
decrementing the counter, the 9 to 10 to 11 decimal-width reallocation in both directions, the
config mirror matching the attribute, a double stake failing without inflating the counter, no
underflow on an invalid unstake, rejection of an asset outside the collection, and direct
inspection of the raw on-chain attribute.

The `collection_stats` unit tests cover the underflow guard at zero and the overflow guard at
`u64::MAX`, locating the counter by key rather than position, and rejecting malformed values. These
are unit tests because the integration path cannot reach the underflow branch, since the missing
stake account rejects the transaction first.

## Known behavior

Two edge cases are left as strict failures rather than being handled:

- `initialize_config` fails if the collection already has an `Attributes` plugin.
- `stake` fails if the asset already has a `FreezeDelegate` or `BurnDelegate` plugin.

Both are conservative. Merging into an existing plugin would require deciding who keeps authority
over it, which is a policy question rather than an implementation detail.
