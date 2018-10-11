
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > amend=
  > cleanobsstore=
  > [experimental]
  > evolution=createmarkers
  > EOF

  $ hg init repo
  $ cd repo
  $ hg debugbuilddag +5
  $ hg up -q tip
  $ hg prune -r .
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  working directory now at 2dc09a01254d
  1 changesets pruned
  hint[strip-hide]: 'hg strip' may be deprecated in the future - use 'hg hide' instead
  hint[hint-ack]: use 'hg hint --ack strip-hide' to silence these hints
  $ hg debugobsolete
  bebd167eb94d257ace0e814aeb98e6972ed2970d 0 {2dc09a01254db841290af0538aa52f6f52c776e3} (Thu Jan 01 00:00:00 1970 +0000) {'operation': 'prune', 'user': 'test'}
  $ HGUSER=baduser hg prune -r .
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  working directory now at 01241442b3c2
  1 changesets pruned
  hint[strip-hide]: 'hg strip' may be deprecated in the future - use 'hg hide' instead
  hint[hint-ack]: use 'hg hint --ack strip-hide' to silence these hints
  $ hg debugobsolete
  bebd167eb94d257ace0e814aeb98e6972ed2970d 0 {2dc09a01254db841290af0538aa52f6f52c776e3} (Thu Jan 01 00:00:00 1970 +0000) {'operation': 'prune', 'user': 'test'}
  2dc09a01254db841290af0538aa52f6f52c776e3 0 {01241442b3c2bf3211e593b549c655ea65b295e3} (Thu Jan 01 00:00:00 1970 +0000) {'operation': 'prune', 'user': 'baduser'}

Run any command (for example, status). Obsstore shouldn't be cleaned because it doesn't exceed the limit
  $ hg --config cleanobsstore.badusernames=baduser st
  $ hg debugobsolete
  bebd167eb94d257ace0e814aeb98e6972ed2970d 0 {2dc09a01254db841290af0538aa52f6f52c776e3} (Thu Jan 01 00:00:00 1970 +0000) {'operation': 'prune', 'user': 'test'}
  2dc09a01254db841290af0538aa52f6f52c776e3 0 {01241442b3c2bf3211e593b549c655ea65b295e3} (Thu Jan 01 00:00:00 1970 +0000) {'operation': 'prune', 'user': 'baduser'}

Run any command again. This time it should be cleaned because we decreased the limit
  $ hg --config cleanobsstore.badusernames=baduser --config cleanobsstore.obsstoresizelimit=1 st
  $ hg debugobsolete
  bebd167eb94d257ace0e814aeb98e6972ed2970d 0 {2dc09a01254db841290af0538aa52f6f52c776e3} (Thu Jan 01 00:00:00 1970 +0000) {'operation': 'prune', 'user': 'test'}

Create bad obsmarker again. Make sure it wasn't cleaned again
  $ echo 1 >> 1
  $ hg add 1
  $ hg ci -q -m 1
  $ HGUSER=baduser hg prune -q -r .
  hint[strip-hide]: 'hg strip' may be deprecated in the future - use 'hg hide' instead
  hint[hint-ack]: use 'hg hint --ack strip-hide' to silence these hints
  $ hg debugobsolete
  bebd167eb94d257ace0e814aeb98e6972ed2970d 0 {2dc09a01254db841290af0538aa52f6f52c776e3} (Thu Jan 01 00:00:00 1970 +0000) {'operation': 'prune', 'user': 'test'}
  73bce0eaaf9d039023d1b34421aceab146636d3e 0 {01241442b3c2bf3211e593b549c655ea65b295e3} (Thu Jan 01 00:00:00 1970 +0000) {'operation': 'prune', 'user': 'baduser'}
  $ hg --config cleanobsstore.badusernames=baduser --config cleanobsstore.obsstoresizelimit=1 st
  $ hg debugobsolete
  bebd167eb94d257ace0e814aeb98e6972ed2970d 0 {2dc09a01254db841290af0538aa52f6f52c776e3} (Thu Jan 01 00:00:00 1970 +0000) {'operation': 'prune', 'user': 'test'}
  73bce0eaaf9d039023d1b34421aceab146636d3e 0 {01241442b3c2bf3211e593b549c655ea65b295e3} (Thu Jan 01 00:00:00 1970 +0000) {'operation': 'prune', 'user': 'baduser'}
