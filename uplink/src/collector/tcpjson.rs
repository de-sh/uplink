use flume::{Receiver, RecvError, Sender};
use log::{debug, error, info};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream};
use tokio::time::{Duration, Instant};
use tokio::{select, time};
use tokio_stream::StreamExt;
use tokio_util::codec::{Framed, LinesCodec, LinesCodecError};

use std::{io, sync::Arc};

use crate::base::actions::{Action, ActionResponse, Error as ActionsError};
use crate::base::{Buffer, Config, Package, Point, Stream};

mod util;
pub use util::DelayMap;
use util::StreamHandler;

#[derive(Error, Debug)]
pub enum Error {
    #[error("Io error {0}")]
    Io(#[from] io::Error),
    #[error("Receiver error {0}")]
    Recv(#[from] RecvError),
    #[error("Stream done")]
    StreamDone,
    #[error("Lines codec error {0}")]
    Codec(#[from] LinesCodecError),
    #[error("Serde error {0}")]
    Json(#[from] serde_json::error::Error),
    #[error("Download OTA error")]
    Actions(#[from] ActionsError),
    #[error("Couldn't fill stream")]
    Stream(#[from] crate::base::Error),
}

pub struct Bridge {
    config: Arc<Config>,
    actions_rx: Receiver<Action>,
    current_action: Option<String>,
    action_status: Stream<ActionResponse>,
    stream_handler: StreamHandler,
}

impl Bridge {
    pub fn new(
        config: Arc<Config>,
        data_tx: Sender<Box<dyn Package>>,
        actions_rx: Receiver<Action>,
        action_status: Stream<ActionResponse>,
    ) -> Bridge {
        let stream_handler = StreamHandler::new(config.clone(), data_tx);
        Bridge { config, actions_rx, current_action: None, action_status, stream_handler }
    }

    pub async fn start(&mut self) -> Result<(), Error> {
        let mut action_status = self.action_status.clone();

        loop {
            let addr = format!("0.0.0.0:{}", self.config.bridge_port);
            let listener = TcpListener::bind(&addr).await?;

            let (stream, addr) = loop {
                select! {
                    v = listener.accept() =>  {
                        match v {
                            Ok(s) => break s,
                            Err(e) => {
                                error!("Tcp connection accept error = {:?}", e);
                                continue;
                            }
                        }
                    }
                    action = self.actions_rx.recv_async() => {
                        let action = action?;
                        error!("Bridge down!! Action ID = {}", action.action_id);
                        let status = ActionResponse::failure(&action.action_id, "Bridge down");
                        if let Err(e) = action_status.fill(status).await {
                            error!("Failed to send busy status. Error = {:?}", e);
                        }
                    }
                }
            };

            info!("Accepted new connection from {:?}", addr);
            let framed = Framed::new(stream, LinesCodec::new());
            if let Err(e) = self.collect(framed).await {
                error!("Bridge failed. Error = {:?}", e);
            }
        }
    }

    pub async fn collect(
        &mut self,
        mut framed: Framed<TcpStream, LinesCodec>,
    ) -> Result<(), Error> {
        let mut action_status = self.action_status.clone();
        let action_timeout = time::sleep(Duration::from_secs(100));
        tokio::pin!(action_timeout);

        loop {
            select! {
                frame = framed.next() => {
                    let frame = frame.ok_or(Error::StreamDone)??;
                    debug!("Received line = {:?}", frame);

                    let data: Payload = match serde_json::from_str(&frame) {
                        Ok(d) => d,
                        Err(e) => {
                            error!("Deserialization error = {:?}", e);
                            continue
                        }
                    };

                    // If incoming data is a response for an action, drop it
                    // if timeout is already sent to cloud
                    if data.stream == "action_status" {
                        match self.current_action.take() {
                            Some(id) => debug!("Response for action = {:?}", id),
                            None => {
                                error!("Action timed out already");
                                continue
                            }
                        }
                    }

                    self.stream_handler.handle_data(data).await;
                }

                action = self.actions_rx.recv_async() => {
                    let action = action?;
                    self.current_action = Some(action.action_id.to_owned());

                    action_timeout.as_mut().reset(Instant::now() + Duration::from_secs(10));
                    let data = match serde_json::to_vec(&action) {
                        Ok(d) => d,
                        Err(e) => {
                            error!("Serialization error = {:?}", e);
                            continue
                        }
                    };

                    framed.get_mut().write_all(&data).await?;
                    framed.get_mut().write_all(b"\n").await?;
                }

                _ = &mut action_timeout, if self.current_action.is_some() => {
                    let action = self.current_action.take().unwrap();
                    error!("Timeout waiting for action response. Action ID = {}", action);

                    // Send failure response to cloud
                    let status = ActionResponse::failure(&action, "Action timed out");
                    if let Err(e) = action_status.fill(status).await {
                        error!("Failed to fill. Error = {:?}", e);
                    }
                }

                // Flush streams that timeout, return any errors
                _ = self.stream_handler.next(), if !self.stream_handler.is_empty() => {}
            }
        }
    }
}

// TODO Don't do any deserialization on payload. Read it a Vec<u8> which is in turn a json
// TODO which cloud will double deserialize (Batch 1st and messages next)
#[derive(Debug, Serialize, Deserialize)]
pub struct Payload {
    #[serde(skip_serializing)]
    pub stream: String,
    pub sequence: u32,
    pub timestamp: u64,
    #[serde(flatten)]
    pub payload: Value,
}

impl Payload {
    pub fn from_string<S: Into<String>>(input: S) -> Result<Self, Error> {
        Ok(serde_json::from_str(&input.into())?)
    }
}

impl Point for Payload {
    fn sequence(&self) -> u32 {
        self.sequence
    }

    fn timestamp(&self) -> u64 {
        self.timestamp
    }
}

impl Package for Buffer<Payload> {
    fn topic(&self) -> Arc<String> {
        self.topic.clone()
    }

    fn serialize(&self) -> serde_json::Result<Vec<u8>> {
        serde_json::to_vec(&self.buffer)
    }

    fn anomalies(&self) -> Option<(String, usize)> {
        self.anomalies()
    }
}
