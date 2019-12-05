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
  $ gitimport "$GIT_REPO" --derive-trees --hggit-compatibility
  * using repo "repo" repoid RepositoryId(0) (glob)
  Created e45fd71023e1daf8bcadd9a63086c66180aa8c64 => ChangesetId(Blake2(3e169314bafbb68d9db7e42eeace9c829a11d32be3b6847cb841fefafaf9d31a))
  Ref: Some("refs/heads/master"): Some(ChangesetId(Blake2(3e169314bafbb68d9db7e42eeace9c829a11d32be3b6847cb841fefafaf9d31a)))
  1 tree(s) are valid!

# Set master (gitimport does not do this yet)
  $ mononoke_admin bookmarks set master 3e169314bafbb68d9db7e42eeace9c829a11d32be3b6847cb841fefafaf9d31a
  * using repo "repo" repoid RepositoryId(0) (glob)
  * changeset resolved as: ChangesetId(Blake2(3e169314bafbb68d9db7e42eeace9c829a11d32be3b6847cb841fefafaf9d31a)) (glob)

# Start Mononoke
  $ mononoke
  $ wait_for_mononoke

# Clone the repository
  $ cd "$TESTTMP"
  $ hgmn_clone 'ssh://user@dummy/repo' "$HG_REPO"
  $ cd "$HG_REPO"
  $ cat "file1"
  this is file1

# Try out hggit compatibility
  $ hg --config extensions.hggit= git-updatemeta
  $ hg --config extensions.hggit= log -T '{gitnode}'
  e45fd71023e1daf8bcadd9a63086c66180aa8c64 (no-eol)
