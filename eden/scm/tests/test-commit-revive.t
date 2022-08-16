#chg-compatible
#debugruntest-compatible

  $ configure modern

"import" can revive a commit

  $ newrepo

  $ drawdag <<'EOS'
  > B
  > |
  > A
  > EOS

  $ hg export $B > $TESTTMP/b.patch

  $ hg hide -q $B
  $ hg log -r 'all()' -T '{desc}\n'
  A

  $ hg up -q $A
  $ hg import -q --exact $TESTTMP/b.patch
  $ hg log -r 'all()' -T '{desc}\n'
  A
  B

"commit" can revive a commit

  $ newrepo

  $ hg commit --config ui.allowemptycommit=1 -m A

  $ hg hide -q .
  $ hg log -r 'all()' -T '{desc}\n'

  $ hg commit --config ui.allowemptycommit=1 -m A
  $ hg log -r 'all()' -T '{desc}\n'
  A

