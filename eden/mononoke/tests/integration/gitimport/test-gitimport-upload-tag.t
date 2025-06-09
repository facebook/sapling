# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ GIT_REPO="${TESTTMP}/repo-git"
  $ setup_common_config blob_files

# Setup git repository
  $ mkdir -p "$GIT_REPO"
  $ cd "$GIT_REPO"
  $ git init -q
  $ echo "this is file1" > file1
  $ git add file1
  $ git commit -qa -m "Commit"
  $ git tag -a -m "Tag" tag1
  $ tag_hash=$(git rev-parse tags/tag1)
  $ git tag -a -m "Another Tag" tag2
  $ tag2_hash=$(git rev-parse tags/tag2)

# Import just the tag into Mononoke
  $ with_stripped_logs gitimport "$GIT_REPO" upload-tags $tag_hash $tag2_hash
  using repo "repo" repoid RepositoryId(0)
  Uploaded tag with ID 929a3a6ccd846af11aa4384cc99d63691b480d9d
  Uploaded tag with ID ec2d3c28a6524f5bd4d16b21020b4cffec95db15

# Ensure that the uploaded tags are visible in Mononoke
  $ mononoke_admin git-objects -R repo fetch --id $tag_hash
  The object is a Git Tag
  
  Tag {
      target: Sha1(15cc4e9575665b507ee372f97b716ff552842136),
      target_kind: Commit,
      name: "tag1",
      tagger: Some(
          Signature {
              name: "mononoke",
              email: "mononoke@mononoke",
              time: Time {
                  seconds: 946684800,
                  offset: 0,
              },
          },
      ),
      message: "Tag\n",
      pgp_signature: None,
  }

  $ mononoke_admin git-objects -R repo fetch --id $tag2_hash
  The object is a Git Tag
  
  Tag {
      target: Sha1(15cc4e9575665b507ee372f97b716ff552842136),
      target_kind: Commit,
      name: "tag2",
      tagger: Some(
          Signature {
              name: "mononoke",
              email: "mononoke@mononoke",
              time: Time {
                  seconds: 946684800,
                  offset: 0,
              },
          },
      ),
      message: "Another Tag\n",
      pgp_signature: None,
  }
