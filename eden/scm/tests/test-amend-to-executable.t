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

# FIXME: This should succeed and preserve the executable bit. The panic is
# reported as a Python exception with a traceback; only check the key lines.
  $ sl amend --to "desc(target)" 2>&1 | grep -E "panicked at|^not implemented|Rust panic"
  thread 'main' (*) panicked at fbcode/eden/scm/lib/checkout/src/merge.rs:*: (glob)
  not implemented
  SystemError: Rust panic: not implemented
  $ sl status
  M target
