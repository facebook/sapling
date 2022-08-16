#chg-compatible
#debugruntest-compatible

  $ enable remotefilelog

  $ newrepo
  $ echo remotefilelog >> .hg/requires

  $ echo a > a
  $ hg commit -qAm init

Make sure merge state is cleared when we have a clean tree.
  $ mkdir .hg/merge
  $ echo abcd > .hg/merge/state
  $ hg debugmergestate
  * version 1 records
  local: abcd
  $ hg up -qC . --config experimental.nativecheckout=True
  $ hg debugmergestate
  no merge state found
