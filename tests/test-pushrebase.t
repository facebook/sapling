Setup

  $ PYTHONPATH=$TESTDIR/..:$PYTHONPATH
  $ export PYTHONPATH

  $ cat >> $HGRCPATH <<EOF
  > [ui]
  > ssh = python "$RUNTESTDIR/dummyssh"
  > username = nobody <no.reply@fb.com>
  > [extensions]
  > strip =
  > EOF

  $ commit() {
  >   hg commit -d "0 0" -A -m "$@"
  > }

  $ log() {
  >   hg log -G -T "{desc} [{phase}:{node|short}] {bookmarks}" "$@"
  > }

Set up server repository

  $ hg init server
  $ cd server
  $ echo foo > a
  $ echo foo > b
  $ commit 'initial'
  adding a
  adding b

Set up client repository

  $ cd ..
  $ hg clone ssh://user@dummy/server client -q
  $ cd client
  $ echo "[extensions]" >> .hg/hgrc
  $ echo "bundle2hooks =" >> .hg/hgrc
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
  $ hg push
  pushing to ssh://user@dummy/server
  searching for changes
  remote has heads on branch 'default' that are not known locally: add0c792bfce
  abort: push creates new remote head 0e3997dc0733!
  (pull and merge or see 'hg help push' for details about pushing new heads)
  [255]

  $ hg --config experimental.bundle2-exp=False push --to default
  pushing to ssh://user@dummy/server
  searching for changes
  abort: no server support for 'b2x:rebase'
  [255]

  $ echo "[experimental]" >> .hg/hgrc
  $ echo "bundle2-exp = True" >> .hg/hgrc
  $ hg push --to default
  pushing to ssh://user@dummy/server
  searching for changes
  abort: no server support for 'b2x:rebase'
  [255]

  $ echo "[experimental]" >> ../server/.hg/hgrc
  $ echo "bundle2-exp = True" >> ../server/.hg/hgrc
  $ hg push --to default
  pushing to ssh://user@dummy/server
  searching for changes
  abort: no server support for 'b2x:rebase'
  [255]

Stack of non-conflicting commits should be accepted

  $ cd ../server
  $ echo "[extensions]" >> .hg/hgrc
  $ echo "bundle2hooks =" >> .hg/hgrc
  $ echo "pushrebase =" >> .hg/hgrc
  $ log
  @  a => bar [draft:add0c792bfce]
  |
  o  initial [draft:2bb9d20e471c]
  

  $ cd ../client
  $ log
  @  b => baz [draft:0e3997dc0733]
  |
  o  b => xxx [draft:46a2df24e272]
  |
  o  initial [public:2bb9d20e471c]
  
  $ hg push --to default --config devel.bundle2.debug=1 --debug | tee stuff | grep -v bundle2-
  pushing to ssh://user@dummy/server
  running python * user@dummy 'hg -R server serve --stdio' (glob)
  sending hello command
  sending between command
  remote: 388
  remote: capabilities: lookup changegroupsubset branchmap pushkey known getbundle unbundlehash batch streamreqs=generaldelta,revlogv1 bundle2=HG20%0Ab2x%253Arebase%0Achangegroup%3D01%2C02%0Adigests%3Dmd5%2Csha1%2Csha512%0Aerror%3Dabort%2Cunsupportedcontent%2Cpushraced%2Cpushkey%0Ahgtagsfnodes%0Alistkeys%0Apushkey%0Aremote-changegroup%3Dhttp%2Chttps unbundle=HG10GZ,HG10BZ,HG10UN httpheader=1024
  remote: 1
  query 1; heads
  sending batch command
  searching for changes
  taking quick initial sample
  query 2; still undecided: 2, sample size is: 2
  sending known command
  2 total queries
  preparing listkeys for "phases"
  sending listkeys command
  received listkey for "phases": 58 bytes
  checking for updated bookmarks
  preparing listkeys for "bookmarks"
  sending listkeys command
  received listkey for "bookmarks": 0 bytes
  validated revset for rebase
  2 changesets found
  list of changesets:
  46a2df24e27273bb06dbf28b085fcc2e911bf986
  0e3997dc073308e42715a44d345466690abfd09a
  sending unbundle command
  remote: pushing 2 changsets:
  remote:     46a2df24e272  b => xxx
  remote:     0e3997dc0733  b => baz
  remote: 3 new changesets from the server will be downloaded
  adding changesets
  add changeset add0c792bfce
  add changeset 6a6d9484552c
  add changeset 4cfedb0dc25f
  adding manifests
  adding file changes
  adding a revisions
  adding b revisions
  added 3 changesets with 1 changes to 2 files (+1 heads)
  updating the branch cache
  preparing listkeys for "phases"
  sending listkeys command
  received listkey for "phases": 15 bytes

Check that we did not generate any check:heads parts

  $ grep check:heads stuff
  [1]
  $ rm stuff

  $ cd ../server
  $ hg update default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ log
  @  b => baz [public:4cfedb0dc25f]
  |
  o  b => xxx [public:6a6d9484552c]
  |
  o  a => bar [public:add0c792bfce]
  |
  o  initial [public:2bb9d20e471c]
  

  $ cd ../client
  $ hg strip 1
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  saved backup bundle to $TESTTMP/client/.hg/strip-backup/46a2df24e272-1b034f5b-backup.hg (glob)
  $ hg pull
  pulling from ssh://user@dummy/server
  searching for changes
  no changes found
  $ hg update default
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved

Regular commits should go through without changing hash

  $ cd ../client
  $ echo '[experimental]' >> .hg/hgrc
  $ echo 'bundle2.pushback = True' >> .hg/hgrc

  $ echo 'quux' > b
  $ commit 'b => quux'
  $ log -r tip
  @  b => quux [draft:741fd2094512]
  |
  ~

  $ hg push --to default
  pushing to ssh://user@dummy/server
  searching for changes
  remote: pushing 1 changset:
  remote:     741fd2094512  b => quux

  $ cd ../server
  $ hg update default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ log
  @  b => quux [public:741fd2094512]
  |
  o  b => baz [public:4cfedb0dc25f]
  |
  o  b => xxx [public:6a6d9484552c]
  |
  o  a => bar [public:add0c792bfce]
  |
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
  $ hg push --to default
  pushing to ssh://user@dummy/server
  searching for changes
  remote: conflicting changes in:
      a
  
  abort: push failed on remote
  [255]

  $ hg strip 5
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  saved backup bundle to $TESTTMP/client/.hg/strip-backup/11a6a93eb344-7140e689-backup.hg (glob)
  $ cd ../server
  $ log
  @  a => baz [draft:fb983dc509b6]
  |
  o  b => quux [public:741fd2094512]
  |
  o  b => baz [public:4cfedb0dc25f]
  |
  o  b => xxx [public:6a6d9484552c]
  |
  o  a => bar [public:add0c792bfce]
  |
  o  initial [public:2bb9d20e471c]
  

Stack with conflict in head should abort

  $ cd ../client
  $ echo 'foofoo' > b
  $ commit 'b => foofoo'
  $ echo 'quux' > a
  $ commit 'a => quux'
  $ hg push --to default
  pushing to ssh://user@dummy/server
  searching for changes
  remote: conflicting changes in:
      a
  
  abort: push failed on remote
  [255]

  $ hg strip 5
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  saved backup bundle to $TESTTMP/client/.hg/strip-backup/98788efd81b0-93572e45-backup.hg (glob)

  $ cd ../server
  $ log
  @  a => baz [draft:fb983dc509b6]
  |
  o  b => quux [public:741fd2094512]
  |
  o  b => baz [public:4cfedb0dc25f]
  |
  o  b => xxx [public:6a6d9484552c]
  |
  o  a => bar [public:add0c792bfce]
  |
  o  initial [public:2bb9d20e471c]
  
Pushing a merge should rebase only the latest side of the merge

  $ hg book master -r tip
  $ cd ../client
  $ hg pull -q > /dev/null
  $ hg strip -q -r tip
  $ hg up 741fd2094512
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo "tux" > other
  $ hg add other
  $ hg commit -qm "branch left"
  $ hg book master -r tip
  moving bookmark 'master' forward from 741fd2094512
  $ hg up -q 2
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
  $ log
  @  on top of merge [draft:9007d6a204f8] master
  |
  o    merge [draft:2c0c699d7086]
  |\
  | o  branch middle [draft:5a0cbf3df4ef]
  | |
  | o  branch start [draft:add5ec74853d]
  | |
  o |  branch left [draft:e6b7549904cd]
  | |
  o |  b => quux [public:741fd2094512]
  | |
  o |  b => baz [public:4cfedb0dc25f]
  |/
  o  b => xxx [public:6a6d9484552c]
  |
  o  a => bar [public:add0c792bfce]
  |
  o  initial [public:2bb9d20e471c]
  
  $ hg push --to master -B master
  pushing to ssh://user@dummy/server
  searching for changes
  remote: pushing 5 changsets:
  remote:     e6b7549904cd  branch left
  remote:     add5ec74853d  branch start
  remote:     5a0cbf3df4ef  branch middle
  remote:     2c0c699d7086  merge
  remote:     9007d6a204f8  on top of merge
  remote: 6 new changesets from the server will be downloaded
  adding changesets
  adding manifests
  adding file changes
  added 4 changesets with 1 changes to 3 files (+1 heads)
  updating bookmark master
  $ cd ../server
  $ log
  o  on top of merge [public:7548c79a5591] master
  |
  o    merge [public:93a3cc822f6a]
  |\
  | o  branch middle [public:5a0cbf3df4ef]
  | |
  | o  branch start [public:add5ec74853d]
  | |
  o |  branch left [public:cf07bdf4226e]
  | |
  @ |  a => baz [public:fb983dc509b6]
  | |
  o |  b => quux [public:741fd2094512]
  | |
  o |  b => baz [public:4cfedb0dc25f]
  |/
  o  b => xxx [public:6a6d9484552c]
  |
  o  a => bar [public:add0c792bfce]
  |
  o  initial [public:2bb9d20e471c]
  
- Verify the content of the merge files is correct
  $ hg cat -r "master^" c
  branched2
  $ hg cat -r "master^" other
  tux

  $ hg strip -r add5ec74853d -q
  $ cd ../client
  $ hg strip -r add5ec74853d -q
  $ hg book -d master
  $ hg -R ../server book -d master

With evolution enabled, should set obsolescence markers

  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > rebase =
  > 
  > [experimental]
  > evolution = all
  > EOF

  $ cd ../client
  $ hg strip -qr fb983dc509b6
  $ hg strip -qr e6b7549904cd2a7991ef25bc2e0fd910801af2cd
  $ hg up -q 741fd2094512
  $ hg mv b k
  $ commit 'b => k'
  $ hg mv k b
  $ echo 'foobar' > b
  $ commit 'b => foobar'
  $ log
  @  b => foobar [draft:e73acfaeee82]
  |
  o  b => k [draft:9467a8ee5d0d]
  |
  o  b => quux [public:741fd2094512]
  |
  o  b => baz [public:4cfedb0dc25f]
  |
  o  b => xxx [public:6a6d9484552c]
  |
  o  a => bar [public:add0c792bfce]
  |
  o  initial [public:2bb9d20e471c]
  
  $ hg push --to default
  pushing to ssh://user@dummy/server
  searching for changes
  remote: pushing 2 changsets:
  remote:     9467a8ee5d0d  b => k
  remote:     e73acfaeee82  b => foobar
  remote: 4 new changesets from the server will be downloaded
  adding changesets
  adding manifests
  adding file changes
  added 4 changesets with 2 changes to 4 files (+1 heads)
  2 new obsolescence markers

  $ hg pull
  pulling from ssh://user@dummy/server
  searching for changes
  no changes found
  $ hg debugobsolete | sort
  9467a8ee5d0d993ba68d94946c9d4a3cae8d31ff 0d76868c25e6789734c06e056f235e1fa223da74 * (glob)
  e73acfaeee82005b2379f82efb73123cbb74a733 d53a62ed14be0980584e1f92f9c47031ef806a62 * (glob)
  $ hg up d53a62ed14be
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ log
  @  b => foobar [public:d53a62ed14be]
  |
  o  b => k [public:0d76868c25e6]
  |
  o  branch left [public:cf07bdf4226e]
  |
  o  a => baz [public:fb983dc509b6]
  |
  o  b => quux [public:741fd2094512]
  |
  o  b => baz [public:4cfedb0dc25f]
  |
  o  b => xxx [public:6a6d9484552c]
  |
  o  a => bar [public:add0c792bfce]
  |
  o  initial [public:2bb9d20e471c]
  

  $ cd ../server
  $ hg log -r 0d76868c25e6 -T '{file_copies}\n'
  k (b)
  $ log
  o  b => foobar [public:d53a62ed14be]
  |
  o  b => k [public:0d76868c25e6]
  |
  o  branch left [public:cf07bdf4226e]
  |
  @  a => baz [public:fb983dc509b6]
  |
  o  b => quux [public:741fd2094512]
  |
  o  b => baz [public:4cfedb0dc25f]
  |
  o  b => xxx [public:6a6d9484552c]
  |
  o  a => bar [public:add0c792bfce]
  |
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
  pushing to ssh://user@dummy/server
  searching for changes
  remote: pushing 1 changset:
  remote:     56b2e0949966  b => babar
  updating bookmark master
  $ hg log -r master -R ../server -T"{node}\n"
  56b2e094996609874ae1c9aae1626bfba61d07d8

Test pushing bookmark with no new commit

  $ hg book stable -r fb983dc509b6
  $ hg book stable -r "fb983dc509b6^" -R ../server
  $ hg push -r stable --to stable
  pushing to ssh://user@dummy/server
  searching for changes
  no changes found
  updating bookmark stable
  [1]
  $ hg log -r stable -R ../server
  changeset:   5:fb983dc509b6
  bookmark:    stable
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     a => baz
  

  $ cd ..

Test that the prepushrebase hook can run against the bundle repo

  $ cat >> $TESTTMP/prerebase.sh <<EOF
  > hg log -r tip
  > echo "Checking if lock exists (it should not):"
  > ls -l .hg/store/lock
  > EOF
  $ chmod a+x $TESTTMP/prerebase.sh
  $ hg init prepushrebaseserver
  $ cd prepushrebaseserver
  $ cat >> .hg/hgrc <<EOF
  > [hooks]
  > prepushrebase = $TESTTMP/prerebase.sh
  > [extensions]
  > bundle2hooks =
  > pushrebase =
  > [experimental]
  > bundle2lazylocking = True
  > EOF
  $ touch a && hg add a && hg commit -qm a
  $ hg book master
  $ cd ..

  $ hg clone ssh://user@dummy/prepushrebaseserver prepushrebaseclient
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd prepushrebaseclient
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > bundle2hooks =
  > pushrebase =
  > EOF
  $ touch b && hg add b && hg commit -qm b
  $ hg push --to master
  pushing to ssh://user@dummy/prepushrebaseserver
  searching for changes
  remote: changeset:   1:0e067c57feba
  remote: tag:         tip
  remote: user:        test
  remote: date:        Thu Jan 01 00:00:00 1970 +0000
  remote: summary:     b
  remote: 
  remote: Checking if lock exists (it should not):
  remote: ls: *.hg/store/lock*: No such file or directory (glob)
  remote: prepushrebase hook exited with status * (glob)
  abort: push failed on remote
  [255]

  $ cd ..

Test that hooks are fired with the correct variables

  $ hg init hookserver
  $ cd hookserver
  $ cat >> .hg/hgrc <<EOF
  > [hooks]
  > changegroup = python "$RUNTESTDIR/printenv.py" changegroup
  > incoming = python "$RUNTESTDIR/printenv.py" incoming
  > outgoing = python "$RUNTESTDIR/printenv.py" outgoing
  > prechangegroup = python "$RUNTESTDIR/printenv.py" prechangegroup
  > preoutgoing = python "$RUNTESTDIR/printenv.py" preoutgoing
  > pretxnchangegroup = python "$RUNTESTDIR/printenv.py" pretxnchangegroup
  > txnclose = python "$RUNTESTDIR/printenv.py" txnclose
  > pretxnclose = python "$RUNTESTDIR/printenv.py" pretxnclose
  > prepushrebase = python "$RUNTESTDIR/printenv.py" prepushrebase
  > prepushkey = python "$RUNTESTDIR/printenv.py" prepushkey
  > [extensions]
  > bundle2hooks =
  > pushrebase =
  > EOF
  $ touch file && hg ci -Aqm initial
  pretxnclose hook: HG_PENDING=$TESTTMP/hookserver HG_PHASES_MOVED=1 HG_TXNID=TXN:* HG_TXNNAME=commit (glob)
  txnclose hook: HG_PHASES_MOVED=1 HG_TXNID=TXN:* HG_TXNNAME=commit (glob)
  $ hg bookmark master
  pretxnclose hook: HG_BOOKMARK_MOVED=1 HG_PENDING=$TESTTMP/hookserver HG_TXNID=TXN:* HG_TXNNAME=bookmark (glob)
  txnclose hook: HG_BOOKMARK_MOVED=1 HG_TXNID=TXN:* HG_TXNNAME=bookmark (glob)
  $ cd ../

  $ hg clone hookserver hookclient
  preoutgoing hook: HG_SOURCE=clone
  outgoing hook: HG_NODE=0000000000000000000000000000000000000000 HG_SOURCE=clone
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd hookclient
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > bundle2hooks =
  > pushrebase =
  > EOF
  $ hg update master
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (activating bookmark master)
  $ echo >> file && hg ci -Aqm first
  $ hg push --to master -B master
  pushing to $TESTTMP/hookserver (glob)
  searching for changes
  prepushrebase hook: HG_BUNDLE2=1 HG_HOOK_BUNDLEPATH=* HG_NODE=4fcee35c508c1019667f72cae9b843efa8908701 HG_SOURCE=push (glob)
  prechangegroup hook: HG_BUNDLE2=1 HG_SOURCE=push HG_TXNID=TXN:* HG_URL=file:$TESTTMP/hookserver (glob)
  pushing 1 changset:
      4fcee35c508c  first
  pretxnchangegroup hook: HG_BUNDLE2=1 HG_NODE=4fcee35c508c1019667f72cae9b843efa8908701 HG_PENDING=$TESTTMP/hookserver HG_SOURCE=push HG_TXNID=TXN:* HG_URL=file:$TESTTMP/hookserver (glob)
  prepushkey hook: HG_BUNDLE2=1 HG_KEY=4fcee35c508c1019667f72cae9b843efa8908701 HG_NAMESPACE=phases HG_NEW=0 HG_NODE=4fcee35c508c1019667f72cae9b843efa8908701 HG_OLD=1 HG_PENDING=$TESTTMP/hookserver HG_PHASES_MOVED=1 HG_SOURCE=push HG_TXNID=TXN:* HG_URL=file:$TESTTMP/hookserver (glob)
  prepushkey hook: HG_BUNDLE2=1 HG_KEY=master HG_NAMESPACE=bookmarks HG_NEW=4fcee35c508c1019667f72cae9b843efa8908701 HG_NODE=4fcee35c508c1019667f72cae9b843efa8908701 HG_OLD=e95be919ac60f0c114075e32a0a4301afabadb60 HG_PENDING=$TESTTMP/hookserver HG_PHASES_MOVED=1 HG_SOURCE=push HG_TXNID=TXN:* HG_URL=file:$TESTTMP/hookserver (glob)
  pretxnclose hook: HG_BOOKMARK_MOVED=1 HG_BUNDLE2=1 HG_NODE=4fcee35c508c1019667f72cae9b843efa8908701 HG_PENDING=$TESTTMP/hookserver HG_PHASES_MOVED=1 HG_SOURCE=push HG_TXNID=TXN:* HG_TXNNAME=push HG_URL=file:$TESTTMP/hookserver (glob)
  txnclose hook: HG_BOOKMARK_MOVED=1 HG_BUNDLE2=1 HG_NODE=4fcee35c508c1019667f72cae9b843efa8908701 HG_PHASES_MOVED=1 HG_SOURCE=push HG_TXNID=TXN:* HG_TXNNAME=push HG_URL=file:$TESTTMP/hookserver (glob)
  changegroup hook: HG_BUNDLE2=1 HG_NODE=4fcee35c508c1019667f72cae9b843efa8908701 HG_SOURCE=push HG_TXNID=TXN:* HG_URL=file:$TESTTMP/hookserver (glob)
  incoming hook: HG_BUNDLE2=1 HG_NODE=4fcee35c508c1019667f72cae9b843efa8908701 HG_SOURCE=push HG_TXNID=TXN:* HG_URL=file:$TESTTMP/hookserver (glob)
  updating bookmark master


  $ cd ../

Test date rewriting

  $ hg init rewritedate
  $ cd rewritedate
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > bundle2hooks =
  > pushrebase =
  > [pushrebase]
  > rewritedates = True
  > EOF
  $ touch a && hg commit -Aqm a
  $ touch b && hg commit -Aqm b
  $ hg book master
  $ cd ..

  $ hg clone rewritedate rewritedateclient
  updating to branch default
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd rewritedateclient
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > bundle2hooks =
  > pushrebase =
  > EOF
  $ hg up 0
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ touch c && hg commit -Aqm c

  $ cat > $TESTTMP/daterewrite.py <<EOF
  > import sys, time
  > from mercurial import extensions
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
  pushing to $TESTTMP/rewritedate (glob)
  searching for changes
  pushing 1 changset:
      d5e255ef74f8  c
  1 new changeset from the server will be downloaded
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 1 files
  1 new obsolescence markers
  $ hg log -G -T '{desc} {date|isodate}'
  o  c 2001-09-09 01:46 +0000
  |
  | @  c 1970-01-01 00:00 +0000
  | |
  o |  b 1970-01-01 00:00 +0000
  |/
  o  a 1970-01-01 00:00 +0000
  
Test date rewriting with a merge commit

  $ hg up -q 0
  $ echo x >> x
  $ hg commit -qAm x
  $ hg up -q 3
  $ echo y >> y
  $ hg commit -qAm y
  $ hg merge -q 4
  $ hg commit -qm merge
  $ hg push --to master
  pushing to $TESTTMP/rewritedate (glob)
  searching for changes
  pushing 3 changsets:
      a5f9a9a43049  x
      c1392466a61e  y
      4514adb1f536  merge
  3 new changesets from the server will be downloaded
  adding changesets
  adding manifests
  adding file changes
  added 3 changesets with 0 changes to 2 files (+1 heads)
  3 new obsolescence markers

  $ cd ..

Test force pushes
  $ hg init forcepushserver
  $ cd forcepushserver
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > bundle2hooks =
  > pushrebase =
  > EOF
  $ echo a > a && hg commit -Aqm a
  $ cd ..

  $ hg clone forcepushserver forcepushclient
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd forcepushserver
  $ echo a >> a && hg commit -Aqm aa

  $ cd ../forcepushclient
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > bundle2hooks =
  > pushrebase =
  > EOF
  $ hg up 0
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo b >> a && hg commit -Aqm b
  $ hg book master
  $ hg push -f --to master -B master
  pushing to $TESTTMP/forcepushserver (glob)
  searching for changes
  pushing 1 changset:
      1846eede8b68  b
  exporting bookmark master
  $ hg pull
  pulling from $TESTTMP/forcepushserver (glob)
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files (+1 heads)
  (run 'hg heads' to see heads, 'hg merge' to merge)
  $ hg log -G -T '{rev} {desc} {bookmarks}'
  o  2 aa
  |
  | @  1 b master
  |/
  o  0 a
  
