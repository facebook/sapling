#require fsmonitor icasefs

(Run this test using HGFSMONITOR_TESTS=1)

Updating across a rename

  $ newrepo

  $ echo >> a
  $ hg commit -Aqm "add a"
  $ hg mv a A
  $ hg commit -qm "move a to A"
  $ hg up -q '.^'
  $ hg status
