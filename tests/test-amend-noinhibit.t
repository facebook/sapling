Set up test environment.
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > amend=
  > rebase=
  > [experimental]
  > evolution = createmarkers
  > EOF
  $ hg init repo && cd repo

Perform restack without inhibit extension.
  $ hg debugbuilddag -m +3
  $ hg update 1
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg amend -m "Amended" --no-rebase
  hint[amend-restack]: descendants of c05912b45f80 are left behind - use 'hg restack' to rebase them
  hint[hint-ack]: use 'hg hint --ack amend-restack' to silence these hints
  $ hg rebase --restack
  rebasing 2:* "r2" (glob)
