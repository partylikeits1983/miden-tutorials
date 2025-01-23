# Deploying a Counter Contract
*Using the Miden client in Rust to deploy and interact with a custom smart contract on Miden*

## Overview
In this tutorial, we will build a simple counter smart contract that maintains a count, deploy it to the Miden testnet, and interact with it by incrementing the count.

Using a script, we will invoke the increment function within the counter contract to update the count. This tutorial provides a foundational understanding of developing and deploying custom smart contracts on Miden.

## What we'll cover
* Getting up to speed with the basics of Miden assembly
* Calling procedures in an account
* Pure vs state changing procedures

## Prerequisites
This tutorial assumes you have a basic understanding of Miden Assembly. To quickly get up to speed with Miden assembly (MASM), please play around with running Miden programs in the [Miden playground](https://0xpolygonmiden.github.io/examples/).

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
In the previous section, we explained how to instantiate the Miden client. We can reuse the same `initialize_client` function for our counter contract.

Copy and paste the following code into your `src/main.rs` file:
```rust
use std::{fs, path::Path, sync::Arc};

use rand::Rng;
use rand_chacha::rand_core::SeedableRng;
use rand_chacha::ChaCha20Rng;
use tokio::time::Duration;

use miden_client::{
    accounts::{Account, AccountData, AccountStorageMode},
    config::{Endpoint, RpcConfig},
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

    let endpoint = Endpoint::new("http".to_string(), "18.203.155.106".to_string(), 57291);
    let rpc_config = RpcConfig {
        endpoint,
        timeout_ms: 10000,
    };

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
    // Create a deterministic RNG with a zeroed seed for this example.
    let seed = [0_u8; 32];
    let mut rng = ChaCha20Rng::from_seed(seed);

    // Generate a new Falcon-512 secret key.
    let sec_key = SecretKey::with_rng(&mut rng);

    // Convert the Falcon-512 public key into a `Word` (a 4xFelt representation).
    let pub_key: Word = sec_key.public_key().into();

    // Wrap the secret key in an `AuthSecretKey` for account authentication.
    let auth_secret_key = AuthSecretKey::RpoFalcon512(sec_key);

    (pub_key, auth_secret_key)
}

pub fn create_new_account(
    account_component: AccountComponent,
) -> (Account, Option<Word>, AuthSecretKey) {
    // Generate a new public/secret keypair (Falcon-512).
    let (pub_key, auth_secret_key) = get_new_pk_and_authenticator();

    // Build a new `Account` using the provided component plus the Falcon-512 verifier.
    // Uses a random seed for the account’s RNG.
    let (account, seed) = AccountBuilder::new()
        .init_seed(ChaCha20Rng::from_entropy().gen()) // Random seed
        .storage_mode(AccountStorageMode::Public)
        .with_component(account_component) // The main contract logic
        .with_component(RpoFalcon512::new(PublicKey::new(pub_key))) // The auth verifier
        .build()
        .unwrap();

    (account, Some(seed), auth_secret_key)
}

#[tokio::main]
async fn main() -> Result<(), ClientError> {
    let mut client = initialize_client().await?;
    println!("Client initialized successfully.");

    let sync_summary = client.sync_state().await.unwrap();
    println!("Latest block: {}", sync_summary.block_num);

    Ok(())
}
```

*When running the code above, there will be some unused imports, however, we will use these imports later on in the tutorial.*

## Step 2: Build the counter contract
For better code organization, we will separate the Miden Assembly code from our Rust code.

Create a directory named `masm` at the **root** of your `miden-counter-contract` directory. This will contain our contract and script masm code. 

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

### Custom Miden smart contract
Below is our counter contract. It has a single exported procedure `increment_count`. At the beginning of the MASM file we define our imports. In this case we are importing `miden::account` and `std::sys`.

The import `miden::account` contains useful procedures for interacting with a smart contract's state. 

The import `std::sys` contains a useful procedure for truncating the operand stack at the end of a procedure.

Here's a breakdown of what the `increment_count` procedure does:

1) Pushes the 0 onto the stack, which is the index of the storage slot we want to read
2) Calls `account::get_item` with the index of 0
3) Pushes 1 onto the stack
4) Adds 1 and the count value returned from `account::get_item`
5) Pushes 0 onto the stack, which is the index of the storage slot we want to write to
6) Calls `account::set_item` which saves the incremented count to storage at index 0
7) *For demonstration purposes*, pushes 111 onto the stack, calls `debug.stack`, then drops 111
8) Calls `sys::truncate_stack` to truncate the stack to size 16.

Inside of the `accounts/` directory, create the `counter.masm` file:
```masm
use.miden::account
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

### Custom script
This is a Miden assembly script that will call the `increment_count` procedure during the transaction. 

The string `{increment_count}` will be replaced with the hash of the `increment_count` procedure in our rust program.

Inside of the `scripts/` directory, create the `counter_script.masm` file:
```masm
begin
    # => []
    call.{increment_count}
end
```

## Step 3: Build the counter smart contract in Rust
To build the counter contract copy and paste the following code into your src/main.rs file:
```rs
// File path
let file_path = Path::new("./masm/accounts/counter.masm");

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

Each Miden assembly procedure has an associated hash. When calling a procedure in a smart contract, we need to know the hash of the procedure. The hashes of the procedures form a [Merkelized Abstract Syntax Tree (MAST).](https://0xpolygonmiden.github.io/miden-vm/design/programs.html)

To get the procedures of the counter contract, add this code snippet to your `main()` function:
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

The first procedure is the `increment_count` procedure. @Dominik, not really sure what the procedure ending in "e462" is. All accounts have this procedure?

## Step 4: Incrementing the count

Now that we know the hash of the `increment_count` procedure, we can call the procedure in the counter contract. In the Rust code below, we replace the `{increment_count}` string with the hash of the `increment_count` procedure.

Then we create a new transaction request with our custom script, and then pass the transaction request to the client. 

Paste the following code into your src/main.rs file:
```rust
// -------------------------------------------------------------------------
// STEP 2: Call the Counter Contract with a script
// -------------------------------------------------------------------------
println!("\n[STEP 2] Call Counter Contract With Script");

// 2A) Grab the compiled procedure hash (in this case, the first procedure).
let procedure_2_hash = procedures_vec[0].to_hex();
let procedure_call = format!("{}", procedure_2_hash);

// 2B) Load a MASM script that will reference our increment procedure.
let file_path = Path::new("./masm/scripts/counter_script.masm");
let original_code = fs::read_to_string(file_path).unwrap();

// 2C) Replace the placeholder `{increment_count}` in the script with the actual procedure call.
let replaced_code = original_code.replace("{increment_count}", &procedure_call);
println!("Final script:\n{}", replaced_code);

// 2D) Compile the script (which now references our procedure).
let tx_script = client.compile_tx_script(vec![], &replaced_code).unwrap();

// 2E) Build a transaction request using the custom script.
let tx_increment_request = TransactionRequest::new()
    .with_custom_script(tx_script)
    .unwrap();

// 2F) Execute the transaction locally (producing a result).
let tx_result = client
    .new_transaction(counter_contract.id(), tx_increment_request)
    .await
    .unwrap();

let tx_id = tx_result.executed_transaction().id();
println!(
    "View transaction on MidenScan: https://testnet.midenscan.com/tx/{:?}",
    tx_id
);

// 2G) Submit the transaction to the network.
let _ = client.submit_transaction(tx_result).await;

// Wait a bit for the network to process the transaction, then re-sync.
tokio::time::sleep(Duration::from_secs(3)).await;
client.sync_state().await.unwrap();

// 2H) Retrieve the updated contract data and observe the incremented counter.
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
    config::{Endpoint, RpcConfig},
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

    let endpoint = Endpoint::new("http".to_string(), "18.203.155.106".to_string(), 57291);
    let rpc_config = RpcConfig {
        endpoint,
        timeout_ms: 10000,
    };

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
    // Create a deterministic RNG with a zeroed seed for this example.
    let seed = [0_u8; 32];
    let mut rng = ChaCha20Rng::from_seed(seed);

    // Generate a new Falcon-512 secret key.
    let sec_key = SecretKey::with_rng(&mut rng);

    // Convert the Falcon-512 public key into a `Word` (a 4xFelt representation).
    let pub_key: Word = sec_key.public_key().into();

    // Wrap the secret key in an `AuthSecretKey` for account authentication.
    let auth_secret_key = AuthSecretKey::RpoFalcon512(sec_key);

    (pub_key, auth_secret_key)
}

pub fn create_new_account(
    account_component: AccountComponent,
) -> (Account, Option<Word>, AuthSecretKey) {
    // Generate a new public/secret keypair (Falcon-512).
    let (pub_key, auth_secret_key) = get_new_pk_and_authenticator();

    // Build a new `Account` using the provided component plus the Falcon-512 verifier.
    // Uses a random seed for the account’s RNG.
    let (account, seed) = AccountBuilder::new()
        .init_seed(ChaCha20Rng::from_entropy().gen()) // Random seed
        .storage_mode(AccountStorageMode::Public)
        .with_component(account_component) // The main contract logic
        .with_component(RpoFalcon512::new(PublicKey::new(pub_key))) // The auth verifier
        .build()
        .unwrap();

    (account, Some(seed), auth_secret_key)
}

#[tokio::main]
async fn main() -> Result<(), ClientError> {
    // -------------------------------------------------------------------------
    // Initialize the Miden client
    // -------------------------------------------------------------------------
    let mut client = initialize_client().await?;
    println!("Client initialized successfully.");

    // Fetch and display the latest synchronized block number from the node.
    let sync_summary = client.sync_state().await.unwrap();
    println!("Latest block: {}", sync_summary.block_num);

    // -------------------------------------------------------------------------
    // STEP 1: Build the Counter Contract
    // -------------------------------------------------------------------------
    println!("\n[STEP 1] Build the Counter Contract");

    // 1A) Load the MASM file containing an account definition (e.g. a 'counter' contract).
    let file_path = Path::new("./masm/accounts/counter.masm");
    let account_code = fs::read_to_string(file_path).unwrap();

    // 1B) Prepare the assembler for compiling contract code (debug mode = true).
    let assembler: Assembler = TransactionKernel::assembler().with_debug_mode(true);

    // 1C) Compile the account code into an `AccountComponent`
    //     and initialize it with one storage slot (for our counter).
    let account_component = AccountComponent::compile(
        account_code,
        assembler,
        vec![StorageSlot::Value(Word::default())],
    )
    .unwrap()
    .with_supports_all_types();

    // 1D) Build a new account for the counter contract, retrieve the account, seed, and secret key.
    let (counter_contract, counter_seed, auth_secret_key) = create_new_account(account_component);

    println!(
        "counter_contract hash: {:?}",
        counter_contract.hash().to_hex()
    );
    println!("contract id: {:?}", counter_contract.id().to_hex());

    // 1E) Wrap the contract into `AccountData` with its seed and secret key, then import into the client.
    let counter_contract_account_data = AccountData::new(
        counter_contract.clone(),
        counter_seed,
        auth_secret_key.clone(),
    );

    client
        .import_account(counter_contract_account_data)
        .await
        .unwrap();

    // 1F) Print out procedure root hashes for debugging/inspection.
    let procedures = counter_contract.code().procedure_roots();
    let procedures_vec: Vec<RpoDigest> = procedures.collect();
    for (index, procedure) in procedures_vec.iter().enumerate() {
        println!("Procedure {}: {:?}", index + 1, procedure.to_hex());
    }
    println!("number of procedures: {}", procedures_vec.len());

    // -------------------------------------------------------------------------
    // STEP 2: Call the Counter Contract with a script
    // -------------------------------------------------------------------------
    println!("\n[STEP 2] Call Counter Contract With Script");

    // 2A) Grab the compiled procedure hash (in this case, the first procedure).
    let procedure_2_hash = procedures_vec[0].to_hex();
    let procedure_call = format!("{}", procedure_2_hash);

    // 2B) Load a MASM script that will reference our increment procedure.
    let file_path = Path::new("./masm/scripts/counter_script.masm");
    let original_code = fs::read_to_string(file_path).unwrap();

    // 2C) Replace the placeholder `{increment_count}` in the script with the actual procedure call.
    let replaced_code = original_code.replace("{increment_count}", &procedure_call);
    println!("Final script:\n{}", replaced_code);

    // 2D) Compile the script (which now references our procedure).
    let tx_script = client.compile_tx_script(vec![], &replaced_code).unwrap();

    // 2E) Build a transaction request using the custom script.
    let tx_increment_request = TransactionRequest::new()
        .with_custom_script(tx_script)
        .unwrap();

    // 2F) Execute the transaction locally (producing a result).
    let tx_result = client
        .new_transaction(counter_contract.id(), tx_increment_request)
        .await
        .unwrap();

    let tx_id = tx_result.executed_transaction().id();
    println!(
        "View transaction on MidenScan: https://testnet.midenscan.com/tx/{:?}",
        tx_id
    );

    // 2G) Submit the transaction to the network.
    let _ = client.submit_transaction(tx_result).await;

    // Wait a bit for the network to process the transaction, then re-sync.
    tokio::time::sleep(Duration::from_secs(3)).await;
    client.sync_state().await.unwrap();

    // 2H) Retrieve the updated contract data and observe the incremented counter.
    let (account, _data) = client.get_account(counter_contract.id()).await.unwrap();
    println!("storage item 0: {:?}", account.storage().get_item(0));

    Ok(())
}
```

The output of our program will look something like this:
```
Client initialized successfully.
Latest block: 622
counter_contract hash: RpoDigest([9185992507701071673, 6960916831737267302, 1553714328610042343, 12936151506213499503])
contract id: "0x118c7e3eb1017498"
Procedure 1: "0x2259e69ba0e49a85f80d5ffc348e25a0386a0bbe7dbb58bc45b3f1493a03c725"
Procedure 2: "0x6f2eccd43c2cbf47b87c443da93739d2d739b1e14cc42ca55fed8ac9b743e462"
number of procedures: 2

[STEP 2] Call Counter Contract With Script
Final script:
begin
    # => []
    call.0x2259e69ba0e49a85f80d5ffc348e25a0386a0bbe7dbb58bc45b3f1493a03c725
end
Stack state before step 2804:
├──  0: 111
├──  1: 0
├──  2: 0
├──  3: 0
├──  4: 0
├──  5: 0
├──  6: 0
├──  7: 0
├──  8: 0
├──  9: 0
├── 10: 0
├── 11: 0
├── 12: 0
├── 13: 0
├── 14: 0
├── 15: 0
├── 16: 0
├── 17: 0
├── 18: 0
├── 19: 0
├── 20: 0
├── 21: 0
├── 22: 0
├── 23: 0
└── 24: 0

View transaction on MidenScan: https://testnet.midenscan.com/tx/0x8dc90ff6db9fac3672175e15d68721d50765cf96d157540dc17a482101e7fdb8
storage item 0: Ok(RpoDigest([0, 0, 0, 1]))
```

The line in the output `Stack state before step 2804` ouputs the stack state when we call "debug.stack" in the `counter.masm` file.

### Running the Example
To run a full working example navigate to the `rust-client` directory in the [miden-tutorials](https://github.com/0xPolygonMiden/miden-tutorials/) repository and run this command:
```bash
cd rust-client
cargo run --release --bin counter_contract_increment
```
