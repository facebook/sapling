#chg-compatible

  $ enable amend obsstore

"import" can revive a commit

  $ newrepo

  $ drawdag <<'EOS'
  > B
  > |
  > A
  > EOS

  $ hg export $B > $TESTTMP/b.patch

  $ hg hide -q $B
  $ hg log -r 'obsolete()' --hidden -T '{desc}'
  B (no-eol)

  $ hg up -q $A
  $ hg import -q --exact $TESTTMP/b.patch
  $ hg log -r 'obsolete()' --hidden -T '{desc}'

"commit" can revive a commit

  $ newrepo

  $ hg commit --config ui.allowemptycommit=1 -m A

  $ hg hide -q .
  $ hg log -r 'obsolete()' --hidden -T '{desc}'
  A (no-eol)

  $ hg commit --config ui.allowemptycommit=1 -m A
  $ hg log -r 'obsolete()' --hidden -T '{desc}'

