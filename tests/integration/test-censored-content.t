  $ . "${TEST_FIXTURES}/library.sh"

setup configuration

  $ REPOTYPE="blob:files"
  $ setup_common_config $REPOTYPE

  $ cd $TESTTMP

setup hg server repo

  $ hginit_treemanifest repo-hg
  $ cd repo-hg
  $ touch a && hg ci -A -q -m 'add a'

  $ hg log -T '{short(node)}\n'
  ac82d8b1f7c4

create master bookmark
  $ hg bookmark master_bookmark -r tip

  $ cd $TESTTMP

setup repo-pull and repo-push
  $ hgclone_treemanifest ssh://user@dummy/repo-hg repo-push --noupdate
  $ hgclone_treemanifest ssh://user@dummy/repo-hg repo-push2 --noupdate
  $ hgclone_treemanifest ssh://user@dummy/repo-hg repo-push3 --noupdate
  $ hgclone_treemanifest ssh://user@dummy/repo-hg repo-pull --noupdate

blobimport
  $ blobimport repo-hg/.hg repo

start mononoke
  $ mononoke
  $ wait_for_mononoke $TESTTMP/repo
  $ cd repo-push
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > pushrebase =
  > rebase =
  > remotenames =
  > EOF

  $ cd ../repo-push2
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > pushrebase =
  > remotenames =
  > EOF

  $ cd ../repo-push3
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > rebase =
  > EOF

  $ cd ../repo-push

  $ hgmn up -q 0
Push files
  $ echo b > b
  $ hg ci -A -q -m "add b"

  $ hgmn push -q -r .  --to master_bookmark

  $ echo c > c
  $ hg ci -A -q -m "add censored c"

  $ hgmn push -q -r .  --to master_bookmark

  $ tglogpnr
  @  064d994d0240 public 'add censored c'
  |
  o  14961831bd3a public 'add b'
  |
  o  ac82d8b1f7c4 public 'add a' master_bookmark
  

  $ cd "$TESTTMP/repo-push2"
  $ hgmn pull -q

  $ hgmn up -q 064d994d0240
  $ tglogpnr
  @  064d994d0240 public 'add censored c'
  |
  o  14961831bd3a public 'add b'
  |
  o  ac82d8b1f7c4 public 'add a' master_bookmark
  

  $ cd "$TESTTMP/repo-pull"

  $ hgmn pull -q

  $ tglogpnr
  o  064d994d0240 public 'add censored c' master_bookmark
  |
  o  14961831bd3a public 'add b'
  |
  o  ac82d8b1f7c4 public 'add a'
  

  $ cd "$TESTTMP/repo-push3"

  $ hgmn pull
  pulling from ssh://user@dummy/repo
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 0 changes to 0 files
  updating bookmark master_bookmark
  new changesets 14961831bd3a:064d994d0240

  $ tglogpnr
  o  064d994d0240 public 'add censored c' master_bookmark
  |
  o  14961831bd3a public 'add b'
  |
  o  ac82d8b1f7c4 public 'add a'
  

  $ hgmn up master_bookmark
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (activating bookmark master_bookmark)

Update blacklisted blob
  $ cd "$TESTTMP/repo-push"
  $ echo "testcupdate" > c
  $ hg ci -q -m "uncensore c"

  $ hgmn push -q -r .  --to master_bookmark

  $ tglogpnr
  @  bbb84cdc8ec0 public 'uncensore c'
  |
  o  064d994d0240 public 'add censored c'
  |
  o  14961831bd3a public 'add b'
  |
  o  ac82d8b1f7c4 public 'add a' master_bookmark
  

Censore the blacklisted blob (file 'c' contained in commit '064d994d0240')
  $ sqlite3 "$TESTTMP/repo/censored_contents" \
  > "insert into censored_contents \
  > (id, content_key, task, add_timestamp) \
  > values(0, 'repo0000.content.blake2.096c8cc4a38f793ac05fc3506ed6346deb5b857100642adbf4de6720411b10e2', 'task', 0)";

Restart mononoke
  $ kill $MONONOKE_PID
  $ rm -rf $TESTTMP/mononoke-config
  $ setup_common_config blob:files
  $ mononoke
  $ wait_for_mononoke $TESTTMP/repo

  $ cd "$TESTTMP/repo-pull"
  $ tglogpnr
  o  064d994d0240 public 'add censored c' master_bookmark
  |
  o  14961831bd3a public 'add b'
  |
  o  ac82d8b1f7c4 public 'add a'
  

Expect error with a suggestive explanation (the last commit contains a censored blob) in repo-pull
  $ hgmn up master_bookmark
  remote: Command failed
  remote:   Error:
  remote:     Error while deserializing file contents retrieved from key 'content.blake2.096c8cc4a38f793ac05fc3506ed6346deb5b857100642adbf4de6720411b10e2'
  remote:   Root cause:
  remote:     Censored(
  remote:         "repo0000.content.blake2.096c8cc4a38f793ac05fc3506ed6346deb5b857100642adbf4de6720411b10e2",
  remote:         "task",
  remote:     )
  remote:   Caused by:
  remote:     While fetching content blob
  remote:   Caused by:
  remote:     The blob repo0000.content.blake2.096c8cc4a38f793ac05fc3506ed6346deb5b857100642adbf4de6720411b10e2 is censored. 
  remote:      Task/Sev: task
  abort: error downloading file contents:
  'connection closed early for filename * and node *' (glob)
  [255]

  $ tglogpnr
  o  064d994d0240 public 'add censored c' master_bookmark
  |
  o  14961831bd3a public 'add b'
  |
  o  ac82d8b1f7c4 public 'add a'
  

Try push a new version of a blacklisted blob
  $ cd "$TESTTMP/repo-push2"
  $ touch "test12" > c
  $ hg ci -q -m "update c"

  $ tglogpnr
  @  bb65510879c8 draft 'update c'
  |
  o  064d994d0240 public 'add censored c'
  |
  o  14961831bd3a public 'add b'
  |
  o  ac82d8b1f7c4 public 'add a' master_bookmark
  

Should send an error since the blob is blacklisted
  $ hgmn push -r . --to master_bookmark
  pushing rev bb65510879c8 to destination ssh://user@dummy/repo bookmark master_bookmark
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     While resolving Changegroup
  remote:   Root cause:
  remote:     SharedError {
  remote:         error: Compat {
  remote:             error: Censored("repo0000.content.blake2.096c8cc4a38f793ac05fc3506ed6346deb5b857100642adbf4de6720411b10e2", "task")
  remote:             
  remote:             While fetching content blob
  remote:             
  remote:             Error while deserializing file contents retrieved from key 'content.blake2.096c8cc4a38f793ac05fc3506ed6346deb5b857100642adbf4de6720411b10e2'
  remote:             
  remote:             While looking for base HgNodeHash(Sha1(149da44f2a4e14f488b7bd4157945a9837408c00)) to apply on delta HgNodeHash(Sha1(51fbfc693e1534e3e7be909e2966777573efc917)),
  remote:         },
  remote:     }
  remote:   Caused by:
  remote:     While uploading File Blobs
  remote:   Caused by:
  remote:     While decoding delta cache for file id 51fbfc693e1534e3e7be909e2966777573efc917, path c
  remote:   Caused by:
  remote:     While looking for base HgNodeHash(Sha1(149da44f2a4e14f488b7bd4157945a9837408c00)) to apply on delta HgNodeHash(Sha1(51fbfc693e1534e3e7be909e2966777573efc917))
  abort: stream ended unexpectedly (got 0 bytes, expected 4)
  [255]

  $ tglogpnr
  @  bb65510879c8 draft 'update c'
  |
  o  064d994d0240 public 'add censored c'
  |
  o  14961831bd3a public 'add b'
  |
  o  ac82d8b1f7c4 public 'add a' master_bookmark
  

  $ cd "$TESTTMP/repo-pull"
  $ tglogpnr
  o  064d994d0240 public 'add censored c' master_bookmark
  |
  o  14961831bd3a public 'add b'
  |
  o  ac82d8b1f7c4 public 'add a'
  

  $ hgmn pull -q

  $ tglogpnr
  o  bbb84cdc8ec0 public 'uncensore c' master_bookmark
  |
  o  064d994d0240 public 'add censored c'
  |
  o  14961831bd3a public 'add b'
  |
  o  ac82d8b1f7c4 public 'add a'
  

Expect error with a suggestive explanation (the commit contains a censored file)
  $ hgmn up 064d994d0240
  remote: Command failed
  remote:   Error:
  remote:     Error while deserializing file contents retrieved from key 'content.blake2.096c8cc4a38f793ac05fc3506ed6346deb5b857100642adbf4de6720411b10e2'
  remote:   Root cause:
  remote:     Censored(
  remote:         "repo0000.content.blake2.096c8cc4a38f793ac05fc3506ed6346deb5b857100642adbf4de6720411b10e2",
  remote:         "task",
  remote:     )
  remote:   Caused by:
  remote:     While fetching content blob
  remote:   Caused by:
  remote:     The blob repo0000.content.blake2.096c8cc4a38f793ac05fc3506ed6346deb5b857100642adbf4de6720411b10e2 is censored. 
  remote:      Task/Sev: task
  abort: error downloading file contents:
  'connection closed early for filename * and node *' (glob)
  [255]

Expect success (no blob in this commit is blacklisted)
  $ hgmn up bbb84cdc8ec0
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ tglogpnr
  @  bbb84cdc8ec0 public 'uncensore c' master_bookmark
  |
  o  064d994d0240 public 'add censored c'
  |
  o  14961831bd3a public 'add b'
  |
  o  ac82d8b1f7c4 public 'add a'
  

Test rebasing local commit on top of master_bookmark, when base commit contains censored blob
  $ cd "$TESTTMP/repo-push3"
  $ tglogpnr
  @  064d994d0240 public 'add censored c' master_bookmark
  |
  o  14961831bd3a public 'add b'
  |
  o  ac82d8b1f7c4 public 'add a'
  

  $ echo "aa" > a
  $ hg ci -q -m "update a"

  $ hgmn pull -q
  divergent bookmark master_bookmark stored as master_bookmark@default
  $ tglogpnr
  o  bbb84cdc8ec0 public 'uncensore c' master_bookmark@default
  |
  | @  c6e4e7cae299 draft 'update a' master_bookmark
  |/
  o  064d994d0240 public 'add censored c'
  |
  o  14961831bd3a public 'add b'
  |
  o  ac82d8b1f7c4 public 'add a'
  

Should be successful
  $ hgmn rebase -s . -d bbb84cdc8ec0
  rebasing 3:c6e4e7cae299 "update a" (master_bookmark)

  $ tglogpnr
  @  d967612e0cc1 draft 'update a' master_bookmark
  |
  o  bbb84cdc8ec0 public 'uncensore c'
  |
  o  064d994d0240 public 'add censored c'
  |
  o  14961831bd3a public 'add b'
  |
  o  ac82d8b1f7c4 public 'add a'
  
