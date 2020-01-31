#chg-compatible

Test logging of "M" entries

  $ newrepo
  $ setconfig experimental.samplestatus=2 blackbox.track=status

  $ echo 1 > a
  $ hg commit -A a -m a

  $ echo 2 >> a
  $ hg status
  M a
  $ hg blackbox --no-timestamp --no-sid --pattern '{"legacy_log":{"service":"status"}}'
  [legacy][status] M a: size changed (2 -> 4)

  $ sleep 1
  $ rm -rf a .hg/blackbox*
  $ touch a
  $ hg status
  M a
  $ hg blackbox --no-timestamp --no-sid --pattern '{"legacy_log":{"service":"status"}}'
  [legacy][status] M a: size changed (2 -> 0), os.stat size = 0

  $ sleep 1
  $ rm -rf .hg/blackbox*
  $ echo 1 > a
  $ hg status
  $ hg blackbox --no-timestamp --no-sid --pattern '{"legacy_log":{"service":"status"}}'
  [legacy][status] L a: mtime changed (* -> *) (glob)
  [legacy][status] C a: checked in filesystem
