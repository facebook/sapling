/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use scs_client_raw::thrift;

#[derive(clap::Args, Clone)]
pub(crate) struct SparseProfilesArgs {
    #[clap(long, short = 'P', value_delimiter = ',')]
    /// Sparse profiles to calculate the size for (defaults to all profiles)
    sparse_profiles: Vec<String>,
}

impl SparseProfilesArgs {
    pub fn into_sparse_profiles(self) -> thrift::SparseProfiles {
        // clap yields empty names for a trailing or doubled delimiter.
        let profiles: Vec<String> = self
            .sparse_profiles
            .into_iter()
            .filter(|profile| !profile.is_empty())
            .collect();
        if profiles.is_empty() {
            thrift::SparseProfiles::all_profiles(thrift::AllSparseProfiles {
                ..Default::default()
            })
        } else {
            thrift::SparseProfiles::profiles(profiles)
        }
    }
}

#[cfg(test)]
mod test {
    use clap::Parser;
    use mononoke_macros::mononoke;

    use super::*;

    #[derive(Parser)]
    struct TestCommand {
        #[clap(flatten)]
        sparse_profiles: SparseProfilesArgs,
    }

    fn parse(args: &[&str]) -> SparseProfilesArgs {
        let mut argv = vec!["test"];
        argv.extend_from_slice(args);
        TestCommand::parse_from(argv).sparse_profiles
    }

    #[mononoke::test]
    fn test_clap_yields_empty_names_for_empty_delimited_fields() {
        assert_eq!(
            parse(&["-P", "sparse/base,"]).sparse_profiles,
            vec!["sparse/base".to_string(), "".to_string()]
        );
        assert_eq!(
            parse(&["-P", "sparse/base,,sparse/other"]).sparse_profiles,
            vec![
                "sparse/base".to_string(),
                "".to_string(),
                "sparse/other".to_string()
            ]
        );
    }

    #[mononoke::test]
    fn test_empty_profile_names_are_dropped() {
        match parse(&["-P", "sparse/base,,sparse/other,"]).into_sparse_profiles() {
            thrift::SparseProfiles::profiles(profiles) => assert_eq!(
                profiles,
                vec!["sparse/base".to_string(), "sparse/other".to_string()]
            ),
            other => panic!("expected an exact list of profiles, got {other:?}"),
        }
    }

    #[mononoke::test]
    fn test_no_profiles_requests_all_of_them() {
        for args in [&[][..], &["-P", ""][..], &["-P", ","][..]] {
            match parse(args).into_sparse_profiles() {
                thrift::SparseProfiles::all_profiles(_) => {}
                other => panic!("expected all profiles for {args:?}, got {other:?}"),
            }
        }
    }
}
