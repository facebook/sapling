# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ ENABLED_DERIVED_DATA='["git_trees", "filenodes", "hgchangesets"]' setup_common_config
  $ GIT_REPO="${TESTTMP}/repo-git"
  $ HG_REPO="${TESTTMP}/repo-hg"

# Setup git repository
  $ mkdir "$GIT_REPO"
  $ cd "$GIT_REPO"
  $ git init -q
  $ echo "this is file1" > file1
  $ git add file1
  $ git commit -am "Add file1"
  [master (root-commit) 8ce3eae] Add file1
   1 file changed, 1 insertion(+)
   create mode 100644 file1
  $ git log
  commit 8ce3eae44760b500bf3f2c3922a95dcd3c908e9e
  Author: mononoke <mononoke@mononoke>
  Date:   Sat Jan 1 00:00:00 2000 +0000
  
      Add file1

  $ cd "$TESTTMP"
  $ git clone repo-git repo-git-clone
  Cloning into 'repo-git-clone'...
  done.
  $ cd "$TESTTMP/repo-git"
  $ git checkout --orphan another_committer
  Switched to a new branch 'another_committer'
  $ echo "this is file1" > file1
  $ git add file1
  $ export GIT_COMMITTER_NAME="second_committer"
  $ export GIT_COMMITTER_EMAIL="second_committer@fb.com"
  $ export GIT_COMMITTER_DATE="1000000000"
  $ git_set_only_author commit -am "Add file1"
  [another_committer (root-commit) 69a2653] Add file1
   Author: mononoke <mononoke@mononoke>
   1 file changed, 1 insertion(+)
   create mode 100644 file1
  $ git log --pretty=fuller
  commit 69a265312a2c29cdf5667ff401d895a66e6ac02a
  Author:     mononoke <mononoke@mononoke>
  AuthorDate: Sat Jan 1 00:00:00 2000 +0000
  Commit:     second_committer <second_committer@fb.com>
  CommitDate: Sun Sep 9 01:46:40 2001 +0000
  
      Add file1

# Import it into Mononoke
  $ cd "$TESTTMP"
  $ gitimport "$GIT_REPO" --derive-trees --derive-hg --bonsai-git-mapping full-repo
  * using repo "repo" repoid RepositoryId(0) (glob)
  * GitRepo:$TESTTMP/repo-git commit 1 of 2 - Oid:8ce3eae4 => Bid:631c03b2 (glob)
  * GitRepo:$TESTTMP/repo-git commit 2 of 2 - Oid:69a26531 => Bid:5539313b (glob)
  * 2 tree(s) are valid! (glob)
  * Hg: Sha1(8ce3eae44760b500bf3f2c3922a95dcd3c908e9e): HgManifestId(HgNodeHash(Sha1(009adbc8d457927d2e1883c08b0692bc45089839))) (glob)
  * Hg: Sha1(69a265312a2c29cdf5667ff401d895a66e6ac02a): HgManifestId(HgNodeHash(Sha1(009adbc8d457927d2e1883c08b0692bc45089839))) (glob)
  * Ref: Some("refs/heads/another_committer"): Some(ChangesetId(Blake2(5539313bc3c3888d3856a808458c9766fe2f901cc2da0e848cc05ce513e6c84e))) (glob)
  * Ref: Some("refs/heads/master"): Some(ChangesetId(Blake2(631c03b250b34b3f9ee3b6acfb597123ec8340adaa91e3ba84ef0f4c54c6641a))) (glob)

# Check hg hash before overwriting
  $ mononoke_admin convert --from bonsai --to hg 5539313bc3c3888d3856a808458c9766fe2f901cc2da0e848cc05ce513e6c84e
  * using repo "repo" repoid RepositoryId(0) (glob)
  70361aa80b8d681cc78fcccba6d5671df7ee4d23

# Now rederive HG_SET_COMMITTER_EXTRA=true. This changes hg hash, so let's run it with --rederive and make sure
# hg hash was overwritten.
  $ HG_SET_COMMITTER_EXTRA=true ENABLED_DERIVED_DATA='["git_trees", "filenodes", "hgchangesets"]' setup_common_config
  $ mononoke_admin derived-data derive hgchangesets --changeset 5539313bc3c3888d3856a808458c9766fe2f901cc2da0e848cc05ce513e6c84e --rederive
  * using repo "repo" repoid RepositoryId(0) (glob)
  * changeset resolved as: ChangesetId(Blake2(5539313bc3c3888d3856a808458c9766fe2f901cc2da0e848cc05ce513e6c84e)) (glob)
  * about to rederive 1 commits (glob)
  * deriving 5539313bc3c3888d3856a808458c9766fe2f901cc2da0e848cc05ce513e6c84e (glob)
  * derived 5539313bc3c3888d3856a808458c9766fe2f901cc2da0e848cc05ce513e6c84e in *, Ok("MappedHgChangesetId(HgChangesetId(HgNodeHash(Sha1(c87b24576a61fd421922284be86769baa165ef43))))") (glob)

# Check hg hash after overwriting
  $ mononoke_admin convert --from bonsai --to hg 5539313bc3c3888d3856a808458c9766fe2f901cc2da0e848cc05ce513e6c84e
  * using repo "repo" repoid RepositoryId(0) (glob)
  c87b24576a61fd421922284be86769baa165ef43
