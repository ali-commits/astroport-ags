use serde_json::Value;
use std::io::{self, Read, Write};

pub const MAX_FRAME: usize = 1 << 20;

pub fn read_frame(r: &mut impl Read) -> io::Result<Option<Value>> {
    let mut header = [0; 4];
    match r.read_exact(&mut header[..1]) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    r.read_exact(&mut header[1..])?;
    let len = u32::from_be_bytes(header) as usize;
    if len > MAX_FRAME {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "frame exceeds 1 MiB",
        ));
    }

    let mut body = vec![0; len];
    r.read_exact(&mut body)?;
    serde_json::from_slice(&body)
        .map(Some)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

pub fn write_frame(w: &mut impl Write, v: &Value) -> io::Result<()> {
    let body = serde_json::to_vec(v)?;
    if body.len() > MAX_FRAME {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "frame exceeds 1 MiB",
        ));
    }

    w.write_all(&(body.len() as u32).to_be_bytes())?;
    w.write_all(&body)?;
    w.flush()
}

pub struct RpcError {
    pub code: &'static str,
    pub message: String,
}

impl RpcError {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn frame_round_trip() {
        let mut buf = Vec::new();
        write_frame(&mut buf, &json!({"id": 1, "method": "hello"})).unwrap();
        assert_eq!(&buf[..4], &(buf.len() as u32 - 4).to_be_bytes());
        assert_eq!(
            read_frame(&mut &buf[..]).unwrap(),
            Some(json!({"id": 1, "method": "hello"}))
        );
        assert_eq!(read_frame(&mut &[][..]).unwrap(), None);
    }

    #[test]
    fn frame_too_large() {
        let len = ((1u32 << 20) + 1).to_be_bytes();
        assert_eq!(
            read_frame(&mut &len[..]).unwrap_err().kind(),
            std::io::ErrorKind::InvalidData
        );
    }

    #[test]
    fn truncated_frame_is_not_eof() {
        for bytes in [&[0, 0, 0][..], &[0, 0, 0, 3, b'{'][..]] {
            assert_eq!(
                read_frame(&mut &bytes[..]).unwrap_err().kind(),
                std::io::ErrorKind::UnexpectedEof
            );
        }
    }
}
