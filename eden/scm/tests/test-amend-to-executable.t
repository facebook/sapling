#inprocess-hg-incompatible
#require no-windows execbit

Test replaying an executable file added and then modified in descendant commits
  $ newclientrepo
  $ echo target > target
  $ sl ci -m "target" -Aq
  $ echo one > executable
  $ chmod +x executable
  $ sl ci -m "add executable" -Aq
  $ echo two >> executable
  $ sl ci -m "modify executable" -Aq
  $ echo amended >> target

# FIXME: This should succeed and preserve the executable bit.
  $ sl amend --to "desc(target)"
  
  thread 'main' (*) panicked at fbcode/eden/scm/lib/checkout/src/merge.rs:*: (glob)
  not implemented
  note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace
  [-11]
  $ sl status
  M target
