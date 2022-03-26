use crate::base::{Config, Package};

use bytes::Bytes;
use disk::Storage;
use flume::{Receiver, RecvError};
use log::{error, info};
use rumqttc::*;
use serde::Serialize;
use std::io;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use thiserror::Error;
use tokio::{select, time};

#[derive(Error, Debug)]
pub enum Error {
    #[error("Collector recv error {0}")]
    Collector(#[from] RecvError),
    #[error("Serde error {0}")]
    Serde(#[from] serde_json::Error),
    #[error("Io error {0}")]
    Io(#[from] io::Error),
    #[error("Mqtt client error {0}")]
    Client(#[from] ClientError),
    #[error("Storage is disabled/missing")]
    MissingPersistence,
}

enum Status {
    Normal,
    SlowEventloop(Publish),
    EventLoopReady,
    EventLoopCrash(Publish),
}

/// The uplink Serializer is the component that deals with sending data to the Bytebeam platform.
/// In case of network issues, the Serializer enters various states depending on severeness, managed by `Serializer::start()`.                                                                                       
///
/// ```text
///        ┌───────────────────┐
///        │Serializer::start()│
///        └─────────┬─────────┘
///                  │
///                  │ State transitions happen
///                  │ within the loop{}             Load data in Storage from
///                  │                               previouse sessions/iterations                  AsyncClient has crashed
///          ┌───────▼──────┐                       ┌─────────────────────┐                      ┌───────────────────────┐
///          │EventLoopReady├───────────────────────►Serializer::catchup()├──────────────────────►EventLoopCrash(publish)│
///          └───────▲──────┘                       └──────────┬──────────┘                      └───────────┬───────────┘
///                  │                                         │                                             │
///                  │                                         │ No more data left in Storage                │
///                  │                                         │                                             │
///     ┌────────────┴────────────┐                        ┌───▼──┐                             ┌────────────▼─────────────┐
///     │Serializer::disk(publish)│                        │Normal│                             │Serializer::crash(publish)├──┐
///     └────────────▲────────────┘                        └───┬──┘                             └─────────────────────────▲┘  │
///                  │                                         │                                 Write all data to Storage└───┘
///                  │                                         │
///                  │                                         │
///      ┌───────────┴──────────┐                   ┌──────────▼─────────┐
///      │SlowEventloop(publish)◄───────────────────┤Serializer::normal()│
///      └──────────────────────┘                   └────────────────────┘
///       Slow Network,                             Forward all data to Bytebeam,
///       save to Storage before forwarding         through AsyncClient
///
///```
pub struct Serializer {
    config: Arc<Config>,
    collector_rx: Receiver<Box<dyn Package>>,
    client: AsyncClient,
    storage: Option<Storage>,
    metrics: Metrics,
}

impl Serializer {
    pub fn new(
        config: Arc<Config>,
        collector_rx: Receiver<Box<dyn Package>>,
        client: AsyncClient,
    ) -> Result<Serializer, Error> {
        let metrics_config = config.streams.get("metrics").unwrap();
        let metrics = Metrics::new(&metrics_config.topic);

        let storage = match &config.persistence {
            Some(persistence) => {
                let storage = Storage::new(
                    &persistence.path,
                    persistence.max_file_size,
                    persistence.max_file_count,
                )?;
                Some(storage)
            }
            None => None,
        };

        Ok(Serializer { config, collector_rx, client, storage, metrics })
    }

    /// Write all data received, from here-on, to disk only.
    async fn crash(&mut self, mut publish: Publish) -> Result<Status, Error> {
        // Write failed publish to disk first
        publish.pkid = 1;

        loop {
            let data = self.collector_rx.recv_async().await?;
            // NOTE: Metrics will be updated unnecessarily
            write_to_storage(data, &mut self.metrics, &mut self.storage)?
        }
    }

    /// Write new data to disk until back pressure due to slow n/w is resolved
    async fn disk(&mut self, publish: Publish) -> Result<Status, Error> {
        info!("Switching to slow eventloop mode!!");

        // Note: send_publish() is executing code before await point
        // in publish method every time. Verify this behaviour later
        let client = self.client.clone();
        let publish = send_publish(client, publish.topic, publish.payload);
        tokio::pin!(publish);

        loop {
            select! {
                data = self.collector_rx.recv_async() => write_to_storage(data?, &mut self.metrics, &mut self.storage)?,
                o = &mut publish => {
                    o?;
                    return Ok(Status::EventLoopReady)
                }
            }
        }
    }

    /// Write new collector data to disk while sending existing data on
    /// disk to mqtt eventloop. Collector rx is selected with blocking
    /// `publish` instead of `try publish` to ensure that transient back
    /// pressure due to a lot of data on disk doesn't switch state to
    /// `Status::SlowEventLoop`
    async fn catchup(&mut self) -> Result<Status, Error> {
        info!("Switching to catchup mode!!");

        let client = self.client.clone();
        let max_packet_size = self.config.max_packet_size;

        let publish = match read_from_storage(&mut self.storage, max_packet_size)? {
            Some(p) => p,
            _ => return Ok(Status::Normal),
        };

        let send = send_publish(client, publish.topic, publish.payload);
        tokio::pin!(send);

        loop {
            select! {
                data = self.collector_rx.recv_async() => write_to_storage(data?, &mut self.metrics, &mut self.storage)?,
                o = &mut send => {
                    // Send failure implies eventloop crash. Switch state to
                    // indefinitely write to disk to not loose data
                    let client = match o {
                        Ok(c) => c,
                        Err(ClientError::Request(request)) => match request.into_inner() {
                            Request::Publish(publish) => return Ok(Status::EventLoopCrash(publish)),
                            request => unreachable!("{:?}", request),
                        },
                        Err(e) => return Err(e.into()),
                    };

                    // Load next publish to be sent from storage
                    let publish = match read_from_storage(&mut self.storage, max_packet_size)? {
                        Some(p) => p,
                        _ => return Ok(Status::Normal),
                    };

                    let payload = publish.payload;
                    let payload_size = payload.len();
                    self.metrics.sub_total_disk_size(payload_size);
                    self.metrics.add_total_sent_size(payload_size);
                    send.set(send_publish(client, publish.topic, payload));
                }
            }
        }
    }

    /// Normal mode of serializer operation, data received is directly
    /// sent on network with `try publish` to ensure that when  switch state to
    /// `Status::SlowEventLoop`
    async fn normal(&mut self) -> Result<Status, Error> {
        info!("Switching to normal mode!!");
        let mut interval = time::interval(time::Duration::from_secs(10));

        loop {
            let (payload_size, publish_result) = select! {
                data = self.collector_rx.recv_async() => {
                    let data = data?;
                    let (topic, payload_size, payload) = get_payload(data, &mut self.metrics);
                    let publish_result = self.client.try_publish(topic.as_ref(), QoS::AtLeastOnce, false, payload);
                    (payload_size, publish_result)
                }
                _ = interval.tick() => {
                    let (topic, payload) = self.metrics.next();
                    let payload_size = payload.len();
                    let publish_result = self.client.try_publish(topic, QoS::AtLeastOnce, false, payload);
                    (payload_size, publish_result)
                }
            };

            let failed = match publish_result {
                Ok(_) => {
                    self.metrics.add_total_sent_size(payload_size);
                    continue;
                }
                Err(ClientError::TryRequest(request)) => request,
                Err(e) => return Err(e.into()),
            };

            match failed.into_inner() {
                Request::Publish(publish) => return Ok(Status::SlowEventloop(publish)),
                request => unreachable!("{:?}", request),
            };
        }
    }

    /// Direct mode is used in case uplink is used with persistence disabled.
    /// It is operated differently from all other modes. Failure is terminal.
    async fn direct(&mut self) -> Result<(), Error> {
        let mut interval = time::interval(time::Duration::from_secs(10));

        loop {
            let payload_size = select! {
                data = self.collector_rx.recv_async() => {
                    let data = data?;
                    let (topic, payload_size, payload) = get_payload(data, &mut self.metrics);
                    self.client.publish(topic.as_ref(), QoS::AtLeastOnce, false, payload).await?;
                    payload_size
                }
                _ = interval.tick() => {
                    let (topic, payload) = self.metrics.next();
                    let payload_size = payload.len();
                    self.client.publish(topic, QoS::AtLeastOnce, false, payload).await?;
                    payload_size
                }
            };

            self.metrics.add_total_sent_size(payload_size);
        }
    }

    pub async fn start(&mut self) -> Result<(), Error> {
        if self.storage.is_none() {
            return self.direct().await;
        }

        let mut status = Status::EventLoopReady;

        loop {
            let next_status = match status {
                Status::Normal => self.normal().await?,
                Status::SlowEventloop(publish) => self.disk(publish).await?,
                Status::EventLoopReady => self.catchup().await?,
                Status::EventLoopCrash(publish) => self.crash(publish).await?,
            };

            status = next_status;
        }
    }
}

async fn send_publish(
    client: AsyncClient,
    topic: String,
    payload: Bytes,
) -> Result<AsyncClient, ClientError> {
    client.publish_bytes(topic, QoS::AtLeastOnce, false, payload).await?;
    Ok(client)
}

#[derive(Debug, Default, Serialize)]
struct Metrics {
    #[serde(skip_serializing)]
    topic: String,
    sequence: u32,
    timestamp: u64,
    total_sent_size: usize,
    total_disk_size: usize,
    lost_segments: usize,
    errors: String,
    error_count: usize,
}

impl Metrics {
    pub fn new<T: Into<String>>(topic: T) -> Metrics {
        Metrics { topic: topic.into(), errors: String::with_capacity(1024), ..Default::default() }
    }

    pub fn add_total_sent_size(&mut self, size: usize) {
        self.total_sent_size = self.total_sent_size.saturating_add(size);
    }

    pub fn add_total_disk_size(&mut self, size: usize) {
        self.total_disk_size = self.total_disk_size.saturating_add(size);
    }

    pub fn sub_total_disk_size(&mut self, size: usize) {
        self.total_disk_size = self.total_disk_size.saturating_sub(size);
    }

    pub fn increment_lost_segments(&mut self) {
        self.lost_segments += 1;
    }

    // pub fn add_error<S: Into<String>>(&mut self, error: S) {
    //     self.error_count += 1;
    //     if self.errors.len() > 1024 {
    //         return;
    //     }
    //
    //     self.errors.push_str(", ");
    //     self.errors.push_str(&error.into());
    // }

    pub fn add_errors<S: Into<String>>(&mut self, error: S, count: usize) {
        self.error_count += count;
        if self.errors.len() > 1024 {
            return;
        }

        self.errors.push_str(&error.into());
        self.errors.push_str(" | ");
    }

    pub fn next(&mut self) -> (&str, Vec<u8>) {
        let timestamp =
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or(Duration::from_secs(0));
        self.timestamp = timestamp.as_millis() as u64;
        self.sequence += 1;

        let payload = serde_json::to_vec(&vec![&self]).unwrap();
        self.errors.clear();
        self.lost_segments = 0;
        (&self.topic, payload)
    }
}

fn write_to_storage(
    data: Box<dyn Package>,
    metrics: &mut Metrics,
    storage: &mut Option<Storage>,
) -> Result<(), Error> {
    let storage = match storage {
        Some(s) => s,
        None => return Err(Error::MissingPersistence),
    };

    let (topic, payload_size, payload) = get_payload(data, metrics);
    let mut publish = Publish::new(topic.as_ref(), QoS::AtLeastOnce, payload);
    publish.pkid = 1;

    match publish.write(&mut storage.writer()) {
        Ok(_) => metrics.add_total_disk_size(payload_size),
        Err(e) => {
            error!("Failed to fill disk buffer. Error = {:?}", e);
            return Ok(());
        }
    }

    match storage.flush_on_overflow() {
        Ok(deleted) => {
            if deleted.is_some() {
                metrics.increment_lost_segments();
            }
        }
        Err(e) => {
            error!("Failed to flush write buffer to disk during catchup. Error = {:?}", e);
        }
    }

    Ok(())
}

// Load next publish to be sent from storage
fn read_from_storage(
    storage: &mut Option<Storage>,
    max_packet_size: usize,
) -> Result<Option<Publish>, Error> {
    let storage = match storage {
        Some(s) => s,
        None => return Err(Error::MissingPersistence),
    };

    // Done reading all the pending files
    match storage.reload_on_eof() {
        // Done reading all pending files
        Ok(true) => return Ok(None),
        Ok(false) => {}
        Err(e) => {
            error!("Failed to reload storage. Forcing into Normal mode. Error = {:?}", e);
            return Ok(None);
        }
    }

    match read(storage.reader(), max_packet_size) {
        Ok(Packet::Publish(publish)) => Ok(Some(publish)),
        Ok(packet) => unreachable!("{:?}", packet),
        Err(e) => {
            error!("Failed to read from storage. Forcing into Normal mode. Error = {:?}", e);
            return Ok(None);
        }
    }
}

type Data = (Arc<String>, usize, Vec<u8>);

// Extract data from package and update metrics
fn get_payload(data: Box<dyn Package>, metrics: &mut Metrics) -> Data {
    // Extract anomalies detected by package during collection
    if let Some((errors, count)) = data.anomalies() {
        metrics.add_errors(errors, count);
    }

    let payload = data.serialize();
    (data.topic(), payload.len(), payload)
}
