#debugruntest-compatible
#require fsmonitor no-eden

  $ eagerepo

  $ hg init repo
  $ cd repo
  $ hg st
  $ hg debugtreestate list

Sanity that dirstate is normally updated by status:
  $ touch foo
  $ hg st
  ? foo
  $ hg debugtreestate list
  foo: * NEED_CHECK  (glob)

Mutate dirstate in a transaction - should not be visible outside transaction:
  $ hg dbsh <<EOF
  > with repo.wlock(), repo.lock(), repo.transaction("foo"):
  >   repo.dirstate.add("foo")
  >   print("pending adds:", repo.status().added)
  >   import subprocess
  >   print("external adds:", subprocess.run(["hg", "st", "-an"], check=True, capture_output=True).stdout.strip().decode() or "<none>")
  > EOF
  pending adds: ['foo']
  external adds: <none>

Now it should be visible
  $ hg st
  A foo

Make sure things are okay if Rust flushes the treestate and then Python makes a change:
  $ touch bar
  $ hg dbsh <<EOF
  > with repo.wlock(), repo.lock(), repo.transaction("foo"):
  >   # This will trigger treestate flush adding "bar".
  >   repo.status()
  >   import subprocess
  >   print("external unknown:", subprocess.run(["hg", "st", "-un"], check=True, capture_output=True).stdout.strip().decode() or "<none>")
  >   # Make another dirstate change - this needs to get flushed properly.
  >   repo.dirstate.add("bar")
  > EOF
  external unknown: bar

  $ hg st
  A bar
  A foo
