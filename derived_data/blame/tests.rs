/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use crate::fetch_blame;
use bytes::Bytes;
use context::CoreContext;
use failure::{err_msg, Error};
use fbinit::FacebookInit;
use futures::Future;
use maplit::{btreemap, hashmap};
use mononoke_types::{Blame, ChangesetId, MPath};
use std::collections::HashMap;
use tests_utils::{create_commit, store_files, store_rename};

// File with multiple changes and a merge
const F0: &[&str] = &[
    // c0
    r#"|
1 0
1 1
"#,
    // c1
    r#"|
2 0
1 0
2 1
"#,
    // c2
    r#"|
2 0
1 0
3 0
3 1
2 1
3 2
"#,
    // c3
    r#"|
1 0
1 1
3 2
4 0
"#,
    // c4
    r#"|
2 0
1 0
3 0
3 1
2 1
3 2
4 0
"#,
];

const F0_AT_C4: &str = r#"c0: |
c1: 2 0
c0: 1 0
c2: 3 0
c2: 3 1
c1: 2 1
c2: 3 2
c3: 4 0
"#;

// file with multiple change only in one parent and a merge
const F1: &[&str] = &[
    // c0
    r#"|
1 0
1 1
"#,
    // c3
    r#"|
1 0
4 0
1 1
"#,
];

const F1_AT_C4: &str = r#"c0: |
c0: 1 0
c3: 4 0
c0: 1 1
"#;

// renamed file
const F2: &[&str] = &[
    // c0 as _f2
    r#"|
1 0
1 1
"#,
    // c1 as _f2 => f2
    r#"|
1 0
2 0
1 1
"#,
    // c3 as new f2
    r#"|
1 0
4 0
1 1
"#,
    // c4 as f2
    r#"|
5 0
1 0
2 0
4 0
1 1
"#,
];

const F2_AT_C4: &str = r#"c0: |
c4: 5 0
c0: 1 0
c1: 2 0
c3: 4 0
c0: 1 1
"#;

#[fbinit::test]
fn test_blame(fb: FacebookInit) -> Result<(), Error> {
    // Commits structure
    //
    //   0
    //  / \
    // 1   3
    // |   |
    // 2   |
    //  \ /
    //   4
    //
    async_unit::tokio_unit_test(move || {
        let ctx = CoreContext::test_mock(fb);
        let repo = blobrepo_factory::new_memblob_empty(None)?;

        let c0 = create_commit(
            ctx.clone(),
            repo.clone(),
            vec![],
            store_files(
                ctx.clone(),
                btreemap! {
                    "f0" => Some(F0[0]),
                    "f1" => Some(F1[0]),
                    "_f2" => Some(F2[0]),
                },
                repo.clone(),
            ),
        );

        let mut c1_changes =
            store_files(ctx.clone(), btreemap! {"f0" => Some(F0[1])}, repo.clone());
        let (f2_path, f2_change) = store_rename(
            ctx.clone(),
            (MPath::new("_f2")?, c0),
            "f2",
            F2[1],
            repo.clone(),
        );
        c1_changes.insert(f2_path, f2_change);
        let c1 = create_commit(ctx.clone(), repo.clone(), vec![c0], c1_changes);

        let c2 = create_commit(
            ctx.clone(),
            repo.clone(),
            vec![c1],
            store_files(ctx.clone(), btreemap! {"f0" => Some(F0[2])}, repo.clone()),
        );

        let c3 = create_commit(
            ctx.clone(),
            repo.clone(),
            vec![c0],
            store_files(
                ctx.clone(),
                btreemap! {
                    "f0" => Some(F0[3]),
                    "f1" => Some(F1[1]),
                    "f2" => Some(F2[2]),
                },
                repo.clone(),
            ),
        );

        let c4 = create_commit(
            ctx.clone(),
            repo.clone(),
            vec![c2, c3],
            store_files(
                ctx.clone(),
                btreemap! {
                    "f0" => Some(F0[4]),
                    "f1" => Some(F1[1]), // did not change after c3
                    "f2" => Some(F2[3]),
                },
                repo.clone(),
            ),
        );

        let names = hashmap! {
            c0 => "c0",
            c1 => "c1",
            c2 => "c2",
            c3 => "c3",
            c4 => "c4",
        };

        let (content, blame) =
            fetch_blame(ctx.clone(), repo.clone(), c4, MPath::new("f0")?).wait()?;
        assert_eq!(annotate(content, blame, &names)?, F0_AT_C4);

        let (content, blame) =
            fetch_blame(ctx.clone(), repo.clone(), c4, MPath::new("f1")?).wait()?;
        assert_eq!(annotate(content, blame, &names)?, F1_AT_C4);

        let (content, blame) =
            fetch_blame(ctx.clone(), repo.clone(), c4, MPath::new("f2")?).wait()?;
        assert_eq!(annotate(content, blame, &names)?, F2_AT_C4);

        Ok(())
    })
}

fn annotate(
    content: Bytes,
    blame: Blame,
    names: &HashMap<ChangesetId, &'static str>,
) -> Result<String, Error> {
    let content = std::str::from_utf8(content.as_ref())?;
    let mut result = String::new();
    let mut ranges = blame.ranges().iter();
    let mut range = ranges
        .next()
        .ok_or_else(|| err_msg("empty blame for non empty content"))?;
    for (index, line) in content.lines().enumerate() {
        if index as u32 >= range.offset + range.length {
            range = ranges
                .next()
                .ok_or_else(|| err_msg("not enough ranges in a blame"))?;
        }
        let name = names
            .get(&range.csid)
            .ok_or_else(|| err_msg("unresolved csid"))?;
        result.push_str(name);
        result.push_str(&": ");
        result.push_str(line);
        result.push_str("\n");
    }
    Ok(result)
}
