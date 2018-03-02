Setup

  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > treedirstate=
  > [treedirstate]
  > useinnewrepos=True
  > EOF

  $ hg init repo
  $ cd repo
  $ echo base > base
  $ hg add base
  $ hg commit -m "base"

Deliberately corrupt the dirstate.

  $ dd if=/dev/zero bs=4096 count=1 of=.hg/dirstate 2> /dev/null
  $ hg debugrebuilddirstate
