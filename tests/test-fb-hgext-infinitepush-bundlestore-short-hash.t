Create an ondisk bundlestore
  $ . "$TESTDIR/library.sh"
  $ . "$TESTDIR/infinitepush/library.sh"
  $ cp $HGRCPATH $TESTTMP/defaulthgrc
  $ setupcommon
  $ hg init repo
  $ cd repo
  $ setupserver
  $ cd ..

Test `hg up` command for the commit that doesn't exist locally but does remotely.
We are making commit in repo (server) and will recover it in client 1 via short hash.

  $ hg clone ssh://user@dummy/repo client2 -q
  $ (cd repo && mkcommit somecommit && hg log -r .)
  changeset:   0:f8b49bf62d4d
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     somecommit
  
  $ (cd ./client2 &&  hg up f8b49b)
  'f8b49b' does not exist locally - looking for it remotely...
  pulling from ssh://user@dummy/repo
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  new changesets f8b49bf62d4d
  (run 'hg update' to get a working copy)
  'f8b49b' found remotely
  pull finished in * sec (glob)
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

Test `hg up` command for the commit that doesn't exist locally
Also doesn't exist remotely
But can be recovered from backup
We are making a test commit in client 1 and will recover it from client2

  $ hg clone ssh://user@dummy/repo client1 -q
  $ cd client1
  $ mkcommit someothercommit
  $ hg log -r .
  changeset:   1:c1b6fe8fce73
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     someothercommit
  
  $ hg pushbackup
  starting backup * (glob)
  searching for changes
  remote: pushing 1 commit:
  remote:     c1b6fe8fce73  someothercommit
  finished in * seconds (glob)
  $ cd ../

  $ (cd ./client2 && hg up c1b6fe)
  'c1b6fe' does not exist locally - looking for it remotely...
  pulling from ssh://user@dummy/repo
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  new changesets c1b6fe8fce73
  (run 'hg update' to get a working copy)
  'c1b6fe' found remotely
  pull finished in * sec (glob)
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

Test pulling a commit with the same prefix by creating fake files
  $ echo ' ' > ./repo/.hg/scratchbranches/index/nodemap/b1b6fe8fce73221de4162469dac9a6f8d01744a1
  $ echo ' ' > ./repo/.hg/scratchbranches/index/nodemap/b1b6fe8fce73221de4162469dac9a6f8d01744a2
  $ (cd ./client2 && hg up b1b6fe)
  'b1b6fe' does not exist locally - looking for it remotely...
  pulling from ssh://user@dummy/repo
  pull failed: ambiguous identifier 'b1b6fe'
  abort: unknown revision 'b1b6fe'!
  [255]

Clean up from the previous tests
  $ rm -r ./client1
  $ rm -r ./client2

Set up similar test but with sql infinitepush storage

#if no-osx
  $ mkcommit() {
  >    echo "$1" > "$1"
  >    hg add "$1"
  >    hg ci -d "0 0" -m "$1"
  > }
  $ . "$TESTDIR/infinitepush/library.sh"
  $ setupcommon

With no configuration it should abort
  $ hg init server
  $ cd server
  $ setupsqlserverhgrc babar
  $ setupdb
  $ cd ..
  $ hg clone -q ssh://user@dummy/server client1
  $ hg clone -q ssh://user@dummy/server client2
  $ cd ./client1
  $ setupsqlclienthgrc
  $ cd ../client2
  $ setupsqlclienthgrc
  $ cd ../client1
  $ mkcommit someothercommit
  $ hg log -r .
  changeset:   0:af79492bdbc1
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     someothercommit
  
  $ hg pushbackup
  starting backup * (glob)
  searching for changes
  remote: pushing 1 commit:
  remote:     af79492bdbc1  someothercommit
  finished in * seconds (glob)
  $ cd ../

  $ (cd ./client2 && hg up af7949)
  'af7949' does not exist locally - looking for it remotely...
  pulling from ssh://user@dummy/server
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  new changesets af79492bdbc1
  (run 'hg update' to get a working copy)
  'af7949' found remotely
  pull finished in * sec (glob)
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

Test when it is not found
  $ (cd ./client2 && hg up af7948)
  'af7948' does not exist locally - looking for it remotely...
  pulling from ssh://user@dummy/server
  pull failed: unknown revision 'af7948'
  abort: unknown revision 'af7948'!
  [255]

#endif
