use std::io::{Read, Write};

use serde::{Serialize, de::DeserializeOwned};

pub const MAX_FRAME_BYTES: u32 = 1 << 20;

#[derive(Debug, thiserror::Error)]
pub enum FrameError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("frame of {0} bytes exceeds the {MAX_FRAME_BYTES} byte limit")]
    TooLarge(u32),
}

pub fn write_frame<W: Write, T: Serialize>(w: &mut W, msg: &T) -> Result<(), FrameError> {
    let body = serde_json::to_vec(msg)?;
    let len = u32::try_from(body.len()).map_err(|_| FrameError::TooLarge(u32::MAX))?;
    if len > MAX_FRAME_BYTES {
        return Err(FrameError::TooLarge(len));
    }
    w.write_all(&len.to_le_bytes())?;
    w.write_all(&body)?;
    w.flush()?;
    Ok(())
}

pub fn read_frame<R: Read, T: DeserializeOwned>(r: &mut R) -> Result<T, FrameError> {
    let mut len_bytes = [0u8; 4];
    r.read_exact(&mut len_bytes)?;
    let len = u32::from_le_bytes(len_bytes);
    if len > MAX_FRAME_BYTES {
        return Err(FrameError::TooLarge(len));
    }
    let mut body = vec![0u8; len as usize];
    r.read_exact(&mut body)?;
    Ok(serde_json::from_slice(&body)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{HostToPi, PROTOCOL_VERSION};

    #[test]
    fn round_trips_a_message() {
        let msg = HostToPi::Hello {
            protocol_version: PROTOCOL_VERSION,
            host_version: "0.1.0".into(),
        };
        let mut buf = Vec::new();
        write_frame(&mut buf, &msg).unwrap();
        let back: HostToPi = read_frame(&mut buf.as_slice()).unwrap();
        assert_eq!(msg, back);
    }

    #[test]
    fn reads_frames_back_to_back() {
        let mut buf = Vec::new();
        write_frame(&mut buf, &HostToPi::Heartbeat).unwrap();
        write_frame(&mut buf, &HostToPi::SyncComplete).unwrap();

        let mut cursor = buf.as_slice();
        assert_eq!(
            read_frame::<_, HostToPi>(&mut cursor).unwrap(),
            HostToPi::Heartbeat
        );
        assert_eq!(
            read_frame::<_, HostToPi>(&mut cursor).unwrap(),
            HostToPi::SyncComplete
        );
    }

    #[test]
    fn rejects_an_oversized_length_prefix() {
        let mut buf = (MAX_FRAME_BYTES + 1).to_le_bytes().to_vec();
        let err = read_frame::<_, HostToPi>(&mut buf.as_slice()).unwrap_err();
        assert!(matches!(err, FrameError::TooLarge(_)));
        buf.clear();
    }
}
