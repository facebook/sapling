Test of warning for evolve users when inhibit is enabled
  $ cat >> $HGRCPATH <<EOF
  > [ui]
  > username = nobody <no.reply@fb.com>
  > [experimental]
  > evolution= all
  > [inhibit]
  > cutoff=2015-07-04
  > [extensions]
  > inhibit=
  > directaccess=
  > evolve=
  > EOF
  $ echo "inhibitwarn = $TESTDIR/../inhibitwarn.py" >> $HGRCPATH
  $ hg init repo 
  $ cd repo
  $ mkcommit() {
  >    echo "$1" > "$1"
  >    hg add "$1"
  >    hg ci -m "add $1"
  > }

  $ mkcommit a
  $ mkcommit b
  $ mkcommit c
  $ mkcommit d
  $ mkcommit e
  $ hg log -r .
  changeset:   4:9d206ffc875e
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add e
  

One marker after the cutoff date should show no warning
  $ hg prune -d '2015-07-16' -r .
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  working directory now at 47d2a3944de8
  1 changesets pruned
  $ rm .hg/store/obsstore # Since we look only at the first marker
  $ hg log -r .
  changeset:   3:47d2a3944de8
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add d
  
One marker before the cutoff date should show a warning
  $ hg prune -d '2015-07-03' -r .
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  working directory now at 4538525df7e2
  1 changesets pruned
  $ hg log -r .
  
  +------------------------------------------------------------------------------+
  |You seem to be an evolve beta user. We installed the inhibit extension        |
  |on your computer and it will inhibit the effect of evolve and disturb         |
  |your workflow. You need to disable inhibit in your .hgrc to keep working      |
  |with evolve. Use hg config --local to open your local config and add the      |
  |following lines:                                                              |
  |[extensions]                                                                  |
  |inhibit=!                                                                     |
  |directaccess=!                                                                |
  |[experimental]                                                                |
  |evolution=all                                                                 |
  |                                                                              |
  |If you are no longer an evolve beta user and you don't want to see this error |
  |with evolve use hg config --local to open your local config and add the next  |
  |two lines:                                                                    |
  |[inhibit]                                                                     |
  |bypass-warning=True                                                           |
  |You shouldn't need to do anything else to make inhibit work for this repo.    |
  +------------------------------------------------------------------------------+
  changeset:   2:4538525df7e2
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add c
  
