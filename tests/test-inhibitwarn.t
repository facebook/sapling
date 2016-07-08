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
  $ echo "inhibitwarn = $TESTDIR/../hgext3rd/inhibitwarn.py" >> $HGRCPATH
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
  |You seems to be a beta user of Changeset Evolution                            |
  |https://fb.facebook.com/groups/630370820344870/                               |
  |                                                                              |
  |We just rolled out a major change to our mercurial                            |
  |https://fb.facebook.com/groups/scm.fyi/permalink/711128702353004/             |
  |                                                                              |
  |The rollout contains a lightweight version of Evolution that break your usual |
  |workflow using the "hg evolve" commands:                                      |
  | https://fb.facebook.com/groups/630370820344870/permalink/907861022595847/    |
  |                                                                              |
  |If you want to keep using evolve run `hg config -e` and add this to your      |
  |config:                                                                       |
  |[extensions]                                                                  |
  |inhibit=!                                                                     |
  |directaccess=!                                                                |
  |[experimental]                                                                |
  |evolution=all                                                                 |
  |                                                                              |
  |If you have no recollection of using evolution or stopped using it. run       |
  |`hg config -e` and add this to your config:                                   |
  |[inhibit]                                                                     |
  |bypass-warning=True                                                           |
  +------------------------------------------------------------------------------+
  changeset:   2:4538525df7e2
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add c
  
