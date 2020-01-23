/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use bindag::TestContext;
use dag::{spanset::SpanSet, Id};

#[allow(dead_code)]
pub fn test_gca(context: &TestContext, namedag_revs: Vec<usize>) {
    let namedag_revs = SpanSet::from_spans(namedag_revs.iter().map(|&i| Id(i as u64)));
    let plain_revs = context.to_plain_revs(&namedag_revs);
    let mut plain_gca = bindag::gca(&context.parents, &plain_revs);
    let mut namedag_gca = context.to_plain_revs(&context.id_dag().gca_all(namedag_revs).unwrap());

    plain_gca.sort_unstable();
    namedag_gca.sort_unstable();
    assert_eq!(plain_gca, namedag_gca, "gca({:?})", &plain_revs);
}
