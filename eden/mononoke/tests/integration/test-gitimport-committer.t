# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ HG_SET_COMMITTER_EXTRA=true ENABLED_DERIVED_DATA='["git_trees", "filenodes", "hgchangesets"]' setup_common_config
  $ GIT_REPO="${TESTTMP}/repo-git"
  $ HG_REPO="${TESTTMP}/repo-hg"

# Setup git repsitory
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
  $ gitimport "$GIT_REPO" --derive-hg full-repo
  * using repo "repo" repoid RepositoryId(0) (glob)
  * GitRepo:$TESTTMP/repo-git commit 1 of 2 - Oid:8ce3eae4 => Bid:032cd4dc (glob)
  * GitRepo:$TESTTMP/repo-git commit 2 of 2 - Oid:69a26531 => Bid:1213979c (glob)
  * Hg: Sha1(8ce3eae44760b500bf3f2c3922a95dcd3c908e9e): HgManifestId(HgNodeHash(Sha1(009adbc8d457927d2e1883c08b0692bc45089839))) (glob)
  * Hg: Sha1(69a265312a2c29cdf5667ff401d895a66e6ac02a): HgManifestId(HgNodeHash(Sha1(009adbc8d457927d2e1883c08b0692bc45089839))) (glob)
  * Ref: "refs/heads/another_committer": Some(ChangesetId(Blake2(1213979c6023f23e70dbe8845d773078ac1e0506bc2ab98382a329da0cb379a7))) (glob)
  * Ref: "refs/heads/master": Some(ChangesetId(Blake2(032cd4dce0406f1c1dd1362b6c3c9f9bdfa82f2fc5615e237a890be4fe08b044))) (glob)

# Set master (gitimport does not do this yet)
  $ mononoke_admin bookmarks set another_committer 1213979c6023f23e70dbe8845d773078ac1e0506bc2ab98382a329da0cb379a7
  * using repo "repo" repoid RepositoryId(0) (glob)
  * changeset resolved as: ChangesetId(Blake2(1213979c6023f23e70dbe8845d773078ac1e0506bc2ab98382a329da0cb379a7)) (glob)
  * Current position of BookmarkName { bookmark: "another_committer" } is None (glob)
  $ mononoke_admin bookmarks set master 032cd4dce0406f1c1dd1362b6c3c9f9bdfa82f2fc5615e237a890be4fe08b044
  * using repo "repo" repoid RepositoryId(0) (glob)
  * changeset resolved as: ChangesetId(Blake2(*)) (glob)
  * Current position of BookmarkName { bookmark: "master" } is None (glob)

# Start Mononoke
  $ start_and_wait_for_mononoke_server
# Clone the repository
  $ cd "$TESTTMP"
  $ hgmn_clone mononoke://$(mononoke_address)/repo "$HG_REPO"
  $ cd "$HG_REPO"
  $ cat "file1"
  this is file1
  $ hg log -r master
  commit:      b48ed4600785
  bookmark:    master
  bookmark:    default/master
  hoistedname: master
  user:        mononoke <mononoke@mononoke>
  date:        Sat Jan 01 00:00:00 2000 +0000
  summary:     Add file1
  


# No committer extra here, because committer is the same as author
  $ hg log -r master -T '{extras}'
  branch=defaultconvert_revision=8ce3eae44760b500bf3f2c3922a95dcd3c908e9ehg-git-rename-source=git (no-eol)
  $ hg log -r another_committer -T '{extras}'
  branch=defaultcommitter=second_committer <second_committer@fb.com> 1000000000 0convert_revision=69a265312a2c29cdf5667ff401d895a66e6ac02ahg-git-rename-source=git (no-eol)
