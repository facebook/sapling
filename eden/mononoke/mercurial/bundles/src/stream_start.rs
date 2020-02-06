/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use std::collections::HashMap;

use anyhow::{bail, Context, Error, Result};
use byteorder::{BigEndian, ByteOrder};
use bytes::BytesMut;
use tokio_io::codec::Decoder;

use crate::errors::ErrorKind;
use crate::types::StreamHeader;
use crate::utils::is_mandatory_param;

#[derive(Debug)]
pub struct StartDecoder;

impl Decoder for StartDecoder {
    type Item = StreamHeader;
    type Error = Error;

    fn decode(&mut self, buf: &mut BytesMut) -> Result<Option<StreamHeader>> {
        // bundle2 spec: "HG20" + u32 length of header + header + payload
        if buf.len() <= 8 {
            return Ok(None);
        }

        let header_len = {
            if &buf[..4] != b"HG20" {
                bail!(ErrorKind::Bundle2Decode(
                    "invalid bundle magic string".into(),
                ));
            }
            BigEndian::read_u32(&buf[4..8]) as usize
        };

        if buf.len() <= 8 + header_len {
            // Still more data to read.
            return Ok(None);
        }

        let _ = buf.split_to(8);

        let (m_stream_params, a_stream_params) = decode_stream_params(buf, header_len)?;

        Ok(Some(StreamHeader {
            m_stream_params,
            a_stream_params,
        }))
    }
}

fn decode_stream_params(
    buf: &mut BytesMut,
    header_len: usize,
) -> Result<(HashMap<String, String>, HashMap<String, String>)> {
    let mut m_stream_params = HashMap::new();
    let mut a_stream_params = HashMap::new();

    if header_len == 0 {
        return Ok((m_stream_params, a_stream_params));
    }

    // mutate the buffer to get the headers out of the way
    let header_buf = buf.split_to(header_len);
    let buf_slice = header_buf.as_ref();

    let headers = buf_slice.split(|c| *c == b' ');
    for header in headers {
        let mut key_val = header.splitn(2, |c| *c == b'=');
        let key = key_val
            .next()
            .ok_or(ErrorKind::Bundle2Decode("bad stream level key".into()))?;
        let val = key_val
            .next()
            .ok_or(ErrorKind::Bundle2Decode("bad stream level val".into()))?;
        let key_decoded = percent_encoding::percent_decode(key);
        let val_decoded = percent_encoding::percent_decode(val);
        let key_str = key_decoded.decode_utf8().with_context(|| {
            ErrorKind::Bundle2Decode("stream level key is invalid UTF-8".into())
        })?;
        let val_str = val_decoded.decode_utf8().with_context(|| {
            ErrorKind::Bundle2Decode("stream level val is invalid UTF-8".into())
        })?;
        if is_mandatory_param(&key_str)
            .with_context(|| ErrorKind::Bundle2Decode(format!("stream key is invalid")))?
        {
            m_stream_params.insert(key_str.to_lowercase(), val_str.into_owned());
        } else {
            a_stream_params.insert(key_str.to_lowercase(), val_str.into_owned());
        }
    }

    Ok((m_stream_params, a_stream_params))
}
