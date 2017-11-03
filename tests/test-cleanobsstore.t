
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > cleanobsstore=$TESTDIR/../hgext3rd/cleanobsstore.py
  > fbamend=$TESTDIR/../hgext3rd/fbamend
  > [experimental]
  > evolution=createmarkers
  > EOF

  $ hg init repo
  $ cd repo
  $ hg debugbuilddag +5
  $ hg up -q tip
  $ hg prune -r .
  advice: 'hg hide' provides a better UI for hiding commits
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  working directory now at 2dc09a01254d
  1 changesets pruned
  $ hg debugobsolete
  bebd167eb94d257ace0e814aeb98e6972ed2970d 0 {2dc09a01254db841290af0538aa52f6f52c776e3} (*) {'ef1': '0', 'operation': 'prune', 'user': 'test'} (glob)
  $ HGUSER=baduser hg prune -r .
  advice: 'hg hide' provides a better UI for hiding commits
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  working directory now at 01241442b3c2
  1 changesets pruned
  $ hg debugobsolete
  bebd167eb94d257ace0e814aeb98e6972ed2970d 0 {2dc09a01254db841290af0538aa52f6f52c776e3} (*) {'ef1': '0', 'operation': 'prune', 'user': 'test'} (glob)
  2dc09a01254db841290af0538aa52f6f52c776e3 0 {01241442b3c2bf3211e593b549c655ea65b295e3} (*) {'ef1': '0', 'operation': 'prune', 'user': 'baduser'} (glob)

Run any command (for example, status). Obsstore shouldn't be cleaned because it doesn't exceed the limit
  $ hg --config cleanobsstore.badusernames=baduser st
  $ hg debugobsolete
  bebd167eb94d257ace0e814aeb98e6972ed2970d 0 {2dc09a01254db841290af0538aa52f6f52c776e3} (*) {'ef1': '0', 'operation': 'prune', 'user': 'test'} (glob)
  2dc09a01254db841290af0538aa52f6f52c776e3 0 {01241442b3c2bf3211e593b549c655ea65b295e3} (*) {'ef1': '0', 'operation': 'prune', 'user': 'baduser'} (glob)

Run any command again. This time it should be cleaned because we decreased the limit
  $ hg --config cleanobsstore.badusernames=baduser --config cleanobsstore.obsstoresizelimit=1 st
  $ hg debugobsolete
  bebd167eb94d257ace0e814aeb98e6972ed2970d 0 {2dc09a01254db841290af0538aa52f6f52c776e3} (*) {'ef1': '0', 'operation': 'prune', 'user': 'test'} (glob)

Create bad obsmarker again. Make sure it wasn't cleaned again
  $ echo 1 >> 1
  $ hg add 1
  $ hg ci -q -m 1
  $ HGUSER=baduser hg prune -q -r .
  advice: 'hg hide' provides a better UI for hiding commits
  $ hg debugobsolete
  bebd167eb94d257ace0e814aeb98e6972ed2970d 0 {2dc09a01254db841290af0538aa52f6f52c776e3} (*) {'ef1': '0', 'operation': 'prune', 'user': 'test'} (glob)
  73bce0eaaf9d039023d1b34421aceab146636d3e 0 {01241442b3c2bf3211e593b549c655ea65b295e3} (*) {'ef1': '0', 'operation': 'prune', 'user': 'baduser'} (glob)
  $ hg --config cleanobsstore.badusernames=baduser --config cleanobsstore.obsstoresizelimit=1 st
  $ hg debugobsolete
  bebd167eb94d257ace0e814aeb98e6972ed2970d 0 {2dc09a01254db841290af0538aa52f6f52c776e3} (*) {'ef1': '0', 'operation': 'prune', 'user': 'test'} (glob)
  73bce0eaaf9d039023d1b34421aceab146636d3e 0 {01241442b3c2bf3211e593b549c655ea65b295e3} (*) {'ef1': '0', 'operation': 'prune', 'user': 'baduser'} (glob)
