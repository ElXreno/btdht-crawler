use log::info;
use rustydht_lib::common::Id;
use rustydht_lib::dht::DHT;
use rustydht_lib::packets::MessageBuilder;
use rustydht_lib::packets::MessageType::Response;
use rustydht_lib::packets::ResponseSpecific::SampleInfoHashesResponse;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::task;
use tokio::time::sleep;

pub struct Worker {
    dht: Arc<DHT>,
    infohashes: Arc<Mutex<HashSet<Id>>>,
}

pub enum WorkerError {
    UnknownError,
}

impl Worker {
    pub fn new(dht: Arc<DHT>) -> Self {
        Worker {
            dht,
            infohashes: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    pub async fn run_event_loop(&self) -> Result<(), WorkerError> {
        match tokio::try_join!(self.collect_infohashes()) {
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
}
