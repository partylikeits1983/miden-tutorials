import {
  WebClient,
  AccountStorageMode,
  AccountId,
  NoteType,
} from "@demox-labs/miden-sdk";

const nodeEndpoint = "https://rpc.testnet.miden.io:443";

export async function webClient(): Promise<void> {
  try {
    // 1. Create client
    const client = await WebClient.create_client(nodeEndpoint);

    // 2. Sync and log block
    const state = await client.sync_state();
    console.log("Latest block number:", state.block_num());

    // 3. Create Alice account (public, updatable)
    console.log("Creating account for Alice");
    const aliceAccount = await client.new_wallet(
      AccountStorageMode.public(),
      true,
    );
    const aliceIdHex = aliceAccount.id().to_string();
    console.log("Alice's account ID:", aliceIdHex);

    // 4. Create faucet
    console.log("Creating faucet...");
    const faucetAccount = await client.new_faucet(
      AccountStorageMode.public(),
      false,
      "MID",
      8,
      BigInt(1_000_000),
    );
    const faucetIdHex = faucetAccount.id().to_string();
    console.log("Faucet account ID:", faucetIdHex);

    // 5. Mint tokens to Alice
    await client.fetch_and_cache_account_auth_by_pub_key(
      AccountId.from_hex(faucetIdHex),
    );
    await client.sync_state();

    console.log("Minting tokens to Alice...");
    await client.new_mint_transaction(
      AccountId.from_hex(aliceIdHex),
      AccountId.from_hex(faucetIdHex),
      NoteType.public(),
      BigInt(1000),
    );

    console.log("Waiting 15 seconds for transaction confirmation...");
    await new Promise((resolve) => setTimeout(resolve, 15000));
    await client.sync_state();

    await client.fetch_and_cache_account_auth_by_pub_key(
      AccountId.from_hex(aliceIdHex),
    );

    // 6. Fetch minted notes
    const mintedNotes = await client.get_consumable_notes(
      AccountId.from_hex(aliceIdHex),
    );
    const mintedNoteIds = mintedNotes.map((n) =>
      n.input_note_record().id().to_string(),
    );
    console.log("Minted note IDs:", mintedNoteIds);

    // 7. Consume minted notes
    console.log("Consuming minted notes...");
    await client.new_consume_transaction(
      AccountId.from_hex(aliceIdHex),
      mintedNoteIds,
    );
    await client.sync_state();
    console.log("Notes consumed.");

    // 8. Send tokens to a dummy account
    const dummyIdHex = "0x599a54603f0cf9000000ed7a11e379";
    console.log("Sending tokens to dummy account...");
    await client.new_send_transaction(
      AccountId.from_hex(aliceIdHex),
      AccountId.from_hex(dummyIdHex),
      AccountId.from_hex(faucetIdHex),
      NoteType.public(),
      BigInt(100),
    );
    await client.sync_state();
    console.log("Tokens sent.");
  } catch (error) {
    console.error("Error:", error);
    throw error;
  }
}
