/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use fbinit::FacebookInit;
use http::HeaderMap;
use metaconfig_types::Identity;
use permission_checker::MononokeIdentitySet;

pub fn try_get_cats_idents(
    fb: FacebookInit,
    headers: &HeaderMap,
    verifier_identity: &Identity,
) -> Option<MononokeIdentitySet> {
    catmod::try_get_cats_idents_impl(fb, headers, verifier_identity)
}

#[cfg(not(fbcode_build))]
mod catmod {
    use super::*;

    pub fn try_get_cats_idents_impl(
        _fb: FacebookInit,
        _headers: &HeaderMap,
        _verifier_identity: &Identity,
    ) -> Option<MononokeIdentitySet> {
        None
    }
}

#[cfg(fbcode_build)]
mod catmod {
    use anyhow::Error;
    use cats_constants::X_AUTH_CATS_HEADER;
    use login_objects_thrift::EnvironmentType;
    use tracing::debug;
    use tracing::warn;

    use super::*;

    /// Extract identities from CAT tokens in the request headers.
    ///
    /// Returns `None` when no CAT header is present or when the header itself is
    /// malformed (e.g. invalid base64). When the header parses, returns
    /// `Some(set)` containing the identities of every token that successfully
    /// verified — invalid tokens are silently dropped.
    ///
    /// The resulting identities are `MononokeIdentity::Authenticated` carrying the
    /// full `AuthenticatedIdentity` thrift struct (including attributes extracted
    /// from the verified token's `metaIdUri`, matching srserver's
    /// `authenticated_identities_cats_struct` path).
    pub fn try_get_cats_idents_impl(
        fb: FacebookInit,
        headers: &HeaderMap,
        verifier_identity: &Identity,
    ) -> Option<MononokeIdentitySet> {
        match parse_cat_token_list(headers) {
            Ok(None) => None,
            Ok(Some(cat_list)) => Some(verify_cat_tokens(fb, cat_list, verifier_identity)),
            Err(e) => {
                warn!(
                    "Error extracting CATs identities: {}. Ignoring CAT token.",
                    e
                );
                None
            }
        }
    }

    fn parse_cat_token_list(
        headers: &HeaderMap,
    ) -> Result<Option<cryptocat::CryptoAuthTokenList>, Error> {
        let cats = match headers.get(X_AUTH_CATS_HEADER) {
            Some(cats) => cats,
            None => {
                debug!("CAT extraction: no {} header present", X_AUTH_CATS_HEADER);
                return Ok(None);
            }
        };
        let s_cats = cats.to_str()?;
        let cat_list = cryptocat::deserialize_crypto_auth_tokens(s_cats)?;
        debug!(
            "CAT extraction: received {} token(s) in {} header",
            cat_list.tokens.len(),
            X_AUTH_CATS_HEADER,
        );
        Ok(Some(cat_list))
    }

    fn verify_cat_tokens(
        fb: FacebookInit,
        cat_list: cryptocat::CryptoAuthTokenList,
        verifier_identity: &Identity,
    ) -> MononokeIdentitySet {
        let svc_scm_ident = cryptocat::Identity {
            id_type: verifier_identity.id_type.clone(),
            id_data: verifier_identity.id_data.clone(),
            ..Default::default()
        };

        debug!(
            "CAT extraction: bulk-verifying {} token(s) via authenticated_identity path",
            cat_list.tokens.len(),
        );
        match cryptocat::verify_and_extract_authenticated_identities(
            fb,
            cat_list,
            &svc_scm_ident,
            None,
            vec![EnvironmentType::PROD, EnvironmentType::CORP],
        ) {
            Ok(idents) => idents
                .into_iter()
                .map(|auth_id| {
                    debug!(
                        "CAT extraction: extracted identity {}:{}",
                        auth_id.identity.id_type, auth_id.identity.id_data,
                    );
                    permission_checker::MononokeIdentity::Authenticated(auth_id)
                })
                .collect(),
            Err(e) => {
                warn!(
                    "CAT extraction: bulk verify failed: {}. Returning empty set.",
                    e
                );
                MononokeIdentitySet::new()
            }
        }
    }
}
