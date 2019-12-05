/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use failure_ext::{bail, ensure};
use futures::{try_ready, Async, Future, Poll, Stream};
use futures_ext::{BoxStream, StreamExt};

use mercurial_bundles::changegroup::{Part, Section};

use crate::changegroup::changeset::ChangesetDeltaed;
use crate::changegroup::filelog::FilelogDeltaed;
use crate::errors::*;

pub fn split_changegroup<S>(
    cg2s: S,
) -> (
    BoxStream<ChangesetDeltaed, Error>,
    BoxStream<FilelogDeltaed, Error>,
)
where
    S: Stream<Item = Part, Error = Error> + Send + 'static,
{
    let cg2s = CheckEnd::new(cg2s);

    let (changesets, remainder) = cg2s
        .take_while(|part| match part {
            &Part::CgChunk(Section::Changeset, _) => Ok(true),
            &Part::SectionEnd(Section::Changeset) => Ok(false),
            bad => bail!("Expected Changeset chunk or end, found: {:?}", bad),
        })
        .return_remainder();

    let changesets = changesets
        .and_then(|part| match part {
            Part::CgChunk(Section::Changeset, chunk) => Ok(ChangesetDeltaed { chunk }),
            bad => bail!("Expected Changeset chunk, found: {:?}", bad),
        })
        .map_err(|err| {
            err.context("While extracting Changesets from Changegroup")
                .into()
        })
        .boxify();

    let filelogs = remainder
        .from_err()
        .map(|take_while_stream| take_while_stream.into_inner())
        .flatten_stream()
        .skip_while({
            let mut seen_manifest_end = false;
            move |part| match part {
                &Part::SectionEnd(Section::Manifest) if !seen_manifest_end => {
                    seen_manifest_end = true;
                    Ok(true)
                }
                _ if seen_manifest_end => Ok(false),
                bad => bail!("Expected Manifest end, found: {:?}", bad),
            }
        })
        .map_err(|err| {
            err.context("While skipping Manifests in Changegroup")
                .into()
        })
        .and_then({
            let mut seen_path = None;
            move |part| {
                if let &Some(ref seen_path) = &seen_path {
                    match &part {
                        &Part::CgChunk(Section::Filelog(ref path), _)
                        | &Part::SectionEnd(Section::Filelog(ref path)) => {
                            if seen_path != path {
                                bail!(
                                    "Mismatched path found {0} ({0:?}) != {1} ({1:?}), for part: {2:?}",
                                    seen_path,
                                    path,
                                    part
                                );
                            }
                        }
                        _ => (), // Handled in the next pattern-match
                    }
                }

                match part {
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
                    bad => {
                        if seen_path.is_some() {
                            bail!(
                                "Expected Filelog chunk or end, seen_path was {:?}, found: {:?}",
                                seen_path,
                                bad
                            )
                        } else {
                            bail!(
                                "Expected Filelog chunk or Part::End, seen_path was {:?}, found: {:?}",
                                seen_path,
                                bad
                            )
                        }
                    }
                }
            }
        })
        .map_err(|err| {
            err.context("While extracting Filelogs from Changegroup")
                .into()
        })
        .filter_map(|x| x)
        .boxify();

    (changesets, filelogs)
}

/// Wrapper for Stream of Part that is supposed to ensure that there is exactly one Part::End in
/// the stream and that it is at the end of it
struct CheckEnd<S> {
    cg2s: S,
    seen_end: bool,
}

impl<S> CheckEnd<S>
where
    S: Stream<Item = Part, Error = Error> + Send + 'static,
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
    S: Stream<Item = Part, Error = Error> + Send + 'static,
{
    type Item = S::Item;
    type Error = S::Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        let part = try_ready!(self.cg2s.poll());
        match &part {
            &Some(Part::End) => {
                ensure!(!self.seen_end, "More than one Part::End noticed");
                self.seen_end = true;
            }
            &None => ensure!(
                self.seen_end,
                "End of stream reached, but no Part::End noticed"
            ),
            _ => (), // all good, proceed
        }
        Ok(Async::Ready(part))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use futures::stream::iter_ok;
    use itertools::{assert_equal, equal};
    use quickcheck::quickcheck;

    use mercurial_bundles::changegroup::CgDeltaChunk;
    use mercurial_types::MPath;

    fn check_splitting<S, I, J>(cg2s: S, exp_cs: I, exp_fs: J) -> bool
    where
        S: Stream<Item = Part, Error = Error> + Send + 'static,
        I: IntoIterator<Item = ChangesetDeltaed>,
        J: IntoIterator<Item = FilelogDeltaed>,
    {
        let (cs, fs) = split_changegroup(cg2s);

        let cs = cs.collect().wait().expect("error in changesets");
        let fs = fs.collect().wait().expect("error in changesets");

        equal(cs, exp_cs) && equal(fs, exp_fs)
    }

    #[test]
    fn splitting_empty() {
        assert!(check_splitting(
            iter_ok(
                vec![
                    Part::SectionEnd(Section::Changeset),
                    Part::SectionEnd(Section::Manifest),
                    Part::End,
                ]
                .into_iter(),
            ),
            vec![],
            vec![],
        ))
    }

    quickcheck! {
        fn splitting_minimal(c: CgDeltaChunk, f: CgDeltaChunk, f_p: MPath) -> bool {
            check_splitting(
                iter_ok(
                    vec![
                        Part::CgChunk(Section::Changeset, c.clone()),
                        Part::SectionEnd(Section::Changeset),
                        Part::SectionEnd(Section::Manifest),
                        Part::End,
                    ].into_iter(),
                ),
                vec![ChangesetDeltaed { chunk: c.clone() }],
                vec![],
            ) && check_splitting(
                iter_ok(
                    vec![
                        Part::SectionEnd(Section::Changeset),
                        Part::SectionEnd(Section::Manifest),
                        Part::CgChunk(Section::Filelog(f_p.clone()), f.clone()),
                        Part::SectionEnd(Section::Filelog(f_p.clone())),
                        Part::End,
                    ].into_iter(),
                ),
                vec![],
                vec![
                    FilelogDeltaed {
                        path: f_p.clone(),
                        chunk: f.clone(),
                    },
                ],
            ) && check_splitting(
                iter_ok(
                    vec![
                        Part::CgChunk(Section::Changeset, c.clone()),
                        Part::SectionEnd(Section::Changeset),
                        Part::SectionEnd(Section::Manifest),
                        Part::CgChunk(Section::Filelog(f_p.clone()), f.clone()),
                        Part::SectionEnd(Section::Filelog(f_p.clone())),
                        Part::End,
                    ].into_iter(),
                ),
                vec![ChangesetDeltaed { chunk: c.clone() }],
                vec![
                    FilelogDeltaed {
                        path: f_p.clone(),
                        chunk: f.clone(),
                    },
                ],
            )
        }

        fn splitting_complex(
            c1: CgDeltaChunk,
            c2: CgDeltaChunk,
            f1: CgDeltaChunk,
            f1_bis: CgDeltaChunk,
            f1_p: MPath,
            f2: CgDeltaChunk,
            f2_bis: CgDeltaChunk,
            f2_p: MPath
        ) -> bool {
            check_splitting(
                iter_ok(
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
                    ].into_iter(),
                ),
                vec![ChangesetDeltaed { chunk: c1 }, ChangesetDeltaed { chunk: c2 }],
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
        }

        fn splitting_error_filelog_end(f: CgDeltaChunk, f1_p: MPath, f2_p: MPath) -> bool {
            {
                let (cs, fs) = split_changegroup(iter_ok(
                    vec![
                        Part::SectionEnd(Section::Changeset),
                        Part::SectionEnd(Section::Manifest),
                        Part::SectionEnd(Section::Filelog(f1_p.clone())),
                        Part::End,
                    ].into_iter(),
                ));

                assert_equal(cs.collect().wait().unwrap(), vec![]);
                assert!(fs.collect().wait().is_err());
            }

            {
                let (cs, fs) = split_changegroup(iter_ok(
                    vec![
                        Part::SectionEnd(Section::Changeset),
                        Part::SectionEnd(Section::Manifest),
                        Part::CgChunk(Section::Filelog(f1_p.clone()), f.clone()),
                        Part::End,
                    ].into_iter(),
                ));

                assert_equal(cs.collect().wait().unwrap(), vec![]);
                assert!(fs.collect().wait().is_err());
            }

            {
                let (cs, fs) = split_changegroup(iter_ok(
                    vec![
                        Part::SectionEnd(Section::Changeset),
                        Part::SectionEnd(Section::Manifest),
                        Part::CgChunk(Section::Filelog(f1_p.clone()), f.clone()),
                        Part::SectionEnd(Section::Filelog(f2_p.clone())),
                        Part::End,
                    ].into_iter(),
                ));

                assert_equal(cs.collect().wait().unwrap(), vec![]);
                assert!(f1_p == f2_p || fs.collect().wait().is_err());
            }

            true
        }

        fn splitting_error_manifest(
            c: CgDeltaChunk,
            m: CgDeltaChunk,
            f: CgDeltaChunk,
            f_p: MPath
        ) -> bool {
            let (cs, fs) = split_changegroup(iter_ok(
                vec![
                    Part::CgChunk(Section::Changeset, c.clone()),
                    Part::SectionEnd(Section::Changeset),
                    Part::CgChunk(Section::Manifest, m.clone()),
                    Part::SectionEnd(Section::Manifest),
                    Part::CgChunk(Section::Filelog(f_p.clone()), f.clone()),
                    Part::SectionEnd(Section::Filelog(f_p.clone())),
                    Part::End,
                ].into_iter(),
            ));

            equal(cs.collect().wait().unwrap(), vec![ChangesetDeltaed { chunk: c }])
                && fs.collect().wait().is_err()
        }
    }

    #[test]
    fn splitting_error_two_ends() {
        {
            let (cs, fs) = split_changegroup(iter_ok(
                vec![
                    Part::SectionEnd(Section::Changeset),
                    Part::SectionEnd(Section::Changeset),
                    Part::SectionEnd(Section::Manifest),
                    Part::End,
                ]
                .into_iter(),
            ));

            assert_equal(cs.collect().wait().unwrap(), vec![]);
            assert!(fs.collect().wait().is_err());
        }

        {
            let (cs, fs) = split_changegroup(iter_ok(
                vec![
                    Part::SectionEnd(Section::Changeset),
                    Part::SectionEnd(Section::Manifest),
                    Part::SectionEnd(Section::Manifest),
                    Part::End,
                ]
                .into_iter(),
            ));

            assert_equal(cs.collect().wait().unwrap(), vec![]);
            assert!(fs.collect().wait().is_err());
        }

        {
            let (cs, fs) = split_changegroup(iter_ok(
                vec![
                    Part::SectionEnd(Section::Changeset),
                    Part::SectionEnd(Section::Manifest),
                    Part::End,
                    Part::End,
                ]
                .into_iter(),
            ));

            assert_equal(cs.collect().wait().unwrap(), vec![]);
            assert!(fs.collect().wait().is_err());
        }
    }

    #[test]
    fn splitting_error_missing_end() {
        {
            let (cs, fs) = split_changegroup(iter_ok(
                vec![Part::SectionEnd(Section::Manifest), Part::End].into_iter(),
            ));

            assert!(cs.collect().wait().is_err());
            assert!(fs.collect().wait().is_err());
        }

        {
            let (cs, fs) = split_changegroup(iter_ok(
                vec![Part::SectionEnd(Section::Changeset), Part::End].into_iter(),
            ));

            assert_equal(cs.collect().wait().unwrap(), vec![]);
            assert!(fs.collect().wait().is_err());
        }

        {
            let (cs, fs) = split_changegroup(iter_ok(
                vec![
                    Part::SectionEnd(Section::Changeset),
                    Part::SectionEnd(Section::Manifest),
                ]
                .into_iter(),
            ));

            assert_equal(cs.collect().wait().unwrap(), vec![]);
            assert!(fs.collect().wait().is_err());
        }
    }
}
