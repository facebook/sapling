// Copyright (c) 2017-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use blobrepo::BlobRepo;
use context::CoreContext;
use failure::Error;
use futures::future::Future;
use futures::stream::Stream;
use mononoke_types::ChangesetId;

pub fn single_changeset_id(
    ctx: CoreContext,
    cs_id: ChangesetId,
    repo: &BlobRepo,
) -> impl Stream<Item = ChangesetId, Error = Error> {
    repo.changeset_exists_by_bonsai(ctx, &cs_id)
        .map(move |exists| if exists { Some(cs_id) } else { None })
        .into_stream()
        .filter_map(|maybenode| maybenode)
}

#[cfg(test)]
mod test {
    use super::*;
    use async_unit;
    use context::CoreContext;
    use fixtures::linear;
    use futures_ext::StreamExt;
    use mononoke_types_mocks::changesetid::ONES_CSID;
    use std::sync::Arc;
    use tests::assert_changesets_sequence;
    use tests::string_to_bonsai;

    #[test]
    fn valid_changeset() {
        async_unit::tokio_unit_test(|| {
            let ctx = CoreContext::test_mock();
            let repo = Arc::new(linear::getrepo(None));
            let bcs_id = string_to_bonsai(&repo, "a5ffa77602a066db7d5cfb9fb5823a0895717c5a");
            let changeset_stream = single_changeset_id(ctx.clone(), bcs_id.clone(), &repo);

            assert_changesets_sequence(
                ctx.clone(),
                &repo,
                vec![bcs_id].into_iter(),
                changeset_stream.boxify(),
            );
        });
    }

    #[test]
    fn invalid_changeset() {
        async_unit::tokio_unit_test(|| {
            let ctx = CoreContext::test_mock();
            let repo = Arc::new(linear::getrepo(None));
            let cs_id = ONES_CSID;
            let changeset_stream = single_changeset_id(ctx.clone(), cs_id, &repo.clone());

            assert_changesets_sequence(ctx, &repo, vec![].into_iter(), changeset_stream.boxify());
        });
    }
}
