  $ setconfig extensions.treemanifest=!
  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > smartlog=
  > [experimental]
  > graphstyle.grandparent=|
  > graphstyle.missing=|
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
  $ touch c2 && hg add c2 && hg ci -mc2
  $ hg book feature2
  $ touch d && hg add d && hg ci -md

  $ hg phase -r master --public
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
  .    c2
  .
  | o  2[feature1]   49cdb4091aca   1970-01-01 00:00 +0000   test
  |/     b
  |
  o  1   b68836a6e2ca   1970-01-01 00:00 +0000   test
  |    a2
  |

With commit info
  $ echo "hello" >c2 && hg ci --amend
  $ hg smartlog -T compact --commit-info
  @  6[tip][feature2]:4   05d10250273e   1970-01-01 00:00 +0000   test
  |    d
  |
  |   M c2
  |   A d
  |
  o  4[master]   38d85b506754   1970-01-01 00:00 +0000   test
  .    c2
  .
  | o  2[feature1]   49cdb4091aca   1970-01-01 00:00 +0000   test
  |/     b
  |
  o  1   b68836a6e2ca   1970-01-01 00:00 +0000   test
  |    a2
  |

As a revset
  $ hg log -G -T compact -r 'smartlog()'
  @  6[tip][feature2]:4   05d10250273e   1970-01-01 00:00 +0000   test
  |    d
  |
  o  4[master]   38d85b506754   1970-01-01 00:00 +0000   test
  |    c2
  |
  | o  2[feature1]   49cdb4091aca   1970-01-01 00:00 +0000   test
  |/     b
  |
  o  1   b68836a6e2ca   1970-01-01 00:00 +0000   test
  |    a2
  |

With --master
  $ hg phase -r 'all()' --draft -f
  $ hg phase -r 1 --public

  $ hg smartlog -T compact --master 1
  @  6[tip][feature2]:4   05d10250273e   1970-01-01 00:00 +0000   test
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

  $ hg phase -r master --public

Specific revs
  $ hg smartlog -T compact -r 2 -r 4
  o  4[master]   38d85b506754   1970-01-01 00:00 +0000   test
  .    c2
  .
  | o  2[feature1]   49cdb4091aca   1970-01-01 00:00 +0000   test
  |/     b
  |
  o  1   b68836a6e2ca   1970-01-01 00:00 +0000   test
  |    a2
  |

  $ hg smartlog -T compact -r 'smartlog()' -r 0
  @  6[tip][feature2]:4   05d10250273e   1970-01-01 00:00 +0000   test
  |    d
  |
  o  4[master]   38d85b506754   1970-01-01 00:00 +0000   test
  .    c2
  .
  | o  2[feature1]   49cdb4091aca   1970-01-01 00:00 +0000   test
  |/     b
  |
  o  1   b68836a6e2ca   1970-01-01 00:00 +0000   test
  |    a2
  |
  o  0   df4fd610a3d6   1970-01-01 00:00 +0000   test
       a1
  

Test master ordering
  $ hg phase -r 'all()' --draft -f
  $ hg phase -r 49cdb4091aca --public

  $ hg boo -f master -r 49cdb4091aca
  $ hg smartlog -T compact
  o  2[feature1,master]   49cdb4091aca   1970-01-01 00:00 +0000   test
  |    b
  |
  | @  6[tip][feature2]:4   05d10250273e   1970-01-01 00:00 +0000   test
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
  $ hg phase -r 'all()' --draft -f
  $ hg phase -r 38d85b506754 --public

  $ hg boo -f master -r 38d85b506754
  $ hg smartlog -T compact
  @  6[tip][feature2]:4   05d10250273e   1970-01-01 00:00 +0000   test
  |    d
  |
  o  4[master]   38d85b506754   1970-01-01 00:00 +0000   test
  .    c2
  .
  | o  2[feature1]   49cdb4091aca   1970-01-01 00:00 +0000   test
  |/     b
  |
  o  1   b68836a6e2ca   1970-01-01 00:00 +0000   test
  |    a2
  |

  $ hg phase -r 'all()' --draft -f
  $ hg phase -r feature1 --public

  $ hg smartlog -T compact --master feature1
  o  2[feature1]   49cdb4091aca   1970-01-01 00:00 +0000   test
  |    b
  |
  | @  6[tip][feature2]:4   05d10250273e   1970-01-01 00:00 +0000   test
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
  | @  6[tip][feature2]:4   05d10250273e   1970-01-01 00:00 +0000   test
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
  | @  6[tip][feature2]:4   05d10250273e   1970-01-01 00:00 +0000   test
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

  $ hg phase -r 'all()' --draft -f
  $ hg phase -r . --public

Test with weird bookmark names

  $ hg book -r 2 foo-bar
  $ hg smartlog -r 'foo-bar + .' -T compact
  @  6[tip][feature2]:4   05d10250273e   1970-01-01 00:00 +0000   test
  .    d
  .
  | o  2[feature1,foo-bar]   49cdb4091aca   1970-01-01 00:00 +0000   test
  |/     b
  |
  o  1   b68836a6e2ca   1970-01-01 00:00 +0000   test
  |    a2
  |

  $ hg phase -r 'all()' --draft -f
  $ hg phase -r foo-bar --public

  $ hg smartlog --config smartlog.master=foo-bar -T compact
  o  2[feature1,foo-bar]   49cdb4091aca   1970-01-01 00:00 +0000   test
  |    b
  |
  | @  6[tip][feature2]:4   05d10250273e   1970-01-01 00:00 +0000   test
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
  $ hg smartlog --config smartlog.master=xxxx -T compact
  abort: unknown revision 'xxxx'!
  (if xxxx is a remote bookmark or commit, try to 'hg pull' it first)
  [255]

  $ hg phase -r 'all()' --draft -f
  $ hg phase -r master --public

Test with two unrelated histories
  $ hg update null
  0 files updated, 0 files merged, 5 files removed, 0 files unresolved
  (leaving bookmark feature2)
  $ touch u1 && hg add u1 && hg ci -mu1
  $ touch u2 && hg add u2 && hg ci -mu2

  $ hg smartlog  -T compact
  @  8[tip]   806aaef35296   1970-01-01 00:00 +0000   test
  |    u2
  |
  o  7:-1   8749dc393678   1970-01-01 00:00 +0000   test
       u1
  
  o  6[feature2]:4   05d10250273e   1970-01-01 00:00 +0000   test
  |    d
  |
  o  4[master]   38d85b506754   1970-01-01 00:00 +0000   test
  .    c2
  .
  | o  2[feature1,foo-bar]   49cdb4091aca   1970-01-01 00:00 +0000   test
  |/     b
  |
  o  1   b68836a6e2ca   1970-01-01 00:00 +0000   test
  |    a2
  |

Test singlepublicsuccessor  template keyword
  $ echo "[extensions]" >> $HGRCPATH
  $ echo "rebase=" >> $HGRCPATH
  $ echo "[experimental]" >> $HGRCPATH
  $ echo "evolution=all" >> $HGRCPATH
  $ cd ..
  $ hg init kwrepo && cd kwrepo
  $ echo a > a && hg ci -Am a
  adding a
  $ echo b > b && hg ci -Am b
  adding b
  $ hg up 0
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo c > c && hg ci -Am c
  adding c
  $ hg rebase -s 2 -d 1
  rebasing 2:d36c0562f908 "c" (tip)
  $ hg phase -r 3 --public
  $ hg smartlog -r 2 -T "SPS: {singlepublicsuccessor}" --hidden
  x  SPS: 2b5806c2ca1e228838315bbffeb7d1504c38c9d6
  |
  o  SPS:
  

A draft stack at the top
  $ cd ..
  $ hg init repo2
  $ cd repo2
  $ hg debugbuilddag '+4'
  $ hg bookmark curr
  $ hg bookmark master -r 1
  $ hg phase --public -r 1
  $ hg smartlog -T compact --all
  o  3[tip]   2dc09a01254d   1970-01-01 00:00 +0000   debugbuilddag
  |    r3
  |
  o  2   01241442b3c2   1970-01-01 00:00 +0000   debugbuilddag
  |    r2
  |
  o  1[master]   66f7d451a68b   1970-01-01 00:00 +0000   debugbuilddag
  |    r1
  |
  $ hg smartlog -T compact --all --config smartlog.indentnonpublic=1
    o  3[tip]   2dc09a01254d   1970-01-01 00:00 +0000   debugbuilddag
    |    r3
    |
    o  2   01241442b3c2   1970-01-01 00:00 +0000   debugbuilddag
   /     r2
  |
  o  1[master]   66f7d451a68b   1970-01-01 00:00 +0000   debugbuilddag
  |    r1
  |

Different number of lines per node

  $ hg smartlog -T '{rev}' --all --config smartlog.indentnonpublic=1
    o  3
    |
    o  2
   /
  o  1
  |
  $ hg smartlog -T 'default' --all --config smartlog.indentnonpublic=1
    o  changeset:   3:2dc09a01254d
    |  tag:         tip
    |  user:        debugbuilddag
    |  date:        Thu Jan 01 00:00:03 1970 +0000
    |  summary:     r3
    |
    o  changeset:   2:01241442b3c2
   /   user:        debugbuilddag
  |    date:        Thu Jan 01 00:00:02 1970 +0000
  |    summary:     r2
  |
  o  changeset:   1:66f7d451a68b
  |  bookmark:    master
  |  user:        debugbuilddag
  |  date:        Thu Jan 01 00:00:01 1970 +0000
  |  summary:     r1
  |

Add other draft stacks
  $ hg up 1 -q
  $ echo 1 > a
  $ hg ci -A a -m a -q
  $ echo 2 >> a
  $ hg ci -A a -m a -q
  $ hg up 2 -q
  $ echo 2 > b
  $ hg ci -A b -m b -q
  $ hg smartlog -T compact --all --config smartlog.indentnonpublic=1
    o  5   a60fccdcd9e9   1970-01-01 00:00 +0000   test
    |    a
    |
    o  4:1   8d92afe5abfd   1970-01-01 00:00 +0000   test
   /     a
  |
  | @  6[tip]:2   401cd6213b51   1970-01-01 00:00 +0000   test
  | |    b
  | |
  | | o  3   2dc09a01254d   1970-01-01 00:00 +0000   debugbuilddag
  | |/     r3
  | |
  | o  2   01241442b3c2   1970-01-01 00:00 +0000   debugbuilddag
  |/     r2
  |
  o  1[master]   66f7d451a68b   1970-01-01 00:00 +0000   debugbuilddag
  |    r1
  |

Recent arg select days correctly
  $ echo 1 >> b
  $ myday=`$PYTHON -c 'import time; print(int(time.time()) - 24 * 3600 * 20)'`
  $ hg commit --date "$myday 0" -m test2
  $ hg update 0 -q
  $ hg log -Gr 'smartlog(master="master", heads=((date(-15) & draft()) + .))' -T compact
  o  1[master]   66f7d451a68b   1970-01-01 00:00 +0000   debugbuilddag
  |    r1
  |
  @  0   1ea73414a91b   1970-01-01 00:00 +0000   debugbuilddag
       r0
  

  $ hg log -Gr 'smartlog((date(-25) & draft()) + .)' -T compact
  o  7[tip] * (glob)
  |    test2
  |
  o  6:2   401cd6213b51   1970-01-01 00:00 +0000   test
  |    b
  |
  o  2   01241442b3c2   1970-01-01 00:00 +0000   debugbuilddag
  |    r2
  |
  o  1[master]   66f7d451a68b   1970-01-01 00:00 +0000   debugbuilddag
  |    r1
  |
  @  0   1ea73414a91b   1970-01-01 00:00 +0000   debugbuilddag
       r0
  
Make sure public commits that are descendants of master are not drawn
  $ cd ..
  $ hg init repo3
  $ cd repo3
  $ hg debugbuilddag '+5'
  $ hg bookmark master -r 1
  $ hg phase --public -r 1
  $ hg smartlog -T compact --all --config smartlog.indentnonpublic=1
    o  4[tip]   bebd167eb94d   1970-01-01 00:00 +0000   debugbuilddag
    |    r4
    |
    o  3   2dc09a01254d   1970-01-01 00:00 +0000   debugbuilddag
    |    r3
    |
    o  2   01241442b3c2   1970-01-01 00:00 +0000   debugbuilddag
   /     r2
  |
  o  1[master]   66f7d451a68b   1970-01-01 00:00 +0000   debugbuilddag
  |    r1
  |
  $ hg phase -r 3 --public --force
  $ hg up -q 4
  $ hg smartlog -T compact --all --config smartlog.indentnonpublic=1
    @  4[tip]   bebd167eb94d   1970-01-01 00:00 +0000   debugbuilddag
   /     r4
  |
  o  3   2dc09a01254d   1970-01-01 00:00 +0000   debugbuilddag
  .    r3
  .
  o  1[master]   66f7d451a68b   1970-01-01 00:00 +0000   debugbuilddag
  |    r1
  |
  $ hg phase -r 4 --public --force
  $ hg smartlog -T compact --all --config smartlog.indentnonpublic=1
  @  4[tip]   bebd167eb94d   1970-01-01 00:00 +0000   debugbuilddag
  .    r4
  .
  o  1[master]   66f7d451a68b   1970-01-01 00:00 +0000   debugbuilddag
  |    r1
  |

Make sure the template keywords are documented correctly
  $ hg help templates | egrep '(amend|fold|histedit|rebase|singlepublic|split|undo|node.s )'successor
      amendsuccessors
                    Return all of the node's successors created as a result of
      foldsuccessors
                    Return all of the node's successors created as a result of
      histeditsuccessors
                    Return all of the node's successors created as a result of
      rebasesuccessors
                    Return all of the node's successors created as a result of
      singlepublicsuccessor
      splitsuccessors
                    Return all of the node's successors created as a result of
      undosuccessors
                    Return all of the node's successors created as a result of
