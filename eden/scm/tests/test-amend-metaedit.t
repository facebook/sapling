#chg-compatible
#debugruntest-compatible

  $ setconfig devel.segmented-changelog-rev-compat=true
  $ configure mutation-norecord
  $ enable amend rebase
  $ export HGIDENTITY=sl
  $ readconfig <<EOF
  > [defaults]
  > fold=--date "0 0"
  > metaedit=--date "0 0"
  > [web]
  > push_ssl = false
  > allow_push = *
  > [phases]
  > publish = False
  > [alias]
  > qlog = log --template='{node|short} {desc} ({phase})\n'
  > [diff]
  > git = 1
  > unified = 0
  > EOF
  $ mkcommit() {
  >    echo "$1" > "$1"
  >    hg add "$1"
  >    hg ci -m "$1"
  > }

  $ mkstack() {
  >    # Creates a stack of commit based on $1 with messages from $2, $3 ..
  >    hg goto $1 -C
  >    shift
  >    mkcommits $*
  > }

  $ glog() {
  >   hg log -G -T '{node|short}@{branch}({phase}) {desc|firstline}\n' "$@"
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
  > logtemplate = "{bookmarks}: {desc|firstline} - {author|user}\n"
  > EOF

HG METAEDIT
===============================

Setup the Base Repo
-------------------

We start with a plain base repo::

  $ hg init $TESTTMP/metaedit; cd $TESTTMP/metaedit
  $ mkcommit "ROOT"
  $ hg debugmakepublic "desc(ROOT)"
  $ mkcommit "A"
  $ mkcommit "B"
  $ hg up "desc(A)"
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ mkcommit "C"
  $ mkcommit "D"
  $ echo "D'" > D
  $ hg commit --amend -m "D2"
  $ hg up "desc(C)"
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ mkcommit "E"
  $ mkcommit "F"

Test
----

  $ hg log -G
  @  : F - test
  │
  o  : E - test
  │
  │ o  : D2 - test
  ├─╯
  o  : C - test
  │
  │ o  : B - test
  ├─╯
  o  : A - test
  │
  o  : ROOT - test
  

  $ hg goto --clean .
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg metaedit -r 'desc(ROOT)'
  abort: cannot edit commit information for public revisions
  [255]
  $ hg metaedit --fold
  abort: revisions must be specified with --fold
  [255]
  $ hg metaedit -r 'desc(ROOT)' --fold
  abort: cannot fold public revisions
  [255]
  $ hg metaedit 'desc(C) + desc(F)' --fold
  abort: cannot fold non-linear revisions (multiple roots given)
  [255]
  $ hg metaedit "desc(C)::desc(D2) + desc(E)" --fold
  abort: cannot fold non-linear revisions (multiple heads given)
  [255]

  $ hg metaedit --user foobar  -T "{nodechanges|json}\n"
  {"587528abfffe33d49f94f9d6223dbbd58d6197c6": ["212b2a2b87cdbae992f001e9baba64db389fbce7"]}
  $ hg log --template '{author}\n' -r 'desc(F):' --hidden
  test
  foobar
  $ hg log --template '{author}\n' -r .
  foobar

  $ HGEDITOR=cat hg metaedit '.^::.' --fold
  SL: This is a fold of 2 changesets.
  SL: Commit message of c2bd843aa246.
  
  E
  
  SL: Commit message of 212b2a2b87cd.
  
  F
  
  
  
  SL: Enter commit message.  Lines beginning with 'SL:' are removed.
  SL: Leave message empty to abort commit.
  SL: --
  SL: user: test
  SL: branch 'default'
  SL: added E
  SL: added F
  2 changesets folded
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved







  $ glog -r .
  @  a08d35fd7d9d@default(draft) E
  │
  ~

no new commit is created here because the date is the same
  $ HGEDITOR=cat hg metaedit
  SL: Commit message of changeset a08d35fd7d9d
  E
  
  
  F
  
  
  SL: Enter commit message.  Lines beginning with 'SL:' are removed.
  SL: Leave message empty to abort commit.
  SL: --
  SL: user: test
  SL: branch 'default'
  SL: added E
  SL: added F
  nothing changed
  [1]





  $ glog -r '.^::.'
  @  a08d35fd7d9d@default(draft) E
  │
  o  3260958f1169@default(draft) C
  │
  ~

TODO: don't create a new commit in this case, we should take the date of the
old commit (we add a default date with a value to show that metaedit is taking
the current date to generate the hash, this way we still have a stable hash
but highlight the bug)
  $ hg metaedit --config defaults.metaedit= --config devel.default-date="42 0"
  $ hg log -r '.^::.' --template '{desc|firstline}\n'
  C
  E

  $ hg up '.^'
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ hg metaedit --user foobar2 tip
  $ hg log --template '{author}\n' -r "user(foobar):" --hidden
  foobar
  test
  test
  foobar2
  $ hg diff -r "10" -r "11" --hidden

'fold' one commit
  $ hg metaedit "desc(D2)" --fold --user foobar3 --hidden
  1 changesets folded
  $ hg log -r "tip" --template '{author}\n'
  foobar3

metaedit a commit in the middle of the stack:

  $ cd $TESTTMP
  $ hg init metaedit2
  $ cd metaedit2
  $ hg debugbuilddag '+5'
  $ hg goto tip
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ glog -r 'all()'
  @  bebd167eb94d@default(draft) r4
  │
  o  2dc09a01254d@default(draft) r3
  │
  o  01241442b3c2@default(draft) r2
  │
  o  66f7d451a68b@default(draft) r1
  │
  o  1ea73414a91b@default(draft) r0
  

  $ hg metaedit -m "metaedit" -r 'desc(r2)'
  $ glog -r 'all()'
  @  8c1f124031e7@default(draft) r4
  │
  o  af1447d6a312@default(draft) r3
  │
  o  1aed0f31debd@default(draft) metaedit
  │
  o  66f7d451a68b@default(draft) r1
  │
  o  1ea73414a91b@default(draft) r0
  

  $ hg metaedit -m "metaedit" -r 1aed0f31debd
  nothing changed
  [1]

metaedit more than one commit at once without --fold
  $ hg metaedit -m "metaedit" -r 'desc(metaedit)'::
  $ glog -r 'all()'
  @  972f190d63f3@default(draft) metaedit
  │
  o  a1c80e4c2636@default(draft) metaedit
  │
  o  1aed0f31debd@default(draft) metaedit
  │
  o  66f7d451a68b@default(draft) r1
  │
  o  1ea73414a91b@default(draft) r0
  

make the top commit non-empty
  $ echo xx > xx
  $ hg add xx
  $ hg amend
  $ glog -r 'all()'
  @  90ef4d40a825@default(draft) metaedit
  │
  o  a1c80e4c2636@default(draft) metaedit
  │
  o  1aed0f31debd@default(draft) metaedit
  │
  o  66f7d451a68b@default(draft) r1
  │
  o  1ea73414a91b@default(draft) r0
  

test histedit compat

  $ echo '[extensions]' >> $HGRCPATH
  $ echo "fbhistedit=" >> $HGRCPATH
  $ echo "histedit=" >> $HGRCPATH

  $ hg export -r .
  # SL changeset patch
  # User debugbuilddag
  # Date 0 0
  #      Thu Jan 01 00:00:00 1970 +0000
  # Node ID 90ef4d40a82572a220d8329eefb1d96a1fac3597
  # Parent  a1c80e4c26360f913ae3bdc5c70d6f29d465bfb0
  metaedit
  
  diff --git a/xx b/xx
  new file mode 100644
  --- /dev/null
  +++ b/xx
  @@ -0,0 +1,1 @@
  +xx


  $ hg histedit ".^^" --commands - <<EOF
  > pick 1aed0f31debd
  > x hg metaedit -m "histedit test"
  > x hg commit --amend -m 'message from exec'
  > pick a1c80e4c2636
  > pick 90ef4d40a825
  > EOF
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  a1c80e4c2636: skipping changeset (no changes)

  $ glog -r 'all()'
  @  942d79297adf@default(draft) metaedit
  │
  o  b5e5d076151f@default(draft) message from exec
  │
  o  66f7d451a68b@default(draft) r1
  │
  o  1ea73414a91b@default(draft) r0
  

metaedit noncontinuous set of commits in the stack:

  $ cd $TESTTMP
  $ hg init metaeditnoncontinues
  $ cd metaeditnoncontinues
  $ hg debugbuilddag '+5'
  $ hg goto tip
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ glog -r 'all()'
  @  bebd167eb94d@default(draft) r4
  │
  o  2dc09a01254d@default(draft) r3
  │
  o  01241442b3c2@default(draft) r2
  │
  o  66f7d451a68b@default(draft) r1
  │
  o  1ea73414a91b@default(draft) r0
  

  $ hg metaedit -m "metaedit" 0 2 4
  $ glog -r 'all()'
  @  2b037168acb5@default(draft) metaedit
  │
  o  1a9c34db0e76@default(draft) r3
  │
  o  4d7251aa2bec@default(draft) metaedit
  │
  o  16ad2130f633@default(draft) r1
  │
  o  e37e0d87697f@default(draft) metaedit
  

Test copying obsmarkers

  $ hg init $TESTTMP/autorel
  $ cd $TESTTMP/autorel
  $ drawdag<<'EOS'
  > D
  > |
  > C C1 # amend: C -> C1
  > |/
  > B
  > |
  > A
  > EOS
  $ hg metaedit -r $B -m B1
  $ glog -r 'all()'
  o  52bc6136aa97@default(draft) D
  │
  │ o  1be7301b35ae@default(draft) C1
  │ │
  x │  19437442f9e4@default(draft) C
  ├─╯
  o  888bb4818188@default(draft) B1
  │
  o  426bada5c675@default(draft) A
  

  $ hg log -r 'successors(19437442f9e4)-19437442f9e4' -T '{node}\n'
  1be7301b35ae8ac3543a07a5d0ce5ca615be709f

  $ hg log -r 'precursors(19437442f9e4)-19437442f9e4' -T '{desc} {node}\n' --hidden
  C 26805aba1e600a82e93661149f2313866a221a7b

  $ hg debugmutation -r 'desc(C1)'
   *  5577c14fa08d51a4644b9b4b6e001835594cadd2 amend by test at 1970-01-01T00:00:00 from:
      26805aba1e600a82e93661149f2313866a221a7b
  
   *  1be7301b35ae8ac3543a07a5d0ce5ca615be709f amend-copy by test at 1970-01-01T00:00:00 from:
      |-  5577c14fa08d51a4644b9b4b6e001835594cadd2 amend by test at 1970-01-01T00:00:00 from:
      |   26805aba1e600a82e93661149f2313866a221a7b
      '-  19437442f9e42aa92f504afb1a352caa3e6040f5 metaedit by test at 1970-01-01T00:00:00 from:
          26805aba1e600a82e93661149f2313866a221a7b
  
Slightly more complex: with double amends

  $ newrepo autorel1
  $ drawdag<<'EOS'
  > D
  > |
  > C C0 # amend: C -> C0 -> C1
  >  \| C1
  >   |/
  >   B
  >   |
  >   A
  > EOS
  $ hg metaedit -r $B -m B1
  $ glog -r 'all()'
  o  1be7301b35ae@default(draft) C1
  │
  │ o  52bc6136aa97@default(draft) D
  │ │
  │ x  19437442f9e4@default(draft) C
  ├─╯
  o  888bb4818188@default(draft) B1
  │
  o  426bada5c675@default(draft) A
  

  $ hg log -r 'successors(19437442f9e4)-19437442f9e4' -T '{node}\n'
  1be7301b35ae8ac3543a07a5d0ce5ca615be709f

  $ hg log -r 'precursors(19437442f9e4)-19437442f9e4' -T '{desc} {node}\n' --hidden
  C 26805aba1e600a82e93661149f2313866a221a7b

  $ hg debugmutation -r 'desc(C1)'
   *  5577c14fa08d51a4644b9b4b6e001835594cadd2 amend by test at 1970-01-01T00:00:00 from:
      bf080f2103efc214ac3a4638254d4c5370a9294b amend by test at 1970-01-01T00:00:00 from:
      26805aba1e600a82e93661149f2313866a221a7b
  
   *  1be7301b35ae8ac3543a07a5d0ce5ca615be709f amend-copy by test at 1970-01-01T00:00:00 from:
      |-  5577c14fa08d51a4644b9b4b6e001835594cadd2 amend by test at 1970-01-01T00:00:00 from:
      |   bf080f2103efc214ac3a4638254d4c5370a9294b amend by test at 1970-01-01T00:00:00 from:
      |   26805aba1e600a82e93661149f2313866a221a7b
      '-  19437442f9e42aa92f504afb1a352caa3e6040f5 metaedit by test at 1970-01-01T00:00:00 from:
          26805aba1e600a82e93661149f2313866a221a7b
  

Test empty commit
  $ hg co -q 1be7301b35ae
  $ hg commit --config ui.allowemptycommit=true -m empty
  $ hg metaedit -r ".^" -m "parent of empty commit"
  $ glog -r 'all()'
  @  e582f22eefc0@default(draft) empty
  │
  o  539393debc47@default(draft) parent of empty commit
  │
  │ o  52bc6136aa97@default(draft) D
  │ │
  │ x  19437442f9e4@default(draft) C
  ├─╯
  o  888bb4818188@default(draft) B1
  │
  o  426bada5c675@default(draft) A
  
Create some commits for testing the editing of commits in batch using `--batch`
option

  $ newrepo multi-commits
  $ drawdag << 'EOS'
  > A3
  > |
  > A2
  > |
  > A1
  > EOS

Editing a single commit using `--batch` uses the single-commit template

  $ HGEDITOR=cat hg metaedit --batch -r 'tip'
  SL: Commit message of changeset dad6906767c0
  A3
  
  
  SL: Enter commit message.  Lines beginning with 'SL:' are removed.
  SL: Leave message empty to abort commit.
  SL: --
  SL: user: test
  SL: branch 'default'
  SL: added A3
  nothing changed
  [1]

Test editing mutiple commits in a batch (--batch)

  $ HGEDITOR=cat hg metaedit --batch -r 'all()'
  SL: Editing 3 commits in batch. Do not change lines starting with 'SL:'.
  SL: Begin of commit b008d5d798a3
  A1
  SL: End of commit b008d5d798a3
  SL: -----------------------------------------------------------------------------
  SL: Begin of commit 9083513d0ea9
  A2
  SL: End of commit 9083513d0ea9
  SL: -----------------------------------------------------------------------------
  SL: Begin of commit dad6906767c0
  A3
  SL: End of commit dad6906767c0
  nothing changed
  [1]
  $ hg log -Gr 'all()' -T '{desc}'
  o  A3
  │
  o  A2
  │
  o  A1
  

#if no-osx
Test actually editing the commits

- Test editing a single commit
(BSD-flavored sed has an incompatible -'i')
  $ HGEDITOR="s'e'd -i 's/A/B/g'" hg metaedit --batch -r 'tip'
  $ hg log -Gr 'all()' -T '{desc}'
  o  B3
  │
  o  A2
  │
  o  A1
  
- Test editing multiple commits
(BSD-flavored sed has an incompatible -'i')
  $ HGEDITOR="s'e'd -i 's/A/B/g'" hg metaedit --batch -r 'all()'
  $ hg log -Gr 'all()' -T '{desc}'
  o  B3
  │
  o  B2
  │
  o  B1
  
#endif

Create some commits for testing the editing of commits in batch using JSON input

  $ newrepo json-input
  $ drawdag << 'EOS'
  > A3
  > |
  > A2
  > |
  > A1
  > EOS

  $ cat << EOF >> jsoninput
  > {
  >   "$(shaof A3)": {
  >     "message": "C3",
  >     "user": "C3PO <c3po@tatooine.com>"
  >   },
  >   "$(shaof A2)": {
  >     "message": "R2\nD2",
  >     "user": "R2D2 <r2d2@naboo.com>"
  >   }
  > }
  > EOF

  $ hg metaedit -r "$A2::" --json-input-file jsoninput

  $ hg log -Gr 'all()' -T '{desc|firstline} {author}'
  o  C3 C3PO <c3po@tatooine.com>
  │
  o  R2 R2D2 <r2d2@naboo.com>
  │
  o  A1 test
  

  $ cat << EOF > jsoninput
  > {
  >   "a3b": {
  >     "message": "C3",
  >     "user": "C3PO <c3po@tatooine.com>"
  >   },
  > EOF
  $ hg metaedit -r "$A2::" --json-input-file jsoninput
  abort: can't decode JSON input file 'jsoninput': * (glob)
  [255]

  $ cat << EOF > jsoninput
  > {
  >   "not a hash)": {
  >     "message": [1,2,3],
  >     "user": "C3PO <c3po@tatooine.com>"
  >   }
  > }
  > EOF
  $ hg metaedit -r "$A2::" --json-input-file jsoninput
  abort: invalid JSON input
  [255]

  $ hg metaedit -r "$A2::" --json-input-file jsoninput_other
  abort: can't read JSON input file 'jsoninput_other': $ENOENT$
  [255]


Test reusing commit message from another commit

  $ newrepo
  $ drawdag << 'EOS'
  > B
  > |
  > A
  > EOS

  $ hg metaedit -r "$B" -M "$A"
  $ hg log -Gr 'all()' -T '{desc}'
  o  A
  │
  o  A
  
Test commit template.

  $ setconfig committemplate.changeset='SL: ParentCount={parents|count}\n'
  $ HGEDITOR=cat hg metaedit -r 'max(all())'
  SL: ParentCount=1
  abort: empty commit message
  [255]
