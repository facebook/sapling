#require fsmonitor

  $ newrepo
  $ enable fsmonitor
  $ touch a b c d e f
  $ hg ci -m init -A a b c d e f

The fsmonitor extension should bump clock even if there are nothing changed for
treestate, but too many results returned by watchman.

  $ hg status

 (Run status again after 1 second to make sure mtime < fsnow)
  $ sleep 1
  $ hg status

In this case, treestate has 0 files marked NEED_CHECK, but fsmonitor returns
many files:

  $ touch a b c d e f

  $ hg debugstatus
  len(dirstate) = 6
  len(nonnormal) = 0
  len(filtered nonnormal) = 0
  clock = * (glob)

  $ rm -rf .hg/blackbox*
  $ hg status

  $ hg blackbox --no-timestamp --no-sid --pattern '{"fsmonitor":"_"}'
  [fsmonitor] clock: "c:*" -> "c:*"; need check: * + ["a", "b", "c", "d", "e"] and 1 entries (glob)

The watchman clock remains unchanged. Watchman still returns 6 files, which
means the "status" command could still be slow.

  $ rm -rf .hg/blackbox*
  $ hg status
  $ hg blackbox --no-timestamp --no-sid --pattern '{"fsmonitor":"_"}'
  [fsmonitor] clock: "c:*" -> "c:*"; need check: * + ["a", "b", "c", "d", "e"] and 1 entries (glob)

With watchman-changed-file-threshold set, clock is bumped and watchman can
return an empty list:

  $ hg status
  $ setconfig fsmonitor.watchman-changed-file-threshold=5

  $ rm -rf .hg/blackbox*
  $ hg status
  $ hg blackbox --no-timestamp --no-sid --pattern '{"fsmonitor":"_"}'
  [fsmonitor] clock: "c:*" -> "c:*"; need check: * + ["a", "b", "c", "d", "e"] and 1 entries (glob)

  $ sleep 1

  $ rm -rf .hg/blackbox*
  $ hg status
  $ hg blackbox --no-timestamp --no-sid --pattern '{"fsmonitor":"_"}'
  [fsmonitor] clock: "c:*" -> "c:*"; need check: [] + [] (glob)
