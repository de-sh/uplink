use std::collections::hash_map::Entry;
use std::{collections::HashMap, hash::Hash, sync::Arc, time::Duration};

use flume::Sender;
use log::error;
use tokio_stream::StreamExt;
use tokio_util::time::{delay_queue::Key, DelayQueue};

use crate::{Config, Package, Payload, Stream};

/// A map to store and retrieve delays from a DelayQueue.
pub struct DelayMap<T> {
    queue: DelayQueue<T>,
    map: HashMap<T, Key>,
}

impl<T: Eq + Hash + Clone> DelayMap<T> {
    pub fn new() -> Self {
        Self { queue: DelayQueue::new(), map: HashMap::new() }
    }

    // Removes timeout if it exists, else do nothing.
    pub fn remove(&mut self, item: &T) {
        if let Some(key) = self.map.remove(item) {
            self.queue.remove(&key);
        }
    }

    // Resets timeout if it exists, else do nothing.
    pub fn reset(&mut self, item: &T, period: Duration) {
        if let Some(key) = self.map.remove(item) {
            self.queue.reset(&key, period);
        }
    }

    // Check if map contains key for stream timeout.
    pub fn contains(&self, item: &T) -> bool {
        self.map.contains_key(item)
    }

    // Insert new timeout.
    pub fn insert(&mut self, item: T, period: Duration) {
        let key = self.queue.insert(item.clone(), period);
        self.map.insert(item, key);
    }

    // Remove a key from map if it has timedout.
    pub async fn next(&mut self) -> Option<T> {
        if let Some(item) = self.queue.next().await {
            self.map.remove(item.get_ref());
            return Some(item.into_inner());
        }

        None
    }

    // Check if queue is empty.
    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }
}

/// An internal structure to manage sending data on and flushing [Streams], with the help of a [DelayQueue]
pub struct StreamHandler {
    config: Arc<Config>,
    data_tx: Sender<Box<dyn Package>>,
    flush_handler: DelayMap<String>,
    streams: HashMap<String, Stream<Payload>>,
    period: Duration,
}

impl StreamHandler {
    pub fn new(config: Arc<Config>, data_tx: Sender<Box<dyn Package>>) -> Self {
        let period = Duration::from_secs(config.flush_period.unwrap_or(10));

        let mut streams = HashMap::new();
        for (stream, config) in config.streams.clone() {
            streams.insert(
                stream.clone(),
                Stream::new(stream, config.topic, config.buf_size, data_tx.clone()),
            );
        }

        Self { flush_handler: DelayMap::new(), config, streams, period, data_tx }
    }

    /// Send data to proper stream and manage timeouts.
    /// If a stream gets filled with data, ensure one of following 3 states:
    /// 1. Stream is filled and flushed: remove any timeouts associated with Stream from the queue.
    /// 2. Stream is filled but not flushed and timeout already exists in queue: reset timeout.
    /// 3. Stream is filled but not flushed and timeout doesn't exist in queue: add new timeout.
    pub async fn handle_data(&mut self, data: Payload) {
        // select stream to send data onto
        let stream = match self.streams.get_mut(&data.stream) {
            Some(s) => s,
            None => {
                if self.streams.len() > 20 {
                    error!("Failed to create {:?} stream. More than max 20 streams", &data.stream);
                    return;
                }

                let stream = Stream::dynamic(
                    &data.stream,
                    &self.config.project_id,
                    &self.config.device_id,
                    self.data_tx.clone(),
                );
                self.streams.entry(data.stream.to_owned()).or_insert(stream)
            }
        };

        // send data onto stream
        let flushed = match stream.fill(data).await {
            Ok(f) => f,
            Err(e) => {
                error!("Failed to send data. Error = {:?}", e);
                return;
            }
        };

        // update stream's timeout if necessary
        match self.flush_handler.map.entry(stream.name.to_string()) {
            // Remove timeout from queue and map for selected stream if flushed.
            Entry::Occupied(e) if flushed => {
                let key = e.remove();
                self.flush_handler.queue.remove(&key);
            }
            // Reset timeout from queue and map for selected stream if not flushed.
            Entry::Occupied(e) => {
                let key = e.get();
                self.flush_handler.queue.reset(key, self.period);
            }
            // Add new timeout to queue if not flushed and not in map.
            Entry::Vacant(e) => {
                let key = self.flush_handler.queue.insert(stream.name.to_string(), self.period);
                e.insert(key);
            }
        }
    }

    /// Remove a key from map if it has timedout and flush relevant stream.
    pub async fn next(&mut self) -> Option<()> {
        let stream = self.flush_handler.next().await?;
        if let Err(e) = self.streams.get_mut(&stream).unwrap().flush().await {
            error!("Failed to send data. Error = {:?}", e);
        }

        Some(())
    }

    /// Check if the queue is empty, in which case `self.next()` need not be polled
    pub fn is_empty(&self) -> bool {
        self.flush_handler.is_empty()
    }
}
