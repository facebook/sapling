/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::anyhow;
use anyhow::Result;
use futures::future;
use futures::ready;
use futures::task::Context;
use futures::task::Poll;
use futures::Stream;
use futures::StreamExt;
use futures::TryFutureExt;
use futures::TryStreamExt;
use futures_ext::FbStreamExt;
use pin_project::pin_project;
use std::pin::Pin;

use mercurial_bundles::changegroup::Part;
use mercurial_bundles::changegroup::Section;

use crate::changegroup::changeset::ChangesetDeltaed;
use crate::changegroup::filelog::FilelogDeltaed;

pub(crate) fn split_changegroup(
    cg2s: impl Stream<Item = Result<Part>> + Send + 'static,
) -> (
    impl Stream<Item = Result<ChangesetDeltaed>>,
    impl Stream<Item = Result<FilelogDeltaed>>,
) {
    let cg2s = CheckEnd::new(cg2s).boxed();

    let (changesets, remainder) = cg2s
        .try_take_while(|part| {
            future::ready(match part {
                &Part::CgChunk(Section::Changeset, _) => Ok(true),
                &Part::SectionEnd(Section::Changeset) => Ok(false),
                bad => Err(anyhow!("Expected Changeset chunk or end, found: {:?}", bad)),
            })
        })
        .return_remainder();

    let changesets = changesets
        .and_then(|part| {
            future::ready(match part {
                Part::CgChunk(Section::Changeset, chunk) => Ok(ChangesetDeltaed { chunk }),
                bad => Err(anyhow!("Expected Changeset chunk, found: {:?}", bad)),
            })
        })
        .map_err(|err| err.context("While extracting Changesets from Changegroup"));

    let filelogs = remainder
        .err_into()
        .map_ok(|take_while_stream| take_while_stream.into_inner())
        .try_flatten_stream()
        .try_skip_while({
            let mut seen_manifest_end = false;
            move |part| future::ready(match part {
                &Part::SectionEnd(Section::Manifest) if !seen_manifest_end => {
                    seen_manifest_end = true;
                    Ok(true)
                }
                _ if seen_manifest_end => Ok(false),
                bad => Err(anyhow!("Expected Manifest end, found: {:?}", bad)),
            })
        })
        .map_err(|err| {
            err.context("While skipping Manifests in Changegroup")
        })
        .try_filter_map({
            let mut seen_path = None;
            move |part| {
                if let &Some(ref seen_path) = &seen_path {
                    match &part {
                        &Part::CgChunk(Section::Filelog(ref path), _)
                        | &Part::SectionEnd(Section::Filelog(ref path)) => {
                            if seen_path != path {
                                return future::ready(Err(anyhow!(
                                    "Mismatched path found {0} ({0:?}) != {1} ({1:?}), for part: {2:?}",
                                    seen_path,
                                    path,
                                    part
                                )));
                            }
                        }
                        _ => (), // Handled in the next pattern-match
                    }
                }

                future::ready(match part {
                    Part::CgChunk(Section::Filelog(path), chunk) => {
                        seen_path = Some(path.clone());
                        Ok(Some(FilelogDeltaed { path, chunk }))
                    }
                    Part::SectionEnd(Section::Filelog(_)) if seen_path.is_some() => {
                        seen_path = None;
                        Ok(None)
                    }
                    Part::SectionEnd(Section::Treemanifest) => Ok(None),
                    // Checking that there is exactly one Part::end is is covered by CheckEnd
                    // wrapper
                    Part::End if seen_path.is_none() => Ok(None),
                    bad => Err(
                        if seen_path.is_some() {
                            anyhow!(
                                "Expected Filelog chunk or end, seen_path was {:?}, found: {:?}",
                                seen_path,
                                bad
                            )
                        } else {
                            anyhow!(
                                "Expected Filelog chunk or Part::End, seen_path was {:?}, found: {:?}",
                                seen_path,
                                bad
                            )
                        }
                    )
                })
            }
        })
        .map_err(|err| {
            err.context("While extracting Filelogs from Changegroup")
        });

    (changesets, filelogs)
}

/// Wrapper for Stream of Part that is supposed to ensure that there is exactly one Part::End in
/// the stream and that it is at the end of it
#[pin_project]
struct CheckEnd<S> {
    #[pin]
    cg2s: S,
    seen_end: bool,
}

impl<S> CheckEnd<S>
where
    S: Stream<Item = Result<Part>>,
{
    fn new(cg2s: S) -> Self {
        Self {
            cg2s,
            seen_end: false,
        }
    }
}

impl<S> Stream for CheckEnd<S>
where
    S: Stream<Item = Result<Part>>,
{
    type Item = S::Item;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        let mut this = self.project();

        let part = ready!(this.cg2s.as_mut().poll_next(cx));
        Poll::Ready((|| {
            match part {
                Some(Ok(Part::End)) => {
                    if *this.seen_end {
                        return Some(Err(anyhow!("More than one Part::End noticed")));
                    }
                    *this.seen_end = true;
                }
                None => {
                    if !*this.seen_end {
                        return Some(Err(anyhow!(
                            "End of stream reached, but no Part::End noticed"
                        )));
                    }
                }
                _ => {} // all good, proceed
            }
            part
        })())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use futures::stream::iter;
    use itertools::assert_equal;
    use itertools::equal;

    use mercurial_bundles::changegroup::CgDeltaChunk;
    use mercurial_types::MPath;

    async fn check_splitting<S, I, J>(cg2s: S, exp_cs: I, exp_fs: J) -> bool
    where
        S: Stream<Item = Result<Part>> + Send + 'static,
        I: IntoIterator<Item = ChangesetDeltaed>,
        J: IntoIterator<Item = FilelogDeltaed>,
    {
        let (cs, fs) = split_changegroup(cg2s);

        let cs = cs
            .try_collect::<Vec<_>>()
            .await
            .expect("error in changesets");
        let fs = fs
            .try_collect::<Vec<_>>()
            .await
            .expect("error in changesets");

        equal(cs, exp_cs) && equal(fs, exp_fs)
    }

    #[tokio::test]
    async fn splitting_empty() {
        assert!(
            check_splitting(
                iter(
                    vec![
                        Part::SectionEnd(Section::Changeset),
                        Part::SectionEnd(Section::Manifest),
                        Part::End,
                    ]
                    .into_iter()
                    .map(Ok),
                ),
                vec![],
                vec![],
            )
            .await
        )
    }

    #[quickcheck_async::tokio]
    async fn splitting_minimal(c: CgDeltaChunk, f: CgDeltaChunk, f_p: MPath) -> bool {
        check_splitting(
            iter(
                vec![
                    Part::CgChunk(Section::Changeset, c.clone()),
                    Part::SectionEnd(Section::Changeset),
                    Part::SectionEnd(Section::Manifest),
                    Part::End,
                ]
                .into_iter()
                .map(Ok),
            ),
            vec![ChangesetDeltaed { chunk: c.clone() }],
            vec![],
        )
        .await
            && check_splitting(
                iter(
                    vec![
                        Part::SectionEnd(Section::Changeset),
                        Part::SectionEnd(Section::Manifest),
                        Part::CgChunk(Section::Filelog(f_p.clone()), f.clone()),
                        Part::SectionEnd(Section::Filelog(f_p.clone())),
                        Part::End,
                    ]
                    .into_iter()
                    .map(Ok),
                ),
                vec![],
                vec![FilelogDeltaed {
                    path: f_p.clone(),
                    chunk: f.clone(),
                }],
            )
            .await
            && check_splitting(
                iter(
                    vec![
                        Part::CgChunk(Section::Changeset, c.clone()),
                        Part::SectionEnd(Section::Changeset),
                        Part::SectionEnd(Section::Manifest),
                        Part::CgChunk(Section::Filelog(f_p.clone()), f.clone()),
                        Part::SectionEnd(Section::Filelog(f_p.clone())),
                        Part::End,
                    ]
                    .into_iter()
                    .map(Ok),
                ),
                vec![ChangesetDeltaed { chunk: c.clone() }],
                vec![FilelogDeltaed {
                    path: f_p.clone(),
                    chunk: f.clone(),
                }],
            )
            .await
    }

    #[quickcheck_async::tokio]
    async fn splitting_complex(
        c1: CgDeltaChunk,
        c2: CgDeltaChunk,
        f1: CgDeltaChunk,
        f1_bis: CgDeltaChunk,
        f1_p: MPath,
        f2: CgDeltaChunk,
        f2_bis: CgDeltaChunk,
        f2_p: MPath,
    ) -> bool {
        check_splitting(
            iter(
                vec![
                    Part::CgChunk(Section::Changeset, c1.clone()),
                    Part::CgChunk(Section::Changeset, c2.clone()),
                    Part::SectionEnd(Section::Changeset),
                    Part::SectionEnd(Section::Manifest),
                    Part::CgChunk(Section::Filelog(f1_p.clone()), f1.clone()),
                    Part::CgChunk(Section::Filelog(f1_p.clone()), f1_bis.clone()),
                    Part::SectionEnd(Section::Filelog(f1_p.clone())),
                    Part::CgChunk(Section::Filelog(f2_p.clone()), f2.clone()),
                    Part::CgChunk(Section::Filelog(f2_p.clone()), f2_bis.clone()),
                    Part::SectionEnd(Section::Filelog(f2_p.clone())),
                    Part::End,
                ]
                .into_iter()
                .map(Ok),
            ),
            vec![
                ChangesetDeltaed { chunk: c1 },
                ChangesetDeltaed { chunk: c2 },
            ],
            vec![
                FilelogDeltaed {
                    path: f1_p.clone(),
                    chunk: f1,
                },
                FilelogDeltaed {
                    path: f1_p,
                    chunk: f1_bis,
                },
                FilelogDeltaed {
                    path: f2_p.clone(),
                    chunk: f2,
                },
                FilelogDeltaed {
                    path: f2_p,
                    chunk: f2_bis,
                },
            ],
        )
        .await
    }

    #[quickcheck_async::tokio]
    async fn splitting_error_filelog_end(f: CgDeltaChunk, f1_p: MPath, f2_p: MPath) -> bool {
        {
            let (cs, fs) = split_changegroup(
                iter(
                    vec![
                        Part::SectionEnd(Section::Changeset),
                        Part::SectionEnd(Section::Manifest),
                        Part::SectionEnd(Section::Filelog(f1_p.clone())),
                        Part::End,
                    ]
                    .into_iter()
                    .map(Ok),
                )
                .boxed(),
            );

            assert_equal(cs.try_collect::<Vec<_>>().await.unwrap(), vec![]);
            assert!(fs.try_collect::<Vec<_>>().await.is_err());
        }

        {
            let (cs, fs) = split_changegroup(
                iter(
                    vec![
                        Part::SectionEnd(Section::Changeset),
                        Part::SectionEnd(Section::Manifest),
                        Part::CgChunk(Section::Filelog(f1_p.clone()), f.clone()),
                        Part::End,
                    ]
                    .into_iter()
                    .map(Ok),
                )
                .boxed(),
            );

            assert_equal(cs.try_collect::<Vec<_>>().await.unwrap(), vec![]);
            assert!(fs.try_collect::<Vec<_>>().await.is_err());
        }

        {
            let (cs, fs) = split_changegroup(
                iter(
                    vec![
                        Part::SectionEnd(Section::Changeset),
                        Part::SectionEnd(Section::Manifest),
                        Part::CgChunk(Section::Filelog(f1_p.clone()), f.clone()),
                        Part::SectionEnd(Section::Filelog(f2_p.clone())),
                        Part::End,
                    ]
                    .into_iter()
                    .map(Ok),
                )
                .boxed(),
            );

            assert_equal(cs.try_collect::<Vec<_>>().await.unwrap(), vec![]);
            assert!(f1_p == f2_p || fs.try_collect::<Vec<_>>().await.is_err());
        }

        true
    }

    #[quickcheck_async::tokio]
    async fn splitting_error_manifest(
        c: CgDeltaChunk,
        m: CgDeltaChunk,
        f: CgDeltaChunk,
        f_p: MPath,
    ) -> bool {
        let (cs, fs) = split_changegroup(
            iter(
                vec![
                    Part::CgChunk(Section::Changeset, c.clone()),
                    Part::SectionEnd(Section::Changeset),
                    Part::CgChunk(Section::Manifest, m.clone()),
                    Part::SectionEnd(Section::Manifest),
                    Part::CgChunk(Section::Filelog(f_p.clone()), f.clone()),
                    Part::SectionEnd(Section::Filelog(f_p.clone())),
                    Part::End,
                ]
                .into_iter()
                .map(Ok),
            )
            .boxed(),
        );

        equal(
            cs.try_collect::<Vec<_>>().await.unwrap(),
            vec![ChangesetDeltaed { chunk: c }],
        ) && fs.try_collect::<Vec<_>>().await.is_err()
    }

    #[tokio::test]
    async fn splitting_error_two_ends() {
        {
            let (cs, fs) = split_changegroup(
                iter(
                    vec![
                        Part::SectionEnd(Section::Changeset),
                        Part::SectionEnd(Section::Changeset),
                        Part::SectionEnd(Section::Manifest),
                        Part::End,
                    ]
                    .into_iter()
                    .map(Ok),
                )
                .boxed(),
            );

            assert_equal(cs.try_collect::<Vec<_>>().await.unwrap(), vec![]);
            assert!(fs.try_collect::<Vec<_>>().await.is_err());
        }

        {
            let (cs, fs) = split_changegroup(
                iter(
                    vec![
                        Part::SectionEnd(Section::Changeset),
                        Part::SectionEnd(Section::Manifest),
                        Part::SectionEnd(Section::Manifest),
                        Part::End,
                    ]
                    .into_iter()
                    .map(Ok),
                )
                .boxed(),
            );

            assert_equal(cs.try_collect::<Vec<_>>().await.unwrap(), vec![]);
            assert!(fs.try_collect::<Vec<_>>().await.is_err());
        }

        {
            let (cs, fs) = split_changegroup(
                iter(
                    vec![
                        Part::SectionEnd(Section::Changeset),
                        Part::SectionEnd(Section::Manifest),
                        Part::End,
                        Part::End,
                    ]
                    .into_iter()
                    .map(Ok),
                )
                .boxed(),
            );

            assert_equal(cs.try_collect::<Vec<_>>().await.unwrap(), vec![]);
            assert!(fs.try_collect::<Vec<_>>().await.is_err());
        }
    }

    #[tokio::test]
    async fn splitting_error_missing_end() {
        {
            let (cs, fs) = split_changegroup(
                iter(
                    vec![Part::SectionEnd(Section::Manifest), Part::End]
                        .into_iter()
                        .map(Ok),
                )
                .boxed(),
            );

            assert!(cs.try_collect::<Vec<_>>().await.is_err());
            assert!(fs.try_collect::<Vec<_>>().await.is_err());
        }

        {
            let (cs, fs) = split_changegroup(
                iter(
                    vec![Part::SectionEnd(Section::Changeset), Part::End]
                        .into_iter()
                        .map(Ok),
                )
                .boxed(),
            );

            assert_equal(cs.try_collect::<Vec<_>>().await.unwrap(), vec![]);
            assert!(fs.try_collect::<Vec<_>>().await.is_err());
        }

        {
            let (cs, fs) = split_changegroup(
                iter(
                    vec![
                        Part::SectionEnd(Section::Changeset),
                        Part::SectionEnd(Section::Manifest),
                    ]
                    .into_iter()
                    .map(Ok),
                )
                .boxed(),
            );

            assert_equal(cs.try_collect::<Vec<_>>().await.unwrap(), vec![]);
            assert!(fs.try_collect::<Vec<_>>().await.is_err());
        }
    }
}
