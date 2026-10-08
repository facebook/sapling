
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

Moving our fixed work onto D would lose the fix. A human is warned and the
rebase still goes ahead; an agent is stopped before anything is rewritten.

  $ cp -R $TESTTMP/repo1 $TESTTMP/human
  $ cp -R $TESTTMP/repo1 $TESTTMP/agent
  $ cp -R $TESTTMP/repo1 $TESTTMP/keep
  $ cp -R $TESTTMP/repo1 $TESTTMP/onto-obsolete
  $ cd $TESTTMP/human
  $ sl rebase -r $XFIX -d $D --config devel.print-metrics=rebase.obsolete
  warning: the destination is based on old versions of commits in your stack, so the rebased commits will lose their newer changes:
  - 6adbea717d1e -> d8d8344623c8 (rewrite)
  rebasing 1ab84340a70e "X"
  rebase.obsolete.human_warned: 1
  $ sl cat -r "children($D)" proto
  v1 buggy
  $ cd $TESTTMP/agent
  $ CODING_AGENT_METADATA=id=test_agent sl rebase -r $XFIX -d $D --config devel.print-metrics=rebase.obsolete
  abort: the destination is based on old versions of commits in your stack, so the rebased commits will lose their newer changes:
  - 6adbea717d1e -> d8d8344623c8 (rewrite)
  (check out the destination and run 'sl restack' to move it onto the newer versions first, or use '--keep' to copy the commits onto the old versions without hiding them)
  rebase.obsolete.agent_rejected: 1
  [255]
  $ sl log -r $XFIX -T '{node|short} on {p1node|short}\n'
  1ab84340a70e on d8d8344623c8

With --keep an agent takes the copy deliberately: the copy lands on the old
prototype, but the original keeps the fix.

  $ cd $TESTTMP/keep
  $ CODING_AGENT_METADATA=id=test_agent sl rebase -q -r $XFIX -d $D --keep
  $ sl cat -r $XFIX proto
  v3 fixed
  $ sl cat -r "children($D)" proto
  v1 buggy

Rebasing straight onto an obsolete commit is a different situation: the moved
commit gets an obsolete parent directly, which the existing guard already
reports. The new guard stays silent, so only one warning fires.

  $ cd $TESTTMP/onto-obsolete
  $ sl rebase -r $XFIX -d $P
  rebasing 1ab84340a70e "X"
  warning: creating a child of an old version of a commit will diverge your stack:
  - 6adbea717d1e -> d8d8344623c8 (rewrite)

A chained rebase can smuggle the same regression past a naive per-destination
check: moving X onto D and D onto Z in one command, where D and Z both sit on
the obsolete P. The guard follows each moved commit to its final destination,
so it still sees that X ends up on the old P.

  $ newclientrepo chained
  $ drawdag <<'EOS'
  > X   D   Z
  > |   |   |
  > Pf  P   P
  >     # amend: P -> Pf
  >     # P/proto = v1 buggy\n
  >     # Pf/proto = v3 fixed\n
  >     # X/x = x\n
  >     # D/d = d\n
  >     # Z/z = z\n
  > EOS
  $ CX=$(sl log -r 'desc(X)' -T '{node}')
  $ CD=$(sl log -r 'desc(D)' -T '{node}')
  $ CZ=$(sl log -r 'desc(Z)' -T '{node}')
  $ sl log -G -r 'all()'
  o  632f5a450cb1 X
  │
  │ o  250632287de4 Z
  │ │
  o │  2b256bdce3af Pf
    │
  o │  537a7300bfd7 D
  ├─╯
  x  6adbea717d1e P (obsolete)
  $ sl rebase -r $CX -d $CD -r $CD -d $CZ
  warning: the destination is based on old versions of commits in your stack, so the rebased commits will lose their newer changes:
  - 6adbea717d1e -> 2b256bdce3af (amend)
  rebasing 537a7300bfd7 "D"
  rebasing 632f5a450cb1 "X"

Restacking a stack onto the newer version of its base is the opposite move and
must not trip the guard: the destination is the fix, not an old version.

  $ newclientrepo restack
  $ drawdag <<'EOS'
  > C
  > |
  > P   # P/proto = v1 buggy\n
  >     # C/c = c\n
  > EOS
  $ sl goto -q $P
  $ echo 'v3 fixed' > proto
  $ sl amend -q --no-rebase
  $ sl rebase --restack
  rebasing 60f508579790 "C"
  $ sl log -G -r 'all()'
  o  41cef8aadc96 C
  │
  @  855b5d5c832e P
