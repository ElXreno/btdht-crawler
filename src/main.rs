use dotenvy::dotenv;
use log::info;
use rustydht_lib::common::Id;
use rustydht_lib::dht;
use rustydht_lib::dht::DHTSettingsBuilder;
use rustydht_lib::packets::{MessageBuilder, MessageType, ResponseSpecific};
use std::collections::HashSet;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4};
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::task;
use tokio::time::sleep;
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

    let info_hashes: Arc<Mutex<HashSet<Id>>> = Arc::new(Mutex::new(HashSet::new()));
    let dht_clone = dht.clone();
    let worker = async move {
        loop {
            sleep(Duration::from_secs(60)).await;

            let nodes = dht_clone.get_nodes();

            info!("Node count: {}", nodes.len());

            let mut jobs = vec![];

            for node in nodes {
                let dht_clone = dht_clone.clone();
                let job = task::spawn(async move {
                    info!(
                        "Asking node {} with id {} for infohashes...",
                        node.node.address, node.node.id
                    );
                    let message = MessageBuilder::new_sample_infohashes_request()
                        .sender_id(dht_clone.get_id())
                        .target(node.node.id)
                        .build()
                        .unwrap();

                    let result = dht_clone
                        .send_request(
                            message,
                            node.node.address,
                            Some(node.node.id),
                            Some(Duration::from_secs(30)),
                        )
                        .await;

                    info!(
                        "Got response from node {} with id {}: {:?}",
                        node.node.address, node.node.id, result
                    );

                    match result {
                        Ok(res) => match res.message_type {
                            MessageType::Response(res) => match res {
                                ResponseSpecific::SampleInfoHashesResponse(info_hashes_res) => {
                                    info_hashes_res.samples
                                }
                                _ => vec![],
                            },
                            _ => vec![],
                        },
                        _ => vec![],
                    }
                });
                jobs.push(job);
            }

            let mut info_hashes = info_hashes.lock().await;
            for job in jobs {
                job.await.unwrap().iter().for_each(|id| {
                    info_hashes.insert(*id);
                });
            }

            info!("Total infohashes: {}", info_hashes.len());
        }
    };

    tokio::select! {
        _ = dht.run_event_loop() => {},
        _ = http_server => {},
        _ = worker => {},
        _ = tokio::signal::ctrl_c() => {
            info!("Ctrl+C detected - sending shutdown signal");
            drop(dht);
            drop(shutdown_rx);
            shutdown_tx.shutdown().await;
        },
    }
}
