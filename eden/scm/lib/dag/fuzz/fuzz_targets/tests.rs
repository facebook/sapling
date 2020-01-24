/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use bindag::GeneralTestContext;
use dag::{spanset::SpanSet, Id};

#[allow(dead_code)]
pub fn test_gca<T: AsRef<[usize]>>(context: &GeneralTestContext<T>, namedag_revs: Vec<usize>) {
    let namedag_revs = SpanSet::from_spans(namedag_revs.iter().map(|&i| Id(i as u64)));
    let plain_revs = context.to_plain_revs(&namedag_revs);
    let mut plain_gca = bindag::gca(&context.parents, &plain_revs);
    let mut namedag_gca = context.to_plain_revs(&context.id_dag().gca_all(namedag_revs).unwrap());

    plain_gca.sort_unstable();
    namedag_gca.sort_unstable();
    assert_eq!(plain_gca, namedag_gca, "gca({:?})", &plain_revs);
}

#[allow(dead_code)]
pub fn test_range<T: AsRef<[usize]>>(
    context: &GeneralTestContext<T>,
    roots: Vec<usize>,
    heads: Vec<usize>,
) {
    let plain_roots: Vec<_> = roots.iter().map(|&i| context.idmap[i]).collect();
    let plain_heads: Vec<_> = heads.iter().map(|&i| context.idmap[i]).collect();

    let mut plain_range = bindag::range(&context.parents, &plain_roots, &plain_heads);
    plain_range.sort_unstable();

    let namedag_roots = SpanSet::from_spans(roots.iter().map(|&i| Id(i as u64)));
    let namedag_heads = SpanSet::from_spans(heads.iter().map(|&i| Id(i as u64)));
    let mut namedag_range = context.to_plain_revs(
        &context
            .id_dag()
            .range(namedag_roots, namedag_heads)
            .expect("IdDag::range"),
    );

    namedag_range.sort_unstable();
    assert_eq!(
        plain_range, namedag_range,
        "range({:?}::{:?})",
        &roots, &heads
    );
}
