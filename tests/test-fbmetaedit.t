based on test-evolve.t from mutable-history extension
  $ . $TESTDIR/require-ext.sh evolve

  $ REPOROOT=`dirname $TESTDIR`
  $ cat >> $HGRCPATH <<EOF
  > [defaults]
  > amend=-d "0 0"
  > fold=-d "0 0"
  > metaedit=-d "0 0"
  > [web]
  > push_ssl = false
  > allow_push = *
  > [phases]
  > publish = False
  > [alias]
  > qlog = log --template='{rev} - {node|short} {desc} ({phase})\n'
  > [diff]
  > git = 1
  > unified = 0
  > [extensions]
  > hgext.graphlog=
  > fbmetaedit=$REPOROOT/hgext3rd/fbmetaedit.py
  > evolve=
  > EOF
  $ mkcommit() {
  >    echo "$1" > "$1"
  >    hg add "$1"
  >    hg ci -m "add $1"
  > }

  $ mkstack() {
  >    # Creates a stack of commit based on $1 with messages from $2, $3 ..
  >    hg update $1 -C
  >    shift
  >    mkcommits $*
  > }

  $ glog() {
  >   hg glog --template '{rev}:{node|short}@{branch}({phase}) {desc|firstline}\n' "$@"
  > }

  $ shaof() {
  >   hg log -T {node} -r "first(desc($1))"
  > }

  $ mkcommits() {
  >   for i in $@; do mkcommit $i ; done
  > }

various init

  $ hg init local
  $ cd local
  $ mkcommit a
  $ mkcommit b
  $ cat >> .hg/hgrc << EOF
  > [phases]
  > publish = True
  > EOF
  $ hg pull -q . # make 1 public
  $ rm .hg/hgrc
  $ mkcommit c
  $ mkcommit d
  $ hg up 1
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ mkcommit e -q
  created new head
  $ mkcommit f
  $ mkcommit g
  $ hg qlog
  6 - c802b7e6fec9 add g (draft)
  5 - e44648563c73 add f (draft)
  4 - fbb94e3a0ecf add e (draft)
  3 - 47d2a3944de8 add d (draft)
  2 - 4538525df7e2 add c (draft)
  1 - 7c3bad9141dc add b (public)
  0 - 1f0dee641bb7 add a (public)


hg metaedit
-----------

  $ glog
  @  6:c802b7e6fec9@default(draft) add g
  |
  o  5:e44648563c73@default(draft) add f
  |
  o  4:fbb94e3a0ecf@default(draft) add e
  |
  | o  3:47d2a3944de8@default(draft) add d
  | |
  | o  2:4538525df7e2@default(draft) add c
  |/
  o  1:7c3bad9141dc@default(public) add b
  |
  o  0:1f0dee641bb7@default(public) add a
  
  $ hg metaedit -r 0 -m "xx"
  abort: cannot edit commit information for public revisions
  [255]
  $ hg metaedit --fold
  abort: revisions must be specified with --fold
  [255]
  $ hg metaedit -r 0 --fold
  abort: cannot fold public revisions
  [255]
  $ hg metaedit '2 + 5' --fold
  abort: cannot fold non-linear revisions (multiple roots given)
  [255]

check that metaedit respects allowunstable
  $ hg metaedit '4::5' --fold --config 'experimental.evolution=createmarkers, allnewcommands'
  abort: cannot fold chain not ending with a head or with branching
  (new unstable changesets are not allowed)
  [255]
  $ hg metaedit --user foobar
  $ hg log --template '{rev}: {author}\n' -r '2:' --hidden
  2: test
  3: test
  4: test
  5: test
  6: test
  7: foobar
  $ hg log --template '{rev}: {author}\n' -r .
  7: foobar

  $ cat >> $TESTTMP/modifymsg.sh <<EOF
  > #!/bin/bash
  > sed -e 's/add f/add f nicely/g' \$1 > $TESTTMP/newmsg
  > mv $TESTTMP/newmsg \$1
  > EOF
  $ chmod a+x $TESTTMP/modifymsg.sh
  $ HGEDITOR="$TESTTMP/modifymsg.sh" hg metaedit -d '1 1' '.^::.'

  $ HGEDITOR=cat hg metaedit '.^::.' --fold
  HG: This is a fold of 2 changesets.
  HG: Commit message of changeset 8.
  
  add f nicely
  
  HG: Commit message of changeset 9.
  
  add g
  
  
  
  HG: Enter commit message.  Lines beginning with 'HG:' are removed.
  HG: Leave message empty to abort commit.
  HG: --
  HG: user: test
  HG: branch 'default'
  HG: added f
  HG: added g
  2 changesets folded

  $ glog -r .
  @  10:09b4ac0f24fc@default(draft) add f nicely
  |
  ~

no new commit is created here because the date is the same
  $ HGEDITOR=cat hg metaedit
  HG: Commit message of changeset 09b4ac0f24fc
  add f nicely
  
  
  add g
  
  
  HG: Enter commit message.  Lines beginning with 'HG:' are removed.
  HG: Leave message empty to abort commit.
  HG: --
  HG: user: test
  HG: branch 'default'
  HG: added f
  HG: added g
  nothing changed

  $ glog -r '.^::.'
  @  10:09b4ac0f24fc@default(draft) add f nicely
  |
  o  4:fbb94e3a0ecf@default(draft) add e
  |
  ~

  $ hg up ".^"
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ hg metaedit --user foobar2 .
  $ hg log --template '{rev}: {author}\n' -r '3:' --hidden
  3: test
  4: test
  5: test
  6: test
  7: foobar
  8: test
  9: foobar
  10: test
  11: foobar2

metaedit a commit in the middle of the stack:
  $ glog -r '(.^)::'
  @  11:682abdd0f684@default(draft) add e
  |
  | o  10:09b4ac0f24fc@default(draft) add f nicely
  | |
  | x  4:fbb94e3a0ecf@default(draft) add e
  |/
  | o  3:47d2a3944de8@default(draft) add d
  | |
  | o  2:4538525df7e2@default(draft) add c
  |/
  o  1:7c3bad9141dc@default(public) add b
  |
  ~
  $ hg metaedit -m "add uu (with metaedit)" --config 'experimental.evolution=createmarkers, allnewcommands'
  $ glog -r '(.^)::'
  @  12:f6c3c1613ce9@default(draft) add uu (with metaedit)
  |
  | o  10:09b4ac0f24fc@default(draft) add f nicely
  | |
  | x  4:fbb94e3a0ecf@default(draft) add e
  |/
  | o  3:47d2a3944de8@default(draft) add d
  | |
  | o  2:4538525df7e2@default(draft) add c
  |/
  o  1:7c3bad9141dc@default(public) add b
  |
  ~
  $ hg metaedit -m "add uu (with metaedit)"
  nothing changed
  $ glog -r '(.^)::'
  @  12:f6c3c1613ce9@default(draft) add uu (with metaedit)
  |
  | o  10:09b4ac0f24fc@default(draft) add f nicely
  | |
  | x  4:fbb94e3a0ecf@default(draft) add e
  |/
  | o  3:47d2a3944de8@default(draft) add d
  | |
  | o  2:4538525df7e2@default(draft) add c
  |/
  o  1:7c3bad9141dc@default(public) add b
  |
  ~
