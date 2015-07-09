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
  $ echo 'bar' > b
  $ commit 'b => bar'
  $ echo 'baz' > b
  $ commit 'b => baz'
  $ hg push
  pushing to ssh://user@dummy/server
  searching for changes
  remote has heads on branch 'default' that are not known locally: add0c792bfce
  abort: push creates new remote head 2e6d0db3b0dd!
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
  @  b => baz [draft:2e6d0db3b0dd]
  |
  o  b => bar [draft:7585d2e4bf9a]
  |
  o  initial [public:2bb9d20e471c]
  
  $ hg push --to default
  pushing to ssh://user@dummy/server
  searching for changes
  preoutgoing hook: HG_SOURCE=push
  outgoing hook: HG_NODE=7585d2e4bf9ab3b58237c20d51ad5ef8778934d0 HG_SOURCE=push
  remote: prechangegroup hook: HG_BUNDLE2=1 HG_SOURCE=serve HG_TXNID=TXN:* HG_URL=remote:ssh:127.0.0.1 (glob)
  remote: pretxnchangegroup hook: HG_BUNDLE2=1 HG_PENDING=$TESTTMP/server HG_SOURCE=serve HG_TXNID=TXN:* HG_URL=remote:ssh:127.0.0.1 (glob)
  remote: preoutgoing hook: HG_SOURCE=rebase:reply
  remote: changegroup hook: HG_BUNDLE2=1 HG_NODE=fe66d1686ec2a43093fb79e196ab9c4ae7cd835a HG_SOURCE=serve HG_TXNID=TXN:* HG_URL=remote:ssh:127.0.0.1 (glob)
  remote: incoming hook: HG_BUNDLE2=1 HG_NODE=fe66d1686ec2a43093fb79e196ab9c4ae7cd835a HG_SOURCE=serve HG_TXNID=TXN:* HG_URL=remote:ssh:127.0.0.1 (glob)
  remote: incoming hook: HG_BUNDLE2=1 HG_NODE=7ba922f02e46f2426e728a97137be032470cdd1b HG_SOURCE=serve HG_TXNID=TXN:* HG_URL=remote:ssh:127.0.0.1 (glob)
  prechangegroup hook: HG_SOURCE=push-response HG_TXNID=TXN:* HG_URL=ssh://user@dummy/server (glob)
  adding changesets
  remote: outgoing hook: HG_NODE=add0c792bfce89610d277fd5b1e32f5287994d1d HG_SOURCE=rebase:reply
  adding manifests
  adding file changes
  added 3 changesets with 1 changes to 2 files (+1 heads)
  pretxnchangegroup hook: HG_NODE=add0c792bfce89610d277fd5b1e32f5287994d1d HG_PENDING=$TESTTMP/client HG_SOURCE=push-response HG_TXNID=TXN:* HG_URL=ssh://user@dummy/server (glob)
  changegroup hook: HG_NODE=add0c792bfce89610d277fd5b1e32f5287994d1d HG_SOURCE=push-response HG_TXNID=TXN:* HG_URL=ssh://user@dummy/server (glob)
  incoming hook: HG_NODE=add0c792bfce89610d277fd5b1e32f5287994d1d HG_SOURCE=push-response HG_TXNID=TXN:* HG_URL=ssh://user@dummy/server (glob)
  incoming hook: HG_NODE=fe66d1686ec2a43093fb79e196ab9c4ae7cd835a HG_SOURCE=push-response HG_TXNID=TXN:* HG_URL=ssh://user@dummy/server (glob)
  incoming hook: HG_NODE=7ba922f02e46f2426e728a97137be032470cdd1b HG_SOURCE=push-response HG_TXNID=TXN:* HG_URL=ssh://user@dummy/server (glob)

  $ cd ../server
  $ hg update default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ log
  @  b => baz [public:7ba922f02e46]
  |
  o  b => bar [public:fe66d1686ec2]
  |
  o  a => bar [public:add0c792bfce]
  |
  o  initial [public:2bb9d20e471c]
  

  $ cd ../client
  $ hg strip 1
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  preoutgoing hook: HG_SOURCE=strip
  outgoing hook: HG_NODE=7585d2e4bf9ab3b58237c20d51ad5ef8778934d0 HG_SOURCE=strip
  saved backup bundle to $TESTTMP/client/.hg/strip-backup/7585d2e4bf9a-1d6b2021-backup.hg (glob)
  preoutgoing hook: HG_SOURCE=strip
  outgoing hook: HG_NODE=add0c792bfce89610d277fd5b1e32f5287994d1d HG_SOURCE=strip
  prechangegroup hook: HG_SOURCE=strip HG_TXNID=TXN:* HG_URL=bundle:$TESTTMP/client/.hg/strip-backup/7585d2e4bf9a-e5e817a4-temp.hg (glob)
  pretxnchangegroup hook: HG_NODE=add0c792bfce89610d277fd5b1e32f5287994d1d HG_PENDING=$TESTTMP/client HG_SOURCE=strip HG_TXNID=TXN:* HG_URL=bundle:$TESTTMP/client/.hg/strip-backup/7585d2e4bf9a-e5e817a4-temp.hg (glob)
  changegroup hook: HG_NODE=add0c792bfce89610d277fd5b1e32f5287994d1d HG_SOURCE=strip HG_TXNID=TXN:* HG_URL=bundle:$TESTTMP/client/.hg/strip-backup/7585d2e4bf9a-e5e817a4-temp.hg (glob)
  incoming hook: HG_NODE=add0c792bfce89610d277fd5b1e32f5287994d1d HG_SOURCE=strip HG_TXNID=TXN:* HG_URL=bundle:$TESTTMP/client/.hg/strip-backup/7585d2e4bf9a-e5e817a4-temp.hg (glob)
  incoming hook: HG_NODE=fe66d1686ec2a43093fb79e196ab9c4ae7cd835a HG_SOURCE=strip HG_TXNID=TXN:* HG_URL=bundle:$TESTTMP/client/.hg/strip-backup/7585d2e4bf9a-e5e817a4-temp.hg (glob)
  incoming hook: HG_NODE=7ba922f02e46f2426e728a97137be032470cdd1b HG_SOURCE=strip HG_TXNID=TXN:* HG_URL=bundle:$TESTTMP/client/.hg/strip-backup/7585d2e4bf9a-e5e817a4-temp.hg (glob)
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
  @  b => quux [draft:137b1b6ef903]
  |

  $ hg push --to default
  pushing to ssh://user@dummy/server
  searching for changes
  preoutgoing hook: HG_SOURCE=push
  outgoing hook: HG_NODE=137b1b6ef90327e7addb09edcb005cbe0bee7493 HG_SOURCE=push
  remote: prechangegroup hook: HG_BUNDLE2=1 HG_SOURCE=serve HG_TXNID=TXN:* HG_URL=remote:ssh:127.0.0.1 (glob)
  remote: pretxnchangegroup hook: HG_BUNDLE2=1 HG_PENDING=$TESTTMP/server HG_SOURCE=serve HG_TXNID=TXN:* HG_URL=remote:ssh:127.0.0.1 (glob)
  remote: changegroup hook: HG_BUNDLE2=1 HG_NODE=137b1b6ef90327e7addb09edcb005cbe0bee7493 HG_SOURCE=serve HG_TXNID=TXN:* HG_URL=remote:ssh:127.0.0.1 (glob)
  remote: incoming hook: HG_BUNDLE2=1 HG_NODE=137b1b6ef90327e7addb09edcb005cbe0bee7493 HG_SOURCE=serve HG_TXNID=TXN:* HG_URL=remote:ssh:127.0.0.1 (glob)

  $ cd ../server
  $ hg update default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ log
  @  b => quux [public:137b1b6ef903]
  |
  o  b => baz [public:7ba922f02e46]
  |
  o  b => bar [public:fe66d1686ec2]
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
  outgoing hook: HG_NODE=17000cb5287186f68e3ad728ee9c573feb0fa3c3 HG_SOURCE=push
  abort: conflicting changes in ['a']
  [255]

  $ hg strip 5
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  preoutgoing hook: HG_SOURCE=strip
  outgoing hook: HG_NODE=17000cb5287186f68e3ad728ee9c573feb0fa3c3 HG_SOURCE=strip
  saved backup bundle to $TESTTMP/client/.hg/strip-backup/17000cb52871-8ac986d3-backup.hg (glob)
  $ cd ../server
  $ log
  @  a => baz [draft:ddd9491cc0b4]
  |
  o  b => quux [public:137b1b6ef903]
  |
  o  b => baz [public:7ba922f02e46]
  |
  o  b => bar [public:fe66d1686ec2]
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
  outgoing hook: HG_NODE=6e1d0b2f81801d1de2645ac4295781ff2ee08fb4 HG_SOURCE=push
  abort: conflicting changes in ['a']
  [255]

  $ hg strip 5
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  preoutgoing hook: HG_SOURCE=strip
  outgoing hook: HG_NODE=6e1d0b2f81801d1de2645ac4295781ff2ee08fb4 HG_SOURCE=strip
  saved backup bundle to $TESTTMP/client/.hg/strip-backup/6e1d0b2f8180-84c690c2-backup.hg (glob)

  $ cd ../server
  $ log
  @  a => baz [draft:ddd9491cc0b4]
  |
  o  b => quux [public:137b1b6ef903]
  |
  o  b => baz [public:7ba922f02e46]
  |
  o  b => bar [public:fe66d1686ec2]
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
  outgoing hook: HG_NODE=ddd9491cc0b4965056141b5064ac0c141153b1a9 HG_SOURCE=strip
  $ hg book master -r tip
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
  moving bookmark 'master' forward from 137b1b6ef903
  $ log
  @  on top of merge [draft:a4a78a612a9c] master
  |
  o    merge [draft:cb3482060521]
  |\
  | o  branch middle [draft:25f2e23fb053]
  | |
  | o  branch start [draft:b9f6a18cb261]
  | |
  o |  b => quux [public:137b1b6ef903]
  | |
  o |  b => baz [public:7ba922f02e46]
  |/
  o  b => bar [public:fe66d1686ec2]
  |
  o  a => bar [public:add0c792bfce]
  |
  o  initial [public:2bb9d20e471c]
  
  $ hg push --to master -B master
  pushing to ssh://user@dummy/server
  searching for changes
  preoutgoing hook: HG_SOURCE=push
  outgoing hook: HG_NODE=b9f6a18cb2619a206f6d99dbcbdfbd75b2975506 HG_SOURCE=push
  remote: prechangegroup hook: HG_BUNDLE2=1 HG_SOURCE=serve * (glob)
  remote: pretxnchangegroup hook: HG_BUNDLE2=1 HG_PENDING=$TESTTMP/server HG_SOURCE=serve * (glob)
  remote: preoutgoing hook: HG_SOURCE=rebase:reply
  remote: changegroup hook: HG_BUNDLE2=1 HG_NODE=b9f6a18cb2619a206f6d99dbcbdfbd75b2975506 HG_SOURCE=serve * (glob)
  remote: incoming hook: HG_BUNDLE2=1 HG_NODE=b9f6a18cb2619a206f6d99dbcbdfbd75b2975506 HG_SOURCE=serve * (glob)
  remote: incoming hook: HG_BUNDLE2=1 HG_NODE=25f2e23fb0530fa409515539d3cb936a2e3723a4 HG_SOURCE=serve * (glob)
  remote: incoming hook: HG_BUNDLE2=1 HG_NODE=b3e5033049f316725da840de07b96879ac325775 HG_SOURCE=serve * (glob)
  remote: incoming hook: HG_BUNDLE2=1 HG_NODE=8eaad82b215848062618b309ae58e600a81f87a5 HG_SOURCE=serve * (glob)
  prechangegroup hook: HG_SOURCE=push-response * (glob)
  adding changesets
  remote: outgoing hook: HG_NODE=ddd9491cc0b4965056141b5064ac0c141153b1a9 HG_SOURCE=rebase:reply
  adding manifests
  adding file changes
  added 3 changesets with 1 changes to 2 files (+1 heads)
  pretxnchangegroup hook: HG_NODE=ddd9491cc0b4965056141b5064ac0c141153b1a9 HG_PENDING=$TESTTMP/client HG_SOURCE=push-response * (glob)
  updating bookmark master
  changegroup hook: HG_NODE=ddd9491cc0b4965056141b5064ac0c141153b1a9 HG_SOURCE=push-response * (glob)
  incoming hook: HG_NODE=ddd9491cc0b4965056141b5064ac0c141153b1a9 HG_SOURCE=push-response * (glob)
  incoming hook: HG_NODE=b3e5033049f316725da840de07b96879ac325775 HG_SOURCE=push-response * (glob)
  incoming hook: HG_NODE=8eaad82b215848062618b309ae58e600a81f87a5 HG_SOURCE=push-response * (glob)
  $ cd ../server
  $ log
  o  on top of merge [public:8eaad82b2158] master
  |
  o    merge [public:b3e5033049f3]
  |\
  | o  branch middle [public:25f2e23fb053]
  | |
  | o  branch start [public:b9f6a18cb261]
  | |
  @ |  a => baz [public:ddd9491cc0b4]
  | |
  o |  b => quux [public:137b1b6ef903]
  | |
  o |  b => baz [public:7ba922f02e46]
  |/
  o  b => bar [public:fe66d1686ec2]
  |
  o  a => bar [public:add0c792bfce]
  |
  o  initial [public:2bb9d20e471c]
  
  $ hg strip -r b9f6a18cb261 -q
  preoutgoing hook: HG_SOURCE=strip
  outgoing hook: HG_NODE=b9f6a18cb2619a206f6d99dbcbdfbd75b2975506 HG_SOURCE=strip
  $ cd ../client
  $ hg strip -r b9f6a18cb261 -q
  preoutgoing hook: HG_SOURCE=strip
  outgoing hook: HG_NODE=b9f6a18cb2619a206f6d99dbcbdfbd75b2975506 HG_SOURCE=strip
  preoutgoing hook: HG_SOURCE=strip
  outgoing hook: HG_NODE=ddd9491cc0b4965056141b5064ac0c141153b1a9 HG_SOURCE=strip
  prechangegroup hook: HG_SOURCE=strip * (glob)
  pretxnchangegroup hook: HG_NODE=ddd9491cc0b4965056141b5064ac0c141153b1a9 HG_PENDING=$TESTTMP/client HG_SOURCE=strip * (glob)
  changegroup hook: HG_NODE=ddd9491cc0b4965056141b5064ac0c141153b1a9 HG_SOURCE=strip * (glob)
  incoming hook: HG_NODE=ddd9491cc0b4965056141b5064ac0c141153b1a9 HG_SOURCE=strip * (glob)
  $ hg book -d master
  $ hg -R ../server book -d master

With evolution enabled, should set obsolescence markers

  $ echo "[extensions]" >> $HGRCPATH
  $ echo "rebase =" >> $HGRCPATH
  $ echo "evolve =" >> $HGRCPATH

  $ cd ../client
  $ hg strip -qr ddd9491cc0b4
  preoutgoing hook: HG_SOURCE=strip
  outgoing hook: HG_NODE=ddd9491cc0b4965056141b5064ac0c141153b1a9 HG_SOURCE=strip
  $ hg up -q 137b1b6ef903
  $ echo 'foofoo' > b
  $ commit 'b => foofoo'
  $ echo 'foobar' > b
  $ commit 'b => foobar'
  $ log
  @  b => foobar [draft:a754b7172e58]
  |
  o  b => foofoo [draft:6e1d0b2f8180]
  |
  o  b => quux [public:137b1b6ef903]
  |
  o  b => baz [public:7ba922f02e46]
  |
  o  b => bar [public:fe66d1686ec2]
  |
  o  a => bar [public:add0c792bfce]
  |
  o  initial [public:2bb9d20e471c]
  
  $ hg push --to default
  pushing to ssh://user@dummy/server
  searching for changes
  preoutgoing hook: HG_SOURCE=push
  outgoing hook: HG_NODE=6e1d0b2f81801d1de2645ac4295781ff2ee08fb4 HG_SOURCE=push
  remote: prechangegroup hook: HG_BUNDLE2=1 HG_SOURCE=serve HG_TXNID=TXN:* HG_URL=remote:ssh:127.0.0.1 (glob)
  remote: pretxnchangegroup hook: HG_BUNDLE2=1 HG_PENDING=$TESTTMP/server HG_SOURCE=serve HG_TXNID=TXN:* HG_URL=remote:ssh:127.0.0.1 (glob)
  remote: preoutgoing hook: HG_SOURCE=rebase:reply
  remote: changegroup hook: HG_BUNDLE2=1 HG_NODE=5402bb2493c730b659b638d6a2f67f9d6dd57f84 HG_SOURCE=serve HG_TXNID=TXN:* HG_URL=remote:ssh:127.0.0.1 (glob)
  remote: incoming hook: HG_BUNDLE2=1 HG_NODE=5402bb2493c730b659b638d6a2f67f9d6dd57f84 HG_SOURCE=serve HG_TXNID=TXN:* HG_URL=remote:ssh:127.0.0.1 (glob)
  remote: incoming hook: HG_BUNDLE2=1 HG_NODE=b423e42e554804d21e786126e84a27565a786628 HG_SOURCE=serve HG_TXNID=TXN:* HG_URL=remote:ssh:127.0.0.1 (glob)
  prechangegroup hook: HG_SOURCE=push-response HG_TXNID=TXN:* HG_URL=ssh://user@dummy/server (glob)
  adding changesets
  remote: outgoing hook: HG_NODE=ddd9491cc0b4965056141b5064ac0c141153b1a9 HG_SOURCE=rebase:reply
  adding manifests
  adding file changes
  added 3 changesets with 1 changes to 2 files (+1 heads)
  pretxnchangegroup hook: HG_NODE=ddd9491cc0b4965056141b5064ac0c141153b1a9 HG_PENDING=$TESTTMP/client HG_SOURCE=push-response HG_TXNID=TXN:* HG_URL=ssh://user@dummy/server (glob)
  2 new obsolescence markers
  changegroup hook: HG_NODE=ddd9491cc0b4965056141b5064ac0c141153b1a9 HG_SOURCE=push-response HG_TXNID=TXN:* HG_URL=ssh://user@dummy/server (glob)
  incoming hook: HG_NODE=ddd9491cc0b4965056141b5064ac0c141153b1a9 HG_SOURCE=push-response HG_TXNID=TXN:* HG_URL=ssh://user@dummy/server (glob)
  incoming hook: HG_NODE=5402bb2493c730b659b638d6a2f67f9d6dd57f84 HG_SOURCE=push-response HG_TXNID=TXN:* HG_URL=ssh://user@dummy/server (glob)
  incoming hook: HG_NODE=b423e42e554804d21e786126e84a27565a786628 HG_SOURCE=push-response HG_TXNID=TXN:* HG_URL=ssh://user@dummy/server (glob)

  $ hg pull
  pulling from ssh://user@dummy/server
  searching for changes
  no changes found
  working directory parent is obsolete!

  $ hg evolve
  update:[9] b => foobar
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  working directory is now at b423e42e5548

  $ log
  @  b => foobar [public:b423e42e5548]
  |
  o  b => foofoo [public:5402bb2493c7]
  |
  o  a => baz [public:ddd9491cc0b4]
  |
  o  b => quux [public:137b1b6ef903]
  |
  o  b => baz [public:7ba922f02e46]
  |
  o  b => bar [public:fe66d1686ec2]
  |
  o  a => bar [public:add0c792bfce]
  |
  o  initial [public:2bb9d20e471c]
  

  $ cd ../server
  $ log
  o  b => foobar [public:b423e42e5548]
  |
  o  b => foofoo [public:5402bb2493c7]
  |
  @  a => baz [public:ddd9491cc0b4]
  |
  o  b => quux [public:137b1b6ef903]
  |
  o  b => baz [public:7ba922f02e46]
  |
  o  b => bar [public:fe66d1686ec2]
  |
  o  a => bar [public:add0c792bfce]
  |
  o  initial [public:2bb9d20e471c]
  
TODO: test pushing bookmarks
