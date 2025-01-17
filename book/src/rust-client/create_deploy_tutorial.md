# Creating Accounts and Faucets 

*Using the Miden Client in Rust to Create Accounts and Deploy Faucets*

In this tutorial, we're going to explore how to get started with the Polygon Miden client in Rust, walking through creating accounts and deploying faucets.

## What We'll Cover
* Understanding the difference between public vs. private accounts & notes
* Instantiating the miden-client
* Creating new accounts (public or private)
* Deploying a faucet

## Public vs. Private Accounts & Notes
Before we dive into the coding side of things, let's clarify the concepts of public vs. private Notes and Accounts on Miden:

* Public Accounts: The account's data and code are stored on-chain and are openly visible, including its assets.
* Private Accounts: The account's state and logic are off-chain, only known to its owner.
* Public Notes: The note's state is visible to anyone - perfect for scenarios where transparency is desired.
* Private Notes: The note's state is stored off-chain, you will need to share the note data with the relevant parties (via email or Telegram) for them to be able to consume.

*It is useful to think of notes on Miden as "cryptographic cashier's checks" that allow users to send tokens. If the note is private, the note transfer is only known to the sender and receiver.*

## Overview

In this tutorial we will create a miden account for *Alice* and then deploy a fungible faucet. In the next section we will mint tokens from the faucet, and then send the tokens from Alice's account to other Miden accounts.

## Prerequisites

To begin, make sure you have a miden-node running locally in a separate terminal window. To get the miden-node running locally, you can follow the instructions on the [Miden Node Setup](./miden_node_setup_tutorial.md) page.

## Step 1: Initializing Your Repository
Create a new rust repository for your Miden project and navigate to it using this command:
```bash
cargo new miden-rust-client
cd miden-rust-client 
```

Add the following dependencies to your `Cargo.toml` file:
```toml
[dependencies]
miden-client = { version = "0.6", features = ["testing", "concurrent", "tonic", "sqlite"] }
miden-lib = { version = "0.6", default-features = false }
miden-objects = { version = "0.6", default-features = false }
miden-crypto = { version = "0.13.0", features = ["executable"] }
rand = { version = "0.8" }
serde = { version = "1", features = ["derive"] }
serde_json = { version = "1.0", features = ["raw_value"] }
tokio = { version = "1.40", features = ["rt-multi-thread", "net", "macros"] }
figment = { version = "0.10", features = ["toml", "env"] }
rand_chacha = "0.3.1"
```

## Step 2: Initialize the Client
Before we can interact with the Miden network, we need to instantiate the client. To do so, we need to create a `miden-client.toml` file at the root of our miden-rust-client repository.

```toml
[rpc]
timeout_ms = 10000

[rpc.endpoint]
protocol = "http"
host = "localhost"        # localhost
# host = "18.203.155.106" # testnet
port = 57291

[store]
database_filepath = "store.sqlite3"
```

Next, we need to modify our `src/main.rs` file.

Copy and paste the following code snippet into your `/src/main.rs` file. 
```rust
use figment::{
    providers::{Format, Toml},
    Figment,
};
use miden_client::{
    accounts::{AccountStorageMode,AccountTemplate},
    assets::TokenSymbol,
    config::RpcConfig,
    crypto::RpoRandomCoin,
    rpc::TonicRpcClient,
    store::{
        sqlite_store::{config::SqliteStoreConfig, SqliteStore},
        StoreAuthenticator,
    },
    transactions::{LocalTransactionProver, ProvingOptions},
    Client, ClientError, Felt,
};
use rand::Rng;
use serde::Deserialize;
use std::{path::Path, sync::Arc};

/// Name of your local TOML file containing client config.
/// Adjust this if you store it in another place/name.
const CLIENT_CONFIG_FILE_NAME: &str = "miden-client.toml";

/// Simple container for everything in your TOML file (RPC + store configs, etc.)
#[derive(Debug, Deserialize)]
pub struct ClientConfig {
    /// Describes settings related to the RPC endpoint
    pub rpc: RpcConfig,
    /// Describes settings related to the store.
    pub store: SqliteStoreConfig,
}

impl ClientConfig {
    pub fn from_file<P: AsRef<Path>>(path: P) -> Self {
        let figment = Figment::from(Toml::file(path));
        figment.extract().unwrap_or_else(|e| {
            panic!("Failed to load client config: {}", e);
        })
    }
}

/// This function initializes the `Client` using the parameters
/// from `miden-client.toml`. It loads the store, seeds the RNG,
/// sets up the authenticator, local prover, and returns a `Client`.
pub async fn initialize_client() -> Result<Client<RpoRandomCoin>, ClientError> {
    let client_config = ClientConfig::from_file(CLIENT_CONFIG_FILE_NAME);

    // Create an SQLite store
    let store = SqliteStore::new(&client_config.store)
        .await
        .map_err(ClientError::StoreError)?;
    let arc_store = Arc::new(store);

    // Seed both random coin instances
    let mut seed_rng = rand::thread_rng();
    let coin_seed: [u64; 4] = seed_rng.gen();
    let rng_for_auth = RpoRandomCoin::new(coin_seed.map(Felt::new));
    let rng_for_client = RpoRandomCoin::new(coin_seed.map(Felt::new));

    // Create an authenticator that references the store
    let authenticator = StoreAuthenticator::new_with_rng(arc_store.clone(), rng_for_auth);

    // Local prover (you could swap out for delegated proving)
    let tx_prover = LocalTransactionProver::new(ProvingOptions::default());

    // Build the RPC client
    let rpc_client = Box::new(TonicRpcClient::new(&client_config.rpc));

    // Finally create the client
    let client = Client::new(
        rpc_client,
        rng_for_client,
        arc_store,
        Arc::new(authenticator),
        Arc::new(tx_prover),
        true,
    );

    Ok(client)
}

#[tokio::main]
async fn main() -> Result<(), ClientError> {
    let mut client = initialize_client().await?;

    let sync_summary = client.sync_state().await.unwrap();
    let block_number = sync_summary.block_num;

    println!("Latest block number: {}", block_number);

    Ok(())
}
```

Now, lets run our `src/main.rs` file to initialize the miden-client:
```bash
cargo run --release 
```

After running this file, you should see the latest block number printed to the terminal:
```
Current block number: 607436
```

## Step 3: Creating a Wallet
Now that we've initialized the client, we can now create a wallet for Alice. 

To create a wallet for Alice using the miden client, we specify the account type by specifying if the account code is mutable or immutable and whether the account is public or private. In the examples below we create a mutable public account for Alice.

```rust
let alice_template = AccountTemplate::BasicWallet {
    mutable_code: true,
    storage_mode: AccountStorageMode::Public,
};

let (alice_account, _alice_seed) = client.new_account(alice_template).await?;
```


## Step 4: Deploying a Fungible Faucet
For Alice to have testnet assets, we need to first deploy a faucet. A faucet account on Miden, mints fungible tokens. We'll create a public faucet with a token symbol, decimals, and a max supply. We will use this faucet to mint tokens to Alice's account. 

```rust
let faucet_template = AccountTemplate::FungibleFaucet {
    token_symbol: TokenSymbol::new("MID").unwrap(),
    decimals: 8,
    max_supply: 1_000_000,
    storage_mode: AccountStorageMode::Public,
};

let (faucet_account, _faucet_seed) = client.new_account(faucet_template).await?;
```

*When tokens are minted from this faucet, each token batch is represented as a "note" (UTXO). You can think of a Miden Note as a cryptographic cashier's check that has certain spend conditions attached to it.*


## Summary

Our new `main()` function in the `src/main.rs` file should look something like this:

```rust
#[tokio::main]
async fn main() -> Result<(), ClientError> {
    let mut client = initialize_client().await?;

    let sync_summary = client.sync_state().await.unwrap();
    let block_number = sync_summary.block_num;

    println!("Latest block number: {}", block_number);

    let alice_template = AccountTemplate::BasicWallet {
        mutable_code: true,
        storage_mode: AccountStorageMode::Public,
    };
    
    let (alice_account, _alice_seed) = client.new_account(alice_template).await?;

    println!("Alice's account id: {}", alice_account.id());

    let faucet_template = AccountTemplate::FungibleFaucet {
        token_symbol: TokenSymbol::new("MID").unwrap(),
        decimals: 8,
        max_supply: 1_000_000,
        storage_mode: AccountStorageMode::Public,
    };
    
    let (faucet_account, _faucet_seed) = client.new_account(faucet_template).await?;

    println!("Faucet account id: {}", faucet_account.id());

    Ok(())
}
```

Let's run the `src/main.rs` program again:
```bash
cargo run --release 
```

The output will look like this:
```bash
Latest block number: 607494
Alice's account id: 0x1a8eefbcfef43f48
Faucet account id: 0x2d7969e6125856d0
```

In this section we explained how to instantiate the miden-client, create a wallet account, and deploy a faucet. In the next section we will cover how to mint tokens from the faucet, consume notes, and send tokens to other accounts. 


### Running the Example
To run a full working example navigate to the `rust-client` directory in the miden-tutorials repository and run this command:
```bash
cd rust-client
cargo run --release --bin create_mint_consume_send
```