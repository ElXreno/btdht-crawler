use entity::prelude::Torrent;
use entity::torrent;
use log::info;
use migration::{Migrator, MigratorTrait};
use rustydht_lib::common::Id;
use rustydht_lib::dht::DHT;
use rustydht_lib::packets::MessageBuilder;
use rustydht_lib::packets::MessageType::Response;
use rustydht_lib::packets::ResponseSpecific::SampleInfoHashesResponse;
use sea_orm::{sea_query, ActiveValue, DatabaseConnection, EntityTrait};
use std::collections::HashSet;
use std::env;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::task;
use tokio::time::sleep;

// Env var names
static DATABASE_URL: &str = "DATABASE_URL";

// Env defaults
static DATABASE_URL_DEFAULT: &str = "sqlite://btdht-crawler.db?mode=rwc";

pub struct Worker {
    dht: Arc<DHT>,
    infohashes: Arc<Mutex<HashSet<Id>>>,
    db: DatabaseConnection,
}

pub enum WorkerError {
    UnknownError,
}

impl Worker {
    pub async fn new(dht: Arc<DHT>) -> Self {
        let database_url = env::var(DATABASE_URL).unwrap_or(DATABASE_URL_DEFAULT.to_string());

        let connection = sea_orm::Database::connect(database_url)
            .await
            .expect("Failed to initialize database connection!");
        Migrator::up(&connection, None).await.unwrap();

        Worker {
            dht,
            infohashes: Arc::new(Mutex::new(HashSet::new())),
            db: connection,
        }
    }

    pub async fn run_event_loop(&self) -> Result<(), WorkerError> {
        match tokio::try_join!(self.collect_infohashes(), self.periodic_save_infohashes()) {
            _ => Ok(()), // placeholder
        }
    }

    pub async fn collect_infohashes(&self) -> Result<(), WorkerError> {
        loop {
            match async {
                sleep(Duration::from_secs(60)).await;

                let nodes = self.dht.get_nodes();

                info!("Node count: {}", nodes.len());

                let mut jobs = vec![];

                for node in nodes {
                    let dht_clone = self.dht.clone();
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

                        let mut infohashes = vec![];
                        if let Ok(result) = result {
                            if let Response(response) = result.message_type {
                                if let SampleInfoHashesResponse(infohashes_response) = response {
                                    infohashes = infohashes_response.samples;
                                }
                            }
                        }

                        infohashes
                    });
                    jobs.push(job);
                }

                for job in jobs {
                    let infohashes = job.await.unwrap();
                    for infohash in infohashes {
                        self.infohashes.lock().await.insert(infohash);
                    }
                }

                info!("Total infohashes: {}", self.infohashes.lock().await.len());
                Ok::<(), WorkerError>(())
            }
            .await
            {
                Ok(_) => continue,
                _ => continue,
            }
        }
    }

    pub async fn periodic_save_infohashes(&self) -> Result<(), WorkerError> {
        loop {
            sleep(Duration::from_secs(60)).await;

            let info_hashes = self.infohashes.lock().await;
            info!("Saving {} hashes...", info_hashes.len());
            let mut torrent_models = vec![];
            for info_hash in info_hashes.iter() {
                let torrent = torrent::ActiveModel {
                    info_hash: ActiveValue::set(info_hash.to_string()),
                };

                torrent_models.push(torrent);
            }

            Torrent::insert_many(torrent_models)
                .on_conflict(
                    sea_query::OnConflict::column(torrent::Column::InfoHash)
                        .do_nothing()
                        .to_owned(),
                )
                .on_empty_do_nothing()
                .exec(&self.db)
                .await
                .unwrap();

            info!("Save done!");
        }
    }
}
