#![allow(dead_code)]
use std::fmt::Display;
use std::str::FromStr;
use std::sync::Mutex;
use std::{sync::Arc, time::Instant};
use std::{env, path::Path};

use alloy_primitives::Address;
use clap::Parser;
use futures::future::join_all;
use provider::get_reth_factory;
use reqwest;
use reth_db::tables;
use reth_rpc_types::trace::parity::*;
use serde_json::json;

use reth_db_api::{cursor::DbCursorRO, transaction::DbTx};
use tokio::runtime::Builder;
use tracing_subscriber::EnvFilter;

use rayon::prelude::*;
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct RpcResponse {
    result: Option<Vec<LocalizedTransactionTrace>>,
    jsonrpc: String,
    id: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum TraceType {
    SelfDestruct,
    Create,
}

impl Display for TraceType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TraceType::SelfDestruct => write!(f, "selfdestruct"),
            TraceType::Create => write!(f, "create"),
        }
    }
}

impl FromStr for TraceType {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "selfdestruct" => Ok(TraceType::SelfDestruct),
            "create" => Ok(TraceType::Create),
            _ => panic!("Trace type invalid"),
        }
    }
}

#[derive(Debug, Clone)]
struct TraceResponse {
    trace_type: TraceType,
    contract_address: Address,
    block_number: u64,
    transaction_position: u64,
}
#[derive(Parser, Debug)]
pub struct Cmd {
    #[arg(short, long)]
    pub start_block: u64,
    #[arg(short, long)]
    pub end_block: u64,
}

mod provider;

async fn trace_block(block_num: u64) -> anyhow::Result<Option<Vec<TraceResponse>>> {
    let client = reqwest::Client::new();
    let reth_url = env::var("RPC_URL").unwrap();
    let block_num_hex = format!("0x{:x}", block_num);
    let payload = json!({
        "jsonrpc": "2.0",
        "method": "trace_block",
        "params": [block_num_hex],
        "id": 1
    });
    // Used to remove traces of invalid transactions
    let mut invalid_tx = Vec::new();
    // println!("Block_number {}", block_num);
    let result = client
        .post(reth_url)
        .json(&payload)
        .send()
        .await?
        .json::<RpcResponse>()
        .await?
        .result;

    let address_block_tuple = if let Some(localized_tx_traces) = result {
        localized_tx_traces
            .into_iter()
            .map(|tx_trace| {
                let trace = tx_trace.trace;
                match (trace.action, trace.result, trace.error) {
                    (_, _, Some(_)) => {
                        invalid_tx.push(tx_trace.transaction_hash.unwrap());
                        None
                    }

                    (
                        Action::Selfdestruct(SelfdestructAction {
                            address: destruced_contract,
                            ..
                        }),
                        _,
                        None,
                    ) => {
                        println!("Selfdestruct: {} ", destruced_contract);
                        if invalid_tx.contains(&tx_trace.transaction_hash.unwrap()) {
                            return None;
                        }
                        Some(TraceResponse {
                            trace_type: TraceType::SelfDestruct,
                            contract_address: destruced_contract,
                            block_number: block_num,
                            transaction_position: tx_trace.transaction_position.unwrap(),
                        })
                    }
                    (
                        Action::Create(CreateAction { .. }),
                        Some(TraceOutput::Create(CreateOutput {
                            address: created_contract,
                            ..
                        })),
                        None,
                    ) => {
                        if invalid_tx.contains(&tx_trace.transaction_hash.unwrap()) {
                            return None;
                        }
                        Some(TraceResponse {
                            trace_type: TraceType::Create,
                            contract_address: created_contract,
                            block_number: block_num,
                            transaction_position: tx_trace.transaction_position.unwrap(),
                        })
                    }
                    _ => None,
                }
            })
            .filter_map(|item| item)
            .collect::<Vec<TraceResponse>>()
    } else {
        vec![]
    };

    if address_block_tuple.len() > 0 {
        Ok(Some(address_block_tuple))
    } else {
        Ok(None)
    }
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();
    Builder::new_multi_thread()
        .max_blocking_threads(num_cpus::get())
        .enable_all()
        .build()
        .unwrap()
        .block_on(amain())
        .unwrap();
}

async fn amain() -> anyhow::Result<()> {
    let start = Instant::now();
    dotenv::dotenv().ok();

    let Cmd {
        start_block,
        end_block,
    } = Cmd::parse();

    let mut reinitialized_contracts = Vec::new();

    let handles: Vec<_> = (start_block..=end_block)
        .into_iter()
        .map(|block| tokio::spawn(async move { trace_block(block).await.unwrap() }))
        .collect();
    let results: Vec<std::result::Result<Option<Vec<TraceResponse>>, tokio::task::JoinError>> =
        join_all(handles).await;
    let combined_trace_responses = results
        .into_iter()
        .filter_map(|item| item.ok())
        .flat_map(|item| item.unwrap_or_default())
        .collect::<Vec<TraceResponse>>();

    let (self_destructed_trace_responses, created_trace_responses): (Vec<_>, Vec<_>) =
        combined_trace_responses
            .into_iter()
            .partition(|trace_block_response| {
                trace_block_response.trace_type == TraceType::SelfDestruct
            });

    // Find reinitialized contracts in range [start_block_num, end_block_num], which is necessary if the plain state of contract is not available
    for self_destructed_trace_response in &self_destructed_trace_responses {
        let sda = self_destructed_trace_response.contract_address; // self destructed address
        let sda_block_num = self_destructed_trace_response.block_number;
        let sda_transaction_position = self_destructed_trace_response.transaction_position;

        for created_trace_response in &created_trace_responses {
            let ca = created_trace_response.contract_address;
            let ca_block_num = created_trace_response.block_number;
            let ca_transaction_position = created_trace_response.transaction_position;

            if sda_block_num == ca_block_num
                && sda_transaction_position < ca_transaction_position
                && sda == ca
            {
                println!("Address {} has been recreated", sda);
                reinitialized_contracts.push(sda);
            } else if sda_block_num < ca_block_num && sda == ca {
                println!("Address {} has been recreated", sda);
                reinitialized_contracts.push(sda);
            }
        }
    }

    let db_files = env::var("DB_PATH").unwrap();
    let static_files = env::var("STATIC_FILES_PATH").unwrap();

    let db_path = Path::new(&db_files);
    let static_files_path = Path::new(&static_files);
    let factory = get_reth_factory(db_path, static_files_path)?;
    let provider = factory.provider()?;

    let tx = Arc::new(provider.into_tx());

    let duration = start.elapsed();
    println!(
        "Time elapsed in finding self destruct addresss is: {:?}",
        duration
    );

    let recreated_contracts = Arc::new(Mutex::new(Vec::new()));

    self_destructed_trace_responses
        .par_chunks(10)
        .for_each(|chunk| {
            let recreated_contracts_clone = recreated_contracts.clone();
            for trace_block_response in chunk {
                let sda = trace_block_response.contract_address;
                let mut plain_account_cursor =
                    tx.cursor_read::<tables::PlainAccountState>().unwrap();

                let plain_account = plain_account_cursor.seek_exact(sda).unwrap();

                if plain_account.is_some() {
                    println!("Address {} has been recreated", sda);
                    recreated_contracts_clone.lock().unwrap().push(sda);
                }
            }
        });

    reinitialized_contracts.extend(
        Arc::try_unwrap(recreated_contracts)
            .unwrap()
            .into_inner()
            .unwrap()
            .into_iter(),
    );

    reinitialized_contracts.sort();
    reinitialized_contracts.dedup();

    let reinitialized_contracts_json = serde_json::to_string(&reinitialized_contracts)?;
    let reinitialized_contracts_file = Path::new("reinitialized_contracts.json");
    std::fs::write(reinitialized_contracts_file, reinitialized_contracts_json)?;

    let duration = start.elapsed();
    println!("Time elapsed in total is: {:?}", duration);

    Ok(())
}
