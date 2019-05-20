  $ . $TESTDIR/library.sh

setup configuration
  $ export READ_ONLY_REPO=1
  $ setup_common_config
  $ cd $TESTTMP

setup common configuration for these tests
mononoke + local commit cloud backend
  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > amend =
  > directaccess=
  > commitcloud =
  > infinitepush =
  > rebase =
  > remotenames =
  > share =
  > [infinitepush]
  > server=False
  > [experimental]
  > evolution = createmarkers, allowunstable
  > [commitcloud]
  > hostname = testhost
  > servicetype = local
  > servicelocation = $TESTTMP
  > user_token_path = $TESTTMP
  > owner_team = The Test Team
  > updateonmove = true
  > EOF

setup repo

  $ hginit_treemanifest repo
  $ cd repo
  $ mkcommit "base_commit"
  $ hg log -T '{short(node)}\n'
  8b2dca0c8a72

create master bookmark
  $ hg bookmark master_bookmark -r tip

  $ cd $TESTTMP

setup client1 and client2
  $ hgclone_treemanifest ssh://user@dummy/repo client1 --noupdate
  $ hgclone_treemanifest ssh://user@dummy/repo client2 --noupdate

blobimport

  $ blobimport repo/.hg repo

start mononoke

  $ mononoke
  $ wait_for_mononoke $TESTTMP/repo


  $ cd client1
  $ hgmn cloud auth -t xxxxxx
  setting authentication token
  authentication successful
  $ hgmn cloud join
  commitcloud: this repository is now connected to the 'user/test/default' workspace for the 'client1' repo
  commitcloud: synchronizing 'client1' with 'user/test/default'
  commitcloud: commits synchronized
  finished in * (glob)
  $ hgmn up master_bookmark -q
  $ cd ../client2
  $ hgmn cloud auth -t xxxxxx
  updating authentication token
  authentication successful
  $ hgmn cloud join
  commitcloud: this repository is now connected to the 'user/test/default' workspace for the 'client2' repo
  commitcloud: synchronizing 'client2' with 'user/test/default'
  commitcloud: commits synchronized
  finished in * (glob)
  $ hgmn up master_bookmark -q


Make commits in the first client, and sync it
  $ cd ../client1
  $ mkcommit "commit1"
  $ mkcommit "commit2"
  $ mkcommit "commit3"
  $ hgmn cloud sync
  commitcloud: synchronizing 'client1' with 'user/test/default'
  remote: * DEBG Session with Mononoke started with uuid: * (glob)
  backing up stack rooted at 660cb078da57
  commitcloud: commits synchronized
  finished in * (glob)

  $ tglogp
  @  3: 44641a2b1a42 draft 'commit3'
  |
  o  2: eba3648c3275 draft 'commit2'
  |
  o  1: 660cb078da57 draft 'commit1'
  |
  o  0: 8b2dca0c8a72 public 'base_commit'
  

Sync from the second client - the commits should appear
  $ cd ../client2
  $ hgmn cloud sync
  commitcloud: synchronizing 'client2' with 'user/test/default'
  pulling 44641a2b1a42
  pulling from ssh://user@dummy/repo
  remote: * DEBG Session with Mononoke started with uuid: * (glob)
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 3 changesets with 0 changes to 0 files
  new changesets 660cb078da57:44641a2b1a42
  commitcloud: commits synchronized
  finished in * (glob)

  $ tglogp
  o  3: 44641a2b1a42 draft 'commit3'
  |
  o  2: eba3648c3275 draft 'commit2'
  |
  o  1: 660cb078da57 draft 'commit1'
  |
  @  0: 8b2dca0c8a72 public 'base_commit'
  

Make commits from the second client and sync it
  $ mkcommit "commit4"
  $ mkcommit "commit5"
  $ mkcommit "commit6"
  $ hgmn cloud sync
  commitcloud: synchronizing 'client2' with 'user/test/default'
  remote: * DEBG Session with Mononoke started with uuid: * (glob)
  backing up stack rooted at 15f040cf571c
  commitcloud: commits synchronized
  finished in * (glob)


On the first client, make a bookmark, then sync - the bookmark and the new commits should be synced
  $ cd ../client1
  $ hg bookmark -r 0 bookmark1
  $ hgmn cloud sync
  commitcloud: synchronizing 'client1' with 'user/test/default'
  pulling 58508421158d
  pulling from ssh://user@dummy/repo
  remote: * DEBG Session with Mononoke started with uuid: * (glob)
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 3 changesets with 0 changes to 0 files (+1 heads)
  new changesets 15f040cf571c:58508421158d
  commitcloud: commits synchronized
  finished in * (glob)

  $ tglogp
  o  6: 58508421158d draft 'commit6'
  |
  o  5: a1806767adaa draft 'commit5'
  |
  o  4: 15f040cf571c draft 'commit4'
  |
  | @  3: 44641a2b1a42 draft 'commit3'
  | |
  | o  2: eba3648c3275 draft 'commit2'
  | |
  | o  1: 660cb078da57 draft 'commit1'
  |/
  o  0: 8b2dca0c8a72 public 'base_commit' bookmark1
  
 
On the first client rebase the stack
  $ hgmn rebase -s 15f040cf571c -d 44641a2b1a42
  rebasing 4:15f040cf571c "commit4"
  remote: * DEBG Session with Mononoke started with uuid: * (glob)
  rebasing 5:a1806767adaa "commit5"
  rebasing 6:58508421158d "commit6" (tip)
  $ hgmn cloud sync
  commitcloud: synchronizing 'client1' with 'user/test/default'
  remote: * DEBG Session with Mononoke started with uuid: * (glob)
  backing up stack rooted at 660cb078da57
  commitcloud: commits synchronized
  finished in * (glob)


On the second client sync it
  $ cd ../client2
  $ hgmn cloud sync
  commitcloud: synchronizing 'client2' with 'user/test/default'
  pulling 8e3f03f8d9db
  pulling from ssh://user@dummy/repo
  remote: * DEBG Session with Mononoke started with uuid: * (glob)
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 3 changesets with 0 changes to 0 files
  obsoleted 3 changesets
  new changesets f0345b3976c9:8e3f03f8d9db
  commitcloud: commits synchronized
  finished in * (glob)
  commitcloud: current revision 58508421158d has been moved remotely to 8e3f03f8d9db
  updating to 8e3f03f8d9db
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ tglogp
  @  9: 8e3f03f8d9db draft 'commit6'
  |
  o  8: fc9e76452973 draft 'commit5'
  |
  o  7: f0345b3976c9 draft 'commit4'
  |
  o  3: 44641a2b1a42 draft 'commit3'
  |
  o  2: eba3648c3275 draft 'commit2'
  |
  o  1: 660cb078da57 draft 'commit1'
  |
  o  0: 8b2dca0c8a72 public 'base_commit' bookmark1
  

On the second client hide all draft commits
  $ hgmn hide -r 'draft()'
  hiding commit 660cb078da57 "commit1"
  hiding commit eba3648c3275 "commit2"
  hiding commit 44641a2b1a42 "commit3"
  hiding commit f0345b3976c9 "commit4"
  hiding commit fc9e76452973 "commit5"
  hiding commit 8e3f03f8d9db "commit6"
  0 files updated, 0 files merged, 6 files removed, 0 files unresolved
  working directory now at 8b2dca0c8a72
  6 changesets hidden
  $ hgmn cloud sync
  commitcloud: synchronizing 'client2' with 'user/test/default'
  remote: * DEBG Session with Mononoke started with uuid: * (glob)
  commitcloud: commits synchronized
  finished in * (glob)
  $ hgmn up master_bookmark -q

  $ tglogp
  @  0: 8b2dca0c8a72 public 'base_commit' bookmark1
  

On the first client check that all commits were hidden
  $ cd ../client1
  $ hgmn cloud sync
  commitcloud: synchronizing 'client1' with 'user/test/default'
  remote: * DEBG Session with Mononoke started with uuid: * (glob)
  commitcloud: commits synchronized
  finished in * (glob)
  $ hgmn up master_bookmark -q

  $ tglogp
  @  0: 8b2dca0c8a72 public 'base_commit' bookmark1
  
 
On the first client make 2 stacks
  $ mkcommit 'stack 1 first'
  $ mkcommit 'stack 1 second'
  $ hgmn up -q -r 0
  $ mkcommit 'stack 2 first'
  $ mkcommit 'stack 2 second'

  $ tglogp
  @  13: 88d416aed919 draft 'stack 2 second'
  |
  o  12: 77a917e6c3a5 draft 'stack 2 first'
  |
  | o  11: ec61bf312a03 draft 'stack 1 second'
  | |
  | o  10: 8d621fa11677 draft 'stack 1 first'
  |/
  o  0: 8b2dca0c8a72 public 'base_commit' bookmark1
  
Make one of the commits public when it shouldn't be.
  $ hgmn phase -p 8d621fa11677
  $ hgmn cloud sync
  commitcloud: synchronizing 'client1' with 'user/test/default'
  remote: * DEBG Session with Mononoke started with uuid: * (glob)
  backing up stack rooted at ec61bf312a03
  remote: Command failed
  remote:   Error:
  remote:     bundle2_resolver error
  remote:   Root cause:
  remote:     SharedError {
  remote:         error: Compat {
  remote:             error: SharedError { error: Compat { error: BonsaiMappingNotFound(HgChangesetId(HgNodeHash(Sha1(8d621fa1167779dffcefe5cb813fc11f2f272874)))) } }
  remote:             
  remote:             While waiting for parents to complete
  remote:             
  remote:             While creating Changeset Some(HgNodeHash(Sha1(ec61bf312a03c1ae89f421ca46eba7fc8801129e))), uuid: * (glob)
  remote:         }
  remote:     }
  remote:   Caused by:
  remote:     Error while uploading data for changesets, hashes: [HgChangesetId(HgNodeHash(Sha1(ec61bf312a03c1ae89f421ca46eba7fc8801129e)))]
  remote:   Caused by:
  remote:     While creating Changeset Some(HgNodeHash(Sha1(ec61bf312a03c1ae89f421ca46eba7fc8801129e))), uuid: * (glob)
  push failed: stream ended unexpectedly (got 0 bytes, expected 4)
  retrying push with discovery
  remote: * DEBG Session with Mononoke started with uuid: * (glob)
  searching for changes
  backing up stack rooted at 77a917e6c3a5
  commitcloud: commits synchronized
  finished in * (glob)


Commit still becomes available in the other repo
  $ cd ../client2
  $ hgmn cloud sync
  commitcloud: synchronizing 'client2' with 'user/test/default'
  pulling ec61bf312a03 88d416aed919
  pulling from ssh://user@dummy/repo
  remote: * DEBG Session with Mononoke started with uuid: * (glob)
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 4 changesets with 0 changes to 0 files (+2 heads)
  new changesets * (glob)
  commitcloud: commits synchronized
  finished in * (glob)

# Mononoke order is not stable, so the stacks print stacks separately
  $ tglogpnr -r "::ec61bf312a03 - ::master_bookmark"
  o  ec61bf312a03 draft 'stack 1 second'
  |
  o  8d621fa11677 draft 'stack 1 first'
  |
  ~
  $ tglogpnr -r "::88d416aed919 - ::master_bookmark"
  o  88d416aed919 draft 'stack 2 second'
  |
  o  77a917e6c3a5 draft 'stack 2 first'
  |
  ~

Fix up that public commit, set it back to draft
  $ cd ../client1
  $ hg phase -fd 8d621fa11677

Clean up
  $ hgmn hide -r 'draft()' -q
  $ hgmn cloud sync -q
  $ cd ../client2
  $ hgmn cloud sync -q

  $ tglogp
  @  0: 8b2dca0c8a72 public 'base_commit' bookmark1
  
