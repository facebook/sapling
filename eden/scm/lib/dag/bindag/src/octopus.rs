/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

/// Return a DAG with cross and octopus merges.
pub fn cross_octopus() -> Vec<Vec<usize>> {
    let parents = drawdag::parse(
        r#"
        r17 r18 r19 r20 r21 r22 r23 r24 r25 r26 r27 r28 r29 r30 r31 r32
         |\  |\  |\  |\  |\  |\  |\  |\  |\  |\  |\  |\  |\  |\  |\  |\
       r00 r01 r02 r03 r04 r05 r06 r07 r08 r09 r10 r11 r12 r13 r14 r15 r16

        r17 r18 r19 r20 r21 r22 r23 r24 r25 r26 r27 r28 r29 r30 r31 r32
         |\  |\  |\  |\  |\  |\  |\  |\  |\  |\  |\  |\  |\  |\  |\  |\
       r02 r03 r04 r05 r06 r07 r08 r09 r10 r11 r12 r13 r14 r15 r16 r00 r01

        r17 r18 r19 r20 r21 r22 r23 r24 r25 r26 r27 r28 r29 r30 r31 r32
         |\  |\  |\  |\  |\  |\  |\  |\  |\  |\  |\  |\  |\  |\  |\  |\
       r04 r05 r06 r07 r08 r09 r10 r11 r12 r13 r14 r15 r16 r00 r01 r02 r03

        r17 r18 r19 r20 r21 r22 r23 r24 r25 r26 r27 r28 r29 r30 r31 r32
         |\  |\  |\  |\  |\  |\  |\  |\  |\  |\  |\  |\  |\  |\  |\  |\
       r06 r07 r08 r09 r10 r11 r12 r13 r14 r15 r16 r00 r01 r02 r03 r04 r05
    "#,
    );

    (0..=32)
        .map(|i| {
            parents[&format!("r{:02}", i)]
                .iter()
                .map(|p| p.trim_start_matches('r').parse::<usize>().unwrap())
                .collect()
        })
        .collect()
}
