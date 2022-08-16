#chg-compatible
#debugruntest-compatible

Setup

  $ hg init repo
  $ cd repo
  $ echo base > base
  $ hg add base
  $ hg commit -m "base"

Deliberately corrupt the dirstate.

  >>> with open('.hg/dirstate', 'wb') as f: f.write(b"\0" * 4096) and None

  $ hg debugrebuilddirstate
  warning: failed to inspect working copy parent
