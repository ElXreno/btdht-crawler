use dotenvy::dotenv;
use log::info;
use rustydht_lib::dht;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4};
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use rustydht_lib::common::Id;
use rustydht_lib::dht::operations;
use warp::Filter;

#[tokio::main]
async fn main() {
    dotenv().ok();
    env_logger::init_from_env(env_logger::Env::new().default_filter_or("info"));

    let http_sockaddr: SocketAddr =
        SocketAddr::new(IpAddr::from_str("127.0.0.1").unwrap(), 8080);

    let builder = dht::DHTBuilder::new()
        .listen_addr(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 6881));

    let (mut shutdown_tx, shutdown_rx) =
        rustydht_lib::shutdown::create_shutdown();
    let dht = Arc::new(
        builder
            .build(shutdown_rx.clone())
            .expect("Failed to init DHT"),
    );

    let dht_clone = dht.clone();

    let info_hash = Id::from_hex("8a2bfa70ed6387f6767544dad2694c1b9afff2cf").unwrap();

    let http_server = {
        let handler = {
            let dht = dht.clone();
            warp::path::end().map(move || {
                let id = dht.get_id();
                let nodes = dht.get_nodes();
                let nodes_string = {
                    let mut to_ret = String::new();
                    for node in &nodes {
                        to_ret += &format!("{}\t{}\n", node.node.id, node.node.address);
                    }
                    to_ret
                };
                let info_hashes_string = {
                    let info_hashes = dht.get_info_hashes(None);
                    let mut to_ret = String::new();
                    for hash in info_hashes {
                        to_ret += &format!("{}\t{}\n", hash.0, hash.1.len());
                    }
                    to_ret
                };

                format!(
                    "Id: {id}\n{nodes_count} nodes\nInfo hashes:\n{info_hashes_string}",
                    id=id, nodes_count=nodes.len(), info_hashes_string=info_hashes_string
                )
            })
        };

        let mut shutdown_rx = shutdown_rx.clone();
        let (_addr, server) = warp::serve(handler).bind_with_graceful_shutdown(
            http_sockaddr,
            async move {
                shutdown_rx.watch().await;
            },
        );

        server
    };

    tokio::select! {
        _ = dht.run_event_loop() => {},
        _ = http_server => {},
        _ = tokio::signal::ctrl_c() => {
            info!("Ctrl+C detected - sending shutdown signal");
            drop(dht);
            drop(shutdown_rx);
            shutdown_tx.shutdown().await;
        },
        // _ = async move {
        //     let result = operations::get_peers(&dht_clone, info_hash, Duration::from_secs(60)).await.expect("get_peers hit an error");
        //     println!("Peers:\n{:?}", result.peers());
        // } => {},
    };
}
