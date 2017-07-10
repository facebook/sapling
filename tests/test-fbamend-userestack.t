Set up test environment.
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > fbamend=$TESTDIR/../hgext3rd/fbamend
  > inhibit=$TESTDIR/../hgext3rd/inhibit.py
  > rebase=
  > strip=
  > tweakdefaults=$TESTDIR/../hgext3rd/tweakdefaults.py
  > [experimental]
  > evolution = createmarkers
  > [fbamend]
  > userestack=true
  > EOF
  $ mkcommit() {
  >   echo "$1" > "$1"
  >   hg add "$1"
  >   echo "add $1" > msg
  >   hg ci -l msg
  > }
  $ reset() {
  >   cd ..
  >   rm -rf userestack
  >   hg init userestack
  >   cd userestack
  > }
  $ showgraph() {
  >   hg log --graph -r '(::.)::' -T "{rev} {desc|firstline}" | sed \$d
  > }
  $ hg init userestack && cd userestack

Test that no preamend bookmark is created.
  $ mkcommit a
  $ mkcommit b
  $ mkcommit c
  $ mkcommit d
  $ hg up 1
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ hg amend -m "amended"
  warning: the changeset's children were left behind
  (use 'hg restack' to rebase them)
  $ hg book
  no bookmarks set

Test hg amend --fixup.
  $ showgraph
  @  4 amended
  |
  | o  3 add d
  | |
  | o  2 add c
  | |
  | x  1 add b
  |/
  o  0 add a

  $ hg amend --fixup
  rebasing 2:* "add c" (glob)
  rebasing 3:* "add d" (glob)
  $ showgraph
  o  6 add d
  |
  o  5 add c
  |
  @  4 amended
  |
  o  0 add a

Test that the operation field on the metadata is correctly set.
  $ hg debugobsolete
  [a-f0-9]* [a-f0-9]* 0 .* {'operation': 'amend', 'user': 'test'} (re)
  [a-f0-9]* [a-f0-9]* 0 .* {'operation': 'rebase', 'user': 'test'} (re)
  [a-f0-9]* [a-f0-9]* 0 .* {'operation': 'rebase', 'user': 'test'} (re)

Test hg amend --rebase
  $ hg amend -m "amended again" --rebase
  rebasing 5:* "add c" (glob)
  rebasing 6:* "add d" (glob)
  $ showgraph
  o  9 add d
  |
  o  8 add c
  |
  @  7 amended again
  |
  o  0 add a
