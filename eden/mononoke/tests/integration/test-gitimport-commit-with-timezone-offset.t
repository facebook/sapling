# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ ENABLED_DERIVED_DATA='["unodes", "git_commits", "git_trees", "git_delta_manifests"]' setup_common_config
  $ GIT_REPO="${TESTTMP}/repo-git"
  $ REPOTYPE="blob_files"
  $ ENABLED_DERIVED_DATA='["unodes", "git_commits", "git_trees", "git_delta_manifests"]' setup_common_config $REPOTYPE
  $ cat >> repos/repo/server.toml <<EOF
  > [source_control_service]
  > permit_writes = true
  > EOF

# Setup git repository
  $ mkdir -p "$GIT_REPO"
  $ cd "$GIT_REPO"
  $ git init -q
  $ echo "this is file1" > file1
  $ git add file1
# Create a commit that has a timezone offset
  $ git commit -qa -m "Commit with offset date time"
  $ git commit --amend --no-edit --date="12/12/12 4:40p +0800"
  [master 9695b5c] Commit with offset date time
   Date: Wed Dec 12 04:40:00 2012 +0800
   1 file changed, 1 insertion(+)
   create mode 100644 file1
  $ git show
  commit 9695b5ce077c0fba96f8e75694a4c02e4813bb87
  Author: mononoke <mononoke@mononoke>
  Date:   Wed Dec 12 04:40:00 2012 +0800
  
      Commit with offset date time
  
  diff --git a/file1 b/file1
  new file mode 100644
  index 0000000..433eb17
  --- /dev/null
  +++ b/file1
  @@ -0,0 +1 @@
  +this is file1

# Import it into Mononoke
  $ gitimport "$GIT_REPO" --concurrency 1 full-repo
  * using repo "repo" repoid RepositoryId(0) (glob)
  * GitRepo:$TESTTMP/repo-git commit 1 of 1 - Oid:9695b5ce => Bid:53be2f28 (glob)
  * Ref: "refs/heads/master": Some(ChangesetId(Blake2(53be2f28390c43721be5fc1cdd54f24e1bc7875e0774b2e1bf9b4da150b21fa8))) (glob)


# Derive the git commit for this commit
  $ mononoke_newadmin derived-data -R repo derive --rederive -T git_commits -i 53be2f28390c43721be5fc1cdd54f24e1bc7875e0774b2e1bf9b4da150b21fa8
  Error: Conflicting mapping Some(BonsaiGitMappingEntry { git_sha1: GitSha1(9695b5ce077c0fba96f8e75694a4c02e4813bb87), bcs_id: ChangesetId(Blake2(53be2f28390c43721be5fc1cdd54f24e1bc7875e0774b2e1bf9b4da150b21fa8)) }) detected while inserting git mappings (tried inserting: [BonsaiGitMappingEntry { git_sha1: GitSha1(1fc739d9b6d155d0e07d42ee8a8df4f52c40c004), bcs_id: ChangesetId(Blake2(53be2f28390c43721be5fc1cdd54f24e1bc7875e0774b2e1bf9b4da150b21fa8)) }])
  [1]
  $ mononoke_newadmin git-objects -R repo fetch --id 9695b5ce077c0fba96f8e75694a4c02e4813bb87 > imported
  $ mononoke_newadmin git-objects -R repo fetch --id 1fc739d9b6d155d0e07d42ee8a8df4f52c40c004 > derived

  $ diff --old-line-format="- %L" --new-line-format="+ %L" imported derived
  The object is a Git Commit
  
  Commit {
      tree: Sha1(cb2ef838eb24e4667fee3a8b89c930234ae6e4bb),
      parents: [],
      author: Signature {
          name: "mononoke",
          email: "mononoke@mononoke",
          time: Time {
              seconds: 1355258400,
  -             offset: 28800,
  -             sign: Plus,
  +             offset: -28800,
  +             sign: Minus,
          },
      },
      committer: Signature {
          name: "mononoke",
          email: "mononoke@mononoke",
          time: Time {
              seconds: 946684800,
              offset: 0,
              sign: Plus,
          },
      },
      encoding: None,
      message: "Commit with offset date time\n",
      extra_headers: [],
  }
  [1]
