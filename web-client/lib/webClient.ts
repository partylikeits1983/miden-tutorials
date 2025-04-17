// lib/webClient.ts
export async function webClient(): Promise<void> {
  if (typeof window === "undefined") {
    console.warn("webClient() can only run in the browser");
    return;
  }

  // dynamic import → only in the browser, so WASM is loaded client‑side
  const { WebClient, AccountStorageMode, AccountId, NoteType } = await import(
    "@demox-labs/miden-sdk"
  );

  const nodeEndpoint = "https://rpc.testnet.miden.io:443";
  const client = await WebClient.createClient(nodeEndpoint);

  // 1. Sync and log block
  const state = await client.syncState();
  console.log("Latest block number:", state.blockNum());

  // 2. Create Alice’s account
  console.log("Creating account for Alice…");
  const alice = await client.newWallet(AccountStorageMode.public(), true);
  console.log("Alice ID:", alice.id().toString());

  // 3. Deploy faucet
  console.log("Creating faucet…");
  const faucet = await client.newFaucet(
    AccountStorageMode.public(),
    false,
    "MID",
    8,
    BigInt(1_000_000),
  );
  console.log("Faucet ID:", faucet.id().toString());

  await client.syncState();

  // 4. Mint tokens to Alice
  await client.fetchAndCacheAccountAuthByAccountId(faucet.id());
  await client.syncState();

  console.log("Minting tokens to Alice...");
  let mintTxRequest = client.newMintTransactionRequest(
    alice.id(),
    faucet.id(),
    NoteType.Public,
    BigInt(1000),
  );

  let txResult = await client.newTransaction(faucet.id(), mintTxRequest);

  await client.submitTransaction(txResult);

  let tx_script = client.compileTxScript("begin push.1 drop end");

  console.log(tx_script);
/*   
  let counter_contract = await client.importAccountById("0xb584c3769ab90b000004c780363668");

  let storage = counter_contract.storage().getItem(0);

  console.log(storage);
 */
}
