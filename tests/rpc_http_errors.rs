//! Regression coverage for Bitcoin Core JSON-RPC errors returned with HTTP 500.
mod common;

use bitcoin::{Network, Txid};
use bitcoin_indexer::Indexer;
use bitvmx_bitcoin_rpc::{bitcoin_client::BitcoinClient, rpc_config::RpcConfig};
use common::TestStorage;
use serde_json::{json, Value};
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::TcpListener,
    thread::{self, JoinHandle},
};

const UNKNOWN_TXID: &str = "0257d69ee3f36d47b98aabda42cca1501da5e86af48f16f759fd458bbc011310";

fn unknown_transaction_server() -> (String, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut content_length = None;

        loop {
            let mut line = String::new();
            assert_ne!(reader.read_line(&mut line).unwrap(), 0);
            if line == "\r\n" {
                break;
            }
            if let Some((name, value)) = line.split_once(':') {
                if name.eq_ignore_ascii_case("content-length") {
                    content_length = Some(value.trim().parse::<usize>().unwrap());
                }
            }
        }

        let mut body = vec![0; content_length.expect("missing Content-Length")];
        reader.read_exact(&mut body).unwrap();
        let request: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(request["method"], "getrawtransaction");

        let body = json!({
            "result": null,
            "error": {
                "code": -5,
                "message": "No such mempool or blockchain transaction"
            },
            "id": request["id"]
        })
        .to_string();

        write!(
            stream,
            "HTTP/1.1 500 Internal Server Error\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .unwrap();
    });

    (url, server)
}

#[test]
fn http_500_rpc_not_found_is_none_through_indexer_api() {
    let (url, server) = unknown_transaction_server();
    let config = RpcConfig::new(
        Network::Regtest,
        url,
        String::new(),
        String::new(),
        String::new(),
    );
    let storage = TestStorage::new();
    let client = BitcoinClient::new_from_config(&config).unwrap();
    let indexer = Indexer::new(client, storage.storage(), None).unwrap();
    let txid = UNKNOWN_TXID.parse::<Txid>().unwrap();

    let result = indexer.rpc_get_tx_confirmations(&txid);
    server.join().expect("mock Bitcoin RPC server failed");

    assert_eq!(
        result.expect("JSON-RPC -5 must be treated as transaction not found"),
        None
    );
}
