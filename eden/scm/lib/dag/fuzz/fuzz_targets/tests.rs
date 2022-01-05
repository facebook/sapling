/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use bindag::GeneralTestContext;

#[allow(dead_code)]
pub fn test_gca<T: AsRef<[usize]>>(context: &GeneralTestContext<T>, mut plain_revs: Vec<usize>) {
    plain_revs.sort_unstable();
    plain_revs.dedup();
    let iddag_revs = context.to_dag_revs(&plain_revs);
    let mut plain_gca = bindag::gca(&context.parents, &plain_revs);
    let mut iddag_gca = context.to_plain_revs(&context.id_dag().gca_all(iddag_revs).unwrap());

    plain_gca.sort_unstable();
    iddag_gca.sort_unstable();
    assert_eq!(plain_gca, iddag_gca, "gca({:?})", &plain_revs);
}

#[allow(dead_code)]
pub fn test_range<T: AsRef<[usize]>>(
    context: &GeneralTestContext<T>,
    plain_roots: Vec<usize>,
    plain_heads: Vec<usize>,
) {
    let plain_roots = context.clamp_revs(&plain_roots);
    let plain_heads = context.clamp_revs(&plain_heads);
    // let plain_roots: Vec<_> = roots.iter().map(|i| context.idmap[i]).collect();
    // let plain_heads: Vec<_> = heads.iter().map(|i| context.idmap[i]).collect();

    let mut plain_range = bindag::range(&context.parents, &plain_roots, &plain_heads);
    plain_range.sort_unstable();

    let iddag_roots = context.to_dag_revs(&plain_roots);
    let iddag_heads = context.to_dag_revs(&plain_heads);
    let mut iddag_range = context.to_plain_revs(
        &context
            .id_dag()
            .range(iddag_roots, iddag_heads)
            .expect("IdDag::range"),
    );

    iddag_range.sort_unstable();
    assert_eq!(
        plain_range, iddag_range,
        "range({:?}::{:?})",
        &plain_roots, &plain_heads
    );
}
