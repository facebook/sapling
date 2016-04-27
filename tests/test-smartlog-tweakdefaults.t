  $ extpath=`dirname $TESTDIR`
  $ cp $extpath/smartlog.py $TESTTMP # use $TESTTMP substitution in message
  $ cp $extpath/tweakdefaults.py $TESTTMP
  $ cp $extpath/fbamend.py $TESTTMP
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > rebase=
  > smartlog=$TESTTMP/smartlog.py
  > tweakdefaults=$TESTTMP/tweakdefaults.py
  > fbamend=$TESTTMP/fbamend.py
  > [experimental]
  > evolution=createmarkers
  > allowdivergence=on
  > EOF

Prepare a repo for amend checks
  $ hg init repo
  $ cd repo
  $ echo root > root && hg ci -Am root # rev 0
  adding root
  $ echo base > base && hg ci -Am base # rev 1
  adding base
  $ echo a > a && hg ci -Am a          # rev 2
  adding a
  $ echo aa > a && hg amend            # rev 3 (aux) and 4 (real)
  $ hg up --hidden -r 2
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo aaa > a && hg amend           # rev 5 (aux) and 6 (real)

Check the amend template keywords
  $ hg log --hidden -r 2 -T "{node} amended as {amendsuccessors % '{short(amendsuccessor)} '}\n"
  6e2c701de62843743b3ad0c4397a88605f0aa7c9 amended as [a-f0-9]* [a-f0-9]*  (re)

Prepare a repo for rebase checks
  $ hg up 0
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ echo b > b && hg ci -Am b          # rev 7
  adding b
  created new head
  $ hg rebase --hidden -r 7 -d 1       # rev 8
  rebasing 7:1e9a3c00cbe9 "b" (tip)
  $ hg rebase --hidden -r 7 -d 2       # rev 9
  rebasing 7:1e9a3c00cbe9 "b"

Check the rebase template keywords
  $ hg log --hidden -r 7 -T "{node} rebased as {rebasesuccessors % '{short(rebasesuccessor)} '}\n"
  1e9a3c00cbe90d236ac05ef61efcc5e40b7412bc rebased as [a-f0-9]* [a-f0-9]*  (re)

