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
  > amend=
  > rebase=
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
  @  7	: F - test
  |
  o  6	: E - test
  |
  | o  5	: D2 - test
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

  $ hg metaedit --user foobar  -T "{nodechanges|json}\n"
  {"587528abfffe33d49f94f9d6223dbbd58d6197c6": ["212b2a2b87cdbae992f001e9baba64db389fbce7"]}
  $ hg log --template '{rev}: {author}\n' -r 'desc(F):' --hidden
  7: test
  8: foobar
  $ hg log --template '{rev}: {author}\n' -r .
  8: foobar

  $ HGEDITOR=cat hg metaedit '.^::.' --fold
  HG: This is a fold of 2 changesets.
  HG: Commit message of changeset 6.
  
  E
  
  HG: Commit message of changeset 8.
  
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
  @  9:a08d35fd7d9d@default(draft) E
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
  @  9:a08d35fd7d9d@default(draft) E
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
  10: E

  $ hg up '.^'
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ hg metaedit --user foobar2 tip
  $ hg log --template '{rev}: {author}\n' -r "user(foobar):" --hidden
  8: foobar
  9: test
  10: test
  11: foobar2
  $ hg diff -r "10" -r "11" --hidden

'fold' one commit
  $ hg metaedit "desc(D2)" --fold --user foobar3
  1 changesets folded
  $ hg log -r "tip" --template '{rev}: {author}\n'
  12: foobar3

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
  

make the top commit non-empty
  $ echo xx > xx
  $ hg add xx
  $ hg amend
  $ glog -r 'all()'
  @  10:90ef4d40a825@default(draft) metaedit
  |
  o  8:a1c80e4c2636@default(draft) metaedit
  |
  o  5:1aed0f31debd@default(draft) metaedit
  |
  o  1:66f7d451a68b@default(draft) r1
  |
  o  0:1ea73414a91b@default(draft) r0
  

test histedit compat

  $ echo '[extensions]' >> $HGRCPATH
  $ echo "fbhistedit=" >> $HGRCPATH
  $ echo "histedit=" >> $HGRCPATH

  $ hg export -r .
  # HG changeset patch
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
  @  13:942d79297adf@default(draft) metaedit
  |
  o  12:b5e5d076151f@default(draft) message from exec
  |
  o  1:66f7d451a68b@default(draft) r1
  |
  o  0:1ea73414a91b@default(draft) r0
  

metaedit noncontinuous set of commits in the stack:

  $ cd $TESTTMP
  $ hg init metaeditnoncontinues
  $ cd metaeditnoncontinues
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
  

  $ hg metaedit -m "metaedit" 0 2 4
  $ glog -r 'all()'
  @  9:2b037168acb5@default(draft) metaedit
  |
  o  8:1a9c34db0e76@default(draft) r3
  |
  o  7:4d7251aa2bec@default(draft) metaedit
  |
  o  6:16ad2130f633@default(draft) r1
  |
  o  5:e37e0d87697f@default(draft) metaedit
  

Test copying obsmarkers

  $ hg init $TESTTMP/autorel
  $ cd $TESTTMP/autorel
  $ hg debugdrawdag<<'EOS'
  > D
  > |
  > C C1 # amend: C -> C1
  > |/
  > B
  > |
  > A
  > EOS
  $ hg metaedit -r B -m B1
  $ rm .hg/localtags
  $ glog -r 'all()'
  o  8:52bc6136aa97@default(draft) D
  |
  | o  7:1be7301b35ae@default(draft) C1
  | |
  x |  6:19437442f9e4@default(draft) C
  |/
  o  5:888bb4818188@default(draft) B1
  |
  o  0:426bada5c675@default(draft) A
  

  $ hg log -r 'successors(19437442f9e4)-19437442f9e4' -T '{node}\n'
  1be7301b35ae8ac3543a07a5d0ce5ca615be709f

  $ hg log -r 'precursors(19437442f9e4)-19437442f9e4' -T '{desc} {node}\n' --hidden
  C 26805aba1e600a82e93661149f2313866a221a7b

Test empty commit
  $ hg co -q 1be7301b35ae
  $ hg commit --config ui.allowemptycommit=true -m empty
  $ hg metaedit -r ".^" -m "parent of empty commit"
  $ glog -r 'all()'
  @  11:e582f22eefc0@default(draft) empty
  |
  o  10:539393debc47@default(draft) parent of empty commit
  |
  | o  8:52bc6136aa97@default(draft) D
  | |
  | x  6:19437442f9e4@default(draft) C
  |/
  o  5:888bb4818188@default(draft) B1
  |
  o  0:426bada5c675@default(draft) A
  
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
  HG: Commit message of changeset dad6906767c0
  A3
  
  
  HG: Enter commit message.  Lines beginning with 'HG:' are removed.
  HG: Leave message empty to abort commit.
  HG: --
  HG: user: test
  HG: branch 'default'
  HG: added A3
  nothing changed
  [1]

Test editing mutiple commits in a batch (--batch)

  $ HGEDITOR=cat hg metaedit --batch -r 'all()'
  HG: Editing 3 commits in batch. Do not change lines starting with 'HG:'.
  HG: Begin of commit b008d5d798a3
  A1
  HG: End of commit b008d5d798a3
  HG: -----------------------------------------------------------------------------
  HG: Begin of commit 9083513d0ea9
  A2
  HG: End of commit 9083513d0ea9
  HG: -----------------------------------------------------------------------------
  HG: Begin of commit dad6906767c0
  A3
  HG: End of commit dad6906767c0
  nothing changed
  [1]
  $ hg log -Gr 'all()' -T '{desc}'
  o  A3
  |
  o  A2
  |
  o  A1
  

#if no-osx
Test actually editing the commits

- Test editing a single commit
(BSD-flavored sed has an incompatible -'i')
  $ HGEDITOR="s'e'd -i 's/A/B/g'" hg metaedit --batch -r 'tip'
  $ hg log -Gr 'all()' -T '{desc}'
  o  B3
  |
  o  A2
  |
  o  A1
  
- Test editing multiple commits
(BSD-flavored sed has an incompatible -'i')
  $ HGEDITOR="s'e'd -i 's/A/B/g'" hg metaedit --batch -r 'all()'
  $ hg log -Gr 'all()' -T '{desc}'
  o  B3
  |
  o  B2
  |
  o  B1
  
#endif

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
  |
  o  A
  
