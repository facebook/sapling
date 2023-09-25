#chg-compatible
  $ setconfig devel.segmented-changelog-rev-compat=true
  $ setconfig experimental.allowfilepeer=True

TODO: configure mutation
  $ configure dummyssh
  $ enable remotenames

Setup

  $ setconfig ui.username="nobody <no.reply@fb.com>"

  $ commit() {
  >   hg commit -d "0 0" -A -m "$@"
  > }

  $ log() {
  >   hg log -G -T "{desc} [{phase}:{node|short}] {bookmarks}" "$@"
  > }

Set up server repository

  $ newserver server
  $ echo foo > a
  $ echo foo > b
  $ commit 'initial'
  adding a
  adding b
  $ hg bookmark main

Set up client repository

  $ cd ..
  $ hg clone ssh://user@dummy/server client -q
  $ cd client
  $ echo "[extensions]" >> .hg/hgrc
  $ echo "pushrebase =" >> .hg/hgrc

Without server extension

  $ cd ../server
  $ echo 'bar' > a
  $ commit 'a => bar'

  $ cd ../client
  $ hg rm b
  $ commit 'b => xxx'
  $ echo 'baz' > b
  $ hg add b
  $ commit 'b => baz'

  $ echo "[experimental]" >> .hg/hgrc
  $ echo "bundle2-exp = True" >> .hg/hgrc

  $ echo "[experimental]" >> ../server/.hg/hgrc
  $ echo "bundle2-exp = True" >> ../server/.hg/hgrc

Stack of non-conflicting commits should be accepted

  $ cd ../server
  $ echo "[extensions]" >> .hg/hgrc
  $ echo "pushrebase =" >> .hg/hgrc
  $ log
  @  a => bar [draft:add0c792bfce] main
  │
  o  initial [draft:2bb9d20e471c]
  

  $ cd ../client
  $ log
  @  b => baz [draft:0e3997dc0733]
  │
  o  b => xxx [draft:46a2df24e272]
  │
  o  initial [public:2bb9d20e471c]
  
  $ hg push --to main --config devel.bundle2.debug=1 --debug 2>&1 | tee stuff | grep -v bundle2-
  running * (glob)
  sending hello command
  sending between command
  remote: 542
  remote: capabilities: * (glob)
  remote: 1
  pushing rev 0e3997dc0733 to destination ssh://user@dummy/server bookmark main
  query 1; heads
  sending batch command
  searching for changes
  local heads: 1; remote heads: 1 (explicit: 0); initial common: 0
  sampling from both directions (1 of 1)
  sampling undecided commits (1 of 1)
  query 2; still undecided: 1, sample size is: 1
  sending known command
  2 total queries in *s (glob)
  preparing listkeys for "bookmarks" with pattern "['main']"
  sending listkeyspatterns command
  received listkey for "bookmarks": 45 bytes
  validated revset for rebase
  2 changesets found
  list of changesets:
  46a2df24e27273bb06dbf28b085fcc2e911bf986
  0e3997dc073308e42715a44d345466690abfd09a
  sending unbundle command
  adding changesets
  adding manifests
  adding file changes
  updating bookmark main
  preparing listkeys for "bookmarks"
  sending listkeys command
  received listkey for "bookmarks": 45 bytes
  remote: pushing 2 changesets:
  remote:     46a2df24e272  b => xxx
  remote:     0e3997dc0733  b => baz
  remote: 3 new changesets from the server will be downloaded
  running * (glob)
  sending hello command
  sending between command
  remote: 542
  remote: capabilities: * (glob)
  remote: 1
  sending gettreepack command
  resolving manifests
   branchmerge: False, force: False
   ancestor: 0e3997dc0733, local: 0e3997dc0733+, remote: 4cfedb0dc25f
  reusing connection from pool
  sending getpackv1 command
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ log -R ../server
  o  b => baz [public:4cfedb0dc25f] main
  │
  o  b => xxx [public:6a6d9484552c]
  │
  @  a => bar [public:add0c792bfce]
  │
  o  initial [public:2bb9d20e471c]
  

Check that we did not generate any check:heads parts

  $ grep check:heads stuff
  [1]
  $ rm stuff

  $ cd ../server
  $ hg goto main
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ log
  @  b => baz [public:4cfedb0dc25f] main
  │
  o  b => xxx [public:6a6d9484552c]
  │
  o  a => bar [public:add0c792bfce]
  │
  o  initial [public:2bb9d20e471c]
  
  $ hg debugstrip -r 6a6d9484552c82e5f21b4ed4fce375930812f88c
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ cd ../client
  $ hg debugstrip add0c792bfce89610d277fd5b1e32f5287994d1d
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg up 0e3997dc0733
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ log
  @  b => baz [draft:0e3997dc0733]
  │
  o  b => xxx [draft:46a2df24e272]
  │
  o  initial [draft:2bb9d20e471c]
  

Push using changegroup2

  $ hg push --to main
  pushing rev 0e3997dc0733 to destination ssh://user@dummy/server bookmark main
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  updating bookmark main
  remote: pushing 2 changesets:
  remote:     46a2df24e272  b => xxx
  remote:     0e3997dc0733  b => baz
  remote: 3 new changesets from the server will be downloaded
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ log -R ../server
  o  b => baz [public:4cfedb0dc25f] main
  │
  o  b => xxx [public:6a6d9484552c]
  │
  @  a => bar [public:add0c792bfce]
  │
  o  initial [public:2bb9d20e471c]
  

  $ cd ../client
  $ hg debugstrip 46a2df24e27273bb06dbf28b085fcc2e911bf986
  $ hg pull
  pulling from ssh://user@dummy/server
  searching for changes
  no changes found
  $ hg goto default
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved

Regular commits should go through without changing hash

  $ cd ../client
  $ echo '[experimental]' >> .hg/hgrc
  $ echo 'bundle2.pushback = True' >> .hg/hgrc

  $ echo 'quux' > b
  $ commit 'b => quux'
  $ log -r tip
  @  b => quux [draft:741fd2094512]
  │
  ~

  $ hg push --to main
  pushing rev 741fd2094512 to destination ssh://user@dummy/server bookmark main
  searching for changes
  updating bookmark main
  remote: pushing 1 changeset:
  remote:     741fd2094512  b => quux

  $ log
  @  b => quux [public:741fd2094512]
  │
  o  b => baz [public:4cfedb0dc25f]
  │
  o  b => xxx [public:6a6d9484552c]
  │
  o  a => bar [public:add0c792bfce]
  │
  o  initial [public:2bb9d20e471c]
  
  $ cd ../server
  $ hg goto main
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ log
  @  b => quux [public:741fd2094512] main
  │
  o  b => baz [public:4cfedb0dc25f]
  │
  o  b => xxx [public:6a6d9484552c]
  │
  o  a => bar [public:add0c792bfce]
  │
  o  initial [public:2bb9d20e471c]
  

Stack with conflict in tail should abort

  $ cd ../server
  $ echo 'baz' > a
  $ commit 'a => baz'

  $ cd ../client
  $ echo 'quux' > a
  $ commit 'a => quux'
  $ echo 'foofoo' > b
  $ commit 'b => foofoo'
  $ hg push --to main
  pushing rev e9ea9556a371 to destination ssh://user@dummy/server bookmark main
  searching for changes
  remote: conflicting changes in:
      a
  remote: (pull and rebase your changes locally, then try again)
  abort: push failed on remote
  [255]

  $ hg debugstrip 'max(desc(a))'
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd ../server
  $ log
  @  a => baz [draft:fb983dc509b6] main
  │
  o  b => quux [public:741fd2094512]
  │
  o  b => baz [public:4cfedb0dc25f]
  │
  o  b => xxx [public:6a6d9484552c]
  │
  o  a => bar [public:add0c792bfce]
  │
  o  initial [public:2bb9d20e471c]
  

Stack with conflict in head should abort

  $ cd ../client
  $ echo 'foofoo' > b
  $ commit 'b => foofoo'
  $ echo 'quux' > a
  $ commit 'a => quux'
  $ hg push --to main
  pushing rev f691c6db9875 to destination ssh://user@dummy/server bookmark main
  searching for changes
  remote: conflicting changes in:
      a
  remote: (pull and rebase your changes locally, then try again)
  abort: push failed on remote
  [255]

  $ hg debugstrip 'max(desc(b))'
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ cd ../server
  $ log
  @  a => baz [draft:fb983dc509b6] main
  │
  o  b => quux [public:741fd2094512]
  │
  o  b => baz [public:4cfedb0dc25f]
  │
  o  b => xxx [public:6a6d9484552c]
  │
  o  a => bar [public:add0c792bfce]
  │
  o  initial [public:2bb9d20e471c]
  
Pushing a merge should rebase only the latest side of the merge

  $ hg book master -r tip
  $ cd ../client
  $ hg pull -q > /dev/null
  $ hg debugstrip -q -r tip
  $ hg up 741fd2094512
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo "tux" > other
  $ hg add other
  $ hg commit -qm "branch left"
  $ hg book master -r tip
  $ hg up -q 6a6d9484552c82e5f21b4ed4fce375930812f88c
  $ echo branched > c
  $ hg commit -Aqm "branch start"
  $ echo branched2 > c
  $ hg commit -qm "branch middle"
  $ hg merge -q master
  $ hg commit -qm "merge"
  $ echo ontopofmerge > c
  $ hg commit -qm "on top of merge"
  $ hg book master -r tip
  moving bookmark 'master' forward from e6b7549904cd
  $ hg debugmakepublic 741fd2094512
  $ log
  @  on top of merge [draft:9007d6a204f8] master
  │
  o    merge [draft:2c0c699d7086]
  ├─╮
  │ o  branch middle [draft:5a0cbf3df4ef]
  │ │
  │ o  branch start [draft:add5ec74853d]
  │ │
  o │  branch left [draft:e6b7549904cd]
  │ │
  o │  b => quux [public:741fd2094512]
  │ │
  o │  b => baz [public:4cfedb0dc25f]
  ├─╯
  o  b => xxx [public:6a6d9484552c]
  │
  o  a => bar [public:add0c792bfce]
  │
  o  initial [public:2bb9d20e471c]
  
  $ log -R ../server
  @  a => baz [draft:fb983dc509b6] main master
  │
  o  b => quux [public:741fd2094512]
  │
  o  b => baz [public:4cfedb0dc25f]
  │
  o  b => xxx [public:6a6d9484552c]
  │
  o  a => bar [public:add0c792bfce]
  │
  o  initial [public:2bb9d20e471c]
  
  $ hg push --to main
  pushing rev 9007d6a204f8 to destination ssh://user@dummy/server bookmark main
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  updating bookmark main
  remote: pushing 5 changesets:
  remote:     e6b7549904cd  branch left
  remote:     add5ec74853d  branch start
  remote:     5a0cbf3df4ef  branch middle
  remote:     2c0c699d7086  merge
  remote:     9007d6a204f8  on top of merge
  remote: 6 new changesets from the server will be downloaded
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd ../server
  $ log
  o  on top of merge [public:54b35e8b58eb] main
  │
  o    merge [public:5a512eb2b3f8]
  ├─╮
  │ o  branch middle [public:5a0cbf3df4ef]
  │ │
  │ o  branch start [public:add5ec74853d]
  │ │
  o │  branch left [public:cf07bdf4226e]
  │ │
  @ │  a => baz [public:fb983dc509b6] master
  │ │
  o │  b => quux [public:741fd2094512]
  │ │
  o │  b => baz [public:4cfedb0dc25f]
  ├─╯
  o  b => xxx [public:6a6d9484552c]
  │
  o  a => bar [public:add0c792bfce]
  │
  o  initial [public:2bb9d20e471c]
  
- Verify the content of the merge files is correct
  $ hg cat -r "main^" c
  branched2
  $ hg cat -r "main^" other
  tux

  $ hg debugstrip -r add5ec74853d -q
  $ cd ../client
  $ hg debugstrip -r add5ec74853d -q
  $ hg book -d master
  $ hg -R ../server book -d master

With evolution enabled, should set obsolescence markers

  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > rebase =
  > 
  > [experimental]
  > evolution = createmarkers
  > EOF

  $ cd ../client
  $ hg debugstrip -qr fb983dc509b6
  $ hg debugstrip -qr e6b7549904cd2a7991ef25bc2e0fd910801af2cd
  $ hg up -q 741fd2094512
  $ hg mv b k
  $ commit 'b => k'
  $ hg mv k b
  $ echo 'foobar' > b
  $ commit 'b => foobar'
  $ log
  @  b => foobar [draft:e73acfaeee82]
  │
  o  b => k [draft:9467a8ee5d0d]
  │
  o  b => quux [public:741fd2094512]
  │
  o  b => baz [public:4cfedb0dc25f]
  │
  o  b => xxx [public:6a6d9484552c]
  │
  o  a => bar [public:add0c792bfce]
  │
  o  initial [public:2bb9d20e471c]
  
  $ hg push --to main
  pushing rev e73acfaeee82 to destination ssh://user@dummy/server bookmark main
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  updating bookmark main
  remote: pushing 2 changesets:
  remote:     9467a8ee5d0d  b => k
  remote:     e73acfaeee82  b => foobar
  remote: 4 new changesets from the server will be downloaded
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ hg pull
  pulling from ssh://user@dummy/server
  searching for changes
  no changes found
  $ hg up d53a62ed14be
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ log
  @  b => foobar [public:d53a62ed14be]
  │
  o  b => k [public:0d76868c25e6]
  │
  o  branch left [public:cf07bdf4226e]
  │
  o  a => baz [public:fb983dc509b6]
  │
  o  b => quux [public:741fd2094512]
  │
  o  b => baz [public:4cfedb0dc25f]
  │
  o  b => xxx [public:6a6d9484552c]
  │
  o  a => bar [public:add0c792bfce]
  │
  o  initial [public:2bb9d20e471c]
  

  $ cd ../server
  $ hg log -r 0d76868c25e6 -T '{file_copies}\n'
  k (b)
  $ log
  o  b => foobar [public:d53a62ed14be] main
  │
  o  b => k [public:0d76868c25e6]
  │
  o  branch left [public:cf07bdf4226e]
  │
  @  a => baz [public:fb983dc509b6]
  │
  o  b => quux [public:741fd2094512]
  │
  o  b => baz [public:4cfedb0dc25f]
  │
  o  b => xxx [public:6a6d9484552c]
  │
  o  a => bar [public:add0c792bfce]
  │
  o  initial [public:2bb9d20e471c]
  
Test pushing master bookmark, fast forward

  $ hg book -r fb983dc509b6 master
  $ cd ../client
  $ hg book master
  $ echo 'babar' > b
  $ commit 'b => babar'
  $ hg log -r master -T"{node}\n"
  56b2e094996609874ae1c9aae1626bfba61d07d8
  $ hg push --to master
  pushing rev 56b2e0949966 to destination ssh://user@dummy/server bookmark master
  searching for changes
  updating bookmark master
  remote: pushing 1 changeset:
  remote:     56b2e0949966  b => babar
  $ hg log -r master -R ../server -T"{node}\n"
  56b2e094996609874ae1c9aae1626bfba61d07d8

Test pushing bookmark with no new commit

  $ hg book stable -r fb983dc509b6
  $ hg book stable -r "fb983dc509b6^" -R ../server
  $ hg push -r stable --to stable
  pushing rev fb983dc509b6 to destination ssh://user@dummy/server bookmark stable
  searching for changes
  no changes found
  updating bookmark stable
  $ hg log -r stable -R ../server
  commit:      fb983dc509b6
  bookmark:    stable
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     a => baz
  

  $ cd ..

Test that hooks are fired with the correct variables

  $ newserver hookserver
  $ cat >> .hg/hgrc <<EOF
  > [hooks]
  > changegroup = python "$RUNTESTDIR/printenv.py" changegroup
  > outgoing = python "$RUNTESTDIR/printenv.py" outgoing
  > prechangegroup = python "$RUNTESTDIR/printenv.py" prechangegroup
  > preoutgoing = python "$RUNTESTDIR/printenv.py" preoutgoing
  > pretxnchangegroup = python "$RUNTESTDIR/printenv.py" pretxnchangegroup
  > txnclose = python "$RUNTESTDIR/printenv.py" txnclose
  > pretxnclose = python "$RUNTESTDIR/printenv.py" pretxnclose
  > prepushkey = python "$RUNTESTDIR/printenv.py" prepushkey
  > [extensions]
  > pushrebase=
  > EOF
  $ touch file && hg ci -Aqm initial
  pretxnclose hook: HG_HOOKNAME=pretxnclose HG_HOOKTYPE=pretxnclose HG_PENDING=$TESTTMP/hookserver HG_PENDING_METALOG={"$TESTTMP/hookserver/.hg/store/metalog": "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx"} HG_PHASES_MOVED=1 HG_SHAREDPENDING=$TESTTMP/hookserver HG_TXNID=TXN:$ID$ HG_TXNNAME=commit
  txnclose hook: HG_HOOKNAME=txnclose HG_HOOKTYPE=txnclose HG_PHASES_MOVED=1 HG_TXNID=TXN:$ID$ HG_TXNNAME=commit
  $ hg bookmark master
  pretxnclose hook: HG_BOOKMARK_MOVED=1 HG_HOOKNAME=pretxnclose HG_HOOKTYPE=pretxnclose HG_PENDING=$TESTTMP/hookserver HG_SHAREDPENDING=$TESTTMP/hookserver HG_TXNID=TXN:$ID$ HG_TXNNAME=bookmark
  txnclose hook: HG_BOOKMARK_MOVED=1 HG_HOOKNAME=txnclose HG_HOOKTYPE=txnclose HG_TXNID=TXN:* HG_TXNNAME=bookmark (glob)
  $ cd ../

  $ hg clone -q ssh://user@dummy/hookserver hookclient
  $ cd hookclient
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > pushrebase=
  > EOF
  $ hg goto master
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo >> file && hg ci -Aqm first
  $ echo >> file && hg ci -Aqm second
  $ echo >> file && hg ci -Aqm last
  $ hg push --to master
  pushing rev a5e72ac0df88 to destination ssh://user@dummy/hookserver bookmark master
  searching for changes
  updating bookmark master
  remote: pushing 3 changesets:
  remote:     4fcee35c508c  first
  remote:     11be4ca7f3f4  second
  remote:     a5e72ac0df88  last
  remote: prechangegroup hook: HG_BUNDLE2=1 HG_HOOKNAME=prechangegroup HG_HOOKTYPE=prechangegroup HG_SOURCE=serve HG_TXNID=TXN:$ID$ HG_URL=* (glob)
  remote: pretxnchangegroup hook: HG_BUNDLE2=1 HG_HOOKNAME=pretxnchangegroup HG_HOOKTYPE=pretxnchangegroup HG_NODE=4fcee35c508c1019667f72cae9b843efa8908701 HG_NODE_LAST=a5e72ac0df8881afef34132987e8ae78d2e6cb13 HG_PENDING=$TESTTMP/hookserver HG_PENDING_METALOG={"$TESTTMP/hookserver/.hg/store/metalog": "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx"} HG_SHAREDPENDING=$TESTTMP/hookserver HG_SOURCE=serve HG_TXNID=TXN:$ID$ HG_URL=* (glob)
  remote: prepushkey hook: HG_BUNDLE2=1 HG_HOOKNAME=prepushkey HG_HOOKTYPE=prepushkey HG_KEY=master HG_NAMESPACE=bookmarks HG_NEW=a5e72ac0df8881afef34132987e8ae78d2e6cb13 HG_NODE=4fcee35c508c1019667f72cae9b843efa8908701 HG_OLD=e95be919ac60f0c114075e32a0a4301afabadb60 HG_PENDING=$TESTTMP/hookserver HG_PENDING_METALOG={"$TESTTMP/hookserver/.hg/store/metalog": "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx"} HG_PHASES_MOVED=1 HG_SHAREDPENDING=$TESTTMP/hookserver HG_SOURCE=serve HG_TXNID=TXN:$ID$ HG_URL=* (glob)
  remote: pretxnclose hook: HG_BOOKMARK_MOVED=1 HG_BUNDLE2=1 HG_HOOKNAME=pretxnclose HG_HOOKTYPE=pretxnclose HG_NODE=4fcee35c508c1019667f72cae9b843efa8908701 HG_PENDING=$TESTTMP/hookserver HG_PENDING_METALOG={"$TESTTMP/hookserver/.hg/store/metalog": "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx"} HG_PHASES_MOVED=1 HG_SHAREDPENDING=$TESTTMP/hookserver HG_SOURCE=serve HG_TXNID=TXN:$ID$ HG_TXNNAME=serve HG_URL=* (glob)
  remote: txnclose hook: HG_BOOKMARK_MOVED=1 HG_BUNDLE2=1 HG_HOOKNAME=txnclose HG_HOOKTYPE=txnclose HG_NODE=4fcee35c508c1019667f72cae9b843efa8908701 HG_PHASES_MOVED=1 HG_SOURCE=serve HG_TXNID=TXN:$ID$ HG_TXNNAME=serve HG_URL=* (glob)
  remote: changegroup hook: HG_BUNDLE2=1 HG_HOOKNAME=changegroup HG_HOOKTYPE=changegroup HG_NODE=4fcee35c508c1019667f72cae9b843efa8908701 HG_NODE_LAST=a5e72ac0df8881afef34132987e8ae78d2e6cb13 HG_SOURCE=serve HG_TXNID=TXN:$ID$ HG_URL=* (glob)


  $ cd ../

Test that failing prechangegroup hooks block the push

  $ newserver hookserver2
  $ cat >> .hg/hgrc <<EOF
  > [hooks]
  > prechangegroup = exit 1
  > [extensions]
  > pushrebase=
  > EOF
  $ touch file && hg ci -Aqm initial
  $ hg bookmark master
  $ cd ../

  $ hg clone -q ssh://user@dummy/hookserver2 hookclient2
  $ cd hookclient2
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > pushrebase=
  > EOF
  $ hg goto master
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo >> file && hg ci -Aqm first
  $ echo >> file && hg ci -Aqm second
  $ echo >> file && hg ci -Aqm last
  $ hg push --to master
  pushing rev a5e72ac0df88 to destination ssh://user@dummy/hookserver2 bookmark master
  searching for changes
  remote: prechangegroup hook exited with status 1
  remote: pushing 3 changesets:
  remote:     4fcee35c508c  first
  remote:     11be4ca7f3f4  second
  remote:     a5e72ac0df88  last
  abort: push failed on remote
  [255]

  $ cd ..

Test date rewriting

  $ newserver rewritedate
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > pushrebase=
  > [pushrebase]
  > rewritedates = True
  > EOF
  $ touch a && hg commit -Aqm a
  $ touch b && hg commit -Aqm b
  $ hg book master
  $ cd ..

  $ hg clone ssh://user@dummy/rewritedate rewritedateclient
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  updating to branch default
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd rewritedateclient
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > pushrebase=
  > EOF
  $ hg up 'desc(a)'
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ touch c && hg commit -Aqm c

  $ cat > $TESTTMP/daterewrite.py <<EOF
  > import sys, time
  > from sapling import extensions
  > def extsetup(ui):
  >     def faketime(orig):
  >         return 1000000000
  >     extensions.wrapfunction(time, 'time', faketime)
  > EOF
  $ cat >> ../rewritedate/.hg/hgrc <<EOF
  > [extensions]
  > daterewrite=$TESTTMP/daterewrite.py
  > EOF
  $ hg push --to master
  pushing rev d5e255ef74f8 to destination ssh://user@dummy/rewritedate bookmark master
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  updating bookmark master
  remote: pushing 1 changeset:
  remote:     d5e255ef74f8  c
  remote: 1 new changeset from the server will be downloaded
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg log -G -T '{desc} {date|isodate}'
  @  c 2001-09-09 01:46 +0000
  │
  o  b 1970-01-01 00:00 +0000
  │
  o  a 1970-01-01 00:00 +0000
  
Test date rewriting with a merge commit

  $ hg up -q 'desc(a)'
  $ echo x >> x
  $ hg commit -qAm x
  $ hg up -q 'max(desc(c))'
  $ echo y >> y
  $ hg commit -qAm y
  $ hg merge -q 'desc(x)'
  $ hg commit -qm merge
  $ hg push --to master
  pushing rev 4514adb1f536 to destination ssh://user@dummy/rewritedate bookmark master
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  updating bookmark master
  remote: pushing 3 changesets:
  remote:     a5f9a9a43049  x
  remote:     c1392466a61e  y
  remote:     4514adb1f536  merge
  remote: 3 new changesets from the server will be downloaded
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ cd ..

Test pushrebase on merge commit where master is on the p2 side

  $ newserver p2mergeserver
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > pushrebase=
  > EOF
  $ echo a >> a && hg commit -Aqm 'add a'
  $ hg bookmark master

  $ cd ..
  $ hg clone -q ssh://user@dummy/p2mergeserver p2mergeclient
  $ cd p2mergeclient
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > pushrebase=
  > EOF
  $ hg up -q null
  $ echo b >> b && hg commit -Aqm 'add b'
  $ hg up -q null
  $ echo c >> c && hg commit -Aqm 'add c'
  $ hg merge -q cde40f86152f76163041ff50d68d2e8fddc1b46b
  $ hg commit -m 'merge b and c'
  $ hg push --to master
  pushing rev 4ae459502279 to destination ssh://user@dummy/p2mergeserver bookmark master
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  updating bookmark master
  remote: pushing 3 changesets:
  remote:     cde40f86152f  add b
  remote:     6c337f0241b3  add c
  remote:     4ae459502279  merge b and c
  remote: 3 new changesets from the server will be downloaded
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg -R ../p2mergeserver log -G -T '{desc}'
  o    merge b and c
  ├─╮
  │ o  add c
  │
  o  add b
  │
  @  add a
  
  $ hg -R ../p2mergeserver manifest -r 7c3bad9141dcb46ff89abf5f61856facd56e476c
  a
  b
  $ hg -R ../p2mergeserver manifest -r 'desc(merge)'
  a
  b
  c
  $ cd ..

Test force pushes
  $ newserver forcepushserver
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > pushrebase=
  > EOF
  $ hg bookmark master
  $ echo a > a && hg commit -Aqm a
  $ cd ..

  $ hg clone ssh://user@dummy/forcepushserver forcepushclient
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd forcepushserver
  $ echo a >> a && hg commit -Aqm aa

  $ cd ../forcepushclient
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > pushrebase=
  > EOF
  $ hg up 'desc(a)'
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo b >> a && hg commit -Aqm b
  $ hg push -f --to master
  pushing rev 1846eede8b68 to destination ssh://user@dummy/forcepushserver bookmark master
  searching for changes
  updating bookmark master
  remote: pushing 1 changeset:
  remote:     1846eede8b68  b
  $ hg pull
  pulling from ssh://user@dummy/forcepushserver
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  $ hg log -G -T '{desc} {bookmarks}'
  o  aa
  │
  │ @  b
  ├─╯
  o  a
  
Make sure that no hg-bundle-* files left
(the '|| true' and '*' prefix is because ls has different behavior on linux
and osx)
  $ ls ../server/.hg/hg-bundle-* || true
  ls: *../server/.hg/hg-bundle-*: $ENOENT$ (glob)

Server with obsstore disabled can still send obsmarkers useful to client, and
phase is updated correctly with the marker information.

  $ cat >> $HGRCPATH << EOF
  > [experimental]
  > evolution =
  > EOF

  $ newserver server1
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > pushrebase=
  > EOF
  $ echo a > a
  $ hg commit -m a -A a -q
  $ hg bookmark main
  $ cd ..

  $ hg clone -q ssh://user@dummy/server1 client1
  $ cd client1
  $ enable pushrebase
  $ cd ../server1
  $ echo b > b
  $ hg commit -m b -A b -q

  $ cd ../client1
  $ echo a2 >> a
  $ hg commit -m a2
  $ cat >> .hg/hgrc <<EOF
  > [experimental]
  > evolution = all
  > EOF

  $ hg book -i BOOK
  $ hg push -r . --to main
  pushing rev 045279cde9f0 to destination ssh://user@dummy/server1 bookmark main
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  updating bookmark main
  remote: pushing 1 changeset:
  remote:     045279cde9f0  a2
  remote: 2 new changesets from the server will be downloaded
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg up tip -q
  $ log --hidden
  @  a2 [public:722505d780e3] BOOK
  │
  o  b [public:d2ae7f538514]
  │
  │ x  a2 [draft:045279cde9f0]
  ├─╯
  o  a [public:cb9a9f314b8b]
  
  $ log
  @  a2 [public:722505d780e3] BOOK
  │
  o  b [public:d2ae7f538514]
  │
  o  a [public:cb9a9f314b8b]
  
Push a file-copy changeset and the copy source gets modified by others:

  $ cd $TESTTMP
  $ newserver server2

  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > pushrebase=
  > EOF

  $ echo 1 > A
  $ hg commit -m A -A A
  $ hg bookmark main

  $ cd ..
  $ hg clone -q ssh://user@dummy/server2 client2

  $ cd client2
  $ enable pushrebase
  $ hg cp A B
  $ hg commit -m 'Copy A to B'

  $ cd ../server2
  $ echo 2 >> A
  $ hg commit -m 'Modify A' A

  $ cd ../client2
  $ cat >> .hg/hgrc <<EOF
  > [experimental]
  > evolution = all
  > EOF

  $ hg push -r . --to main
  pushing rev 40d149b24655 to destination ssh://user@dummy/server2 bookmark main
  searching for changes
  remote: conflicting changes in:
      A
  remote: (pull and rebase your changes locally, then try again)
  abort: push failed on remote
  [255]

Push an already-public changeset and confirm it is rejected

  $ hg goto -q '.^'
  $ echo 2 > C
  $ hg commit -m C -A C
  $ hg debugmakepublic -r.
  $ hg push -r . --to main
  pushing rev 3850a85c4706 to destination ssh://user@dummy/server2 bookmark main
  searching for changes
  abort: cannot rebase public changesets: 3850a85c4706
  [255]

  $ echo 3 >> C
  $ hg commit -m C2
  $ echo 4 >> C
  $ hg commit -m C3
  $ echo 5 >> C
  $ hg commit -m C4
  $ hg debugmakepublic -r.
  $ hg push -r . --to main
  pushing rev 5d92bb0ab776 to destination ssh://user@dummy/server2 bookmark main
  searching for changes
  abort: cannot rebase public changesets: 3850a85c4706, 50b1220b7c4e, de211a1843b7, ...
  [255]
