/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::str::FromStr;

use anyhow::Error;
use anyhow::Result;
use fbinit::FacebookInit;
use fixtures::ManyFilesDirs;
use fixtures::TestRepoFixture;
use futures::stream::TryStreamExt;
use futures::FutureExt;
use justknobs::test_helpers::with_just_knobs_async;
use justknobs::test_helpers::JustKnobsInMemory;
use justknobs::test_helpers::KnobVal;
use maplit::hashmap;
use mononoke_types::path::MPath;

use crate::ChangesetFileOrdering;
use crate::ChangesetId;
use crate::CoreContext;
use crate::Mononoke;

async fn commit_find_files_impl(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let mononoke = Mononoke::new_test(vec![(
        "test".to_string(),
        ManyFilesDirs::get_custom_test_repo(fb).await,
    )])
    .await?;
    let repo = mononoke
        .repo(ctx, "test")
        .await?
        .expect("repo exists")
        .build()
        .await?;
    let hash = "b0d1bf77898839595ee0f0cba673dd6e3be9dadaaa78bc6dd2dea97ca6bee77e";
    let cs_id = ChangesetId::from_str(hash)?;
    let cs = repo.changeset(cs_id).await?.expect("changeset exists");

    // Find everything
    let mut files: Vec<_> = cs
        .find_files_unordered(None, None)
        .await?
        .try_collect()
        .await?;
    files.sort();
    let expected_files = vec![
        MPath::try_from("1")?,
        MPath::try_from("2")?,
        MPath::try_from("dir1/file_1_in_dir1")?,
        MPath::try_from("dir1/file_2_in_dir1")?,
        MPath::try_from("dir1/subdir1/file_1")?,
        MPath::try_from("dir1/subdir1/subsubdir1/file_1")?,
        MPath::try_from("dir1/subdir1/subsubdir2/file_1")?,
        MPath::try_from("dir1/subdir1/subsubdir2/file_2")?,
        MPath::try_from("dir2/file_1_in_dir2")?,
    ];
    assert_eq!(files, expected_files);

    // Find everything ordered
    let files: Vec<_> = cs
        .find_files(
            None,
            None,
            None,
            ChangesetFileOrdering::Ordered { after: None },
        )
        .await?
        .try_collect()
        .await?;
    assert_eq!(files, expected_files);

    // Find everything after a particular file
    let files: Vec<_> = cs
        .find_files(
            None,
            None,
            None,
            ChangesetFileOrdering::Ordered {
                after: Some(MPath::try_from("dir1/subdir1/subsubdir2/file_1")?),
            },
        )
        .await?
        .try_collect()
        .await?;
    let expected_files = vec![
        MPath::try_from("dir1/subdir1/subsubdir2/file_2")?,
        MPath::try_from("dir2/file_1_in_dir2")?,
    ];
    assert_eq!(files, expected_files);

    // Prefixes
    let mut files: Vec<_> = cs
        .find_files_unordered(
            Some(vec![
                MPath::try_from("dir1/subdir1/subsubdir1")?,
                MPath::try_from("dir2")?,
            ]),
            None,
        )
        .await?
        .try_collect()
        .await?;
    files.sort();
    let expected_files = vec![
        MPath::try_from("dir1/subdir1/subsubdir1/file_1")?,
        MPath::try_from("dir2/file_1_in_dir2")?,
    ];
    assert_eq!(files, expected_files);

    // Prefixes ordered, starting at the root.  (This has no real effect on
    // the output vs `after: None`, but exercise the code path anyway).
    let files: Vec<_> = cs
        .find_files(
            Some(vec![
                MPath::try_from("dir1/subdir1/subsubdir1")?,
                MPath::try_from("dir2")?,
            ]),
            None,
            None,
            ChangesetFileOrdering::Ordered {
                after: Some(MPath::try_from("")?),
            },
        )
        .await?
        .try_collect()
        .await?;
    assert_eq!(files, expected_files);

    // Prefixes ordered after
    let files: Vec<_> = cs
        .find_files(
            Some(vec![
                MPath::try_from("dir1/subdir1/subsubdir1")?,
                MPath::try_from("dir2")?,
            ]),
            None,
            None,
            ChangesetFileOrdering::Ordered {
                after: Some(MPath::try_from("dir1/subdir1/subsubdir1/file_1")?),
            },
        )
        .await?
        .try_collect()
        .await?;
    let expected_files = vec![MPath::try_from("dir2/file_1_in_dir2")?];
    assert_eq!(files, expected_files);

    // Basenames
    let mut files: Vec<_> = cs
        .find_files_unordered(None, Some(vec![String::from("file_1")]))
        .await?
        .try_collect()
        .await?;
    files.sort();
    let expected_files = vec![
        MPath::try_from("dir1/subdir1/file_1")?,
        MPath::try_from("dir1/subdir1/subsubdir1/file_1")?,
        MPath::try_from("dir1/subdir1/subsubdir2/file_1")?,
    ];
    assert_eq!(files, expected_files);

    // Basenames ordered
    let files: Vec<_> = cs
        .find_files(
            None,
            Some(vec![String::from("file_1")]),
            None,
            ChangesetFileOrdering::Ordered {
                after: Some(MPath::try_from("")?),
            },
        )
        .await?
        .try_collect()
        .await?;
    assert_eq!(files, expected_files);

    // Basenames ordered after
    let files: Vec<_> = cs
        .find_files(
            None,
            Some(vec![String::from("file_1")]),
            None,
            ChangesetFileOrdering::Ordered {
                after: Some(MPath::try_from("dir1/subdir1/subsubdir1/file_1")?),
            },
        )
        .await?
        .try_collect()
        .await?;
    let expected_files = vec![MPath::try_from("dir1/subdir1/subsubdir2/file_1")?];
    assert_eq!(files, expected_files);

    // Basenames and Prefixes
    let mut files: Vec<_> = cs
        .find_files_unordered(
            Some(vec![
                MPath::try_from("dir1/subdir1/subsubdir2")?,
                MPath::try_from("dir2")?,
            ]),
            Some(vec![String::from("file_2"), String::from("file_1_in_dir2")]),
        )
        .await?
        .try_collect()
        .await?;
    files.sort();
    let expected_files = vec![
        MPath::try_from("dir1/subdir1/subsubdir2/file_2")?,
        MPath::try_from("dir2/file_1_in_dir2")?,
    ];
    assert_eq!(files, expected_files);

    // Basenames and Prefixes ordered
    let mut files: Vec<_> = cs
        .find_files(
            Some(vec![
                MPath::try_from("dir1/subdir1/subsubdir2")?,
                MPath::try_from("dir2")?,
            ]),
            Some(vec![String::from("file_2"), String::from("file_1_in_dir2")]),
            None,
            ChangesetFileOrdering::Ordered {
                after: Some(MPath::try_from("")?),
            },
        )
        .await?
        .try_collect()
        .await?;
    let expected_files = vec![
        MPath::try_from("dir1/subdir1/subsubdir2/file_2")?,
        MPath::try_from("dir2/file_1_in_dir2")?,
    ];
    files.sort();
    assert_eq!(files, expected_files);

    // Basenames and Prefixes ordered after
    let files: Vec<_> = cs
        .find_files(
            Some(vec![
                MPath::try_from("dir1/subdir1/subsubdir2")?,
                MPath::try_from("dir2")?,
            ]),
            Some(vec![String::from("file_2"), String::from("file_1_in_dir2")]),
            None,
            ChangesetFileOrdering::Ordered {
                after: Some(MPath::try_from("dir1a/file_1_in_dir2")?),
            },
        )
        .await?
        .try_collect()
        .await?;
    let expected_files = vec![MPath::try_from("dir2/file_1_in_dir2")?];
    assert_eq!(files, expected_files);

    // Suffixes
    let mut files: Vec<_> = cs
        .find_files(
            None,
            None,
            Some(vec![String::from("_1"), String::from("_2")]),
            ChangesetFileOrdering::Unordered,
        )
        .await?
        .try_collect()
        .await?;
    files.sort();
    let expected_files = vec![
        MPath::try_from("dir1/subdir1/file_1")?,
        MPath::try_from("dir1/subdir1/subsubdir1/file_1")?,
        MPath::try_from("dir1/subdir1/subsubdir2/file_1")?,
        MPath::try_from("dir1/subdir1/subsubdir2/file_2")?,
    ];
    assert_eq!(files, expected_files);

    // Suffixes, ordered
    let files: Vec<_> = cs
        .find_files(
            None,
            None,
            Some(vec![String::from("_1"), String::from("_2")]),
            ChangesetFileOrdering::Ordered {
                after: Some(MPath::try_from("")?),
            },
        )
        .await?
        .try_collect()
        .await?;
    let expected_files = vec![
        MPath::try_from("dir1/subdir1/file_1")?,
        MPath::try_from("dir1/subdir1/subsubdir1/file_1")?,
        MPath::try_from("dir1/subdir1/subsubdir2/file_1")?,
        MPath::try_from("dir1/subdir1/subsubdir2/file_2")?,
    ];
    assert_eq!(files, expected_files);

    // Suffixes, ordered after
    let files: Vec<_> = cs
        .find_files(
            None,
            None,
            Some(vec![String::from("_1"), String::from("_2")]),
            ChangesetFileOrdering::Ordered {
                after: Some(MPath::try_from("dir1/subdir1/subsubdir1/file_1")?),
            },
        )
        .await?
        .try_collect()
        .await?;
    let expected_files = vec![
        MPath::try_from("dir1/subdir1/subsubdir2/file_1")?,
        MPath::try_from("dir1/subdir1/subsubdir2/file_2")?,
    ];
    assert_eq!(files, expected_files);

    // Suffixes, prefixes
    let mut files: Vec<_> = cs
        .find_files(
            Some(vec![
                MPath::try_from("dir1/subdir1/subsubdir1")?,
                MPath::try_from("dir1/subdir1/subsubdir2")?,
            ]),
            None,
            Some(vec![String::from("1"), String::from("2")]),
            ChangesetFileOrdering::Unordered,
        )
        .await?
        .try_collect()
        .await?;
    files.sort();
    let expected_files = vec![
        MPath::try_from("dir1/subdir1/subsubdir1/file_1")?,
        MPath::try_from("dir1/subdir1/subsubdir2/file_1")?,
        MPath::try_from("dir1/subdir1/subsubdir2/file_2")?,
    ];
    assert_eq!(files, expected_files);

    // Suffixes, prefixes, ordered
    let files: Vec<_> = cs
        .find_files(
            Some(vec![
                MPath::try_from("dir1/subdir1/subsubdir1")?,
                MPath::try_from("dir1/subdir1/subsubdir2")?,
            ]),
            None,
            Some(vec![String::from("1"), String::from("2")]),
            ChangesetFileOrdering::Ordered {
                after: Some(MPath::try_from("")?),
            },
        )
        .await?
        .try_collect()
        .await?;
    let expected_files = vec![
        MPath::try_from("dir1/subdir1/subsubdir1/file_1")?,
        MPath::try_from("dir1/subdir1/subsubdir2/file_1")?,
        MPath::try_from("dir1/subdir1/subsubdir2/file_2")?,
    ];
    assert_eq!(files, expected_files);

    // Suffixes, prefixes, ordered after
    let files: Vec<_> = cs
        .find_files(
            Some(vec![
                MPath::try_from("dir1/subdir1/subsubdir1")?,
                MPath::try_from("dir1/subdir1/subsubdir2")?,
            ]),
            None,
            Some(vec![String::from("1"), String::from("2")]),
            ChangesetFileOrdering::Ordered {
                after: Some(MPath::try_from("dir1/subdir1/subsubdir1/file_1")?),
            },
        )
        .await?
        .try_collect()
        .await?;
    let expected_files = vec![
        MPath::try_from("dir1/subdir1/subsubdir2/file_1")?,
        MPath::try_from("dir1/subdir1/subsubdir2/file_2")?,
    ];
    assert_eq!(files, expected_files);

    // Suffixes, basenames
    let mut files: Vec<_> = cs
        .find_files(
            None,
            Some(vec![String::from("file_1_in_dir2")]),
            Some(vec![String::from("1")]),
            ChangesetFileOrdering::Unordered,
        )
        .await?
        .try_collect()
        .await?;
    files.sort();
    let expected_files = vec![
        MPath::try_from("1")?,
        MPath::try_from("dir1/file_1_in_dir1")?,
        MPath::try_from("dir1/file_2_in_dir1")?,
        MPath::try_from("dir1/subdir1/file_1")?,
        MPath::try_from("dir1/subdir1/subsubdir1/file_1")?,
        MPath::try_from("dir1/subdir1/subsubdir2/file_1")?,
        MPath::try_from("dir2/file_1_in_dir2")?,
    ];
    assert_eq!(files, expected_files);

    // Suffixes, basenames, ordered
    let files: Vec<_> = cs
        .find_files(
            None,
            Some(vec![String::from("file_1_in_dir2")]),
            Some(vec![String::from("1")]),
            ChangesetFileOrdering::Ordered {
                after: Some(MPath::try_from("")?),
            },
        )
        .await?
        .try_collect()
        .await?;
    // BSSM have different but consistent orders. BSSMv3 produces the same order as BSSM.
    let expected_files =
        if justknobs::eval("scm/mononoke:enable_bssm_v3", None, None).unwrap_or_default() {
            vec![
                MPath::try_from("1")?,
                MPath::try_from("dir1/subdir1/file_1")?,
                MPath::try_from("dir1/subdir1/subsubdir1/file_1")?,
                MPath::try_from("dir1/subdir1/subsubdir2/file_1")?,
                MPath::try_from("dir1/file_1_in_dir1")?,
                MPath::try_from("dir1/file_2_in_dir1")?,
                MPath::try_from("dir2/file_1_in_dir2")?,
            ]
        } else {
            vec![
                MPath::try_from("1")?,
                MPath::try_from("dir1/file_1_in_dir1")?,
                MPath::try_from("dir1/file_2_in_dir1")?,
                MPath::try_from("dir1/subdir1/file_1")?,
                MPath::try_from("dir1/subdir1/subsubdir1/file_1")?,
                MPath::try_from("dir1/subdir1/subsubdir2/file_1")?,
                MPath::try_from("dir2/file_1_in_dir2")?,
            ]
        };

    assert_eq!(files, expected_files);
    // Suffixes, basenames, ordered after
    let files: Vec<_> = cs
        .find_files(
            None,
            Some(vec![String::from("file_1_in_dir2")]),
            Some(vec![String::from("1")]),
            ChangesetFileOrdering::Ordered {
                after: Some(MPath::try_from("dir1/subdir1/subsubdir1/file_1")?),
            },
        )
        .await?
        .try_collect()
        .await?;
    let expected_files =
        if justknobs::eval("scm/mononoke:enable_bssm_v3", None, None).unwrap_or_default() {
            vec![
                MPath::try_from("dir1/subdir1/subsubdir2/file_1")?,
                MPath::try_from("dir1/file_1_in_dir1")?,
                MPath::try_from("dir1/file_2_in_dir1")?,
                MPath::try_from("dir2/file_1_in_dir2")?,
            ]
        } else {
            vec![
                MPath::try_from("dir1/subdir1/subsubdir2/file_1")?,
                MPath::try_from("dir2/file_1_in_dir2")?,
            ]
        };
    assert_eq!(files, expected_files);
    // Suffixes, basenames, prefixes
    let mut files: Vec<_> = cs
        .find_files(
            Some(vec![
                MPath::try_from("dir1/subdir1/subsubdir2")?,
                MPath::try_from("dir2")?,
            ]),
            Some(vec![String::from("file_1_in_dir2")]),
            Some(vec![String::from("1")]),
            ChangesetFileOrdering::Unordered,
        )
        .await?
        .try_collect()
        .await?;
    files.sort();
    let expected_files = vec![
        MPath::try_from("dir1/subdir1/subsubdir2/file_1")?,
        MPath::try_from("dir2/file_1_in_dir2")?,
    ];
    assert_eq!(files, expected_files);

    // Suffixes, basenames, prefixes, ordered
    let files: Vec<_> = cs
        .find_files(
            Some(vec![
                MPath::try_from("dir1/subdir1/subsubdir2")?,
                MPath::try_from("dir2")?,
            ]),
            Some(vec![String::from("file_1_in_dir2")]),
            Some(vec![String::from("1")]),
            ChangesetFileOrdering::Ordered {
                after: Some(MPath::try_from("")?),
            },
        )
        .await?
        .try_collect()
        .await?;
    let expected_files = vec![
        MPath::try_from("dir1/subdir1/subsubdir2/file_1")?,
        MPath::try_from("dir2/file_1_in_dir2")?,
    ];
    assert_eq!(files, expected_files);

    // Suffixes, basenames, prefixes, ordered after
    let files: Vec<_> = cs
        .find_files(
            Some(vec![
                MPath::try_from("dir1/subdir1/subsubdir2")?,
                MPath::try_from("dir2")?,
            ]),
            Some(vec![String::from("file_1_in_dir2")]),
            Some(vec![String::from("1")]),
            ChangesetFileOrdering::Ordered {
                after: Some(MPath::try_from("dir1/subdir1/subsubdir2/file_1")?),
            },
        )
        .await?
        .try_collect()
        .await?;
    let expected_files = vec![MPath::try_from("dir2/file_1_in_dir2")?];
    assert_eq!(files, expected_files);

    Ok(())
}

#[fbinit::test]
async fn commit_find_files_with_bssm_v3(fb: FacebookInit) {
    let justknobs = JustKnobsInMemory::new(hashmap! {
        "scm/mononoke:enable_bssm_v3".to_string() => KnobVal::Bool(true),
        "scm/mononoke:enable_bssm_v3_suffix_query".to_string() => KnobVal::Bool(true),
    });

    with_just_knobs_async(justknobs, commit_find_files_impl(fb).boxed())
        .await
        .unwrap();
}

#[fbinit::test]
async fn commit_find_files_without_bssm(fb: FacebookInit) {
    let justknobs = JustKnobsInMemory::new(hashmap! {
        "scm/mononoke:enable_bssm_v3".to_string() => KnobVal::Bool(false),
        "scm/mononoke:enable_bssm_v3_suffix_query".to_string() => KnobVal::Bool(false),
    });

    with_just_knobs_async(justknobs, commit_find_files_impl(fb).boxed())
        .await
        .unwrap();
}
