  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > smartlog = $TESTDIR/../smartlog.py
  > EOF

Build up a repo

  $ hg init repo
  $ cd repo

Confirm smartlog doesn't error on an empty repo
  $ hg smartlog

Continue repo setup
  $ hg book master
  $ hg sl -r 'smartlog() + master'
  $ touch a1 && hg add a1 && hg ci -ma1
  $ touch a2 && hg add a2 && hg ci -ma2
  $ hg book feature1
  $ touch b && hg add b && hg ci -mb
  $ hg up -q master
  $ touch c1 && hg add c1 && hg ci -mc1
  created new head
  $ touch c2 && hg add c2 && hg ci -mc2
  $ hg book feature2
  $ touch d && hg add d && hg ci -md
  $ hg log -G -T compact
  @  5[tip][feature2]   db92053d5c83   1970-01-01 00:00 +0000   test
  |    d
  |
  o  4[master]   38d85b506754   1970-01-01 00:00 +0000   test
  |    c2
  |
  o  3:1   ec7553f7b382   1970-01-01 00:00 +0000   test
  |    c1
  |
  | o  2[feature1]   49cdb4091aca   1970-01-01 00:00 +0000   test
  |/     b
  |
  o  1   b68836a6e2ca   1970-01-01 00:00 +0000   test
  |    a2
  |
  o  0   df4fd610a3d6   1970-01-01 00:00 +0000   test
       a1
  

Basic test
  $ hg smartlog -T compact
  @  5[tip][feature2]   db92053d5c83   1970-01-01 00:00 +0000   test
  |    d
  |
  o  4[master]   38d85b506754   1970-01-01 00:00 +0000   test
  |    c2
  |
  .
  .
  |
  | o  2[feature1]   49cdb4091aca   1970-01-01 00:00 +0000   test
  |/     b
  |
  o  1   b68836a6e2ca   1970-01-01 00:00 +0000   test
  |    a2
  |

With commit info
  $ echo "hello" >c2 && hg ci --amend
  saved backup bundle to $TESTTMP/repo/.hg/strip-backup/db92053d5c83-f9f5e1aa-amend-backup.hg (glob)
  $ hg smartlog -T compact --commit-info
  @  5[tip][feature2]   05d10250273e   1970-01-01 00:00 +0000   test
  |    d
  |
  |   M c2
  |   A d
  |
  o  4[master]   38d85b506754   1970-01-01 00:00 +0000   test
  |    c2
  |
  .
  .
  |
  | o  2[feature1]   49cdb4091aca   1970-01-01 00:00 +0000   test
  |/     b
  |
  o  1   b68836a6e2ca   1970-01-01 00:00 +0000   test
  |    a2
  |

As a revset
  $ hg log -G -T compact -r 'smartlog()'
  @  5[tip][feature2]   05d10250273e   1970-01-01 00:00 +0000   test
  |    d
  |
  o  4[master]   38d85b506754   1970-01-01 00:00 +0000   test
  |    c2
  |
  | o  2[feature1]   49cdb4091aca   1970-01-01 00:00 +0000   test
  | |    b
  | |

With --master
  $ hg smartlog -T compact --master 1
  @  5[tip][feature2]   05d10250273e   1970-01-01 00:00 +0000   test
  |    d
  |
  o  4[master]   38d85b506754   1970-01-01 00:00 +0000   test
  |    c2
  |
  o  3:1   ec7553f7b382   1970-01-01 00:00 +0000   test
  |    c1
  |
  | o  2[feature1]   49cdb4091aca   1970-01-01 00:00 +0000   test
  |/     b
  |
  o  1   b68836a6e2ca   1970-01-01 00:00 +0000   test
  |    a2
  |

Specific revs
  $ hg smartlog -T compact -r 2 -r 4
  o  4[master]   38d85b506754   1970-01-01 00:00 +0000   test
  |    c2
  |
  .
  .
  |
  | o  2[feature1]   49cdb4091aca   1970-01-01 00:00 +0000   test
  |/     b
  |
  o  1   b68836a6e2ca   1970-01-01 00:00 +0000   test
  |    a2
  |

  $ hg smartlog -T compact -r 'smartlog()' -r 0
  @  5[tip][feature2]   05d10250273e   1970-01-01 00:00 +0000   test
  |    d
  |
  o  4[master]   38d85b506754   1970-01-01 00:00 +0000   test
  |    c2
  |
  | o  2[feature1]   49cdb4091aca   1970-01-01 00:00 +0000   test
  |/     b
  |
  .
  .
  |
  o  0   df4fd610a3d6   1970-01-01 00:00 +0000   test
       a1
  

Test master ordering
  $ hg boo -f master -r 49cdb4091aca
  $ hg smartlog -T compact
  o  2[feature1,master]   49cdb4091aca   1970-01-01 00:00 +0000   test
  |    b
  |
  | @  5[tip][feature2]   05d10250273e   1970-01-01 00:00 +0000   test
  | |    d
  | |
  | o  4   38d85b506754   1970-01-01 00:00 +0000   test
  | |    c2
  | |
  | o  3:1   ec7553f7b382   1970-01-01 00:00 +0000   test
  |/     c1
  |
  o  1   b68836a6e2ca   1970-01-01 00:00 +0000   test
  |    a2
  |

Test overriding master
  $ hg boo -f master -r 38d85b506754
  $ hg smartlog -T compact
  @  5[tip][feature2]   05d10250273e   1970-01-01 00:00 +0000   test
  |    d
  |
  o  4[master]   38d85b506754   1970-01-01 00:00 +0000   test
  |    c2
  |
  .
  .
  |
  | o  2[feature1]   49cdb4091aca   1970-01-01 00:00 +0000   test
  |/     b
  |
  o  1   b68836a6e2ca   1970-01-01 00:00 +0000   test
  |    a2
  |

  $ hg smartlog -T compact --master feature1
  o  2[feature1]   49cdb4091aca   1970-01-01 00:00 +0000   test
  |    b
  |
  | @  5[tip][feature2]   05d10250273e   1970-01-01 00:00 +0000   test
  | |    d
  | |
  | o  4[master]   38d85b506754   1970-01-01 00:00 +0000   test
  | |    c2
  | |
  | o  3:1   ec7553f7b382   1970-01-01 00:00 +0000   test
  |/     c1
  |
  o  1   b68836a6e2ca   1970-01-01 00:00 +0000   test
  |    a2
  |

  $ hg smartlog -T compact --config smartlog.master=feature1
  o  2[feature1]   49cdb4091aca   1970-01-01 00:00 +0000   test
  |    b
  |
  | @  5[tip][feature2]   05d10250273e   1970-01-01 00:00 +0000   test
  | |    d
  | |
  | o  4[master]   38d85b506754   1970-01-01 00:00 +0000   test
  | |    c2
  | |
  | o  3:1   ec7553f7b382   1970-01-01 00:00 +0000   test
  |/     c1
  |
  o  1   b68836a6e2ca   1970-01-01 00:00 +0000   test
  |    a2
  |

  $ hg smartlog -T compact --config smartlog.master=feature2 --master feature1
  o  2[feature1]   49cdb4091aca   1970-01-01 00:00 +0000   test
  |    b
  |
  | @  5[tip][feature2]   05d10250273e   1970-01-01 00:00 +0000   test
  | |    d
  | |
  | o  4[master]   38d85b506754   1970-01-01 00:00 +0000   test
  | |    c2
  | |
  | o  3:1   ec7553f7b382   1970-01-01 00:00 +0000   test
  |/     c1
  |
  o  1   b68836a6e2ca   1970-01-01 00:00 +0000   test
  |    a2
  |

Test draft branches

  $ hg branch foo
  marked working directory as branch foo
  (branches are permanent and global, did you want a bookmark?)
  $ hg commit -m 'create branch foo'
  $ hg sl
  @  changeset:   6:26d4a421c339
  |  branch:      foo
  |  bookmark:    feature2
  |  tag:         tip
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     create branch foo
  |
  .
  .
  |
  o  changeset:   4:38d85b506754
  |  bookmark:    master
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     c2
  |
  .
  .
  |
  | o  changeset:   2:49cdb4091aca
  |/   bookmark:    feature1
  |    user:        test
  |    date:        Thu Jan 01 00:00:00 1970 +0000
  |    summary:     b
  |
  o  changeset:   1:b68836a6e2ca
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     a2
  |

Test with weird bookmark names

  $ hg book -r 2 foo-bar
  $ hg smartlog -r 'foo-bar + .' -T compact
  @  6[tip][feature2]   26d4a421c339   1970-01-01 00:00 +0000   test
  |    create branch foo
  |
  .
  .
  |
  | o  2[feature1,foo-bar]   49cdb4091aca   1970-01-01 00:00 +0000   test
  |/     b
  |
  o  1   b68836a6e2ca   1970-01-01 00:00 +0000   test
  |    a2
  |
  $ hg smartlog --config smartlog.master=foo-bar -T compact
  o  2[feature1,foo-bar]   49cdb4091aca   1970-01-01 00:00 +0000   test
  |    b
  |
  | @  6[tip][feature2]   26d4a421c339   1970-01-01 00:00 +0000   test
  | |    create branch foo
  | |
  | .
  | .
  | |
  | o  4[master]   38d85b506754   1970-01-01 00:00 +0000   test
  | |    c2
  | |
  | o  3:1   ec7553f7b382   1970-01-01 00:00 +0000   test
  |/     c1
  |
  o  1   b68836a6e2ca   1970-01-01 00:00 +0000   test
  |    a2
  |
  $ hg smartlog --config smartlog.master=xxxx -T compact
  abort: unknown revision 'xxxx'!
  [255]
