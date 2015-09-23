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
  > txnclose = python "$TESTDIR/printenv.py" txnclose
  > pretxnclose = python "$TESTDIR/printenv.py" pretxnclose
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
  pretxnclose hook: HG_PENDING=$TESTTMP/server HG_PHASES_MOVED=1 HG_TXNID=TXN:1885008198c44d69d9441fecbbe18b2f8583fb4c HG_XNNAME=commit
  txnclose hook: HG_PHASES_MOVED=1 HG_TXNID=TXN:1885008198c44d69d9441fecbbe18b2f8583fb4c HG_TXNNAME=commit

Set up client repository

  $ cd ..
  $ hg clone ssh://user@dummy/server client -q
  prechangegroup hook: HG_SOURCE=pull HG_TXNID=TXN:* HG_URL=ssh://user@dummy/server (glob)
  pretxnchangegroup hook: HG_NODE=2bb9d20e471c5066592995d4624edb0eafe81ac8 HG_PENDING=$TESTTMP/client HG_SOURCE=pull HG_TXNID=TXN:* HG_URL=ssh://user@dummy/server (glob)
  pretxnclose hook: HG_NODE=2bb9d20e471c5066592995d4624edb0eafe81ac8 HG_PENDING=$TESTTMP/client HG_PHASES_MOVED=1 HG_SOURCE=pull HG_TXNID=TXN:c493275745b4141135759d58e8ca3ce8cff5794d HG_URL=ssh://user@dummy/server HG_XNNAME=pull
  ssh://user@dummy/server
  txnclose hook: HG_NODE=2bb9d20e471c5066592995d4624edb0eafe81ac8 HG_PHASES_MOVED=1 HG_SOURCE=pull HG_TXNID=TXN:c493275745b4141135759d58e8ca3ce8cff5794d HG_TXNNAME=pull
  ssh://user@dummy/server HG_URL=ssh://user@dummy/server
  changegroup hook: HG_NODE=2bb9d20e471c5066592995d4624edb0eafe81ac8 HG_SOURCE=pull HG_TXNID=TXN:c493275745b4141135759d58e8ca3ce8cff5794d HG_URL=ssh://user@dummy/server
  incoming hook: HG_NODE=2bb9d20e471c5066592995d4624edb0eafe81ac8 HG_SOURCE=pull HG_TXNID=TXN:c493275745b4141135759d58e8ca3ce8cff5794d HG_URL=ssh://user@dummy/server
  $ cd client
  $ echo "[extensions]" >> .hg/hgrc
  $ echo "pushrebase = $TESTDIR/../pushrebase.py" >> .hg/hgrc

Without server extension

  $ cd ../server
  $ echo 'bar' > a
  $ commit 'a => bar'
  pretxnclose hook: HG_PENDING=$TESTTMP/server HG_TXNID=TXN:d1b570bbf4b57bb76b39985a1f40a214d3c153dd HG_XNNAME=commit
  txnclose hook: HG_TXNID=TXN:d1b570bbf4b57bb76b39985a1f40a214d3c153dd HG_TXNNAME=commit

  $ cd ../client
  $ hg rm b
  $ commit 'b => xxx'
  pretxnclose hook: HG_PENDING=$TESTTMP/client HG_PHASES_MOVED=1 HG_TXNID=TXN:caf7e61b46ba8a40edf31f752bfe4b688170625a HG_XNNAME=commit
  txnclose hook: HG_PHASES_MOVED=1 HG_TXNID=TXN:caf7e61b46ba8a40edf31f752bfe4b688170625a HG_TXNNAME=commit
  $ echo 'baz' > b
  $ hg add b
  $ commit 'b => baz'
  pretxnclose hook: HG_PENDING=$TESTTMP/client HG_TXNID=TXN:d1431afacd261f4640aa23daef42d5d5c32e63b2 HG_XNNAME=commit
  txnclose hook: HG_TXNID=TXN:d1431afacd261f4640aa23daef42d5d5c32e63b2 HG_TXNNAME=commit
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
  prechangegroup hook: HG_SOURCE=push-response HG_TXNID=TXN:87a43712be6229f920352a259b1951fb9d55cff5 HG_URL=ssh://user@dummy/server
  adding changesets
  adding manifests
  adding file changes
  added 3 changesets with 1 changes to 2 files (+1 heads)
  pretxnchangegroup hook: HG_NODE=add0c792bfce89610d277fd5b1e32f5287994d1d HG_PENDING=$TESTTMP/client HG_SOURCE=push-response HG_TXNID=TXN:87a43712be6229f920352a259b1951fb9d55cff5 HG_URL=ssh://user@dummy/server
  remote: prechangegroup hook: HG_BUNDLE2=1 HG_SOURCE=serve HG_TXNID=TXN:0fa3e253382b916d95279fab37f355284bcb530c HG_URL=remote:ssh:127.0.0.1
  remote: pretxnchangegroup hook: HG_BUNDLE2=1 HG_NODE=4cfedb0dc25f66f5d020564af00d4a39ad56f33b HG_PENDING=$TESTTMP/server HG_SOURCE=serve HG_TXNID=TXN:0fa3e253382b916d95279fab37f355284bcb530c HG_URL=remote:ssh:127.0.0.1
  remote: preoutgoing hook: HG_SOURCE=rebase:reply
  remote: pretxnclose hook: HG_BUNDLE2=1 HG_NODE=6a6d9484552c82e5f21b4ed4fce375930812f88c HG_PENDING=$TESTTMP/server HG_PHASES_MOVED=1 HG_SOURCE=serve HG_TXNID=TXN:0fa3e253382b916d95279fab37f355284bcb530c HG_URL=remote:ssh:127.0.0.1 HG_XNNAME=serve
  remote: txnclose hook: HG_BUNDLE2=1 HG_NODE=6a6d9484552c82e5f21b4ed4fce375930812f88c HG_PHASES_MOVED=1 HG_SOURCE=serve HG_TXNID=TXN:0fa3e253382b916d95279fab37f355284bcb530c HG_TXNNAME=serve HG_URL=remote:ssh:127.0.0.1
  remote: changegroup hook: HG_BUNDLE2=1 HG_NODE=6a6d9484552c82e5f21b4ed4fce375930812f88c HG_SOURCE=serve HG_TXNID=TXN:0fa3e253382b916d95279fab37f355284bcb530c HG_URL=remote:ssh:127.0.0.1
  remote: incoming hook: HG_BUNDLE2=1 HG_NODE=6a6d9484552c82e5f21b4ed4fce375930812f88c HG_SOURCE=serve HG_TXNID=TXN:0fa3e253382b916d95279fab37f355284bcb530c HG_URL=remote:ssh:127.0.0.1
  remote: incoming hook: HG_BUNDLE2=1 HG_NODE=4cfedb0dc25f66f5d020564af00d4a39ad56f33b HG_SOURCE=serve HG_TXNID=TXN:0fa3e253382b916d95279fab37f355284bcb530c HG_URL=remote:ssh:127.0.0.1
  remote: outgoing hook: HG_NODE=add0c792bfce89610d277fd5b1e32f5287994d1d HG_SOURCE=rebase:reply
  pretxnclose hook: HG_NODE=add0c792bfce89610d277fd5b1e32f5287994d1d HG_PENDING=$TESTTMP/client HG_PHASES_MOVED=1 HG_SOURCE=push-response HG_TXNID=TXN:87a43712be6229f920352a259b1951fb9d55cff5 HG_URL=ssh://user@dummy/server HG_XNNAME=push-response
  ssh://user@dummy/server
  txnclose hook: HG_NODE=add0c792bfce89610d277fd5b1e32f5287994d1d HG_PHASES_MOVED=1 HG_SOURCE=push-response HG_TXNID=TXN:87a43712be6229f920352a259b1951fb9d55cff5 HG_TXNNAME=push-response
  ssh://user@dummy/server HG_URL=ssh://user@dummy/server
  changegroup hook: HG_NODE=add0c792bfce89610d277fd5b1e32f5287994d1d HG_SOURCE=push-response HG_TXNID=TXN:87a43712be6229f920352a259b1951fb9d55cff5 HG_URL=ssh://user@dummy/server
  incoming hook: HG_NODE=add0c792bfce89610d277fd5b1e32f5287994d1d HG_SOURCE=push-response HG_TXNID=TXN:87a43712be6229f920352a259b1951fb9d55cff5 HG_URL=ssh://user@dummy/server
  incoming hook: HG_NODE=6a6d9484552c82e5f21b4ed4fce375930812f88c HG_SOURCE=push-response HG_TXNID=TXN:87a43712be6229f920352a259b1951fb9d55cff5 HG_URL=ssh://user@dummy/server
  incoming hook: HG_NODE=4cfedb0dc25f66f5d020564af00d4a39ad56f33b HG_SOURCE=push-response HG_TXNID=TXN:87a43712be6229f920352a259b1951fb9d55cff5 HG_URL=ssh://user@dummy/server

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
  pretxnclose hook: HG_TXNID=TXN:e2eee5db6fe4b260a1489c0759c04f61c6cbcddc HG_XNNAME=strip
  prechangegroup hook: HG_SOURCE=strip HG_TXNID=TXN:5d975b5c3e90b327b4e91d0744f53fffab2f2f89 HG_URL=bundle:$TESTTMP/client/.hg/strip-backup/46a2df24e272-c3f42717-temp.hg
  pretxnchangegroup hook: HG_NODE=add0c792bfce89610d277fd5b1e32f5287994d1d HG_PENDING=$TESTTMP/client HG_SOURCE=strip HG_TXNID=TXN:5d975b5c3e90b327b4e91d0744f53fffab2f2f89 HG_URL=bundle:$TESTTMP/client/.hg/strip-backup/46a2df24e272-c3f42717-temp.hg
  pretxnclose hook: HG_NODE=add0c792bfce89610d277fd5b1e32f5287994d1d HG_PENDING=$TESTTMP/client HG_SOURCE=strip HG_TXNID=TXN:5d975b5c3e90b327b4e91d0744f53fffab2f2f89 HG_URL=bundle:$TESTTMP/client/.hg/strip-backup/46a2df24e272-c3f42717-temp.hg HG_XNNAME=strip
  bundle:$TESTTMP/client/.hg/strip-backup/46a2df24e272-c3f42717-temp.hg
  txnclose hook: HG_TXNID=TXN:e2eee5db6fe4b260a1489c0759c04f61c6cbcddc HG_TXNNAME=strip
  txnclose hook: HG_NODE=add0c792bfce89610d277fd5b1e32f5287994d1d HG_SOURCE=strip HG_TXNID=TXN:5d975b5c3e90b327b4e91d0744f53fffab2f2f89 HG_TXNNAME=strip
  bundle:$TESTTMP/client/.hg/strip-backup/46a2df24e272-c3f42717-temp.hg HG_URL=bundle:$TESTTMP/client/.hg/strip-backup/46a2df24e272-c3f42717-temp.hg
  changegroup hook: HG_NODE=add0c792bfce89610d277fd5b1e32f5287994d1d HG_SOURCE=strip HG_TXNID=TXN:5d975b5c3e90b327b4e91d0744f53fffab2f2f89 HG_URL=bundle:$TESTTMP/client/.hg/strip-backup/46a2df24e272-c3f42717-temp.hg
  incoming hook: HG_NODE=add0c792bfce89610d277fd5b1e32f5287994d1d HG_SOURCE=strip HG_TXNID=TXN:5d975b5c3e90b327b4e91d0744f53fffab2f2f89 HG_URL=bundle:$TESTTMP/client/.hg/strip-backup/46a2df24e272-c3f42717-temp.hg
  incoming hook: HG_NODE=6a6d9484552c82e5f21b4ed4fce375930812f88c HG_SOURCE=strip HG_TXNID=TXN:5d975b5c3e90b327b4e91d0744f53fffab2f2f89 HG_URL=bundle:$TESTTMP/client/.hg/strip-backup/46a2df24e272-c3f42717-temp.hg
  incoming hook: HG_NODE=4cfedb0dc25f66f5d020564af00d4a39ad56f33b HG_SOURCE=strip HG_TXNID=TXN:5d975b5c3e90b327b4e91d0744f53fffab2f2f89 HG_URL=bundle:$TESTTMP/client/.hg/strip-backup/46a2df24e272-c3f42717-temp.hg
  $ hg pull
  pulling from ssh://user@dummy/server
  searching for changes
  no changes found
  pretxnclose hook: HG_PENDING=$TESTTMP/client HG_PHASES_MOVED=1 HG_SOURCE=pull HG_TXNID=TXN:31c6d96f0084046c8abe09d35a17f2af717e807d HG_URL=ssh://user@dummy/server HG_XNNAME=pull
  ssh://user@dummy/server
  txnclose hook: HG_PHASES_MOVED=1 HG_SOURCE=pull HG_TXNID=TXN:31c6d96f0084046c8abe09d35a17f2af717e807d HG_TXNNAME=pull
  ssh://user@dummy/server HG_URL=ssh://user@dummy/server
  $ hg update default
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved

Regular commits should go through without changing hash

  $ cd ../client
  $ echo '[experimental]' >> .hg/hgrc
  $ echo 'bundle2.pushback = True' >> .hg/hgrc

  $ echo 'quux' > b
  $ commit 'b => quux'
  pretxnclose hook: HG_PENDING=$TESTTMP/client HG_PHASES_MOVED=1 HG_TXNID=TXN:a9fdf6cd94cf2dac609e5ca811450b2e881c95ae HG_XNNAME=commit
  txnclose hook: HG_PHASES_MOVED=1 HG_TXNID=TXN:a9fdf6cd94cf2dac609e5ca811450b2e881c95ae HG_TXNNAME=commit
  $ log -r tip
  @  b => quux [draft:741fd2094512]
  |

  $ hg push --to default
  pushing to ssh://user@dummy/server
  searching for changes
  preoutgoing hook: HG_SOURCE=push
  outgoing hook: HG_NODE=741fd2094512a57edc6d93e8257f961c82bf9dde HG_SOURCE=push
  remote: prechangegroup hook: HG_BUNDLE2=1 HG_SOURCE=serve HG_TXNID=TXN:* HG_URL=remote:ssh:127.0.0.1 (glob)
  remote: pretxnchangegroup hook: HG_BUNDLE2=1 HG_NODE=741fd2094512a57edc6d93e8257f961c82bf9dde HG_PENDING=$TESTTMP/server HG_SOURCE=serve HG_TXNID=TXN:0d4ea256a2ddd5a05dda27e60b03fba3f6fdc5dc HG_URL=remote:ssh:127.0.0.1
  remote: pretxnclose hook: HG_BUNDLE2=1 HG_NODE=741fd2094512a57edc6d93e8257f961c82bf9dde HG_PENDING=$TESTTMP/server HG_PHASES_MOVED=1 HG_SOURCE=serve HG_TXNID=TXN:0d4ea256a2ddd5a05dda27e60b03fba3f6fdc5dc HG_URL=remote:ssh:127.0.0.1 HG_XNNAME=serve
  remote: txnclose hook: HG_BUNDLE2=1 HG_NODE=741fd2094512a57edc6d93e8257f961c82bf9dde HG_PHASES_MOVED=1 HG_SOURCE=serve HG_TXNID=TXN:0d4ea256a2ddd5a05dda27e60b03fba3f6fdc5dc HG_TXNNAME=serve HG_URL=remote:ssh:127.0.0.1
  remote: changegroup hook: HG_BUNDLE2=1 HG_NODE=741fd2094512a57edc6d93e8257f961c82bf9dde HG_SOURCE=serve HG_TXNID=TXN:0d4ea256a2ddd5a05dda27e60b03fba3f6fdc5dc HG_URL=remote:ssh:127.0.0.1
  remote: incoming hook: HG_BUNDLE2=1 HG_NODE=741fd2094512a57edc6d93e8257f961c82bf9dde HG_SOURCE=serve HG_TXNID=TXN:0d4ea256a2ddd5a05dda27e60b03fba3f6fdc5dc HG_URL=remote:ssh:127.0.0.1

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
  pretxnclose hook: HG_PENDING=$TESTTMP/server HG_PHASES_MOVED=1 HG_TXNID=TXN:f94ce216255bcd6706b0fddf8d96a0f53a5865f4 HG_XNNAME=commit
  txnclose hook: HG_PHASES_MOVED=1 HG_TXNID=TXN:f94ce216255bcd6706b0fddf8d96a0f53a5865f4 HG_TXNNAME=commit

  $ cd ../client
  $ echo 'quux' > a
  $ commit 'a => quux'
  pretxnclose hook: HG_PENDING=$TESTTMP/client HG_TXNID=TXN:3ef3bd2b0a657b4841b98fa71490bcd76e4b8627 HG_XNNAME=commit
  txnclose hook: HG_TXNID=TXN:3ef3bd2b0a657b4841b98fa71490bcd76e4b8627 HG_TXNNAME=commit
  $ echo 'foofoo' > b
  $ commit 'b => foofoo'
  pretxnclose hook: HG_PENDING=$TESTTMP/client HG_TXNID=TXN:63cc4f6dcd5a2a6313dd42c7262e4341ca6d2efb HG_XNNAME=commit
  txnclose hook: HG_TXNID=TXN:63cc4f6dcd5a2a6313dd42c7262e4341ca6d2efb HG_TXNNAME=commit
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
  pretxnclose hook: HG_TXNID=TXN:84e019efba08aa642953dacc44b55339bf2e12e8 HG_XNNAME=strip
  txnclose hook: HG_TXNID=TXN:84e019efba08aa642953dacc44b55339bf2e12e8 HG_TXNNAME=strip
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
  pretxnclose hook: HG_PENDING=$TESTTMP/client HG_TXNID=TXN:5da0fb7df81cef57c7d58b25045ef87dce644155 HG_XNNAME=commit
  txnclose hook: HG_TXNID=TXN:5da0fb7df81cef57c7d58b25045ef87dce644155 HG_TXNNAME=commit
  $ echo 'quux' > a
  $ commit 'a => quux'
  pretxnclose hook: HG_PENDING=$TESTTMP/client HG_TXNID=TXN:9a028f10afd94abea0cb053f12906f5b8cc524fc HG_XNNAME=commit
  txnclose hook: HG_TXNID=TXN:9a028f10afd94abea0cb053f12906f5b8cc524fc HG_TXNNAME=commit
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
  pretxnclose hook: HG_TXNID=TXN:fdca5f90cb740ee881711f171c8056cebbf6311e HG_XNNAME=strip
  txnclose hook: HG_TXNID=TXN:fdca5f90cb740ee881711f171c8056cebbf6311e HG_TXNNAME=strip

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
  pretxnclose hook: HG_TXNID=TXN:c74c46ba74ee8986bca41db6b34496819b641bc3 HG_XNNAME=strip
  txnclose hook: HG_TXNID=TXN:c74c46ba74ee8986bca41db6b34496819b641bc3 HG_TXNNAME=strip
  $ hg up 741fd2094512
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo "tux" > other
  $ hg add other
  $ hg commit -qm "branch left"
  pretxnclose hook: HG_PENDING=$TESTTMP/client HG_PHASES_MOVED=1 HG_TXNID=TXN:e01132e126cd85974d1613d36540e7058051650a HG_XNNAME=commit
  txnclose hook: HG_PHASES_MOVED=1 HG_TXNID=TXN:e01132e126cd85974d1613d36540e7058051650a HG_TXNNAME=commit
  $ hg book master -r tip
  moving bookmark 'master' forward from 741fd2094512
  $ hg up -q 2
  $ echo branched > c
  $ hg commit -Aqm "branch start"
  pretxnclose hook: HG_PENDING=$TESTTMP/client HG_PHASES_MOVED=1 HG_TXNID=TXN:461bc99f9e7c8e41c44d961e4710815804a461d4 HG_XNNAME=commit
  txnclose hook: HG_PHASES_MOVED=1 HG_TXNID=TXN:461bc99f9e7c8e41c44d961e4710815804a461d4 HG_TXNNAME=commit
  $ echo branched2 > c
  $ hg commit -qm "branch middle"
  pretxnclose hook: HG_PENDING=$TESTTMP/client HG_TXNID=TXN:a2555fa57425125270872e71a8142e4c25099085 HG_XNNAME=commit
  txnclose hook: HG_TXNID=TXN:a2555fa57425125270872e71a8142e4c25099085 HG_TXNNAME=commit
  $ hg merge -q master
  $ hg commit -qm "merge"
  pretxnclose hook: HG_PENDING=$TESTTMP/client HG_TXNID=TXN:5dce47b73ff15cce78d43a364c72cd4e551ce5ac HG_XNNAME=commit
  txnclose hook: HG_TXNID=TXN:5dce47b73ff15cce78d43a364c72cd4e551ce5ac HG_TXNNAME=commit
  $ echo ontopofmerge > c
  $ hg commit -qm "on top of merge"
  pretxnclose hook: HG_PENDING=$TESTTMP/client HG_TXNID=TXN:07edb2eceee7cf2621647451330a97ffc39ca3a3 HG_XNNAME=commit
  txnclose hook: HG_TXNID=TXN:07edb2eceee7cf2621647451330a97ffc39ca3a3 HG_TXNNAME=commit
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
  prechangegroup hook: HG_SOURCE=push-response HG_TXNID=TXN:9b36f1f4bb43df0f46182d2a7cf26c9f7cc55b4f HG_URL=ssh://user@dummy/server
  adding changesets
  adding manifests
  adding file changes
  added 4 changesets with 1 changes to 3 files (+1 heads)
  pretxnchangegroup hook: HG_NODE=fb983dc509b61b92a3f19cc326f62b424bb25d1c HG_PENDING=$TESTTMP/client HG_SOURCE=push-response HG_TXNID=TXN:9b36f1f4bb43df0f46182d2a7cf26c9f7cc55b4f HG_URL=ssh://user@dummy/server
  updating bookmark master
  remote: prechangegroup hook: HG_BUNDLE2=1 HG_SOURCE=serve HG_TXNID=TXN:cb85809e15175ddd6df718421eab9e6bd7a078e6 HG_URL=remote:ssh:127.0.0.1
  remote: pretxnchangegroup hook: HG_BUNDLE2=1 HG_NODE=7548c79a5591fca7a09470b814ead1b3f471aa89 HG_PENDING=$TESTTMP/server HG_SOURCE=serve HG_TXNID=TXN:cb85809e15175ddd6df718421eab9e6bd7a078e6 HG_URL=remote:ssh:127.0.0.1
  remote: preoutgoing hook: HG_SOURCE=rebase:reply
  remote: pretxnclose hook: HG_BOOKMARK_MOVED=1 HG_BUNDLE2=1 HG_NODE=cf07bdf4226ef5167b9f86119e915ff3f239642a HG_PENDING=$TESTTMP/server HG_PHASES_MOVED=1 HG_SOURCE=serve HG_TXNID=TXN:cb85809e15175ddd6df718421eab9e6bd7a078e6 HG_URL=remote:ssh:127.0.0.1 HG_XNNAME=serve
  remote: txnclose hook: HG_BOOKMARK_MOVED=1 HG_BUNDLE2=1 HG_NODE=cf07bdf4226ef5167b9f86119e915ff3f239642a HG_PHASES_MOVED=1 HG_SOURCE=serve HG_TXNID=TXN:cb85809e15175ddd6df718421eab9e6bd7a078e6 HG_TXNNAME=serve HG_URL=remote:ssh:127.0.0.1
  remote: changegroup hook: HG_BUNDLE2=1 HG_NODE=cf07bdf4226ef5167b9f86119e915ff3f239642a HG_SOURCE=serve HG_TXNID=TXN:cb85809e15175ddd6df718421eab9e6bd7a078e6 HG_URL=remote:ssh:127.0.0.1
  remote: incoming hook: HG_BUNDLE2=1 HG_NODE=cf07bdf4226ef5167b9f86119e915ff3f239642a HG_SOURCE=serve HG_TXNID=TXN:cb85809e15175ddd6df718421eab9e6bd7a078e6 HG_URL=remote:ssh:127.0.0.1
  remote: incoming hook: HG_BUNDLE2=1 HG_NODE=add5ec74853d53cf76e4b735e033a2350e7fe4f3 HG_SOURCE=serve HG_TXNID=TXN:cb85809e15175ddd6df718421eab9e6bd7a078e6 HG_URL=remote:ssh:127.0.0.1
  remote: incoming hook: HG_BUNDLE2=1 HG_NODE=5a0cbf3df4ef43ccc96fedd1030d6b8c59f2cd32 HG_SOURCE=serve HG_TXNID=TXN:cb85809e15175ddd6df718421eab9e6bd7a078e6 HG_URL=remote:ssh:127.0.0.1
  remote: incoming hook: HG_BUNDLE2=1 HG_NODE=93a3cc822f6ac9c91c5c645cab8fec47a26da52e HG_SOURCE=serve HG_TXNID=TXN:cb85809e15175ddd6df718421eab9e6bd7a078e6 HG_URL=remote:ssh:127.0.0.1
  remote: incoming hook: HG_BUNDLE2=1 HG_NODE=7548c79a5591fca7a09470b814ead1b3f471aa89 HG_SOURCE=serve HG_TXNID=TXN:cb85809e15175ddd6df718421eab9e6bd7a078e6 HG_URL=remote:ssh:127.0.0.1
  remote: outgoing hook: HG_NODE=fb983dc509b61b92a3f19cc326f62b424bb25d1c HG_SOURCE=rebase:reply
  pretxnclose hook: HG_NODE=fb983dc509b61b92a3f19cc326f62b424bb25d1c HG_PENDING=$TESTTMP/client HG_PHASES_MOVED=1 HG_SOURCE=push-response HG_TXNID=TXN:9b36f1f4bb43df0f46182d2a7cf26c9f7cc55b4f HG_URL=ssh://user@dummy/server HG_XNNAME=push-response
  ssh://user@dummy/server
  txnclose hook: HG_NODE=fb983dc509b61b92a3f19cc326f62b424bb25d1c HG_PHASES_MOVED=1 HG_SOURCE=push-response HG_TXNID=TXN:9b36f1f4bb43df0f46182d2a7cf26c9f7cc55b4f HG_TXNNAME=push-response
  ssh://user@dummy/server HG_URL=ssh://user@dummy/server
  changegroup hook: HG_NODE=fb983dc509b61b92a3f19cc326f62b424bb25d1c HG_SOURCE=push-response HG_TXNID=TXN:9b36f1f4bb43df0f46182d2a7cf26c9f7cc55b4f HG_URL=ssh://user@dummy/server
  incoming hook: HG_NODE=fb983dc509b61b92a3f19cc326f62b424bb25d1c HG_SOURCE=push-response HG_TXNID=TXN:9b36f1f4bb43df0f46182d2a7cf26c9f7cc55b4f HG_URL=ssh://user@dummy/server
  incoming hook: HG_NODE=cf07bdf4226ef5167b9f86119e915ff3f239642a HG_SOURCE=push-response HG_TXNID=TXN:9b36f1f4bb43df0f46182d2a7cf26c9f7cc55b4f HG_URL=ssh://user@dummy/server
  incoming hook: HG_NODE=93a3cc822f6ac9c91c5c645cab8fec47a26da52e HG_SOURCE=push-response HG_TXNID=TXN:9b36f1f4bb43df0f46182d2a7cf26c9f7cc55b4f HG_URL=ssh://user@dummy/server
  incoming hook: HG_NODE=7548c79a5591fca7a09470b814ead1b3f471aa89 HG_SOURCE=push-response HG_TXNID=TXN:9b36f1f4bb43df0f46182d2a7cf26c9f7cc55b4f HG_URL=ssh://user@dummy/server
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
  outgoing hook: HG_NODE=4cfedb0dc25f66f5d020564af00d4a39ad56f33b HG_SOURCE=strip
  pretxnclose hook: HG_TXNID=TXN:a71b3e5424e05be1cbc3381c062878dd1363d657 HG_XNNAME=strip
  txnclose hook: HG_TXNID=TXN:a71b3e5424e05be1cbc3381c062878dd1363d657 HG_TXNNAME=strip
  $ cd ../client
  $ hg strip -r add5ec74853d -q
  preoutgoing hook: HG_SOURCE=strip
  outgoing hook: HG_NODE=4cfedb0dc25f66f5d020564af00d4a39ad56f33b HG_SOURCE=strip
  preoutgoing hook: HG_SOURCE=strip
  outgoing hook: HG_NODE=fb983dc509b61b92a3f19cc326f62b424bb25d1c HG_SOURCE=strip
  pretxnclose hook: HG_TXNID=TXN:8d69494b6b60a87f106c2a500ac98894a9e0c627 HG_XNNAME=strip
  prechangegroup hook: HG_SOURCE=strip HG_TXNID=TXN:61ac08fc21f8dec527783758249cdadf1b4ba6bf HG_URL=bundle:$TESTTMP/client/.hg/strip-backup/add5ec74853d-7448e4af-temp.hg
  pretxnchangegroup hook: HG_NODE=fb983dc509b61b92a3f19cc326f62b424bb25d1c HG_PENDING=$TESTTMP/client HG_SOURCE=strip HG_TXNID=TXN:61ac08fc21f8dec527783758249cdadf1b4ba6bf HG_URL=bundle:$TESTTMP/client/.hg/strip-backup/add5ec74853d-7448e4af-temp.hg
  pretxnclose hook: HG_NODE=fb983dc509b61b92a3f19cc326f62b424bb25d1c HG_PENDING=$TESTTMP/client HG_SOURCE=strip HG_TXNID=TXN:61ac08fc21f8dec527783758249cdadf1b4ba6bf HG_URL=bundle:$TESTTMP/client/.hg/strip-backup/add5ec74853d-7448e4af-temp.hg HG_XNNAME=strip
  bundle:$TESTTMP/client/.hg/strip-backup/add5ec74853d-7448e4af-temp.hg
  txnclose hook: HG_TXNID=TXN:8d69494b6b60a87f106c2a500ac98894a9e0c627 HG_TXNNAME=strip
  txnclose hook: HG_NODE=fb983dc509b61b92a3f19cc326f62b424bb25d1c HG_SOURCE=strip HG_TXNID=TXN:61ac08fc21f8dec527783758249cdadf1b4ba6bf HG_TXNNAME=strip
  bundle:$TESTTMP/client/.hg/strip-backup/add5ec74853d-7448e4af-temp.hg HG_URL=bundle:$TESTTMP/client/.hg/strip-backup/add5ec74853d-7448e4af-temp.hg
  changegroup hook: HG_NODE=fb983dc509b61b92a3f19cc326f62b424bb25d1c HG_SOURCE=strip HG_TXNID=TXN:61ac08fc21f8dec527783758249cdadf1b4ba6bf HG_URL=bundle:$TESTTMP/client/.hg/strip-backup/add5ec74853d-7448e4af-temp.hg
  incoming hook: HG_NODE=fb983dc509b61b92a3f19cc326f62b424bb25d1c HG_SOURCE=strip HG_TXNID=TXN:61ac08fc21f8dec527783758249cdadf1b4ba6bf HG_URL=bundle:$TESTTMP/client/.hg/strip-backup/add5ec74853d-7448e4af-temp.hg
  incoming hook: HG_NODE=cf07bdf4226ef5167b9f86119e915ff3f239642a HG_SOURCE=strip HG_TXNID=TXN:61ac08fc21f8dec527783758249cdadf1b4ba6bf HG_URL=bundle:$TESTTMP/client/.hg/strip-backup/add5ec74853d-7448e4af-temp.hg
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
  pretxnclose hook: HG_TXNID=TXN:129fd996d3f313b9c5b35f3a8e4ca779944cae16 HG_XNNAME=strip
  txnclose hook: HG_TXNID=TXN:129fd996d3f313b9c5b35f3a8e4ca779944cae16 HG_TXNNAME=strip
  $ hg strip -qr e6b7549904cd2a7991ef25bc2e0fd910801af2cd
  preoutgoing hook: HG_SOURCE=strip
  outgoing hook: HG_NODE=e6b7549904cd2a7991ef25bc2e0fd910801af2cd HG_SOURCE=strip
  pretxnclose hook: HG_TXNID=TXN:8a04f226acc558776f83c8d2d5aca03c2a480cf2 HG_XNNAME=strip
  txnclose hook: HG_TXNID=TXN:8a04f226acc558776f83c8d2d5aca03c2a480cf2 HG_TXNNAME=strip
  $ hg up -q 741fd2094512
  $ hg mv b k
  $ commit 'b => k'
  pretxnclose hook: HG_PENDING=$TESTTMP/client HG_PHASES_MOVED=1 HG_TXNID=TXN:5f794f1adfed3e4b2c7bc543234d3c844ec38750 HG_XNNAME=commit
  txnclose hook: HG_PHASES_MOVED=1 HG_TXNID=TXN:5f794f1adfed3e4b2c7bc543234d3c844ec38750 HG_TXNNAME=commit
  $ hg mv k b
  $ echo 'foobar' > b
  $ commit 'b => foobar'
  pretxnclose hook: HG_PENDING=$TESTTMP/client HG_TXNID=TXN:129082e00205bdac189dd2615653557579a1aca3 HG_XNNAME=commit
  txnclose hook: HG_TXNID=TXN:129082e00205bdac189dd2615653557579a1aca3 HG_TXNNAME=commit
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
  prechangegroup hook: HG_SOURCE=push-response HG_TXNID=TXN:0f52e077da95b3e8c278e4cc685b390c91a708d8 HG_URL=ssh://user@dummy/server
  adding changesets
  adding manifests
  adding file changes
  added 4 changesets with 2 changes to 4 files (+1 heads)
  pretxnchangegroup hook: HG_NODE=fb983dc509b61b92a3f19cc326f62b424bb25d1c HG_PENDING=$TESTTMP/client HG_SOURCE=push-response HG_TXNID=TXN:0f52e077da95b3e8c278e4cc685b390c91a708d8 HG_URL=ssh://user@dummy/server
  2 new obsolescence markers
  remote: prechangegroup hook: HG_BUNDLE2=1 HG_SOURCE=serve HG_TXNID=TXN:2f03d0777b9a6cf31f07a0b5a26b5a8e7882d554 HG_URL=remote:ssh:127.0.0.1
  remote: pretxnchangegroup hook: HG_BUNDLE2=1 HG_NODE=d53a62ed14be0980584e1f92f9c47031ef806a62 HG_PENDING=$TESTTMP/server HG_SOURCE=serve HG_TXNID=TXN:2f03d0777b9a6cf31f07a0b5a26b5a8e7882d554 HG_URL=remote:ssh:127.0.0.1
  remote: preoutgoing hook: HG_SOURCE=rebase:reply
  remote: pretxnclose hook: HG_BUNDLE2=1 HG_NEW_OBSMARKERS=2 HG_NODE=0d76868c25e6789734c06e056f235e1fa223da74 HG_PENDING=$TESTTMP/server HG_PHASES_MOVED=1 HG_SOURCE=serve HG_TXNID=TXN:2f03d0777b9a6cf31f07a0b5a26b5a8e7882d554 HG_URL=remote:ssh:127.0.0.1 HG_XNNAME=serve
  remote: txnclose hook: HG_BUNDLE2=1 HG_NEW_OBSMARKERS=2 HG_NODE=0d76868c25e6789734c06e056f235e1fa223da74 HG_PHASES_MOVED=1 HG_SOURCE=serve HG_TXNID=TXN:2f03d0777b9a6cf31f07a0b5a26b5a8e7882d554 HG_TXNNAME=serve HG_URL=remote:ssh:127.0.0.1
  remote: changegroup hook: HG_BUNDLE2=1 HG_NODE=0d76868c25e6789734c06e056f235e1fa223da74 HG_SOURCE=serve HG_TXNID=TXN:2f03d0777b9a6cf31f07a0b5a26b5a8e7882d554 HG_URL=remote:ssh:127.0.0.1
  remote: incoming hook: HG_BUNDLE2=1 HG_NODE=0d76868c25e6789734c06e056f235e1fa223da74 HG_SOURCE=serve HG_TXNID=TXN:2f03d0777b9a6cf31f07a0b5a26b5a8e7882d554 HG_URL=remote:ssh:127.0.0.1
  remote: incoming hook: HG_BUNDLE2=1 HG_NODE=d53a62ed14be0980584e1f92f9c47031ef806a62 HG_SOURCE=serve HG_TXNID=TXN:2f03d0777b9a6cf31f07a0b5a26b5a8e7882d554 HG_URL=remote:ssh:127.0.0.1
  remote: outgoing hook: HG_NODE=fb983dc509b61b92a3f19cc326f62b424bb25d1c HG_SOURCE=rebase:reply
  pretxnclose hook: HG_NEW_OBSMARKERS=2 HG_NODE=fb983dc509b61b92a3f19cc326f62b424bb25d1c HG_PENDING=$TESTTMP/client HG_PHASES_MOVED=1 HG_SOURCE=push-response HG_TXNID=TXN:0f52e077da95b3e8c278e4cc685b390c91a708d8 HG_URL=ssh://user@dummy/server HG_XNNAME=push-response
  ssh://user@dummy/server
  txnclose hook: HG_NEW_OBSMARKERS=2 HG_NODE=fb983dc509b61b92a3f19cc326f62b424bb25d1c HG_PHASES_MOVED=1 HG_SOURCE=push-response HG_TXNID=TXN:0f52e077da95b3e8c278e4cc685b390c91a708d8 HG_TXNNAME=push-response
  ssh://user@dummy/server HG_URL=ssh://user@dummy/server
  changegroup hook: HG_NODE=fb983dc509b61b92a3f19cc326f62b424bb25d1c HG_SOURCE=push-response HG_TXNID=TXN:0f52e077da95b3e8c278e4cc685b390c91a708d8 HG_URL=ssh://user@dummy/server
  incoming hook: HG_NODE=fb983dc509b61b92a3f19cc326f62b424bb25d1c HG_SOURCE=push-response HG_TXNID=TXN:0f52e077da95b3e8c278e4cc685b390c91a708d8 HG_URL=ssh://user@dummy/server
  incoming hook: HG_NODE=cf07bdf4226ef5167b9f86119e915ff3f239642a HG_SOURCE=push-response HG_TXNID=TXN:0f52e077da95b3e8c278e4cc685b390c91a708d8 HG_URL=ssh://user@dummy/server
  incoming hook: HG_NODE=0d76868c25e6789734c06e056f235e1fa223da74 HG_SOURCE=push-response HG_TXNID=TXN:0f52e077da95b3e8c278e4cc685b390c91a708d8 HG_URL=ssh://user@dummy/server
  incoming hook: HG_NODE=d53a62ed14be0980584e1f92f9c47031ef806a62 HG_SOURCE=push-response HG_TXNID=TXN:0f52e077da95b3e8c278e4cc685b390c91a708d8 HG_URL=ssh://user@dummy/server

  $ hg pull
  pulling from ssh://user@dummy/server
  searching for changes
  no changes found
  pretxnclose hook: HG_NEW_OBSMARKERS=0 HG_PENDING=$TESTTMP/client HG_PHASES_MOVED=1 HG_SOURCE=pull HG_TXNID=TXN:d548a3d20dabab8c5d20af5422ab59c3ca1bd32c HG_URL=ssh://user@dummy/server HG_XNNAME=pull
  ssh://user@dummy/server
  txnclose hook: HG_NEW_OBSMARKERS=0 HG_PHASES_MOVED=1 HG_SOURCE=pull HG_TXNID=TXN:d548a3d20dabab8c5d20af5422ab59c3ca1bd32c HG_TXNNAME=pull
  ssh://user@dummy/server HG_URL=ssh://user@dummy/server
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
  pretxnclose hook: HG_PENDING=$TESTTMP/client HG_PHASES_MOVED=1 HG_TXNID=TXN:c3d4f3b2de2be1dcf7b8eac3f84e68c5a11e5572 HG_XNNAME=commit
  txnclose hook: HG_PHASES_MOVED=1 HG_TXNID=TXN:c3d4f3b2de2be1dcf7b8eac3f84e68c5a11e5572 HG_TXNNAME=commit
  $ hg log -r master -T"{node}\n"
  56b2e094996609874ae1c9aae1626bfba61d07d8
  $ hg push --to master
  pushing to ssh://user@dummy/server
  searching for changes
  preoutgoing hook: HG_SOURCE=push
  outgoing hook: HG_NODE=56b2e094996609874ae1c9aae1626bfba61d07d8 HG_SOURCE=push
  updating bookmark master
  remote: prechangegroup hook: HG_BUNDLE2=1 HG_SOURCE=serve HG_TXNID=TXN:b3c5d3ee6d2f15e23de022f395453c08a01b7c67 HG_URL=remote:ssh:127.0.0.1
  remote: pretxnchangegroup hook: HG_BUNDLE2=1 HG_NODE=56b2e094996609874ae1c9aae1626bfba61d07d8 HG_PENDING=$TESTTMP/server HG_SOURCE=serve HG_TXNID=TXN:b3c5d3ee6d2f15e23de022f395453c08a01b7c67 HG_URL=remote:ssh:127.0.0.1
  remote: pretxnclose hook: HG_BOOKMARK_MOVED=1 HG_BUNDLE2=1 HG_NEW_OBSMARKERS=0 HG_NODE=56b2e094996609874ae1c9aae1626bfba61d07d8 HG_PENDING=$TESTTMP/server HG_PHASES_MOVED=1 HG_SOURCE=serve HG_TXNID=TXN:b3c5d3ee6d2f15e23de022f395453c08a01b7c67 HG_URL=remote:ssh:127.0.0.1 HG_XNNAME=serve
  remote: txnclose hook: HG_BOOKMARK_MOVED=1 HG_BUNDLE2=1 HG_NEW_OBSMARKERS=0 HG_NODE=56b2e094996609874ae1c9aae1626bfba61d07d8 HG_PHASES_MOVED=1 HG_SOURCE=serve HG_TXNID=TXN:b3c5d3ee6d2f15e23de022f395453c08a01b7c67 HG_TXNNAME=serve HG_URL=remote:ssh:127.0.0.1
  remote: changegroup hook: HG_BUNDLE2=1 HG_NODE=56b2e094996609874ae1c9aae1626bfba61d07d8 HG_SOURCE=serve HG_TXNID=TXN:b3c5d3ee6d2f15e23de022f395453c08a01b7c67 HG_URL=remote:ssh:127.0.0.1
  remote: incoming hook: HG_BUNDLE2=1 HG_NODE=56b2e094996609874ae1c9aae1626bfba61d07d8 HG_SOURCE=serve HG_TXNID=TXN:b3c5d3ee6d2f15e23de022f395453c08a01b7c67 HG_URL=remote:ssh:127.0.0.1
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
  remote: pretxnclose hook: HG_BOOKMARK_MOVED=1 HG_BUNDLE2=1 HG_PENDING=$TESTTMP/server HG_SOURCE=serve HG_TXNID=TXN:63af7c1456309aed28b8bcdefa3f8a32b95a8268 HG_URL=remote:ssh:127.0.0.1 HG_XNNAME=serve
  remote: txnclose hook: HG_BOOKMARK_MOVED=1 HG_BUNDLE2=1 HG_SOURCE=serve HG_TXNID=TXN:63af7c1456309aed28b8bcdefa3f8a32b95a8268 HG_TXNNAME=serve HG_URL=remote:ssh:127.0.0.1
  [1]
  $ hg log -r stable -R ../server
  changeset:   5:fb983dc509b6
  bookmark:    stable
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     a => baz
  
