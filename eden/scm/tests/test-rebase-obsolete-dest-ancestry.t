
#require no-eden

Re-organising a stack can move commits onto a destination that is a perfectly
live commit, but whose own ancestry still contains obsolete *predecessors* of
the commits being moved. The moved commits are then reparented backwards along
the mutation graph and silently lose whatever the newer versions fixed.

The existing obsolete-commit guards do not catch this: the destination is not
obsolete, and the moved commit's new parent is not obsolete either -- the stale
versions are buried deeper in the destination's ancestry. So today the rebase
succeeds with no warning at all. This test pins that down.

  $ enable rebase amend
  $ setconfig rebase.experimental.inmemory=true
  $ setconfig 'hint.ack=amend-restack'
  $ setconfig 'ui.logtemplate={node|short} {desc}{if(obsolete, " (obsolete)")}\n'

P is a prototype commit whose "proto" file is still buggy. X is our work built
on it. D is a separate, live commit that a teammate built on the same still-buggy
P (think of a diff that was rewritten remotely and left sitting on the old
prototype).

  $ newclientrepo
  $ drawdag <<'EOS'
  > X   D
  > |   |
  > P   P
  >     # P/proto = v1 buggy\n
  >     # X/x = x\n
  >     # D/d = d\n
  > EOS
  $ sl log -G -r 'all()'
  o  2d9019185af5 X
  │
  │ o  537a7300bfd7 D
  ├─╯
  o  6adbea717d1e P

We fix P with two amends; only the last has the fix. Autorestack is off, so our
stack is left behind, and we move just our line onto the fix. D is untouched and
still sits on the original P.

  $ sl goto -q $P
  $ echo 'v2 partial fix' > proto
  $ sl amend -q --no-rebase
  $ echo 'v3 fixed' > proto
  $ sl amend -q --no-rebase
  $ PFIX=$(sl log -r . -T '{node}')
  $ sl rebase -q -s $X -d $PFIX
  $ XFIX=$(sl log -r "children($PFIX)" -T '{node}')
  $ sl log -G -r 'all()'
  o  1ab84340a70e X
  │
  @  d8d8344623c8 P
  
  o  537a7300bfd7 D
  │
  x  6adbea717d1e P (obsolete)
  $ sl cat -r $XFIX proto
  v3 fixed

D is still live and still based on the original, buggy P.

  $ sl log -r "$D + $D^"
  537a7300bfd7 D
  6adbea717d1e P (obsolete)

A check for this intersects the destination's ancestry with the predecessors of
the moved commit's draft base. The original P is two rewrites behind the fixed
one, so it is only found with the full transitive closure, not depth 1.

  $ sl log -r "::$D & (allpredecessors(draft() & ::parents($XFIX)) - (draft() & ::parents($XFIX)))"
  6adbea717d1e P (obsolete)
  $ sl log -r "::$D & (allpredecessors(draft() & ::parents($XFIX), 1) - (draft() & ::parents($XFIX)))"

Now move our fixed work onto D. D is live and so is X's new parent, so none of
the existing guards fire and the rebase succeeds silently.

  $ sl rebase -r $XFIX -d $D
  rebasing 1ab84340a70e "X"
  $ sl log -G -r 'all()'
  o  b66c762a6b45 X
  │
  │ @  d8d8344623c8 P
  │
  o  537a7300bfd7 D
  │
  x  6adbea717d1e P (obsolete)
  $ XNEW=$(sl log -r "children($D)" -T '{node}')

X is now based on the original P again, so the fix is gone.

  $ sl cat -r $XNEW proto
  v1 buggy
