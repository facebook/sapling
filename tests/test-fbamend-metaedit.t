  $ cat >> $HGRCPATH <<EOF
  > [defaults]
  > fold=--date "0 0"
  > metaedit=--date "0 0"
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
  > rebase=
  > fbamend=$TESTDIR/../hgext3rd/fbamend
  > [experimental]
  > evolution=createmarkers, allowunstable
  > EOF
  $ mkcommit() {
  >    echo "$1" > "$1"
  >    hg add "$1"
  >    hg ci -m "$1"
  > }

  $ mkstack() {
  >    # Creates a stack of commit based on $1 with messages from $2, $3 ..
  >    hg update $1 -C
  >    shift
  >    mkcommits $*
  > }

  $ glog() {
  >   hg log -G -T '{rev}:{node|short}@{branch}({phase}) {desc|firstline}\n' "$@"
  > }

  $ shaof() {
  >   hg log -T {node} -r "first(desc($1))"
  > }

  $ mkcommits() {
  >   for i in $@; do mkcommit $i ; done
  > }

##########################
importing Parren test
##########################

  $ cat << EOF >> $HGRCPATH
  > [ui]
  > logtemplate = "{rev}\t{bookmarks}: {desc|firstline} - {author|user}\n"
  > EOF

HG METAEDIT
===============================

Setup the Base Repo
-------------------

We start with a plain base repo::

  $ hg init $TESTTMP/metaedit; cd $TESTTMP/metaedit
  $ mkcommit "ROOT"
  $ hg phase --public "desc(ROOT)"
  $ mkcommit "A"
  $ mkcommit "B"
  $ hg up "desc(A)"
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ mkcommit "C"
  created new head
  $ mkcommit "D"
  $ echo "D'" > D
  $ hg commit --amend -m "D2"
  $ hg up "desc(C)"
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ mkcommit "E"
  created new head
  $ mkcommit "F"

Test
----

  $ hg log -G
  @  8	: F - test
  |
  o  7	: E - test
  |
  | o  6	: D2 - test
  |/
  o  3	: C - test
  |
  | o  2	: B - test
  |/
  o  1	: A - test
  |
  o  0	: ROOT - test
  
  $ hg update --clean .
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg metaedit -r 0
  abort: cannot edit commit information for public revisions
  [255]
  $ hg metaedit --fold
  abort: revisions must be specified with --fold
  [255]
  $ hg metaedit -r 0 --fold
  abort: cannot fold public revisions
  [255]
  $ hg metaedit 'desc(C) + desc(F)' --fold
  abort: cannot fold non-linear revisions (multiple roots given)
  [255]
  $ hg metaedit "desc(C)::desc(D2) + desc(E)" --fold
  abort: cannot fold non-linear revisions (multiple heads given)
  [255]

  $ hg metaedit --user foobar
  $ hg log --template '{rev}: {author}\n' -r 'desc(F):' --hidden
  5: test
  6: test
  7: test
  8: test
  9: foobar
  $ hg log --template '{rev}: {author}\n' -r .
  9: foobar

  $ HGEDITOR=cat hg metaedit '.^::.' --fold
  HG: This is a fold of 2 changesets.
  HG: Commit message of changeset 7.
  
  E
  
  HG: Commit message of changeset 9.
  
  F
  
  
  
  HG: Enter commit message.  Lines beginning with 'HG:' are removed.
  HG: Leave message empty to abort commit.
  HG: --
  HG: user: test
  HG: branch 'default'
  HG: added E
  HG: added F
  2 changesets folded
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ glog -r .
  @  10:a08d35fd7d9d@default(draft) E
  |
  ~

no new commit is created here because the date is the same
  $ HGEDITOR=cat hg metaedit
  HG: Commit message of changeset a08d35fd7d9d
  E
  
  
  F
  
  
  HG: Enter commit message.  Lines beginning with 'HG:' are removed.
  HG: Leave message empty to abort commit.
  HG: --
  HG: user: test
  HG: branch 'default'
  HG: added E
  HG: added F
  nothing changed
  [1]

  $ glog -r '.^::.'
  @  10:a08d35fd7d9d@default(draft) E
  |
  o  3:3260958f1169@default(draft) C
  |
  ~

TODO: don't create a new commit in this case, we should take the date of the
old commit (we add a default date with a value to show that metaedit is taking
the current date to generate the hash, this way we still have a stable hash
but highlight the bug)
  $ hg metaedit --config defaults.metaedit= --config devel.default-date="42 0"
  $ hg log -r '.^::.' --template '{rev}: {desc|firstline}\n'
  3: C
  11: E

  $ hg up '.^'
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ hg metaedit --user foobar2 tip
  $ hg log --template '{rev}: {author}\n' -r "user(foobar):" --hidden
  9: foobar
  10: test
  11: test
  12: foobar2
  $ hg diff -r "10" -r "11" --hidden

'fold' one commit
  $ hg metaedit "desc(D2)" --fold --user foobar3
  1 changesets folded
  $ hg log -r "tip" --template '{rev}: {author}\n'
  13: foobar3

metaedit a commit in the middle of the stack:

  $ cd $TESTTMP
  $ hg init metaedit2
  $ cd metaedit2
  $ hg debugbuilddag '+5'
  $ hg update tip
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ glog -r 'all()'
  @  4:bebd167eb94d@default(draft) r4
  |
  o  3:2dc09a01254d@default(draft) r3
  |
  o  2:01241442b3c2@default(draft) r2
  |
  o  1:66f7d451a68b@default(draft) r1
  |
  o  0:1ea73414a91b@default(draft) r0
  
  $ hg metaedit -m "metaedit" -r 2
  $ glog -r 'all()'
  @  7:8c1f124031e7@default(draft) r4
  |
  o  6:af1447d6a312@default(draft) r3
  |
  o  5:1aed0f31debd@default(draft) metaedit
  |
  o  1:66f7d451a68b@default(draft) r1
  |
  o  0:1ea73414a91b@default(draft) r0
  
  $ hg metaedit -m "metaedit" -r 1aed0f31debd
  nothing changed
  [1]

metaedit more than one commit at once without --fold
  $ hg metaedit -m "metaedit" -r 5::
  $ glog -r 'all()'
  @  9:972f190d63f3@default(draft) metaedit
  |
  o  8:a1c80e4c2636@default(draft) metaedit
  |
  o  5:1aed0f31debd@default(draft) metaedit
  |
  o  1:66f7d451a68b@default(draft) r1
  |
  o  0:1ea73414a91b@default(draft) r0
  
