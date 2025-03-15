use std::{fs, path::Path, sync::Arc};

use rand::Rng;
use rand_chacha::rand_core::SeedableRng;
use rand_chacha::ChaCha20Rng;

use miden_client::{
    account::{AccountStorageMode, AccountType},
    crypto::RpoRandomCoin,
    rpc::{Endpoint, TonicRpcClient},
    store::{sqlite_store::SqliteStore, StoreAuthenticator},
    transaction::{TransactionKernel, TransactionRequestBuilder},
    Client, ClientError, Felt,
};

use miden_objects::{
    account::{AccountBuilder, AccountComponent, AuthSecretKey, StorageMap, StorageSlot},
    assembly::{Assembler, DefaultSourceManager},
    crypto::dsa::rpo_falcon512::SecretKey,
    transaction::TransactionScript,
    Word,
};

use miden_tx::auth::signatures::get_falcon_signature;

use miden_assembly::{
    ast::{Module, ModuleKind},
    LibraryPath,
};

pub async fn initialize_client() -> Result<Client<RpoRandomCoin>, ClientError> {
    // RPC endpoint and timeout
    let endpoint = Endpoint::new(
        "https".to_string(),
        "rpc.testnet.miden.io".to_string(),
        Some(443),
    );
    let timeout_ms = 10_000;

    // Build RPC client
    let rpc_api = Box::new(TonicRpcClient::new(endpoint, timeout_ms));

    // Seed RNG
    let mut seed_rng = rand::thread_rng();
    let coin_seed: [u64; 4] = seed_rng.gen();

    // Create random coin instance
    let rng = RpoRandomCoin::new(coin_seed.map(Felt::new));

    // SQLite path
    let store_path = "store.sqlite3";

    // Initialize SQLite store
    let store = SqliteStore::new(store_path.into())
        .await
        .map_err(ClientError::StoreError)?;
    let arc_store = Arc::new(store);

    // Create authenticator referencing the store and RNG
    let authenticator = StoreAuthenticator::new_with_rng(arc_store.clone(), rng);

    // Instantiate client (toggle debug mode as needed)
    let client = Client::new(rpc_api, rng, arc_store, Arc::new(authenticator), true);

    Ok(client)
}

pub fn get_new_pk_and_authenticator() -> (Word, AuthSecretKey) {
    // Create a deterministic RNG with zeroed seed
    let seed = [0_u8; 32];
    let mut rng = ChaCha20Rng::from_seed(seed);

    // Generate Falcon-512 secret key
    let sec_key = SecretKey::with_rng(&mut rng);

    // Convert public key to `Word` (4xFelt)
    let pub_key: Word = sec_key.public_key().into();

    // Wrap secret key in `AuthSecretKey`
    let auth_secret_key = AuthSecretKey::RpoFalcon512(sec_key);

    (pub_key, auth_secret_key)
}

/// Creates a library from the provided source code and library path.
///
/// # Arguments
/// * `assembler` - The assembler instance used to build the library.
/// * `library_path` - The full library path as a string (e.g., "custom_contract::mapping_example").
/// * `source_code` - The MASM source code for the module.
///
/// # Returns
/// A `miden_assembly::Library` that can be added to the transaction script.
fn create_library(
    assembler: Assembler,
    library_path: &str,
    source_code: &str,
) -> Result<miden_assembly::Library, Box<dyn std::error::Error>> {
    let source_manager = Arc::new(DefaultSourceManager::default());
    let module = Module::parser(ModuleKind::Library).parse_str(
        LibraryPath::new(library_path)?,
        source_code,
        &source_manager,
    )?;
    let library = assembler.clone().assemble_library([module])?;
    Ok(library)
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
    // STEP 1: Deploy a smart contract with a mapping
    // -------------------------------------------------------------------------
    println!("\n[STEP 1] Deploy smart contract");

    // Load the MASM file for the counter contract
    let file_path = Path::new("../masm/accounts/falcon_recover.masm");
    let account_code = fs::read_to_string(file_path).unwrap();

    // Prepare assembler (debug mode = true)
    let assembler: Assembler = TransactionKernel::assembler().with_debug_mode(true);

    // Using an empty storage value in slot 0 since this is usually resurved
    // for the account pub_key and metadata
    let empty_storage_slot = StorageSlot::empty_value();

    // initialize storage map
    let storage_map = StorageMap::new();
    let storage_slot_map = StorageSlot::Map(storage_map.clone());

    // Compile the account code into `AccountComponent` with one storage slot
    let mapping_contract_component = AccountComponent::compile(
        account_code.clone(),
        assembler.clone(),
        vec![empty_storage_slot, storage_slot_map],
    )
    .unwrap()
    .with_supports_all_types();

    // Init seed for the counter contract
    let init_seed = ChaCha20Rng::from_entropy().gen();

    // Anchor block of the account
    let anchor_block = client.get_latest_epoch_block().await.unwrap();

    // Build the new `Account` with the component
    let (mapping_example_contract, _seed) = AccountBuilder::new(init_seed)
        .anchor((&anchor_block).try_into().unwrap())
        .account_type(AccountType::RegularAccountImmutableCode)
        .storage_mode(AccountStorageMode::Public)
        .with_component(mapping_contract_component.clone())
        .build()
        .unwrap();

    let (_, auth_secret_key) = get_new_pk_and_authenticator();

    client
        .add_account(
            &mapping_example_contract.clone(),
            Some(_seed),
            &auth_secret_key,
            false,
        )
        .await
        .unwrap();

    // -------------------------------------------------------------------------
    // STEP 2: Call the Contract with a Script
    // -------------------------------------------------------------------------
    println!("\n[STEP 2] Call Contract With Script");

    let script_code =
        fs::read_to_string(Path::new("../masm/scripts/falcon_recover_script.masm")).unwrap();

    // Create the library from the account source code using the helper function.
    let account_component_lib = create_library(
        assembler.clone(),
        "miden_by_example::falcon_recover",
        &account_code,
    )
    .unwrap();

    let seed = [0_u8; 32];
    let mut rng = ChaCha20Rng::from_seed(seed);
    // let authenticator = BasicAuthenticator::<ChaCha20Rng>::new(&[get_new_pk_and_authenticator()]);
    let (pub_key, auth_secret_key) = get_new_pk_and_authenticator();

    let message = [Felt::new(0), Felt::new(0), Felt::new(0), Felt::new(0)];
    let signature = match auth_secret_key {
        AuthSecretKey::RpoFalcon512(ref secret_key) => {
            get_falcon_signature(secret_key, message, &mut rng)
        }
    }
    .unwrap();

    println!("sign length: {:?}", signature.len());
    println!("pubKey: {:?}", pub_key);

    let mut new_signature = Vec::with_capacity(4 + signature.clone().len());
    new_signature.extend_from_slice(&pub_key); // push the 4-element public key
    new_signature.extend(signature.clone()); // append the rest of the signature

    println!("old signature: {:?}", signature);
    println!("new signature: {:?}", new_signature);

    let inputs = vec![(
        [Felt::new(0), Felt::new(0), Felt::new(0), Felt::new(0)],
        new_signature,
    )];

    // Compile the transaction script with the library.
    let tx_script = TransactionScript::compile(
        script_code,
        inputs,
        assembler.with_library(&account_component_lib).unwrap(),
    )
    .unwrap();

    // Build a transaction request with the custom script
    let tx_increment_request = TransactionRequestBuilder::new()
        .with_custom_script(tx_script)
        .unwrap()
        .build();

    // Execute the transaction locally
    let tx_result = client
        .new_transaction(mapping_example_contract.id(), tx_increment_request)
        .await
        .unwrap();

    let tx_id = tx_result.executed_transaction().id();
    println!(
        "View transaction on MidenScan: https://testnet.midenscan.com/tx/{:?}",
        tx_id
    );

    // Submit transaction to the network
    let _ = client.submit_transaction(tx_result).await;

    client.sync_state().await.unwrap();

    let account = client
        .get_account(mapping_example_contract.id())
        .await
        .unwrap();
    let index = 1;
    let key = [Felt::new(0), Felt::new(0), Felt::new(0), Felt::new(0)];
    println!(
        "Mapping state\n Index: {:?}\n Key: {:?}\n Value: {:?}",
        index,
        key,
        account
            .unwrap()
            .account()
            .storage()
            .get_map_item(index, key)
    );

    Ok(())
}
