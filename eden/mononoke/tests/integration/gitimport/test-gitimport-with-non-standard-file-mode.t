# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ REPOTYPE="blob_files"
  $ setup_common_config $REPOTYPE
  $ GIT_REPO_ORIGIN="${TESTTMP}/origin/repo-git"
  $ GIT_REPO="${TESTTMP}/repo-git"

# Setup git repository
  $ mkdir -p "$GIT_REPO_ORIGIN"
  $ cd "$GIT_REPO_ORIGIN"
  $ git init -q
  $ echo "this is file1" > file1
  $ git add file1
  $ git commit -qam "Add file1"
  $ git tag -a -m"new tag" first_tag
  $ echo "this is file2" > file2
  $ git add file2
  $ git commit -qam "Add file2"
  $ git tag -a empty_tag -m ""
# Mark file2 as executable for everyone
  $ chmod +755 file2
  $ git add .
  $ git commit -qam "Made file2 executable for everyone"
# Show the root tree at this commit
  $ git ls-tree HEAD
  100644 blob 433eb172726bc7b6d60e8d68efb0f0ef4e67a667	file1
  100755 blob f138820097c8ef62a012205db0b1701df516f6d5	file2
# Mark file2 as executable only for the owner.
# Git doesn't even have a way to do this anymore but we do have repos
# in production with this state so let's mimick it by hand
  $ git ls-tree HEAD &> $TESTTMP/orig_tree
  $ sed -i 's/100755/100744/g' $TESTTMP/orig_tree
  $ git mktree < $TESTTMP/orig_tree
  a6232473c2c1a1b2b1130a41121b6b32e5592c00
  $ git commit-tree -p HEAD -m "Made file2 executable just for the owner" a6232473c2c1a1b2b1130a41121b6b32e5592c00
  6887491e28eae67dbedc26e16422bfd39c60caaa
  $ git reset --hard 6887491e28eae67dbedc26e16422bfd39c60caaa
  HEAD is now at 6887491 Made file2 executable just for the owner

  $ cd "$TESTTMP"
  $ git clone "$GIT_REPO_ORIGIN"
  Cloning into 'repo-git'...
  done.

# Capture all the known Git objects from the repo
  $ cd $GIT_REPO
  $ git rev-list --objects --all | git cat-file --batch-check='%(objectname) %(objecttype) %(rest)' | sort > $TESTTMP/object_list

# Import it into Mononoke
  $ with_stripped_logs gitimport "$GIT_REPO_ORIGIN" --derive-hg --generate-bookmarks full-repo | head -6
  using repo "repo" repoid RepositoryId(0)
  GitRepo:$TESTTMP/origin/repo-git commit 4 of 4 - Oid:6887491e => Bid:788ba976, repo: $TESTTMP/origin/repo-git
  Hg: Sha1(8ce3eae44760b500bf3f2c3922a95dcd3c908e9e): HgManifestId(HgNodeHash(Sha1(009adbc8d457927d2e1883c08b0692bc45089839)))
  Hg: Sha1(e8615d6f149b876be0a2f30a1c5bf0c42bf8e136): HgManifestId(HgNodeHash(Sha1(d92f8d2d10e61e62f65acf25cdd638ea214f267f)))
  Hg: Sha1(b00ebe1d3f3fef10a2398ed593b8179ef43cb625): HgManifestId(HgNodeHash(Sha1(6603f73278e14012863aa605262e87af7456b577)))
  Hg: Sha1(6887491e28eae67dbedc26e16422bfd39c60caaa): HgManifestId(HgNodeHash(Sha1(6603f73278e14012863aa605262e87af7456b577)))
