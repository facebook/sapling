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
# Create a commit that will not roundtrip through the import: Bonsais do not store the encoding
  $ git config --global i18n.commitEncoding ISO-8859-1
  $ git commit -qa -m "Unroundtripable commit: we don't store the encoding"

# Import it into Mononoke
  $ gitimport "$GIT_REPO" --concurrency 1 full-repo
  [INFO] using repo "repo" repoid RepositoryId(0)
  [INFO] GitRepo:$TESTTMP/repo-git commit 1 of 1 - Oid:a57065d8 => Bid:f1c2afeb
  [INFO] Ref: "refs/heads/master_bookmark": Some(ChangesetId(Blake2(f1c2afeb1a400c6b7d45af203fd2de012f5c55a08616cdd2a8499278ab1ddf3d)))

  $ mononoke_admin git-objects -R repo fetch --id a57065d80c86fdef0f01cc4c822278257107ccad
  The object is a Git Commit
  
  CommitRef {
      tree: "cb2ef838eb24e4667fee3a8b89c930234ae6e4bb",
      parents: [],
      author: SignatureRef {
          name: "mononoke",
          email: "mononoke@mononoke",
          time: "946684800 +0000",
      },
      committer: SignatureRef {
          name: "mononoke",
          email: "mononoke@mononoke",
          time: "946684800 +0000",
      },
      encoding: Some(
          "ISO-8859-1",
      ),
      message: "Unroundtripable commit: we don\'t store the encoding\n",
      extra_headers: [],
  }
