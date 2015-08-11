  $ cat >> $HGRCPATH <<EOF
  > [ui]
  > ssh = python "$TESTDIR/dummyssh"
  > username = nobody <no.reply@fb.com>
  > [hooks]
  > changegroup = python "$TESTDIR/printenv.py" changegroup
  > incoming = python "$TESTDIR/printenv.py" incoming
  > outgoing = python "$TESTDIR/printenv.py" outgoing
  > prechangegroup = python "$TESTDIR/printenv.py" prechangegroup
  > preoutgoing = python "$TESTDIR/printenv.py" preoutgoing
  > pretxnchangegroup = python "$TESTDIR/printenv.py" pretxnchangegroup
  > b2x-transactionclose = python "$TESTDIR/printenv.py" b2x-transactionclose
  > b2x-pretransactionclose = python "$TESTDIR/printenv.py" b2x-pretransactionclose
  > [extensions]
  > strip =
  > EOF
  $ alias commit='hg commit -d "0 0" -A -m'
  $ alias log='hg log -G -T "{desc} [{phase}:{node|short}] {bookmarks}"'

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
  prechangegroup hook: HG_SOURCE=pull HG_TXNID=TXN:* HG_URL=ssh://user@dummy/server (glob)
  pretxnchangegroup hook: HG_NODE=2bb9d20e471c5066592995d4624edb0eafe81ac8 HG_PENDING=$TESTTMP/client HG_SOURCE=pull HG_TXNID=TXN:* HG_URL=ssh://user@dummy/server (glob)
  changegroup hook: HG_NODE=2bb9d20e471c5066592995d4624edb0eafe81ac8 HG_SOURCE=pull HG_TXNID=TXN:* HG_URL=ssh://user@dummy/server (glob)
  incoming hook: HG_NODE=2bb9d20e471c5066592995d4624edb0eafe81ac8 HG_SOURCE=pull HG_TXNID=TXN:* HG_URL=ssh://user@dummy/server (glob)
  $ cd client
  $ echo "[extensions]" >> .hg/hgrc
  $ echo "pushrebase = $TESTDIR/../pushrebase.py" >> .hg/hgrc

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
  (pull and merge or see "hg help push" for details about pushing new heads)
  [255]

  $ hg --config experimental.bundle2-exp=False push --to default
  pushing to ssh://user@dummy/server
  searching for changes
  abort: bundle2 needs to be enabled on client
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
  $ echo "pushrebase = $TESTDIR/../pushrebase.py" >> .hg/hgrc
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
  
  $ hg push --to default
  pushing to ssh://user@dummy/server
  searching for changes
  preoutgoing hook: HG_SOURCE=push
  outgoing hook: HG_NODE=46a2df24e27273bb06dbf28b085fcc2e911bf986 HG_SOURCE=push
  remote: prechangegroup hook: HG_BUNDLE2=1 HG_SOURCE=serve HG_TXNID=TXN:* HG_URL=remote:ssh:127.0.0.1 (glob)
  remote: pretxnchangegroup hook: HG_BUNDLE2=1 HG_PENDING=$TESTTMP/server HG_SOURCE=serve HG_TXNID=TXN:* HG_URL=remote:ssh:127.0.0.1 (glob)
  remote: preoutgoing hook: HG_SOURCE=rebase:reply
  remote: changegroup hook: HG_BUNDLE2=1 HG_NODE=6a6d9484552c82e5f21b4ed4fce375930812f88c HG_SOURCE=serve HG_TXNID=TXN:* HG_URL=remote:ssh:127.0.0.1 (glob)
  remote: incoming hook: HG_BUNDLE2=1 HG_NODE=6a6d9484552c82e5f21b4ed4fce375930812f88c HG_SOURCE=serve HG_TXNID=TXN:* HG_URL=remote:ssh:127.0.0.1 (glob)
  remote: incoming hook: HG_BUNDLE2=1 HG_NODE=4cfedb0dc25f66f5d020564af00d4a39ad56f33b HG_SOURCE=serve HG_TXNID=TXN:* HG_URL=remote:ssh:127.0.0.1 (glob)
  prechangegroup hook: HG_SOURCE=push-response HG_TXNID=TXN:* HG_URL=ssh://user@dummy/server (glob)
  adding changesets
  remote: outgoing hook: HG_NODE=add0c792bfce89610d277fd5b1e32f5287994d1d HG_SOURCE=rebase:reply
  adding manifests
  adding file changes
  added 3 changesets with 1 changes to 2 files (+1 heads)
  pretxnchangegroup hook: HG_NODE=add0c792bfce89610d277fd5b1e32f5287994d1d HG_PENDING=$TESTTMP/client HG_SOURCE=push-response HG_TXNID=TXN:* HG_URL=ssh://user@dummy/server (glob)
  changegroup hook: HG_NODE=add0c792bfce89610d277fd5b1e32f5287994d1d HG_SOURCE=push-response HG_TXNID=TXN:* HG_URL=ssh://user@dummy/server (glob)
  incoming hook: HG_NODE=add0c792bfce89610d277fd5b1e32f5287994d1d HG_SOURCE=push-response HG_TXNID=TXN:* HG_URL=ssh://user@dummy/server (glob)
  incoming hook: HG_NODE=6a6d9484552c82e5f21b4ed4fce375930812f88c HG_SOURCE=push-response HG_TXNID=TXN:* HG_URL=ssh://user@dummy/server (glob)
  incoming hook: HG_NODE=4cfedb0dc25f66f5d020564af00d4a39ad56f33b HG_SOURCE=push-response HG_TXNID=TXN:* HG_URL=ssh://user@dummy/server (glob)

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
  preoutgoing hook: HG_SOURCE=strip
  outgoing hook: HG_NODE=46a2df24e27273bb06dbf28b085fcc2e911bf986 HG_SOURCE=strip
  saved backup bundle to $TESTTMP/client/.hg/strip-backup/46a2df24e272-1b034f5b-backup.hg (glob)
  preoutgoing hook: HG_SOURCE=strip
  outgoing hook: HG_NODE=add0c792bfce89610d277fd5b1e32f5287994d1d HG_SOURCE=strip
  prechangegroup hook: HG_SOURCE=strip HG_TXNID=TXN:* HG_URL=bundle:$TESTTMP/client/.hg/strip-backup/46a2df24e272-c3f42717-temp.hg (glob)
  pretxnchangegroup hook: HG_NODE=add0c792bfce89610d277fd5b1e32f5287994d1d HG_PENDING=$TESTTMP/client HG_SOURCE=strip HG_TXNID=TXN:* HG_URL=bundle:$TESTTMP/client/.hg/strip-backup/46a2df24e272-c3f42717-temp.hg (glob)
  changegroup hook: HG_NODE=add0c792bfce89610d277fd5b1e32f5287994d1d HG_SOURCE=strip HG_TXNID=TXN:* HG_URL=bundle:$TESTTMP/client/.hg/strip-backup/46a2df24e272-c3f42717-temp.hg (glob)
  incoming hook: HG_NODE=add0c792bfce89610d277fd5b1e32f5287994d1d HG_SOURCE=strip HG_TXNID=TXN:* HG_URL=bundle:$TESTTMP/client/.hg/strip-backup/46a2df24e272-c3f42717-temp.hg (glob)
  incoming hook: HG_NODE=6a6d9484552c82e5f21b4ed4fce375930812f88c HG_SOURCE=strip HG_TXNID=TXN:* HG_URL=bundle:$TESTTMP/client/.hg/strip-backup/46a2df24e272-c3f42717-temp.hg (glob)
  incoming hook: HG_NODE=4cfedb0dc25f66f5d020564af00d4a39ad56f33b HG_SOURCE=strip HG_TXNID=TXN:* HG_URL=bundle:$TESTTMP/client/.hg/strip-backup/46a2df24e272-c3f42717-temp.hg (glob)
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

  $ hg push --to default
  pushing to ssh://user@dummy/server
  searching for changes
  preoutgoing hook: HG_SOURCE=push
  outgoing hook: HG_NODE=741fd2094512a57edc6d93e8257f961c82bf9dde HG_SOURCE=push
  remote: prechangegroup hook: HG_BUNDLE2=1 HG_SOURCE=serve HG_TXNID=TXN:* HG_URL=remote:ssh:127.0.0.1 (glob)
  remote: pretxnchangegroup hook: HG_BUNDLE2=1 HG_PENDING=$TESTTMP/server HG_SOURCE=serve HG_TXNID=TXN:* HG_URL=remote:ssh:127.0.0.1 (glob)
  remote: changegroup hook: HG_BUNDLE2=1 HG_NODE=741fd2094512a57edc6d93e8257f961c82bf9dde HG_SOURCE=serve HG_TXNID=TXN:* HG_URL=remote:ssh:127.0.0.1 (glob)
  remote: incoming hook: HG_BUNDLE2=1 HG_NODE=741fd2094512a57edc6d93e8257f961c82bf9dde HG_SOURCE=serve HG_TXNID=TXN:* HG_URL=remote:ssh:127.0.0.1 (glob)

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
  preoutgoing hook: HG_SOURCE=push
  outgoing hook: HG_NODE=11a6a93eb34480e6848058d7ac2f6c6514be07e6 HG_SOURCE=push
  abort: conflicting changes in ['a']
  [255]

  $ hg strip 5
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  preoutgoing hook: HG_SOURCE=strip
  outgoing hook: HG_NODE=11a6a93eb34480e6848058d7ac2f6c6514be07e6 HG_SOURCE=strip
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
  preoutgoing hook: HG_SOURCE=push
  outgoing hook: HG_NODE=98788efd81b0d6e7f0e90fe90d7dd10595700b24 HG_SOURCE=push
  abort: conflicting changes in ['a']
  [255]

  $ hg strip 5
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  preoutgoing hook: HG_SOURCE=strip
  outgoing hook: HG_NODE=98788efd81b0d6e7f0e90fe90d7dd10595700b24 HG_SOURCE=strip
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
  preoutgoing hook: HG_SOURCE=strip
  outgoing hook: HG_NODE=fb983dc509b61b92a3f19cc326f62b424bb25d1c HG_SOURCE=strip
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
  preoutgoing hook: HG_SOURCE=push
  outgoing hook: HG_NODE=e6b7549904cd2a7991ef25bc2e0fd910801af2cd HG_SOURCE=push
  remote: prechangegroup hook: HG_BUNDLE2=1 HG_SOURCE=serve * (glob)
  remote: pretxnchangegroup hook: HG_BUNDLE2=1 HG_PENDING=$TESTTMP/server HG_SOURCE=serve * (glob)
  remote: preoutgoing hook: HG_SOURCE=rebase:reply
  remote: changegroup hook: HG_BUNDLE2=1 HG_NODE=cf07bdf4226ef5167b9f86119e915ff3f239642a HG_SOURCE=serve * (glob)
  remote: incoming hook: HG_BUNDLE2=1 HG_NODE=cf07bdf4226ef5167b9f86119e915ff3f239642a HG_SOURCE=serve * (glob)
  remote: incoming hook: HG_BUNDLE2=1 HG_NODE=add5ec74853d53cf76e4b735e033a2350e7fe4f3 HG_SOURCE=serve * (glob)
  remote: incoming hook: HG_BUNDLE2=1 HG_NODE=5a0cbf3df4ef43ccc96fedd1030d6b8c59f2cd32 HG_SOURCE=serve * (glob)
  remote: incoming hook: HG_BUNDLE2=1 HG_NODE=93a3cc822f6ac9c91c5c645cab8fec47a26da52e HG_SOURCE=serve * (glob)
  remote: incoming hook: HG_BUNDLE2=1 HG_NODE=7548c79a5591fca7a09470b814ead1b3f471aa89 HG_SOURCE=serve * (glob)
  prechangegroup hook: HG_SOURCE=push-response * (glob)
  adding changesets
  remote: outgoing hook: HG_NODE=fb983dc509b61b92a3f19cc326f62b424bb25d1c HG_SOURCE=rebase:reply
  adding manifests
  adding file changes
  added 4 changesets with 1 changes to 3 files (+1 heads)
  pretxnchangegroup hook: HG_NODE=fb983dc509b61b92a3f19cc326f62b424bb25d1c HG_PENDING=$TESTTMP/client HG_SOURCE=push-response * (glob)
  updating bookmark master
  changegroup hook: HG_NODE=fb983dc509b61b92a3f19cc326f62b424bb25d1c HG_SOURCE=push-response * (glob)
  incoming hook: HG_NODE=fb983dc509b61b92a3f19cc326f62b424bb25d1c HG_SOURCE=push-response * (glob)
  incoming hook: HG_NODE=cf07bdf4226ef5167b9f86119e915ff3f239642a HG_SOURCE=push-response * (glob)
  incoming hook: HG_NODE=93a3cc822f6ac9c91c5c645cab8fec47a26da52e HG_SOURCE=push-response * (glob)
  incoming hook: HG_NODE=7548c79a5591fca7a09470b814ead1b3f471aa89 HG_SOURCE=push-response * (glob)
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
  $ hg cat -r master^ c
  branched2
  $ hg cat -r master^ other
  tux

  $ hg strip -r add5ec74853d -q
  preoutgoing hook: HG_SOURCE=strip
  outgoing hook: HG_NODE=add5ec74853d53cf76e4b735e033a2350e7fe4f3 HG_SOURCE=strip
  $ cd ../client
  $ hg strip -r add5ec74853d -q
  preoutgoing hook: HG_SOURCE=strip
  outgoing hook: HG_NODE=add5ec74853d53cf76e4b735e033a2350e7fe4f3 HG_SOURCE=strip
  preoutgoing hook: HG_SOURCE=strip
  outgoing hook: HG_NODE=fb983dc509b61b92a3f19cc326f62b424bb25d1c HG_SOURCE=strip
  prechangegroup hook: HG_SOURCE=strip * (glob)
  pretxnchangegroup hook: HG_NODE=fb983dc509b61b92a3f19cc326f62b424bb25d1c HG_PENDING=$TESTTMP/client HG_SOURCE=strip * (glob)
  changegroup hook: HG_NODE=fb983dc509b61b92a3f19cc326f62b424bb25d1c HG_SOURCE=strip * (glob)
  incoming hook: HG_NODE=fb983dc509b61b92a3f19cc326f62b424bb25d1c HG_SOURCE=strip * (glob)
  incoming hook: HG_NODE=cf07bdf4226ef5167b9f86119e915ff3f239642a HG_SOURCE=strip * (glob)
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
  preoutgoing hook: HG_SOURCE=strip
  outgoing hook: HG_NODE=fb983dc509b61b92a3f19cc326f62b424bb25d1c HG_SOURCE=strip
  $ hg strip -qr e6b7549904cd2a7991ef25bc2e0fd910801af2cd
  preoutgoing hook: HG_SOURCE=strip
  outgoing hook: HG_NODE=e6b7549904cd2a7991ef25bc2e0fd910801af2cd HG_SOURCE=strip
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
  preoutgoing hook: HG_SOURCE=push
  outgoing hook: HG_NODE=9467a8ee5d0d993ba68d94946c9d4a3cae8d31ff HG_SOURCE=push
  remote: prechangegroup hook: HG_BUNDLE2=1 HG_SOURCE=serve * (glob)
  remote: pretxnchangegroup hook: HG_BUNDLE2=1 HG_PENDING=$TESTTMP/server HG_SOURCE=serve * (glob)
  remote: preoutgoing hook: HG_SOURCE=rebase:reply
  remote: changegroup hook: HG_BUNDLE2=1 HG_NODE=0d76868c25e6789734c06e056f235e1fa223da74 HG_SOURCE=serve * (glob)
  remote: incoming hook: HG_BUNDLE2=1 HG_NODE=0d76868c25e6789734c06e056f235e1fa223da74 HG_SOURCE=serve * (glob)
  remote: incoming hook: HG_BUNDLE2=1 HG_NODE=d53a62ed14be0980584e1f92f9c47031ef806a62 HG_SOURCE=serve * (glob)
  prechangegroup hook: HG_SOURCE=push-response * (glob)
  adding changesets
  remote: outgoing hook: HG_NODE=fb983dc509b61b92a3f19cc326f62b424bb25d1c HG_SOURCE=rebase:reply
  adding manifests
  adding file changes
  added 4 changesets with 2 changes to 4 files (+1 heads)
  pretxnchangegroup hook: HG_NODE=fb983dc509b61b92a3f19cc326f62b424bb25d1c HG_PENDING=$TESTTMP/client HG_SOURCE=push-response * (glob)
  2 new obsolescence markers
  changegroup hook: HG_NODE=fb983dc509b61b92a3f19cc326f62b424bb25d1c HG_SOURCE=push-response * (glob)
  incoming hook: HG_NODE=fb983dc509b61b92a3f19cc326f62b424bb25d1c HG_SOURCE=push-response * (glob)
  incoming hook: HG_NODE=cf07bdf4226ef5167b9f86119e915ff3f239642a HG_SOURCE=push-response * (glob)
  incoming hook: HG_NODE=0d76868c25e6789734c06e056f235e1fa223da74 HG_SOURCE=push-response * (glob)
  incoming hook: HG_NODE=d53a62ed14be0980584e1f92f9c47031ef806a62 HG_SOURCE=push-response * (glob)

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
  preoutgoing hook: HG_SOURCE=push
  outgoing hook: HG_NODE=56b2e094996609874ae1c9aae1626bfba61d07d8 HG_SOURCE=push
  remote: prechangegroup hook: HG_BUNDLE2=1 HG_SOURCE=serve * (glob)
  remote: pretxnchangegroup hook: HG_BUNDLE2=1 HG_PENDING=$TESTTMP/server HG_SOURCE=serve * (glob)
  remote: changegroup hook: HG_BUNDLE2=1 HG_NODE=56b2e094996609874ae1c9aae1626bfba61d07d8 HG_SOURCE=serve * (glob)
  remote: incoming hook: HG_BUNDLE2=1 HG_NODE=56b2e094996609874ae1c9aae1626bfba61d07d8 HG_SOURCE=serve * (glob)
  updating bookmark master
  $ hg log -r master -R ../server -T"{node}\n"
  56b2e094996609874ae1c9aae1626bfba61d07d8

Test pushing bookmark with no new commit

  $ hg book stable -r fb983dc509b6
  $ hg book stable -r fb983dc509b6^ -R ../server
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
  
