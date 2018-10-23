  $ . $TESTDIR/library.sh
  $ hook_test_setup $TESTDIR/hooks/block_cross_repo_commits.lua block_cross_repo_commits PerAddedOrModifiedFile "bypass_commit_string=\"@fbsource-bypass-allowed-directories\""

  $ hg up tip
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved

good top level dirs and file, should pass

  $ mkdir fbcode
  $ echo 'x' > fbcode/foo
  $ mkdir fbandroid
  $ echo 'x' > fbandroid/foo
  $ mkdir fbobjc
  $ echo 'x' > fbobjc/foo
  $ mkdir tools
  $ echo 'x' > tools/foo
  $ mkdir xplat
  $ echo 'x' > xplat/foo
  $ echo 'x' > .topleveldotfile
  $ hg ci -Aqm 1
  $ hgmn push -r . --to master_bookmark
  remote: * DEBG Session with Mononoke started with uuid: * (glob)
  pushing rev 5d34ec2d2319 to destination ssh://user@dummy/repo bookmark master_bookmark
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 0 changesets with 0 changes to 0 files
  server ignored bookmark master_bookmark update

bad top level dir, should fail

  $ hg up -q 0
  $ mkdir badtopleveldir
  $ echo 'x' > badtopleveldir/foo
  $ hg add badtopleveldir/foo && hg ci -m 1
  $ hgmn push -r . --to master_bookmark
  remote: * DEBG Session with Mononoke started with uuid: * (glob)
  pushing rev f626f2055ec3 to destination ssh://user@dummy/repo bookmark master_bookmark
  searching for changes
  remote: * ERRO Command failed, remote: true, error: hooks failed: (glob)
  remote: block_cross_repo_commits for f626f2055ec36d74bef9de03183844f0d6656ef0: File badtopleveldir/foo is not in an allowed directory. , root_cause: ErrorMessage {
  remote:     msg: "hooks failed:\nblock_cross_repo_commits for f626f2055ec36d74bef9de03183844f0d6656ef0: File badtopleveldir/foo is not in an allowed directory. "
  remote: }, backtrace: , session_uuid: * (glob)
  abort: stream ended unexpectedly (got 0 bytes, expected 4)
  [255]

bad file in root, should fail

  $ hg up -q 0
  $ echo 'x' > badtoplevelfile
  $ hg add badtoplevelfile && hg ci -m 1
  $ hgmn push -r . --to master_bookmark
  remote: * DEBG Session with Mononoke started with uuid: * (glob)
  pushing rev 0bcec3988e25 to destination ssh://user@dummy/repo bookmark master_bookmark
  searching for changes
  remote: * ERRO Command failed, remote: true, error: hooks failed: (glob)
  remote: block_cross_repo_commits for 0bcec3988e25203e7ea29f20b44193e4db79051f: File badtoplevelfile is not in an allowed directory. , root_cause: ErrorMessage {
  remote:     msg: "hooks failed:\nblock_cross_repo_commits for 0bcec3988e25203e7ea29f20b44193e4db79051f: File badtoplevelfile is not in an allowed directory. "
  remote: }, backtrace: , session_uuid: * (glob)
  abort: stream ended unexpectedly (got 0 bytes, expected 4)
  [255]

commits to tupperware blacklist, should fail

  $ hg up -q 0
  $ mkdir -p fbcode/tupperware/config/common/
  $ echo 'x' > fbcode/tupperware/config/common/foo
  $ hg add fbcode/tupperware/config/common/foo && hg ci -m 1
  $ hgmn push -r . --to master_bookmark
  remote: * DEBG Session with Mononoke started with uuid: * (glob)
  pushing rev c7901736b123 to destination ssh://user@dummy/repo bookmark master_bookmark
  searching for changes
  remote: * ERRO Command failed, remote: true, error: hooks failed: (glob)
  remote: block_cross_repo_commits for c7901736b12390a1f5930c01889ceacf7019b6e3: File fbcode/tupperware/config/common/foo is not in an allowed directory. , root_cause: ErrorMessage {
  remote:     msg: "hooks failed:\nblock_cross_repo_commits for c7901736b12390a1f5930c01889ceacf7019b6e3: File fbcode/tupperware/config/common/foo is not in an allowed directory. "
  remote: }, backtrace: , session_uuid: * (glob)
  abort: stream ended unexpectedly (got 0 bytes, expected 4)
  [255]

  $ hg up -q 0
  $ mkdir -p fbcode/tupperware/config/twcron/
  $ echo 'x' > fbcode/tupperware/config/twcron/foo
  $ hg add fbcode/tupperware/config/twcron/foo && hg ci -m 1
  $ hgmn push -r . --to master_bookmark
  remote: * DEBG Session with Mononoke started with uuid: * (glob)
  pushing rev a5f50efbcd3f to destination ssh://user@dummy/repo bookmark master_bookmark
  searching for changes
  remote: * ERRO Command failed, remote: true, error: hooks failed: (glob)
  remote: block_cross_repo_commits for a5f50efbcd3f4d03279b79f6b5191e97345a7095: File fbcode/tupperware/config/twcron/foo is not in an allowed directory. , root_cause: ErrorMessage {
  remote:     msg: "hooks failed:\nblock_cross_repo_commits for a5f50efbcd3f4d03279b79f6b5191e97345a7095: File fbcode/tupperware/config/twcron/foo is not in an allowed directory. "
  remote: }, backtrace: , session_uuid: * (glob)
  abort: stream ended unexpectedly (got 0 bytes, expected 4)
  [255]

  $ hg up -q 0
  $ mkdir -p fbcode/tupperware/config/managed_containers/
  $ echo 'x' > fbcode/tupperware/config/managed_containers/foo
  $ hg add fbcode/tupperware/config/managed_containers/foo && hg ci -m 1
  $ hgmn push -r . --to master_bookmark
  remote: * DEBG Session with Mononoke started with uuid: * (glob)
  pushing rev 8cbc4aa6858f to destination ssh://user@dummy/repo bookmark master_bookmark
  searching for changes
  remote: * ERRO Command failed, remote: true, error: hooks failed: (glob)
  remote: block_cross_repo_commits for 8cbc4aa6858fd114c4151d42ca38c884926104b9: File fbcode/tupperware/config/managed_containers/foo is not in an allowed directory. , root_cause: ErrorMessage {
  remote:     msg: "hooks failed:\nblock_cross_repo_commits for 8cbc4aa6858fd114c4151d42ca38c884926104b9: File fbcode/tupperware/config/managed_containers/foo is not in an allowed directory. "
  remote: }, backtrace: , session_uuid: * (glob)
  abort: stream ended unexpectedly (got 0 bytes, expected 4)
  [255]

other tupperware config files should be ok

  $ hg up -q 0
  $ mkdir -p fbcode/tupperware/config/
  $ echo 'x' > fbcode/tupperware/config/foo
  $ hg add fbcode/tupperware/config/foo && hg ci -m 1
  $ hgmn push -r . --to master_bookmark
  remote: * DEBG Session with Mononoke started with uuid: * (glob)
  pushing rev 0e8452b82247 to destination ssh://user@dummy/repo bookmark master_bookmark
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 0 files
  server ignored bookmark master_bookmark update

push to dataswarm pipelines dir should fail

  $ hg up -q 0
  $ mkdir -p fbcode/dataswarm-pipelines/
  $ echo 'x' > fbcode/dataswarm-pipelines/foo
  $ hg add fbcode/dataswarm-pipelines/foo && hg ci -m 1
  $ hgmn push -r . --to master_bookmark
  remote: * DEBG Session with Mononoke started with uuid: * (glob)
  pushing rev 8cd3a1bebf72 to destination ssh://user@dummy/repo bookmark master_bookmark
  searching for changes
  remote: * ERRO Command failed, remote: true, error: hooks failed: (glob)
  remote: block_cross_repo_commits for 8cd3a1bebf72906cdd65a687c3832a9afcecdf1f: File fbcode/dataswarm-pipelines/foo is in fbcode/dataswarm-pipelines/, root_cause: ErrorMessage {
  remote:     msg: "hooks failed:\nblock_cross_repo_commits for 8cd3a1bebf72906cdd65a687c3832a9afcecdf1f: File fbcode/dataswarm-pipelines/foo is in fbcode/dataswarm-pipelines/"
  remote: }, backtrace: , session_uuid: * (glob)
  abort: stream ended unexpectedly (got 0 bytes, expected 4)
  [255]

  $ hg up -q 0
  $ mkdir -p fbcode/dataswarm-pipelines/somedir
  $ echo 'x' > fbcode/dataswarm-pipelines/somedir/foo
  $ hg add fbcode/dataswarm-pipelines/somedir/foo && hg ci -m 1
  $ hgmn push -r . --to master_bookmark
  remote: * DEBG Session with Mononoke started with uuid: * (glob)
  pushing rev 362f933fd9f6 to destination ssh://user@dummy/repo bookmark master_bookmark
  searching for changes
  remote: * ERRO Command failed, remote: true, error: hooks failed: (glob)
  remote: block_cross_repo_commits for 362f933fd9f6d1fd600aac2b974265a76502dae6: File fbcode/dataswarm-pipelines/somedir/foo is in fbcode/dataswarm-pipelines/, root_cause: ErrorMessage {
  remote:     msg: "hooks failed:\nblock_cross_repo_commits for 362f933fd9f6d1fd600aac2b974265a76502dae6: File fbcode/dataswarm-pipelines/somedir/foo is in fbcode/dataswarm-pipelines/"
  remote: }, backtrace: , session_uuid: * (glob)
  abort: stream ended unexpectedly (got 0 bytes, expected 4)
  [255]

bypass hook with bypass string

  $ hg up -q 0
  $ mkdir -p fbcode/tupperware/config/common/
  $ echo 'x' > fbcode/tupperware/config/common/foo
  $ hg add fbcode/tupperware/config/common/foo && hg ci -m "@fbsource-bypass-allowed-directories"
  $ hgmn push -r . --to master_bookmark
  remote: * DEBG Session with Mononoke started with uuid: * (glob)
  pushing rev 3a6776abc141 to destination ssh://user@dummy/repo bookmark master_bookmark
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 0 files
  server ignored bookmark master_bookmark update
