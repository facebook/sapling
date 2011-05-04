  $ hg init dir
  $ cd dir
  $ echo bleh > bar
  $ hg add bar
  $ hg ci -m 'add bar'

  $ hg cp bar foo
  $ echo >> bar
  $ hg ci -m 'cp bar foo; change bar'

  $ hg debugrename foo
  foo renamed from bar:26d3ca0dfd18e44d796b564e38dd173c9668d3a9
  $ hg debugindex bar
     rev    offset  length   base linkrev nodeid       p1           p2
       0         0       6      0       0 26d3ca0dfd18 000000000000 000000000000
       1         6       7      1       1 d267bddd54f7 26d3ca0dfd18 000000000000
