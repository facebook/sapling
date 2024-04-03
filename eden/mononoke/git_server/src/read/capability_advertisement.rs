/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use bytes::Bytes;
use gotham::mime;
use gotham::state::FromState;
use gotham::state::State;
use gotham_ext::error::HttpError;
use gotham_ext::response::BytesBody;
use gotham_ext::response::TryIntoResponse;
use packetline::encode::flush_to_write;
use packetline::encode::write_text_packetline;

use crate::model::ResponseType;
use crate::model::Service;
use crate::model::ServiceType;

const CORE_CAPABILITIES: &[&str] = &[
    "ls-refs=unborn",
    "fetch=shallow",
    "wait-for-done",
    "filter",
    "ref-in-want",
    "object-format=sha1",
];
const VERSION: &str = "2";

async fn advertise_capability(service_type: &Service) -> Result<Vec<u8>, Error> {
    let mut output = Vec::new();
    write_text_packetline(
        format!("# service={}", service_type).as_bytes(),
        &mut output,
    )
    .await?;
    flush_to_write(&mut output).await?;
    write_text_packetline(format!("version {}", VERSION).as_bytes(), &mut output).await?;
    for capability in CORE_CAPABILITIES {
        write_text_packetline(capability.as_bytes(), &mut output).await?;
    }
    flush_to_write(&mut output).await?;
    Ok(output)
}

pub async fn capability_advertisement(
    state: &mut State,
) -> Result<impl TryIntoResponse, HttpError> {
    let service_type = &ServiceType::borrow_from(state).service;
    let output = advertise_capability(service_type)
        .await
        .map_err(HttpError::e500)?;
    state.put(Service::GitUploadPack);
    state.put(ResponseType::Advertisement);
    Ok(BytesBody::new(Bytes::from(output), mime::TEXT_PLAIN))
}
