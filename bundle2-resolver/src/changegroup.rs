// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use futures::{Async, Future, Poll, Stream};
use futures_ext::{BoxStream, StreamExt};
use slog::Logger;

use errors::*;
use mercurial_bundles::changegroup::{CgDeltaChunk, Part, Section};
use mercurial_types::MPath;

#[derive(Debug, Eq, PartialEq)]
pub struct Changeset {
    chunk: CgDeltaChunk,
}

#[derive(Debug, Eq, PartialEq)]
pub struct Filelog {
    path: MPath,
    chunk: CgDeltaChunk,
}

pub fn split_changegroup<S>(
    _logger: Logger,
    cg2s: S,
) -> (BoxStream<Changeset, Error>, BoxStream<Filelog, Error>)
where
    S: Stream<Item = Part, Error = Error> + Send + 'static,
{
    let cg2s = CheckEnd::new(cg2s);

    let (changesets, remainder) = cg2s.take_while(|part| match part {
        &Part::CgChunk(Section::Changeset, _) => Ok(true),
        &Part::SectionEnd(Section::Changeset) => Ok(false),
        bad => bail_msg!("Unexpected changegroup part: {:?}", bad),
    }).return_remainder();

    let changesets = changesets
        .and_then(|part| match part {
            Part::CgChunk(Section::Changeset, chunk) => Ok(Changeset { chunk }),
            bad => bail_msg!("Unexpected changegroup part: {:?}", bad),
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
                bad => bail_msg!("Unexpected changegroup part: {:?}", bad),
            }
        })
        .and_then({
            let mut seen_filelog = None;
            move |part| {
                match seen_filelog.take() {
                    None => match part {
                        Part::CgChunk(Section::Filelog(path), chunk) => {
                            seen_filelog = Some(path.clone());
                            Ok(Some(Filelog { path, chunk }))
                        }
                        Part::End => Ok(None), // this is covered by CheckEnd wrapper
                        bad => bail_msg!("Unexpected changegroup part: {:?}", bad),
                    },
                    Some(path) => match &part {
                        &Part::SectionEnd(Section::Filelog(ref p)) if &path == p => {
                            seen_filelog = None;
                            Ok(None)
                        }
                        bad => bail_msg!("Unexpected changegroup part: {:?}", bad),
                    },
                }
            }
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
                ensure_msg!(!self.seen_end, "More than one Part::End noticed");
                self.seen_end = true;
            }
            &None => ensure_msg!(
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
    use slog::Discard;

    pub fn split_changegroup_no_logger<S>(
        cg2s: S,
    ) -> (BoxStream<Changeset, Error>, BoxStream<Filelog, Error>)
    where
        S: Stream<Item = Part, Error = Error> + Send + 'static,
    {
        split_changegroup(Logger::root(Discard, o!()), cg2s)
    }

    fn check_splitting<S, I, J>(cg2s: S, exp_cs: I, exp_fs: J) -> bool
    where
        S: Stream<Item = Part, Error = Error> + Send + 'static,
        I: IntoIterator<Item = Changeset>,
        J: IntoIterator<Item = Filelog>,
    {
        let (cs, fs) = split_changegroup_no_logger(cg2s);

        let cs: Vec<Changeset> = cs.collect().wait().expect("error in changesets");
        let fs: Vec<Filelog> = fs.collect().wait().expect("error in changesets");

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
                ].into_iter(),
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
                vec![Changeset { chunk: c.clone() }],
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
                    Filelog {
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
                vec![Changeset { chunk: c.clone() }],
                vec![
                    Filelog {
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
            f1_p: MPath,
            f2: CgDeltaChunk,
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
                        Part::SectionEnd(Section::Filelog(f1_p.clone())),
                        Part::CgChunk(Section::Filelog(f2_p.clone()), f2.clone()),
                        Part::SectionEnd(Section::Filelog(f2_p.clone())),
                        Part::End,
                    ].into_iter(),
                ),
                vec![Changeset { chunk: c1 }, Changeset { chunk: c2 }],
                vec![
                    Filelog {
                        path: f1_p,
                        chunk: f1,
                    },
                    Filelog {
                        path: f2_p,
                        chunk: f2,
                    },
                ],
            )
        }

        fn splitting_error_filelog_end(f: CgDeltaChunk, f1_p: MPath, f2_p: MPath) -> bool {
            {
                let (cs, fs) = split_changegroup_no_logger(iter_ok(
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
                let (cs, fs) = split_changegroup_no_logger(iter_ok(
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
                let (cs, fs) = split_changegroup_no_logger(iter_ok(
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
            let (cs, fs) = split_changegroup_no_logger(iter_ok(
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

            equal(cs.collect().wait().unwrap(), vec![Changeset { chunk: c }])
                && fs.collect().wait().is_err()
        }
    }

    #[test]
    fn splitting_error_two_ends() {
        {
            let (cs, fs) = split_changegroup_no_logger(iter_ok(
                vec![
                    Part::SectionEnd(Section::Changeset),
                    Part::SectionEnd(Section::Changeset),
                    Part::SectionEnd(Section::Manifest),
                    Part::End,
                ].into_iter(),
            ));

            assert_equal(cs.collect().wait().unwrap(), vec![]);
            assert!(fs.collect().wait().is_err());
        }

        {
            let (cs, fs) = split_changegroup_no_logger(iter_ok(
                vec![
                    Part::SectionEnd(Section::Changeset),
                    Part::SectionEnd(Section::Manifest),
                    Part::SectionEnd(Section::Manifest),
                    Part::End,
                ].into_iter(),
            ));

            assert_equal(cs.collect().wait().unwrap(), vec![]);
            assert!(fs.collect().wait().is_err());
        }

        {
            let (cs, fs) = split_changegroup_no_logger(iter_ok(
                vec![
                    Part::SectionEnd(Section::Changeset),
                    Part::SectionEnd(Section::Manifest),
                    Part::End,
                    Part::End,
                ].into_iter(),
            ));

            assert_equal(cs.collect().wait().unwrap(), vec![]);
            assert!(fs.collect().wait().is_err());
        }
    }

    #[test]
    fn splitting_error_missing_end() {
        {
            let (cs, fs) = split_changegroup_no_logger(iter_ok(
                vec![Part::SectionEnd(Section::Manifest), Part::End].into_iter(),
            ));

            assert!(cs.collect().wait().is_err());
            assert!(fs.collect().wait().is_err());
        }

        {
            let (cs, fs) = split_changegroup_no_logger(iter_ok(
                vec![Part::SectionEnd(Section::Changeset), Part::End].into_iter(),
            ));

            assert_equal(cs.collect().wait().unwrap(), vec![]);
            assert!(fs.collect().wait().is_err());
        }

        {
            let (cs, fs) = split_changegroup_no_logger(iter_ok(
                vec![
                    Part::SectionEnd(Section::Changeset),
                    Part::SectionEnd(Section::Manifest),
                ].into_iter(),
            ));

            assert_equal(cs.collect().wait().unwrap(), vec![]);
            assert!(fs.collect().wait().is_err());
        }
    }
}
