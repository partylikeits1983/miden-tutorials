use miden_client::{
    accounts::{AccountId, AccountStorageMode, AccountTemplate, AccountType},
    assets::{FungibleAsset, TokenSymbol},
    config::RpcConfig,
    crypto::RpoRandomCoin,
    notes::NoteType,
    rpc::TonicRpcClient,
    store::{
        sqlite_store::{config::SqliteStoreConfig, SqliteStore},
        StoreAuthenticator,
    },
    transactions::{
        LocalTransactionProver, OutputNote, PaymentTransactionData, ProvingOptions,
        TransactionRequest,
    },
    Client, ClientError, Felt,
};
use miden_lib::notes::create_p2id_note;
use rand::Rng;
use std::sync::Arc;
use tokio::time::Duration;

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

#[tokio::main]
async fn main() -> Result<(), ClientError> {
    let mut client = initialize_client().await?;
    println!("Client initialized successfully.");

    //------------------------------------------------------------
    // STEP 1: Create a basic wallet account for Alice
    //------------------------------------------------------------
    println!("\n[STEP 1] Creating new account for Alice");

    // Create a new account for Alice
    println!("Creating a new BasicWallet account for Alice...");
    let alice_template = AccountTemplate::BasicWallet {
        mutable_code: true,
        storage_mode: AccountStorageMode::Public,
    };
    let (alice_account, _alice_seed) = client.new_account(alice_template).await?;
    println!("Alice's account ID: {:?}", alice_account.id());

    //------------------------------------------------------------
    // STEP 2: Deploy a fungible faucet (token)
    //------------------------------------------------------------
    println!("\n[STEP 2] Deploying a new fungible faucet.");

    // Deploy a fungible faucet
    println!("Deploying a new fungible faucet...");
    let faucet_template = AccountTemplate::FungibleFaucet {
        token_symbol: TokenSymbol::new("MID").unwrap(),
        decimals: 8,
        max_supply: 1_000_000,
        storage_mode: AccountStorageMode::Public,
    };
    let (faucet_account, _faucet_seed) = client.new_account(faucet_template).await?;
    println!("Faucet account ID: {:?}", faucet_account.id());

    // Sync state to see newly deployed faucet
    client.sync_state().await?;
    tokio::time::sleep(Duration::from_secs(2)).await;

    //------------------------------------------------------------
    // STEP 3: Mint 5 notes of 100 tokens each for Alice
    //------------------------------------------------------------
    println!("\n[STEP 3] Minting 5 notes of 100 tokens each for Alice.");

    // Mint 5 notes of 100 tokens each for Alice
    println!("Minting 5 notes of 100 tokens each for Alice...");
    for i in 1..=5 {
        let amount: u64 = 100;
        let fungible_asset = FungibleAsset::new(faucet_account.id(), amount).unwrap();

        let transaction_request = TransactionRequest::mint_fungible_asset(
            fungible_asset.clone(),
            alice_account.id(),
            NoteType::Public,
            client.rng(),
        )?;
        let tx_execution_result = client
            .new_transaction(faucet_account.id(), transaction_request)
            .await?;

        client.submit_transaction(tx_execution_result).await?;
        println!("Minted note #{} of {} tokens for Alice.", i, amount);
    }
    println!("All 5 notes minted for Alice successfully!");

    // Re-sync so minted notes become visible in the client
    client.sync_state().await?;

    //------------------------------------------------------------
    // STEP 4: Alice consumes all her notes
    //------------------------------------------------------------
    println!("\n[STEP 4] Alice will now consume all of her notes to consolidate them.");

    // Consume all of Alice's minted notes in a single transaction
    println!("Checking for Alice's consumable notes to consume them...");
    loop {
        client.sync_state().await?;
        let consumable_notes = client
            .get_consumable_notes(Some(alice_account.id()))
            .await?;
        let list_of_note_ids: Vec<_> = consumable_notes.iter().map(|(note, _)| note.id()).collect();

        if list_of_note_ids.len() == 5 {
            println!("Found 5 consumable notes for Alice. Consuming them now...");
            let transaction_request = TransactionRequest::consume_notes(list_of_note_ids);
            let tx_execution_result = client
                .new_transaction(alice_account.id(), transaction_request)
                .await?;

            client.submit_transaction(tx_execution_result).await?;
            println!("All of Alice's notes consumed successfully.");
            break;
        } else {
            println!(
                "Currently, Alice has {} consumable notes. Waiting for 5...",
                list_of_note_ids.len()
            );
            tokio::time::sleep(Duration::from_secs(3)).await;
        }
    }

    //------------------------------------------------------------
    // STEP 5: Using Alice's wallet, send 5 notes of 50 tokens each to list of users
    //------------------------------------------------------------
    println!("\n[STEP 5] Alice sends 5 notes of 50 tokens each to 5 different users.");

    // Send 50 tokens to 5 different accounts in a single transaction
    println!("Creating multiple P2ID notes for 5 target accounts in one transaction...");
    let mut p2id_notes = vec![];
    for _ in 1..=4 {
        // Generate a unique random seed
        let init_seed = {
            let mut seed = [0u8; 32];
            rand::thread_rng().fill(&mut seed);
            seed[0] = 99u8;
            seed
        };
        let target_account_id =
            AccountId::new_dummy(init_seed, AccountType::RegularAccountUpdatableCode);
        let send_amount = 50;
        let fungible_asset = FungibleAsset::new(faucet_account.id(), send_amount).unwrap();

        let p2id_note = create_p2id_note(
            alice_account.id(),
            target_account_id,
            vec![fungible_asset.into()],
            NoteType::Public,
            Felt::new(0),
            client.rng(),
        )?;
        p2id_notes.push(p2id_note);
    }
    let output_notes: Vec<OutputNote> = p2id_notes.into_iter().map(OutputNote::Full).collect();

    let transaction_request = TransactionRequest::new().with_own_output_notes(output_notes)?;

    let tx_execution_result = client
        .new_transaction(alice_account.id(), transaction_request)
        .await?;

    client.submit_transaction(tx_execution_result).await?;
    println!("Submitted a transaction with 4 P2ID notes.");

    // Send 50 tokens to 1 more account as a single P2ID transaction
    println!("Submitting one more single P2ID transaction...");
    let init_seed = {
        let mut seed = [0u8; 32];
        rand::thread_rng().fill(&mut seed);
        seed[0] = 99u8;
        seed
    };
    let target_account_id =
        AccountId::new_dummy(init_seed, AccountType::RegularAccountUpdatableCode);
    let send_amount = 50;
    let fungible_asset = FungibleAsset::new(faucet_account.id(), send_amount).unwrap();
    let payment_transaction = PaymentTransactionData::new(
        vec![fungible_asset.into()],
        alice_account.id(),
        target_account_id,
    );
    let transaction_request = TransactionRequest::pay_to_id(
        payment_transaction,
        None,             // recall_height
        NoteType::Public, // note type
        client.rng(),     // rng
    )?;
    let tx_execution_result = client
        .new_transaction(alice_account.id(), transaction_request)
        .await?;

    client.submit_transaction(tx_execution_result).await?;

    println!("\nAll steps completed successfully!");
    println!("Alice created a wallet, a faucet was deployed,");
    println!("5 notes of 100 tokens were minted to Alice, those notes were consumed,");
    println!("and then Alice sent 5 separate 50-token notes to 5 different users.");

    Ok(())
}
