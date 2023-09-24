mod worker;

use dotenvy::dotenv;
use log::info;
use rustydht_lib::dht;
use rustydht_lib::dht::DHTSettingsBuilder;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4};
use std::str::FromStr;
use std::sync::Arc;
use warp::Filter;

#[tokio::main]
async fn main() {
    dotenv().ok();
    env_logger::init_from_env(env_logger::Env::new().default_filter_or("info"));

    let http_sockaddr: SocketAddr = SocketAddr::new(IpAddr::from_str("127.0.0.1").unwrap(), 8080);

    let dht_settings = DHTSettingsBuilder::new()
        .find_nodes_skip_count(256)
        .read_only(true)
        .max_sample_response(200)
        .reverify_interval_secs(120)
        .reverify_grace_period_secs(150)
        .build();

    let builder = dht::DHTBuilder::new()
        .listen_addr(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 6881))
        .settings(dht_settings);

    let (mut shutdown_tx, shutdown_rx) = rustydht_lib::shutdown::create_shutdown();
    let dht = Arc::new(
        builder
            .build(shutdown_rx.clone())
            .expect("Failed to init DHT"),
    );

    let http_server = {
        let handler = {
            let dht = dht.clone();
            warp::path::end().map(move || {
                let id = dht.get_id();
                let nodes = dht.get_nodes();

                format!(
                    "Id: {id}\n{nodes_count} nodes\n",
                    id = id,
                    nodes_count = nodes.len()
                )
            })
        };

        let mut shutdown_rx = shutdown_rx.clone();
        let (_addr, server) =
            warp::serve(handler).bind_with_graceful_shutdown(http_sockaddr, async move {
                shutdown_rx.watch().await;
            });

        server
    };

    let dht_clone = dht.clone();
    let worker = worker::Worker::new(dht_clone).await;

    tokio::select! {
        _ = dht.run_event_loop() => {},
        _ = worker.run_event_loop() => {},
        _ = http_server => {},
        _ = tokio::signal::ctrl_c() => {
            info!("Ctrl+C detected - sending shutdown signal");
            drop(worker);
            drop(dht);
            drop(shutdown_rx);
            shutdown_tx.shutdown().await;
        },
    }
}
