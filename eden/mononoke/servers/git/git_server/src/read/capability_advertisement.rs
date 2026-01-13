/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use core::str;

use anyhow::Error;
use bytes::Bytes;
use gix_hash::Kind;
use gix_hash::ObjectId;
use gotham::mime;
use gotham::state::FromState;
use gotham::state::State;
use gotham_ext::error::HttpError;
use gotham_ext::response::BytesBody;
use gotham_ext::response::TryIntoResponse;
use http::Response;
use hyper::Body;
use hyper::HeaderMap;
use packetline::encode::flush_to_write;
use packetline::encode::write_text_packetline;
use protocol::generator::ls_refs_response;
use protocol::types::LsRefsRequest;
use protocol::types::ref_line;

use crate::model::GitMethod;
use crate::model::GitMethodInfo;
use crate::model::RepositoryParams;
use crate::model::RepositoryRequestContext;
use crate::model::ResponseType;
use crate::model::Service;
use crate::model::ServiceType;

const UPLOAD_PACK_CAPABILITIES: &[&str] = &[
    "ls-refs=unborn",
    "fetch=shallow wait-for-done filter",
    "ref-in-want",
    "object-format=sha1",
];
const RECEIVE_PACK_CAPABILITIES: &str =
    "report-status atomic delete-refs quiet ofs-delta object-format=sha1";
const BUNDLE_URI_CAPABILITY: &str = "bundle-uri";
const USE_BUNDLE_URI: &str = "x-git-use-bundle-uri";

const VERSION: &str = "2";

async fn advertise_capability(
    request_context: RepositoryRequestContext,
    service_type: Service,
    use_bundle_uri: bool,
) -> Result<Vec<u8>, Error> {
    let client_trusted = !request_context.ctx.metadata().client_untrusted();
    let bundle_trusted_only = request_context.bundle_uri_trusted_only();

    let mut output = Vec::new();

    write_text_packetline(
        format!("# service={}", service_type).as_bytes(),
        &mut output,
    )
    .await?;
    flush_to_write(&mut output).await?;
    match service_type {
        Service::GitUploadPack => {
            read_advertisement(
                &mut output,
                client_trusted,
                use_bundle_uri,
                bundle_trusted_only,
            )
            .await?
        }
        Service::GitReceivePack => write_advertisement(request_context, &mut output).await?,
    }
    flush_to_write(&mut output).await?;
    Ok(output)
}

async fn read_advertisement(
    output: &mut Vec<u8>,
    client_trusted: bool,
    use_bundle_uri: bool,
    bundle_trusted_only: bool,
) -> Result<(), Error> {
    write_text_packetline(format!("version {}", VERSION).as_bytes(), output).await?;
    for capability in UPLOAD_PACK_CAPABILITIES {
        write_text_packetline(capability.as_bytes(), output).await?;
    }
    let client_validated = !bundle_trusted_only || client_trusted;
    if use_bundle_uri && client_validated {
        write_text_packetline(BUNDLE_URI_CAPABILITY.as_bytes(), output).await?;
    }
    Ok(())
}

async fn write_advertisement(
    request_context: RepositoryRequestContext,
    output: &mut Vec<u8>,
) -> Result<(), Error> {
    let mut refs: Vec<_> = ls_refs_response(
        &request_context.ctx,
        &request_context.repo,
        LsRefsRequest::write_advertisement(),
    )
    .await?
    .included_refs
    .into_iter()
    .collect();
    refs.sort_by(|a, b| a.0.cmp(&b.0));

    let mut refs = refs.into_iter();
    match refs.next() {
        Some((ref_name, target)) => {
            let first_ref_line = ref_line(ref_name.as_str(), &target);
            write_text_packetline(
                format!("{}\0{}", first_ref_line, RECEIVE_PACK_CAPABILITIES).as_bytes(),
                output,
            )
            .await?;
        }
        None => {
            write_text_packetline(
                format!(
                    "{} capabilities^{{}}\0{}",
                    ObjectId::null(Kind::Sha1),
                    RECEIVE_PACK_CAPABILITIES
                )
                .as_bytes(),
                output,
            )
            .await?;
        }
    }
    for (ref_name, target) in refs {
        write_text_packetline(ref_line(ref_name.as_str(), &target).as_bytes(), output).await?;
    }
    Ok(())
}

pub async fn capability_advertisement(state: &mut State) -> Result<Response<Body>, HttpError> {
    let service_type = ServiceType::borrow_from(state).service;
    let repo_name = RepositoryParams::borrow_from(state).repo_name();
    let git_method_info = match service_type {
        Service::GitUploadPack => {
            GitMethodInfo::standard(repo_name.clone(), GitMethod::AdvertiseRead)
        }
        Service::GitReceivePack => {
            GitMethodInfo::standard(repo_name.clone(), GitMethod::AdvertiseWrite)
        }
    };
    let request_context = RepositoryRequestContext::instantiate(state, git_method_info).await?;

    let headers = HeaderMap::borrow_from(state);
    let force_bundle_uri = headers
        .get(USE_BUNDLE_URI)
        .and_then(|x| x.to_str().ok())
        .and_then(|x| x.parse().ok());

    let use_bundle_uri =
        force_bundle_uri.unwrap_or_else(|| request_context.repo.repo_config.enable_git_bundle_uri);

    let output = advertise_capability(request_context, service_type, use_bundle_uri)
        .await
        .map_err(HttpError::e500)?;
    let service_type = ServiceType::borrow_from(state).service;
    state.put(service_type.to_owned());
    state.put(ResponseType::Advertisement);
    BytesBody::new(Bytes::from(output), mime::TEXT_PLAIN)
        .try_into_response(state)
        .map_err(HttpError::e500)
}
