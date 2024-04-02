#debugruntest-compatible

#require no-eden


  $ enable amend
  $ configure modern

  $ setconfig 'alias.log=log -T "{desc}\n"'

Linear stack:

  $ newclientrepo
  $ drawdag << 'EOS'
  > K
  > :
  > A
  > python:
  > commit("D", remotename="remote/master")
  > goto("G")
  > EOS

  $ sl log -r 'next()'
  H
  $ sl log -r 'next(2)'
  I
  $ sl log -r 'next(10)'
  reached head changeset
  K

  $ sl log -r 'previous()'
  F
  $ sl log -r 'previous(2)'
  E
  $ sl log -r 'previous(10)'
  reached root changeset
  A

  $ sl log -r 'top()'
  K
  $ sl log -r 'bottom()'
  E

With revset aliases:

  $ sl config -q --local 'revsetalias.prev=previous' 'revsetalias.previous=previous()'

  $ sl log -r prev
  F
  $ sl log -r 'prev(2)'
  E
  $ sl log -r 'previous'
  F
  $ sl log -r 'previous()'
  F
  $ sl log -r 'previous(2)'
  E

Multiple choices:

  $ newclientrepo
  $ drawdag << 'EOS'
  > L   P
  > :   :
  > I   M
  >  \ /
  >   X
  >  / \
  > D   H
  > :   :
  > A   E
  > EOS

  $ sl go -q $X

Next:

  $ sl log -r 'next()' --config ui.interactive=1
  changeset 190ba9608152 has multiple children, namely:
  (1) [9ad522] I
  (2) [6a87f2] M
  which changeset to move to [1-2/(c)ancel]?  abort: response expected
  [255]

  $ sl log -r 'next()'
  changeset 190ba9608152 has multiple children, namely:
  [9ad522] I
  [6a87f2] M
  abort: ambiguous next changeset
  (use the --newest or --towards flags to specify which child to pick)
  [255]

Previous:

  $ sl log -r 'previous()' --config ui.interactive=1
  changeset 190ba9608152 has multiple parents, namely:
  (1) [f58535] D
  (2) [a9ca93] H
  which changeset to move to [1-2/(c)ancel]?  abort: response expected
  [255]

  $ sl log -r 'previous()'
  changeset 190ba9608152 has multiple parents, namely:
  [f58535] D
  [a9ca93] H
  abort: ambiguous previous changeset
  (use the --newest flag to always pick the newest parent at each step)
  [255]

Top:

  $ sl log -r 'top()' --config ui.interactive=1
  current stack has multiple heads, namely:
  (1) [c6dcbf] L
  (2) [72a71a] P
  which changeset to move to [1-2/(c)ancel]?  abort: response expected
  [255]

  $ sl log -r 'top()'
  current stack has multiple heads, namely:
  [c6dcbf] L
  [72a71a] P
  abort: ambiguous next changeset
  (use the --newest flag to always pick the newest child at each step)
  [255]

Bottom:

  $ sl log -r 'bottom()' --config ui.interactive=1
  current stack has multiple bottom changesets, namely:
  (1) [426bad] A
  (2) [e8e0a8] E
  which changeset to move to [1-2/(c)ancel]?  abort: response expected
  [255]

  $ sl log -r 'bottom()'
  current stack has multiple bottom changesets, namely:
  [426bad] A
  [e8e0a8] E
  abort: ambiguous bottom changeset
  [255]

