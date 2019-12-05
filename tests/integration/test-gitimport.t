  $ . "${TEST_FIXTURES}/library.sh"
  $ setup_common_config
  $ GIT_REPO="${TESTTMP}/repo-git"
  $ HG_REPO="${TESTTMP}/repo-hg"

# Setup git repsitory
  $ mkdir "$GIT_REPO"
  $ cd "$GIT_REPO"
  $ git init
  Initialized empty Git repository in $TESTTMP/repo-git/.git/
  $ echo "this is file1" > file1
  $ git add file1
  $ git commit -am "Add file1"
  [master (root-commit) e45fd71] Add file1
   1 file changed, 1 insertion(+)
   create mode 100644 file1

# Import it into Mononoke
  $ cd "$TESTTMP"
  $ gitimport "$GIT_REPO"
  * using repo "repo" repoid RepositoryId(0) (glob)
  Created e45fd71023e1daf8bcadd9a63086c66180aa8c64 => ChangesetId(Blake2(9f0036550c46f77a800c6106c083c70937304def04d9d3eef9d665a8e33ef9dd))
  Ref: Some("refs/heads/master"): Some(ChangesetId(Blake2(9f0036550c46f77a800c6106c083c70937304def04d9d3eef9d665a8e33ef9dd)))

# Set master (gitimport does not do this yet)
  $ mononoke_admin bookmarks set master 9f0036550c46f77a800c6106c083c70937304def04d9d3eef9d665a8e33ef9dd
  * using repo "repo" repoid RepositoryId(0) (glob)
  * changeset resolved as: ChangesetId(Blake2(9f0036550c46f77a800c6106c083c70937304def04d9d3eef9d665a8e33ef9dd)) (glob)

# Start Mononoke
  $ mononoke
  $ wait_for_mononoke

# Clone the repository
  $ cd "$TESTTMP"
  $ hgmn_clone 'ssh://user@dummy/repo' "$HG_REPO"
  $ cd "$HG_REPO"
  $ cat "file1"
  this is file1
