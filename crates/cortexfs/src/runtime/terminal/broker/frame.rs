use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

use serde::Serialize;
use serde::de::DeserializeOwned;

use super::{BrokerProtocolError, MAX_BROKER_FRAME_BYTES};

const FRAME_TIMEOUT: Duration = Duration::from_secs(1);

pub fn read_frame<T: DeserializeOwned>(stream: &mut UnixStream) -> Result<T, BrokerProtocolError> {
    let deadline = Instant::now() + FRAME_TIMEOUT;
    stream.set_nonblocking(true)?;
    let result = read_frame_until(stream, deadline);
    stream.set_nonblocking(false)?;
    result
}

fn read_frame_until<T: DeserializeOwned>(
    stream: &mut UnixStream,
    deadline: Instant,
) -> Result<T, BrokerProtocolError> {
    let mut header = [0_u8; 4];
    read_exact_until(stream, &mut header, deadline)?;
    let length = usize::try_from(u32::from_be_bytes(header))
        .map_err(|_error| BrokerProtocolError::FrameLimit)?;
    if length == 0 || length > MAX_BROKER_FRAME_BYTES {
        return Err(BrokerProtocolError::FrameLimit);
    }
    let mut body = vec![0_u8; length];
    read_exact_until(stream, &mut body, deadline)?;
    serde_json::from_slice(&body).map_err(|_error| BrokerProtocolError::Protocol)
}

pub fn write_frame<T: Serialize>(
    stream: &mut UnixStream,
    frame: &T,
) -> Result<(), BrokerProtocolError> {
    let body = serde_json::to_vec(frame).map_err(|_error| BrokerProtocolError::Protocol)?;
    if body.is_empty() || body.len() > MAX_BROKER_FRAME_BYTES {
        return Err(BrokerProtocolError::FrameLimit);
    }
    let length = u32::try_from(body.len()).map_err(|_error| BrokerProtocolError::FrameLimit)?;
    stream.set_write_timeout(Some(FRAME_TIMEOUT))?;
    stream.write_all(&length.to_be_bytes())?;
    stream.write_all(&body)?;
    stream.set_write_timeout(None)?;
    Ok(())
}

fn read_exact_until(
    stream: &mut UnixStream,
    mut output: &mut [u8],
    deadline: Instant,
) -> Result<(), BrokerProtocolError> {
    while !output.is_empty() {
        match stream.read(output) {
            Ok(0) => {
                return Err(std::io::Error::from(std::io::ErrorKind::UnexpectedEof).into());
            }
            Ok(read) => {
                output = output
                    .get_mut(read..)
                    .ok_or(BrokerProtocolError::Protocol)?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                if Instant::now() >= deadline {
                    return Err(std::io::Error::from(std::io::ErrorKind::TimedOut).into());
                }
                std::thread::sleep(Duration::from_millis(1));
            }
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}
