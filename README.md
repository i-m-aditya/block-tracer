# Block Tracer

This Rust project implements a block tracer to detect reinitialized Ethereum contracts using reth's Trace API and database. It traces Ethereum blocks to find contracts that have been self-destructed and later reinitialized to the same address. The detected reinitialized addresses are stored in a JSON file.

## Requirements

- Rust and Cargo installed
- Reth node running with required APIs
- Environment variables configured for the database and RPC URLs

## Assumptions

- The reth node is fully synchronized and running with the required APIs enabled.
- The environment variables `RPC_URL`, `DB_PATH`, and `STATIC_FILES_PATH` are correctly set.

## Usage

### Prerequisites

1. Ensure you have Rust and Cargo installed.
2. Ensure you have a running reth node with the required APIs enabled. You can start the node with the following command:

   ```bash
   RUST_LOG=info reth node \
   --datadir /mnt/mydata/reth-data-dir \
   --authrpc.addr 127.0.0.1 \
   --authrpc.jwtsecret /mnt/mydata/secrets/jwt.hex \
   --authrpc.port 8551 \
   --metrics 127.0.0.1:9001 \
   --http \
   --http.api "eth,web3,trace,rpc,debug"
   
## Running the project
1. Clone the repository
2. Set up the environment variables in a .env file:
   ```bash
   RPC_URL=
   DB_PATH=
   STATIC_FILES_PATH=
3. Run the project by providing start_block_number and end_block_number
   ```bash
   cargo run <start_block_number> <end_block_number>

## Output
The output will be saved in a file named reinitialized_contracts.json in the current directory, containing the list of reinitialized contract addresses.

## Code Explaination
The code is divided into several parts:
1. **RPC Response Structs and Trace Block Function**: This part defines the structure of the RPC response and the trace_block function that traces a block for self-destruct and create actions.
2. This is the entry point of the program. It does the following:
  - Spawns multiple asynchronous tasks to trace blocks in parallel using tokio::spawn.
  - Collects self-destructed and created addresses from the traced blocks.
  - Processes these addresses to detect reinitialized contracts.
  - Also query, PlainAccountState table to get the contract status if the reinitialization doesn't occur in start end range
  - Writes the detected reinitialized contracts to a JSON file.

## Performance Optimization
The following optimizations are implemented for better performance:
- Parallel processing using tokio::spawn to trace multiple blocks concurrently.
- Semaphore to limit the number of concurrent open file descriptors.
- Chunking the self-destructed addresses for parallel processing during database access.

## Assumtions and Future improvements
- The program expects a start block and end block for tracing
- The program is capable to give contract status if the contract is currently alive and is reinitialized later than end block.
- Parallel processing on db access have a little effect on performance because of small size of self destructed accounts
- Tracing blocks via rpc accounts for 90% of the program time.
     




   

   
