# Deploying a Counter Contract
*Using the Miden client in Rust to deploy and interact with a basic smart contract on Miden*

## Overview
In this tutorial, we will create a basic counter contract that stores a count. Using a script, we will call the increment count procedure in the counter contract to increment the count. This tutorial will help explain the basics of how to build custom smart contracts on Miden.

## What we'll cover
* Getting up to speed with the basics of Miden assembly
* Calling procedures in an account
* Pure vs state changing procedures

## Prerequisites
This tutorial assumes you have basic familiarity with Miden assembly. To quickly get up to speed, please play around with running Miden programs in the [Miden playground](https://0xpolygonmiden.github.io/examples/).


## Step 1: Initialize your repository
Create a new Rust repository for your Miden project and navigate to it with the following command:
```bash
cargo new miden-counter-contract
cd miden-counter-contract
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
rand_chacha = "0.3.1"
```

### Setup your `src/main.rs` file
In the previous sections, we explained how to instantiate the Miden client. We can reuse the same `initialize_client` function for our counter contract.

Copy and paste the following code into your `src/main.rs` file:
```rust
use std::{fs, path::Path, sync::Arc};

use rand::Rng;
use rand_chacha::rand_core::SeedableRng;
use rand_chacha::ChaCha20Rng;
use tokio::time::Duration;

use miden_client::{
    accounts::{Account, AccountData, AccountStorageMode},
    config::RpcConfig,
    crypto::RpoRandomCoin,
    rpc::TonicRpcClient,
    store::{
        sqlite_store::{config::SqliteStoreConfig, SqliteStore},
        StoreAuthenticator,
    },
    transactions::{LocalTransactionProver, ProvingOptions, TransactionKernel, TransactionRequest},
    Client, ClientError, Felt,
};

use miden_lib::accounts::auth::RpoFalcon512;

use miden_objects::{
    accounts::{AccountBuilder, AccountComponent, AuthSecretKey, StorageSlot},
    assembly::Assembler,
    crypto::{
        dsa::rpo_falcon512::{PublicKey, SecretKey},
        hash::rpo::RpoDigest,
    },
    Word,
};

pub async fn initialize_client() -> Result<Client<RpoRandomCoin>, ClientError> {
    // Default values for store and rpc config
    let store_config = SqliteStoreConfig::default();
    let rpc_config = RpcConfig::default();

    // Create an SQLite store
    let store = SqliteStore::new(&store_config)
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
    let rpc_client = Box::new(TonicRpcClient::new(&rpc_config));

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

pub fn get_new_pk_and_authenticator() -> (Word, AuthSecretKey) {
    let seed = [0_u8; 32];
    let mut rng = ChaCha20Rng::from_seed(seed);

    let sec_key = SecretKey::with_rng(&mut rng);
    let pub_key: Word = sec_key.public_key().into();

    let auth_secret_key = AuthSecretKey::RpoFalcon512(sec_key);

    (pub_key, auth_secret_key)
}

pub fn create_new_account(
    account_component: AccountComponent,
) -> (Account, Option<Word>, AuthSecretKey) {
    let (pub_key, auth_secret_key) = get_new_pk_and_authenticator();

    let (account, seed) = AccountBuilder::new()
        .init_seed(ChaCha20Rng::from_entropy().gen())
        .storage_mode(AccountStorageMode::Public)
        .with_component(account_component)
        .with_component(RpoFalcon512::new(PublicKey::new(pub_key)))
        .build()
        .unwrap();

    (account, Some(seed), auth_secret_key)
}

#[tokio::main]
async fn main() -> Result<(), ClientError> {
    let mut client = initialize_client().await?;
    println!("Client initialized successfully.");

    Ok(())
}
```

## Step 2: Create the counter contract
For code cleanliness we will separate the Miden assembly (MASM) code from our Rust code. 

Create a directory named `masm` at the root of your `miden-counter-contract` directory. This will contain our contract and script masm code. 

Initialize the masm directory:
```bash
mkdir -p masm/accounts masm/scripts
```

This will create:
```
masm/
├── accounts/
└── scripts/
```

Inside of the `accounts/` directory, create the `counter.masm` file:
```masm
use.miden::contracts::wallets::basic
use.miden::account
use.miden::tx
use.std::sys

export.increment_count
    # => []
    push.0

    # => [index]
    exec.account::get_item

    # => [count]
    push.1 add

    # => [count+1]
    push.0

    # [index, count+1]
    exec.account::set_item

    # => []
    push.1 exec.account::incr_nonce

    # debug statement with client
    push.111 debug.stack drop

    # => []
    exec.sys::truncate_stack
end
```

Inside of the `scripts/` directory, create the `increment_count.masm` file:
```masm
begin
    # => []
    call.{increment_count}
end
```

## Step 3: Build the counter smart contract



To build the counter contract copy and paste the following code into your src/main.rs file:
```rs
// File path
let file_path = Path::new("../masm/accounts/counter.masm");

// Read the file contents
let account_code = fs::read_to_string(file_path).unwrap();

// Assembler with debug mode on
let assembler: Assembler = TransactionKernel::assembler().with_debug_mode(true);

// Account component
let account_component = AccountComponent::compile(
    account_code,                               // account code
    assembler,                                  // assembler
    vec![StorageSlot::Value(Word::default())],  // storage slots
)
.unwrap()
.with_supports_all_types();

// Building the counter contract
let (counter_contract, counter_seed, auth_secret_key) = create_new_account(account_component);

println!("counter_contract hash: {:?}", counter_contract.hash());
println!("contract id: {:?}", counter_contract.id().to_hex());

let counter_contract_account_data = AccountData::new(
    counter_contract.clone(),
    counter_seed,
    auth_secret_key.clone(),
);

// Import to client
client.import_account(counter_contract_account_data).await.unwrap();
```

Run the following command to execute src/main.rs:
```
cargo run --release 
```

After the program executes, you should see the counter contract hash and contract id printed to the terminal, for example:
```
counter_contract hash: "0x775d453f837db348507e140530dec49d4b2467e3eb0f8a000266c3f5dc726c9e"
contract id: "0x10cfe13090ec4a24"
```

## Step 4: Computing the prodedure roots

Each Miden assembly procedure has an associated hash. When calling a procedure in a smart contract, we need to know the hash of the procedure. 

To get the prodedures of a the counter contract, add this code snippet to your `main()` function:

```rust
// procedure roots
let procedures = counter_contract.code().procedure_roots();
let procedures_vec: Vec<RpoDigest> = procedures.collect();
for (index, procedure) in procedures_vec.iter().enumerate() {
    println!("Procedure {}: {:?}", index + 1, procedure.to_hex());
}

println!("number of procedures: {}", procedures_vec.len());
```

Run the following command to execute src/main.rs:
```
cargo run --release 
```

After the program executes, you should see the procedure hashes printed to the terminal, for example:
```
Procedure 1: "0x2259e69ba0e49a85f80d5ffc348e25a0386a0bbe7dbb58bc45b3f1493a03c725"
Procedure 2: "0x6f2eccd43c2cbf47b87c443da93739d2d739b1e14cc42ca55fed8ac9b743e462"
```

The first procedure is the `increment_count` procedure.

## Step 4: Incrementing the count

Now that we know the hash of the `increment_count` procedure, we can increment the count of the contract.

In the code below, we do a basic text format to replace the `{increment_count}` string with the hash of `increment_count` procedure. Then we create a new transaction request with our custom script, and then pass the transaction request to the client. 

Paste the following code into your src/main.rs file:
```rust
//------------------------------------------------------------
// STEP 2: Call Counter Contract with script
//------------------------------------------------------------
println!("\n[STEP 2] Call Counter Contract With Script");

// --- 1) Grab the procedure #2 hash and prepare it for insertion into the script
let procedure_2_hash = procedures_vec[0].to_hex();
let procedure_call = format!("{}", procedure_2_hash);

// --- 2) Load MASM script
let file_path = Path::new("../masm/scripts/counter_script.masm");
let original_code = fs::read_to_string(file_path).unwrap();

// --- 3) Replace {increment_count} in the script with the actual call line
let replaced_code = original_code.replace("{increment_count}", &procedure_call);
println!("Final script:\n{}", replaced_code);

// --- 4) Compile the script (now containing the procedure #2 hash)
let tx_script = client.compile_tx_script(vec![], &replaced_code).unwrap();

// --- 5) Execute the transaction
let tx_increment_request = TransactionRequest::new()
    .with_custom_script(tx_script)
    .unwrap();

let tx_result = client
    .new_transaction(counter_contract.id(), tx_increment_request)
    .await
    .unwrap();

println!("tx result id: {:?}", tx_result.executed_transaction().id());

let _ = client.submit_transaction(tx_result).await;

tokio::time::sleep(Duration::from_secs(3)).await;
client.sync_state().await.unwrap();

let (account, _data) = client.get_account(counter_contract.id()).await.unwrap();

println!("storage item 0: {:?}", account.storage().get_item(0));
```


## Summary

Our final `src/main.rs` file should look something like this:

```rust
use std::{fs, path::Path, sync::Arc};

use rand::Rng;
use rand_chacha::rand_core::SeedableRng;
use rand_chacha::ChaCha20Rng;
use tokio::time::Duration;

use miden_client::{
    accounts::{Account, AccountData, AccountStorageMode},
    config::RpcConfig,
    crypto::RpoRandomCoin,
    rpc::TonicRpcClient,
    store::{
        sqlite_store::{config::SqliteStoreConfig, SqliteStore},
        StoreAuthenticator,
    },
    transactions::{LocalTransactionProver, ProvingOptions, TransactionKernel, TransactionRequest},
    Client, ClientError, Felt,
};

use miden_lib::accounts::auth::RpoFalcon512;

use miden_objects::{
    accounts::{AccountBuilder, AccountComponent, AuthSecretKey, StorageSlot},
    assembly::Assembler,
    crypto::{
        dsa::rpo_falcon512::{PublicKey, SecretKey},
        hash::rpo::RpoDigest,
    },
    Word,
};

pub async fn initialize_client() -> Result<Client<RpoRandomCoin>, ClientError> {
    // Default values for store and rpc config
    let store_config = SqliteStoreConfig::default();
    let rpc_config = RpcConfig::default();

    // Create an SQLite store
    let store = SqliteStore::new(&store_config)
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
    let rpc_client = Box::new(TonicRpcClient::new(&rpc_config));

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

pub fn get_new_pk_and_authenticator() -> (Word, AuthSecretKey) {
    let seed = [0_u8; 32];
    let mut rng = ChaCha20Rng::from_seed(seed);

    let sec_key = SecretKey::with_rng(&mut rng);
    let pub_key: Word = sec_key.public_key().into();

    let auth_secret_key = AuthSecretKey::RpoFalcon512(sec_key);

    (pub_key, auth_secret_key)
}

pub fn create_new_account(
    account_component: AccountComponent,
) -> (Account, Option<Word>, AuthSecretKey) {
    let (pub_key, auth_secret_key) = get_new_pk_and_authenticator();

    let (account, seed) = AccountBuilder::new()
        .init_seed(ChaCha20Rng::from_entropy().gen())
        .storage_mode(AccountStorageMode::Public)
        .with_component(account_component)
        .with_component(RpoFalcon512::new(PublicKey::new(pub_key)))
        .build()
        .unwrap();

    (account, Some(seed), auth_secret_key)
}

#[tokio::main]
async fn main() -> Result<(), ClientError> {
    let mut client = initialize_client().await?;
    println!("Client initialized successfully.");

    //------------------------------------------------------------
    // STEP 1: Create a basic counter contract
    //------------------------------------------------------------
    println!("\n[STEP 1] Creating Counter Contract.");

    // Initializing Account
    let file_path = Path::new("../masm/accounts/counter.masm");

    // Read the file contents
    let account_code = fs::read_to_string(file_path).unwrap();

    let assembler: Assembler = TransactionKernel::assembler().with_debug_mode(true);
    let account_component = AccountComponent::compile(
        account_code,
        assembler,
        vec![StorageSlot::Value(Word::default())],
    )
    .unwrap()
    .with_supports_all_types();

    let (counter_contract, counter_seed, auth_secret_key) = create_new_account(account_component);

    println!(
        "counter_contract hash: {:?}",
        counter_contract.hash().to_hex()
    );
    println!("contract id: {:?}", counter_contract.id().to_hex());

    let counter_contract_account_data = AccountData::new(
        counter_contract.clone(),
        counter_seed,
        auth_secret_key.clone(),
    );

    // Import to client
    client
        .import_account(counter_contract_account_data)
        .await
        .unwrap();

    // procedure roots
    let procedures = counter_contract.code().procedure_roots();
    let procedures_vec: Vec<RpoDigest> = procedures.collect();
    for (index, procedure) in procedures_vec.iter().enumerate() {
        println!("Procedure {}: {:?}", index + 1, procedure.to_hex());
    }

    println!("number of procedures: {}", procedures_vec.len());

    //------------------------------------------------------------
    // STEP 2: Call Counter Contract with script
    //------------------------------------------------------------
    println!("\n[STEP 2] Call Counter Contract With Script");

    // --- 1) Grab the procedure #2 hash and prepare it for insertion into the script
    let procedure_2_hash = procedures_vec[0].to_hex();
    let procedure_call = format!("{}", procedure_2_hash);

    // --- 2) Load MASM script
    let file_path = Path::new("../masm/scripts/counter_script.masm");
    let original_code = fs::read_to_string(file_path).unwrap();

    // --- 3) Replace {increment_count} in the script with the actual call line
    let replaced_code = original_code.replace("{increment_count}", &procedure_call);
    println!("Final script:\n{}", replaced_code);

    // --- 4) Compile the script (now containing the procedure #2 hash)
    let tx_script = client.compile_tx_script(vec![], &replaced_code).unwrap();

    // --- 5) Execute the transaction
    let tx_increment_request = TransactionRequest::new()
        .with_custom_script(tx_script)
        .unwrap();

    let tx_result = client
        .new_transaction(counter_contract.id(), tx_increment_request)
        .await
        .unwrap();

    println!("tx result id: {:?}", tx_result.executed_transaction().id());

    let _ = client.submit_transaction(tx_result).await;

    tokio::time::sleep(Duration::from_secs(3)).await;
    client.sync_state().await.unwrap();

    let (account, _data) = client.get_account(counter_contract.id()).await.unwrap();

    println!("storage item 0: {:?}", account.storage().get_item(0));

    Ok(())
}
```