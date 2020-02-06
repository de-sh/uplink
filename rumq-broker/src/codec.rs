use bytes::BytesMut;
use rumq_core::{self, MqttRead, MqttWrite, Packet};
use std::io::{self, Cursor, ErrorKind};
use tokio_util::codec::{Decoder, Encoder};

pub struct MqttCodec;

impl MqttCodec {
    pub fn new() -> Self {
        MqttCodec
    }
}

impl Decoder for MqttCodec {
    type Item = Packet;
    type Error = io::Error;

    fn decode(&mut self, buf: &mut BytesMut) -> Result<Option<Packet>, io::Error> {
        // NOTE: `decode` might be called with `buf.len == 0` when prevous
        // decode call read all the bytes in the stream. We should return
        // Ok(None) in those cases or else the `read` call will return
        // Ok(0) => translated to UnexpectedEOF by `byteorder` crate.
        // `read` call Ok(0) happens when buffer specified was 0 bytes in len
        // https://doc.rust-lang.org/std/io/trait.Read.html#tymethod.read
        if buf.len() < 2 {
            return Ok(None);
        }

        let mut buf_ref = buf.as_ref();

        // NOTE: we are reading remaining length twice. once in `header_and_remaining_length` and other
        // in `mqtt_read`. Remove the duplicate later
        let len = buf_ref.header_and_remaining_length().unwrap();
        let packet = buf_ref.mqtt_read().unwrap();

        // NOTE: It's possible that `decode` got called before `buf` has full bytes
        // necessary to frame raw bytes into a packet. In that case return Ok(None)
        // and the next time decode` gets called, there will be more bytes in `buf`,
        // hopefully enough to frame the packet
        if buf.len() < len {
            return Ok(None);
        }

        buf.split_to(len);
        Ok(Some(packet))
    }
}

impl Encoder for MqttCodec {
    type Item = Packet;
    type Error = io::Error;

    fn encode(&mut self, msg: Packet, buf: &mut BytesMut) -> io::Result<()> {
        let mut stream = Cursor::new(Vec::new());

        // TODO: Implement `write_packet` for `&mut BytesMut`
        if let Err(e) = stream.mqtt_write(&msg) {
            error!("Encode error. Error = {:?}", e);
            return Err(io::Error::new(io::ErrorKind::Other, "Unable to encode!"));
        }

        buf.extend(stream.get_ref());

        Ok(())
    }
}
