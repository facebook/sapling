@  (34) head
|
| o  (33) head
| |
o |    (32) expand
|\ \
| o \    (31) expand
| |\ \
| | o \    (30) expand
| | |\ \
| | | o |  (29) regular commit
| | | | |
| | o | |    (28) merge zero known
| | |\ \ \
o | | | | |  (27) collapse
|/ / / / /
| | o---+  (26) merge one known; far right
| | | | |
+---o | |  (25) merge one known; far left
| | | | |
| | o | |  (24) merge one known; immediate right
| | |\| |
| | o | |  (23) merge one known; immediate left
| |/| | |
+---o---+  (22) merge two known; one far left, one far right
| |  / /
o | | |    (21) expand
|\ \ \ \
| o---+-+  (20) merge two known; two far right
|  / / /
o | | |    (19) expand
|\ \ \ \
+---+---o  (18) merge two known; two far left
| | | |
| o | |    (17) expand
| |\ \ \
| | o---+  (16) merge two known; one immediate right, one near right
| | |/ /
o | | |    (15) expand
|\ \ \ \
| o-----+  (14) merge two known; one immediate right, one far right
| |/ / /
o | | |    (13) expand
|\ \ \ \
+---o | |  (12) merge two known; one immediate right, one far left
| | |/ /
| o | |    (11) expand
| |\ \ \
| | o---+  (10) merge two known; one immediate left, one near right
| |/ / /
o | | |    (9) expand
|\ \ \ \
| o-----+  (8) merge two known; one immediate left, one far right
|/ / / /
o | | |    (7) expand
|\ \ \ \
+---o | |  (6) merge two known; one immediate left, one far left
| |/ / /
| o | |    (5) expand
| |\ \ \
| | o | |  (4) merge two known; one immediate left, one immediate right
| |/|/ /
| o / /  (3) collapse
|/ / /
o / /  (2) collapse
|/ /
o /  (1) collapse
|/
o  (0) root


  $ commit()
  > {
  >   rev=$1
  >   msg=$2
  >   shift 2
  >   if [ "$#" -gt 0 ]; then
  >       hg debugsetparents "$@"
  >   fi
  >   echo $rev > a
  >   hg commit -Aqd "$rev 0" -m "($rev) $msg"
  > }

  $ cat > printrevset.py <<EOF
  > from mercurial import extensions, revset, commands, cmdutil
  > 
  > def uisetup(ui):
  >     def printrevset(orig, ui, repo, *pats, **opts):
  >         if opts.get('print_revset'):
  >             expr = cmdutil.getgraphlogrevs(repo, pats, opts)[1]
  >             if expr:
  >                 tree = revset.parse(expr)
  >             else:
  >                 tree = []
  >             ui.write('%r\n' % (opts.get('rev', []),))
  >             ui.write(revset.prettyformat(tree) + '\n')
  >             return 0
  >         return orig(ui, repo, *pats, **opts)
  >     entry = extensions.wrapcommand(commands.table, 'log', printrevset)
  >     entry[1].append(('', 'print-revset', False,
  >                      'print generated revset and exit (DEPRECATED)'))
  > EOF

  $ echo "[extensions]" >> $HGRCPATH
  $ echo "printrevset=`pwd`/printrevset.py" >> $HGRCPATH

  $ hg init repo
  $ cd repo

Empty repo:

  $ hg log -G


Building DAG:

  $ commit 0 "root"
  $ commit 1 "collapse" 0
  $ commit 2 "collapse" 1
  $ commit 3 "collapse" 2
  $ commit 4 "merge two known; one immediate left, one immediate right" 1 3
  $ commit 5 "expand" 3 4
  $ commit 6 "merge two known; one immediate left, one far left" 2 5
  $ commit 7 "expand" 2 5
  $ commit 8 "merge two known; one immediate left, one far right" 0 7
  $ commit 9 "expand" 7 8
  $ commit 10 "merge two known; one immediate left, one near right" 0 6
  $ commit 11 "expand" 6 10
  $ commit 12 "merge two known; one immediate right, one far left" 1 9
  $ commit 13 "expand" 9 11
  $ commit 14 "merge two known; one immediate right, one far right" 0 12
  $ commit 15 "expand" 13 14
  $ commit 16 "merge two known; one immediate right, one near right" 0 1
  $ commit 17 "expand" 12 16
  $ commit 18 "merge two known; two far left" 1 15
  $ commit 19 "expand" 15 17
  $ commit 20 "merge two known; two far right" 0 18
  $ commit 21 "expand" 19 20
  $ commit 22 "merge two known; one far left, one far right" 18 21
  $ commit 23 "merge one known; immediate left" 1 22
  $ commit 24 "merge one known; immediate right" 0 23
  $ commit 25 "merge one known; far left" 21 24
  $ commit 26 "merge one known; far right" 18 25
  $ commit 27 "collapse" 21
  $ commit 28 "merge zero known" 1 26
  $ commit 29 "regular commit" 0
  $ commit 30 "expand" 28 29
  $ commit 31 "expand" 21 30
  $ commit 32 "expand" 27 31
  $ commit 33 "head" 18
  $ commit 34 "head" 32


  $ hg log -G -q
  @  34:fea3ac5810e0
  |
  | o  33:68608f5145f9
  | |
  o |    32:d06dffa21a31
  |\ \
  | o \    31:621d83e11f67
  | |\ \
  | | o \    30:6e11cd4b648f
  | | |\ \
  | | | o |  29:cd9bb2be7593
  | | | | |
  | | o | |    28:44ecd0b9ae99
  | | |\ \ \
  o | | | | |  27:886ed638191b
  |/ / / / /
  | | o---+  26:7f25b6c2f0b9
  | | | | |
  +---o | |  25:91da8ed57247
  | | | | |
  | | o | |  24:a9c19a3d96b7
  | | |\| |
  | | o | |  23:a01cddf0766d
  | |/| | |
  +---o---+  22:e0d9cccacb5d
  | |  / /
  o | | |    21:d42a756af44d
  |\ \ \ \
  | o---+-+  20:d30ed6450e32
  |  / / /
  o | | |    19:31ddc2c1573b
  |\ \ \ \
  +---+---o  18:1aa84d96232a
  | | | |
  | o | |    17:44765d7c06e0
  | |\ \ \
  | | o---+  16:3677d192927d
  | | |/ /
  o | | |    15:1dda3f72782d
  |\ \ \ \
  | o-----+  14:8eac370358ef
  | |/ / /
  o | | |    13:22d8966a97e3
  |\ \ \ \
  +---o | |  12:86b91144a6e9
  | | |/ /
  | o | |    11:832d76e6bdf2
  | |\ \ \
  | | o---+  10:74c64d036d72
  | |/ / /
  o | | |    9:7010c0af0a35
  |\ \ \ \
  | o-----+  8:7a0b11f71937
  |/ / / /
  o | | |    7:b632bb1b1224
  |\ \ \ \
  +---o | |  6:b105a072e251
  | |/ / /
  | o | |    5:4409d547b708
  | |\ \ \
  | | o | |  4:26a8bac39d9f
  | |/|/ /
  | o / /  3:27eef8ed80b4
  |/ / /
  o / /  2:3d9a33b8d1e1
  |/ /
  o /  1:6db2ef61d156
  |/
  o  0:e6eb3150255d
  

  $ hg log -G
  @  changeset:   34:fea3ac5810e0
  |  tag:         tip
  |  parent:      32:d06dffa21a31
  |  user:        test
  |  date:        Thu Jan 01 00:00:34 1970 +0000
  |  summary:     (34) head
  |
  | o  changeset:   33:68608f5145f9
  | |  parent:      18:1aa84d96232a
  | |  user:        test
  | |  date:        Thu Jan 01 00:00:33 1970 +0000
  | |  summary:     (33) head
  | |
  o |    changeset:   32:d06dffa21a31
  |\ \   parent:      27:886ed638191b
  | | |  parent:      31:621d83e11f67
  | | |  user:        test
  | | |  date:        Thu Jan 01 00:00:32 1970 +0000
  | | |  summary:     (32) expand
  | | |
  | o |    changeset:   31:621d83e11f67
  | |\ \   parent:      21:d42a756af44d
  | | | |  parent:      30:6e11cd4b648f
  | | | |  user:        test
  | | | |  date:        Thu Jan 01 00:00:31 1970 +0000
  | | | |  summary:     (31) expand
  | | | |
  | | o |    changeset:   30:6e11cd4b648f
  | | |\ \   parent:      28:44ecd0b9ae99
  | | | | |  parent:      29:cd9bb2be7593
  | | | | |  user:        test
  | | | | |  date:        Thu Jan 01 00:00:30 1970 +0000
  | | | | |  summary:     (30) expand
  | | | | |
  | | | o |  changeset:   29:cd9bb2be7593
  | | | | |  parent:      0:e6eb3150255d
  | | | | |  user:        test
  | | | | |  date:        Thu Jan 01 00:00:29 1970 +0000
  | | | | |  summary:     (29) regular commit
  | | | | |
  | | o | |    changeset:   28:44ecd0b9ae99
  | | |\ \ \   parent:      1:6db2ef61d156
  | | | | | |  parent:      26:7f25b6c2f0b9
  | | | | | |  user:        test
  | | | | | |  date:        Thu Jan 01 00:00:28 1970 +0000
  | | | | | |  summary:     (28) merge zero known
  | | | | | |
  o | | | | |  changeset:   27:886ed638191b
  |/ / / / /   parent:      21:d42a756af44d
  | | | | |    user:        test
  | | | | |    date:        Thu Jan 01 00:00:27 1970 +0000
  | | | | |    summary:     (27) collapse
  | | | | |
  | | o---+  changeset:   26:7f25b6c2f0b9
  | | | | |  parent:      18:1aa84d96232a
  | | | | |  parent:      25:91da8ed57247
  | | | | |  user:        test
  | | | | |  date:        Thu Jan 01 00:00:26 1970 +0000
  | | | | |  summary:     (26) merge one known; far right
  | | | | |
  +---o | |  changeset:   25:91da8ed57247
  | | | | |  parent:      21:d42a756af44d
  | | | | |  parent:      24:a9c19a3d96b7
  | | | | |  user:        test
  | | | | |  date:        Thu Jan 01 00:00:25 1970 +0000
  | | | | |  summary:     (25) merge one known; far left
  | | | | |
  | | o | |  changeset:   24:a9c19a3d96b7
  | | |\| |  parent:      0:e6eb3150255d
  | | | | |  parent:      23:a01cddf0766d
  | | | | |  user:        test
  | | | | |  date:        Thu Jan 01 00:00:24 1970 +0000
  | | | | |  summary:     (24) merge one known; immediate right
  | | | | |
  | | o | |  changeset:   23:a01cddf0766d
  | |/| | |  parent:      1:6db2ef61d156
  | | | | |  parent:      22:e0d9cccacb5d
  | | | | |  user:        test
  | | | | |  date:        Thu Jan 01 00:00:23 1970 +0000
  | | | | |  summary:     (23) merge one known; immediate left
  | | | | |
  +---o---+  changeset:   22:e0d9cccacb5d
  | |   | |  parent:      18:1aa84d96232a
  | |  / /   parent:      21:d42a756af44d
  | | | |    user:        test
  | | | |    date:        Thu Jan 01 00:00:22 1970 +0000
  | | | |    summary:     (22) merge two known; one far left, one far right
  | | | |
  o | | |    changeset:   21:d42a756af44d
  |\ \ \ \   parent:      19:31ddc2c1573b
  | | | | |  parent:      20:d30ed6450e32
  | | | | |  user:        test
  | | | | |  date:        Thu Jan 01 00:00:21 1970 +0000
  | | | | |  summary:     (21) expand
  | | | | |
  | o---+-+  changeset:   20:d30ed6450e32
  |   | | |  parent:      0:e6eb3150255d
  |  / / /   parent:      18:1aa84d96232a
  | | | |    user:        test
  | | | |    date:        Thu Jan 01 00:00:20 1970 +0000
  | | | |    summary:     (20) merge two known; two far right
  | | | |
  o | | |    changeset:   19:31ddc2c1573b
  |\ \ \ \   parent:      15:1dda3f72782d
  | | | | |  parent:      17:44765d7c06e0
  | | | | |  user:        test
  | | | | |  date:        Thu Jan 01 00:00:19 1970 +0000
  | | | | |  summary:     (19) expand
  | | | | |
  +---+---o  changeset:   18:1aa84d96232a
  | | | |    parent:      1:6db2ef61d156
  | | | |    parent:      15:1dda3f72782d
  | | | |    user:        test
  | | | |    date:        Thu Jan 01 00:00:18 1970 +0000
  | | | |    summary:     (18) merge two known; two far left
  | | | |
  | o | |    changeset:   17:44765d7c06e0
  | |\ \ \   parent:      12:86b91144a6e9
  | | | | |  parent:      16:3677d192927d
  | | | | |  user:        test
  | | | | |  date:        Thu Jan 01 00:00:17 1970 +0000
  | | | | |  summary:     (17) expand
  | | | | |
  | | o---+  changeset:   16:3677d192927d
  | | | | |  parent:      0:e6eb3150255d
  | | |/ /   parent:      1:6db2ef61d156
  | | | |    user:        test
  | | | |    date:        Thu Jan 01 00:00:16 1970 +0000
  | | | |    summary:     (16) merge two known; one immediate right, one near right
  | | | |
  o | | |    changeset:   15:1dda3f72782d
  |\ \ \ \   parent:      13:22d8966a97e3
  | | | | |  parent:      14:8eac370358ef
  | | | | |  user:        test
  | | | | |  date:        Thu Jan 01 00:00:15 1970 +0000
  | | | | |  summary:     (15) expand
  | | | | |
  | o-----+  changeset:   14:8eac370358ef
  | | | | |  parent:      0:e6eb3150255d
  | |/ / /   parent:      12:86b91144a6e9
  | | | |    user:        test
  | | | |    date:        Thu Jan 01 00:00:14 1970 +0000
  | | | |    summary:     (14) merge two known; one immediate right, one far right
  | | | |
  o | | |    changeset:   13:22d8966a97e3
  |\ \ \ \   parent:      9:7010c0af0a35
  | | | | |  parent:      11:832d76e6bdf2
  | | | | |  user:        test
  | | | | |  date:        Thu Jan 01 00:00:13 1970 +0000
  | | | | |  summary:     (13) expand
  | | | | |
  +---o | |  changeset:   12:86b91144a6e9
  | | |/ /   parent:      1:6db2ef61d156
  | | | |    parent:      9:7010c0af0a35
  | | | |    user:        test
  | | | |    date:        Thu Jan 01 00:00:12 1970 +0000
  | | | |    summary:     (12) merge two known; one immediate right, one far left
  | | | |
  | o | |    changeset:   11:832d76e6bdf2
  | |\ \ \   parent:      6:b105a072e251
  | | | | |  parent:      10:74c64d036d72
  | | | | |  user:        test
  | | | | |  date:        Thu Jan 01 00:00:11 1970 +0000
  | | | | |  summary:     (11) expand
  | | | | |
  | | o---+  changeset:   10:74c64d036d72
  | | | | |  parent:      0:e6eb3150255d
  | |/ / /   parent:      6:b105a072e251
  | | | |    user:        test
  | | | |    date:        Thu Jan 01 00:00:10 1970 +0000
  | | | |    summary:     (10) merge two known; one immediate left, one near right
  | | | |
  o | | |    changeset:   9:7010c0af0a35
  |\ \ \ \   parent:      7:b632bb1b1224
  | | | | |  parent:      8:7a0b11f71937
  | | | | |  user:        test
  | | | | |  date:        Thu Jan 01 00:00:09 1970 +0000
  | | | | |  summary:     (9) expand
  | | | | |
  | o-----+  changeset:   8:7a0b11f71937
  | | | | |  parent:      0:e6eb3150255d
  |/ / / /   parent:      7:b632bb1b1224
  | | | |    user:        test
  | | | |    date:        Thu Jan 01 00:00:08 1970 +0000
  | | | |    summary:     (8) merge two known; one immediate left, one far right
  | | | |
  o | | |    changeset:   7:b632bb1b1224
  |\ \ \ \   parent:      2:3d9a33b8d1e1
  | | | | |  parent:      5:4409d547b708
  | | | | |  user:        test
  | | | | |  date:        Thu Jan 01 00:00:07 1970 +0000
  | | | | |  summary:     (7) expand
  | | | | |
  +---o | |  changeset:   6:b105a072e251
  | |/ / /   parent:      2:3d9a33b8d1e1
  | | | |    parent:      5:4409d547b708
  | | | |    user:        test
  | | | |    date:        Thu Jan 01 00:00:06 1970 +0000
  | | | |    summary:     (6) merge two known; one immediate left, one far left
  | | | |
  | o | |    changeset:   5:4409d547b708
  | |\ \ \   parent:      3:27eef8ed80b4
  | | | | |  parent:      4:26a8bac39d9f
  | | | | |  user:        test
  | | | | |  date:        Thu Jan 01 00:00:05 1970 +0000
  | | | | |  summary:     (5) expand
  | | | | |
  | | o | |  changeset:   4:26a8bac39d9f
  | |/|/ /   parent:      1:6db2ef61d156
  | | | |    parent:      3:27eef8ed80b4
  | | | |    user:        test
  | | | |    date:        Thu Jan 01 00:00:04 1970 +0000
  | | | |    summary:     (4) merge two known; one immediate left, one immediate right
  | | | |
  | o | |  changeset:   3:27eef8ed80b4
  |/ / /   user:        test
  | | |    date:        Thu Jan 01 00:00:03 1970 +0000
  | | |    summary:     (3) collapse
  | | |
  o | |  changeset:   2:3d9a33b8d1e1
  |/ /   user:        test
  | |    date:        Thu Jan 01 00:00:02 1970 +0000
  | |    summary:     (2) collapse
  | |
  o |  changeset:   1:6db2ef61d156
  |/   user:        test
  |    date:        Thu Jan 01 00:00:01 1970 +0000
  |    summary:     (1) collapse
  |
  o  changeset:   0:e6eb3150255d
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     (0) root
  

File glog:
  $ hg log -G a
  @  changeset:   34:fea3ac5810e0
  |  tag:         tip
  |  parent:      32:d06dffa21a31
  |  user:        test
  |  date:        Thu Jan 01 00:00:34 1970 +0000
  |  summary:     (34) head
  |
  | o  changeset:   33:68608f5145f9
  | |  parent:      18:1aa84d96232a
  | |  user:        test
  | |  date:        Thu Jan 01 00:00:33 1970 +0000
  | |  summary:     (33) head
  | |
  o |    changeset:   32:d06dffa21a31
  |\ \   parent:      27:886ed638191b
  | | |  parent:      31:621d83e11f67
  | | |  user:        test
  | | |  date:        Thu Jan 01 00:00:32 1970 +0000
  | | |  summary:     (32) expand
  | | |
  | o |    changeset:   31:621d83e11f67
  | |\ \   parent:      21:d42a756af44d
  | | | |  parent:      30:6e11cd4b648f
  | | | |  user:        test
  | | | |  date:        Thu Jan 01 00:00:31 1970 +0000
  | | | |  summary:     (31) expand
  | | | |
  | | o |    changeset:   30:6e11cd4b648f
  | | |\ \   parent:      28:44ecd0b9ae99
  | | | | |  parent:      29:cd9bb2be7593
  | | | | |  user:        test
  | | | | |  date:        Thu Jan 01 00:00:30 1970 +0000
  | | | | |  summary:     (30) expand
  | | | | |
  | | | o |  changeset:   29:cd9bb2be7593
  | | | | |  parent:      0:e6eb3150255d
  | | | | |  user:        test
  | | | | |  date:        Thu Jan 01 00:00:29 1970 +0000
  | | | | |  summary:     (29) regular commit
  | | | | |
  | | o | |    changeset:   28:44ecd0b9ae99
  | | |\ \ \   parent:      1:6db2ef61d156
  | | | | | |  parent:      26:7f25b6c2f0b9
  | | | | | |  user:        test
  | | | | | |  date:        Thu Jan 01 00:00:28 1970 +0000
  | | | | | |  summary:     (28) merge zero known
  | | | | | |
  o | | | | |  changeset:   27:886ed638191b
  |/ / / / /   parent:      21:d42a756af44d
  | | | | |    user:        test
  | | | | |    date:        Thu Jan 01 00:00:27 1970 +0000
  | | | | |    summary:     (27) collapse
  | | | | |
  | | o---+  changeset:   26:7f25b6c2f0b9
  | | | | |  parent:      18:1aa84d96232a
  | | | | |  parent:      25:91da8ed57247
  | | | | |  user:        test
  | | | | |  date:        Thu Jan 01 00:00:26 1970 +0000
  | | | | |  summary:     (26) merge one known; far right
  | | | | |
  +---o | |  changeset:   25:91da8ed57247
  | | | | |  parent:      21:d42a756af44d
  | | | | |  parent:      24:a9c19a3d96b7
  | | | | |  user:        test
  | | | | |  date:        Thu Jan 01 00:00:25 1970 +0000
  | | | | |  summary:     (25) merge one known; far left
  | | | | |
  | | o | |  changeset:   24:a9c19a3d96b7
  | | |\| |  parent:      0:e6eb3150255d
  | | | | |  parent:      23:a01cddf0766d
  | | | | |  user:        test
  | | | | |  date:        Thu Jan 01 00:00:24 1970 +0000
  | | | | |  summary:     (24) merge one known; immediate right
  | | | | |
  | | o | |  changeset:   23:a01cddf0766d
  | |/| | |  parent:      1:6db2ef61d156
  | | | | |  parent:      22:e0d9cccacb5d
  | | | | |  user:        test
  | | | | |  date:        Thu Jan 01 00:00:23 1970 +0000
  | | | | |  summary:     (23) merge one known; immediate left
  | | | | |
  +---o---+  changeset:   22:e0d9cccacb5d
  | |   | |  parent:      18:1aa84d96232a
  | |  / /   parent:      21:d42a756af44d
  | | | |    user:        test
  | | | |    date:        Thu Jan 01 00:00:22 1970 +0000
  | | | |    summary:     (22) merge two known; one far left, one far right
  | | | |
  o | | |    changeset:   21:d42a756af44d
  |\ \ \ \   parent:      19:31ddc2c1573b
  | | | | |  parent:      20:d30ed6450e32
  | | | | |  user:        test
  | | | | |  date:        Thu Jan 01 00:00:21 1970 +0000
  | | | | |  summary:     (21) expand
  | | | | |
  | o---+-+  changeset:   20:d30ed6450e32
  |   | | |  parent:      0:e6eb3150255d
  |  / / /   parent:      18:1aa84d96232a
  | | | |    user:        test
  | | | |    date:        Thu Jan 01 00:00:20 1970 +0000
  | | | |    summary:     (20) merge two known; two far right
  | | | |
  o | | |    changeset:   19:31ddc2c1573b
  |\ \ \ \   parent:      15:1dda3f72782d
  | | | | |  parent:      17:44765d7c06e0
  | | | | |  user:        test
  | | | | |  date:        Thu Jan 01 00:00:19 1970 +0000
  | | | | |  summary:     (19) expand
  | | | | |
  +---+---o  changeset:   18:1aa84d96232a
  | | | |    parent:      1:6db2ef61d156
  | | | |    parent:      15:1dda3f72782d
  | | | |    user:        test
  | | | |    date:        Thu Jan 01 00:00:18 1970 +0000
  | | | |    summary:     (18) merge two known; two far left
  | | | |
  | o | |    changeset:   17:44765d7c06e0
  | |\ \ \   parent:      12:86b91144a6e9
  | | | | |  parent:      16:3677d192927d
  | | | | |  user:        test
  | | | | |  date:        Thu Jan 01 00:00:17 1970 +0000
  | | | | |  summary:     (17) expand
  | | | | |
  | | o---+  changeset:   16:3677d192927d
  | | | | |  parent:      0:e6eb3150255d
  | | |/ /   parent:      1:6db2ef61d156
  | | | |    user:        test
  | | | |    date:        Thu Jan 01 00:00:16 1970 +0000
  | | | |    summary:     (16) merge two known; one immediate right, one near right
  | | | |
  o | | |    changeset:   15:1dda3f72782d
  |\ \ \ \   parent:      13:22d8966a97e3
  | | | | |  parent:      14:8eac370358ef
  | | | | |  user:        test
  | | | | |  date:        Thu Jan 01 00:00:15 1970 +0000
  | | | | |  summary:     (15) expand
  | | | | |
  | o-----+  changeset:   14:8eac370358ef
  | | | | |  parent:      0:e6eb3150255d
  | |/ / /   parent:      12:86b91144a6e9
  | | | |    user:        test
  | | | |    date:        Thu Jan 01 00:00:14 1970 +0000
  | | | |    summary:     (14) merge two known; one immediate right, one far right
  | | | |
  o | | |    changeset:   13:22d8966a97e3
  |\ \ \ \   parent:      9:7010c0af0a35
  | | | | |  parent:      11:832d76e6bdf2
  | | | | |  user:        test
  | | | | |  date:        Thu Jan 01 00:00:13 1970 +0000
  | | | | |  summary:     (13) expand
  | | | | |
  +---o | |  changeset:   12:86b91144a6e9
  | | |/ /   parent:      1:6db2ef61d156
  | | | |    parent:      9:7010c0af0a35
  | | | |    user:        test
  | | | |    date:        Thu Jan 01 00:00:12 1970 +0000
  | | | |    summary:     (12) merge two known; one immediate right, one far left
  | | | |
  | o | |    changeset:   11:832d76e6bdf2
  | |\ \ \   parent:      6:b105a072e251
  | | | | |  parent:      10:74c64d036d72
  | | | | |  user:        test
  | | | | |  date:        Thu Jan 01 00:00:11 1970 +0000
  | | | | |  summary:     (11) expand
  | | | | |
  | | o---+  changeset:   10:74c64d036d72
  | | | | |  parent:      0:e6eb3150255d
  | |/ / /   parent:      6:b105a072e251
  | | | |    user:        test
  | | | |    date:        Thu Jan 01 00:00:10 1970 +0000
  | | | |    summary:     (10) merge two known; one immediate left, one near right
  | | | |
  o | | |    changeset:   9:7010c0af0a35
  |\ \ \ \   parent:      7:b632bb1b1224
  | | | | |  parent:      8:7a0b11f71937
  | | | | |  user:        test
  | | | | |  date:        Thu Jan 01 00:00:09 1970 +0000
  | | | | |  summary:     (9) expand
  | | | | |
  | o-----+  changeset:   8:7a0b11f71937
  | | | | |  parent:      0:e6eb3150255d
  |/ / / /   parent:      7:b632bb1b1224
  | | | |    user:        test
  | | | |    date:        Thu Jan 01 00:00:08 1970 +0000
  | | | |    summary:     (8) merge two known; one immediate left, one far right
  | | | |
  o | | |    changeset:   7:b632bb1b1224
  |\ \ \ \   parent:      2:3d9a33b8d1e1
  | | | | |  parent:      5:4409d547b708
  | | | | |  user:        test
  | | | | |  date:        Thu Jan 01 00:00:07 1970 +0000
  | | | | |  summary:     (7) expand
  | | | | |
  +---o | |  changeset:   6:b105a072e251
  | |/ / /   parent:      2:3d9a33b8d1e1
  | | | |    parent:      5:4409d547b708
  | | | |    user:        test
  | | | |    date:        Thu Jan 01 00:00:06 1970 +0000
  | | | |    summary:     (6) merge two known; one immediate left, one far left
  | | | |
  | o | |    changeset:   5:4409d547b708
  | |\ \ \   parent:      3:27eef8ed80b4
  | | | | |  parent:      4:26a8bac39d9f
  | | | | |  user:        test
  | | | | |  date:        Thu Jan 01 00:00:05 1970 +0000
  | | | | |  summary:     (5) expand
  | | | | |
  | | o | |  changeset:   4:26a8bac39d9f
  | |/|/ /   parent:      1:6db2ef61d156
  | | | |    parent:      3:27eef8ed80b4
  | | | |    user:        test
  | | | |    date:        Thu Jan 01 00:00:04 1970 +0000
  | | | |    summary:     (4) merge two known; one immediate left, one immediate right
  | | | |
  | o | |  changeset:   3:27eef8ed80b4
  |/ / /   user:        test
  | | |    date:        Thu Jan 01 00:00:03 1970 +0000
  | | |    summary:     (3) collapse
  | | |
  o | |  changeset:   2:3d9a33b8d1e1
  |/ /   user:        test
  | |    date:        Thu Jan 01 00:00:02 1970 +0000
  | |    summary:     (2) collapse
  | |
  o |  changeset:   1:6db2ef61d156
  |/   user:        test
  |    date:        Thu Jan 01 00:00:01 1970 +0000
  |    summary:     (1) collapse
  |
  o  changeset:   0:e6eb3150255d
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     (0) root
  

File glog per revset:

  $ hg log -G -r 'file("a")'
  @  changeset:   34:fea3ac5810e0
  |  tag:         tip
  |  parent:      32:d06dffa21a31
  |  user:        test
  |  date:        Thu Jan 01 00:00:34 1970 +0000
  |  summary:     (34) head
  |
  | o  changeset:   33:68608f5145f9
  | |  parent:      18:1aa84d96232a
  | |  user:        test
  | |  date:        Thu Jan 01 00:00:33 1970 +0000
  | |  summary:     (33) head
  | |
  o |    changeset:   32:d06dffa21a31
  |\ \   parent:      27:886ed638191b
  | | |  parent:      31:621d83e11f67
  | | |  user:        test
  | | |  date:        Thu Jan 01 00:00:32 1970 +0000
  | | |  summary:     (32) expand
  | | |
  | o |    changeset:   31:621d83e11f67
  | |\ \   parent:      21:d42a756af44d
  | | | |  parent:      30:6e11cd4b648f
  | | | |  user:        test
  | | | |  date:        Thu Jan 01 00:00:31 1970 +0000
  | | | |  summary:     (31) expand
  | | | |
  | | o |    changeset:   30:6e11cd4b648f
  | | |\ \   parent:      28:44ecd0b9ae99
  | | | | |  parent:      29:cd9bb2be7593
  | | | | |  user:        test
  | | | | |  date:        Thu Jan 01 00:00:30 1970 +0000
  | | | | |  summary:     (30) expand
  | | | | |
  | | | o |  changeset:   29:cd9bb2be7593
  | | | | |  parent:      0:e6eb3150255d
  | | | | |  user:        test
  | | | | |  date:        Thu Jan 01 00:00:29 1970 +0000
  | | | | |  summary:     (29) regular commit
  | | | | |
  | | o | |    changeset:   28:44ecd0b9ae99
  | | |\ \ \   parent:      1:6db2ef61d156
  | | | | | |  parent:      26:7f25b6c2f0b9
  | | | | | |  user:        test
  | | | | | |  date:        Thu Jan 01 00:00:28 1970 +0000
  | | | | | |  summary:     (28) merge zero known
  | | | | | |
  o | | | | |  changeset:   27:886ed638191b
  |/ / / / /   parent:      21:d42a756af44d
  | | | | |    user:        test
  | | | | |    date:        Thu Jan 01 00:00:27 1970 +0000
  | | | | |    summary:     (27) collapse
  | | | | |
  | | o---+  changeset:   26:7f25b6c2f0b9
  | | | | |  parent:      18:1aa84d96232a
  | | | | |  parent:      25:91da8ed57247
  | | | | |  user:        test
  | | | | |  date:        Thu Jan 01 00:00:26 1970 +0000
  | | | | |  summary:     (26) merge one known; far right
  | | | | |
  +---o | |  changeset:   25:91da8ed57247
  | | | | |  parent:      21:d42a756af44d
  | | | | |  parent:      24:a9c19a3d96b7
  | | | | |  user:        test
  | | | | |  date:        Thu Jan 01 00:00:25 1970 +0000
  | | | | |  summary:     (25) merge one known; far left
  | | | | |
  | | o | |  changeset:   24:a9c19a3d96b7
  | | |\| |  parent:      0:e6eb3150255d
  | | | | |  parent:      23:a01cddf0766d
  | | | | |  user:        test
  | | | | |  date:        Thu Jan 01 00:00:24 1970 +0000
  | | | | |  summary:     (24) merge one known; immediate right
  | | | | |
  | | o | |  changeset:   23:a01cddf0766d
  | |/| | |  parent:      1:6db2ef61d156
  | | | | |  parent:      22:e0d9cccacb5d
  | | | | |  user:        test
  | | | | |  date:        Thu Jan 01 00:00:23 1970 +0000
  | | | | |  summary:     (23) merge one known; immediate left
  | | | | |
  +---o---+  changeset:   22:e0d9cccacb5d
  | |   | |  parent:      18:1aa84d96232a
  | |  / /   parent:      21:d42a756af44d
  | | | |    user:        test
  | | | |    date:        Thu Jan 01 00:00:22 1970 +0000
  | | | |    summary:     (22) merge two known; one far left, one far right
  | | | |
  o | | |    changeset:   21:d42a756af44d
  |\ \ \ \   parent:      19:31ddc2c1573b
  | | | | |  parent:      20:d30ed6450e32
  | | | | |  user:        test
  | | | | |  date:        Thu Jan 01 00:00:21 1970 +0000
  | | | | |  summary:     (21) expand
  | | | | |
  | o---+-+  changeset:   20:d30ed6450e32
  |   | | |  parent:      0:e6eb3150255d
  |  / / /   parent:      18:1aa84d96232a
  | | | |    user:        test
  | | | |    date:        Thu Jan 01 00:00:20 1970 +0000
  | | | |    summary:     (20) merge two known; two far right
  | | | |
  o | | |    changeset:   19:31ddc2c1573b
  |\ \ \ \   parent:      15:1dda3f72782d
  | | | | |  parent:      17:44765d7c06e0
  | | | | |  user:        test
  | | | | |  date:        Thu Jan 01 00:00:19 1970 +0000
  | | | | |  summary:     (19) expand
  | | | | |
  +---+---o  changeset:   18:1aa84d96232a
  | | | |    parent:      1:6db2ef61d156
  | | | |    parent:      15:1dda3f72782d
  | | | |    user:        test
  | | | |    date:        Thu Jan 01 00:00:18 1970 +0000
  | | | |    summary:     (18) merge two known; two far left
  | | | |
  | o | |    changeset:   17:44765d7c06e0
  | |\ \ \   parent:      12:86b91144a6e9
  | | | | |  parent:      16:3677d192927d
  | | | | |  user:        test
  | | | | |  date:        Thu Jan 01 00:00:17 1970 +0000
  | | | | |  summary:     (17) expand
  | | | | |
  | | o---+  changeset:   16:3677d192927d
  | | | | |  parent:      0:e6eb3150255d
  | | |/ /   parent:      1:6db2ef61d156
  | | | |    user:        test
  | | | |    date:        Thu Jan 01 00:00:16 1970 +0000
  | | | |    summary:     (16) merge two known; one immediate right, one near right
  | | | |
  o | | |    changeset:   15:1dda3f72782d
  |\ \ \ \   parent:      13:22d8966a97e3
  | | | | |  parent:      14:8eac370358ef
  | | | | |  user:        test
  | | | | |  date:        Thu Jan 01 00:00:15 1970 +0000
  | | | | |  summary:     (15) expand
  | | | | |
  | o-----+  changeset:   14:8eac370358ef
  | | | | |  parent:      0:e6eb3150255d
  | |/ / /   parent:      12:86b91144a6e9
  | | | |    user:        test
  | | | |    date:        Thu Jan 01 00:00:14 1970 +0000
  | | | |    summary:     (14) merge two known; one immediate right, one far right
  | | | |
  o | | |    changeset:   13:22d8966a97e3
  |\ \ \ \   parent:      9:7010c0af0a35
  | | | | |  parent:      11:832d76e6bdf2
  | | | | |  user:        test
  | | | | |  date:        Thu Jan 01 00:00:13 1970 +0000
  | | | | |  summary:     (13) expand
  | | | | |
  +---o | |  changeset:   12:86b91144a6e9
  | | |/ /   parent:      1:6db2ef61d156
  | | | |    parent:      9:7010c0af0a35
  | | | |    user:        test
  | | | |    date:        Thu Jan 01 00:00:12 1970 +0000
  | | | |    summary:     (12) merge two known; one immediate right, one far left
  | | | |
  | o | |    changeset:   11:832d76e6bdf2
  | |\ \ \   parent:      6:b105a072e251
  | | | | |  parent:      10:74c64d036d72
  | | | | |  user:        test
  | | | | |  date:        Thu Jan 01 00:00:11 1970 +0000
  | | | | |  summary:     (11) expand
  | | | | |
  | | o---+  changeset:   10:74c64d036d72
  | | | | |  parent:      0:e6eb3150255d
  | |/ / /   parent:      6:b105a072e251
  | | | |    user:        test
  | | | |    date:        Thu Jan 01 00:00:10 1970 +0000
  | | | |    summary:     (10) merge two known; one immediate left, one near right
  | | | |
  o | | |    changeset:   9:7010c0af0a35
  |\ \ \ \   parent:      7:b632bb1b1224
  | | | | |  parent:      8:7a0b11f71937
  | | | | |  user:        test
  | | | | |  date:        Thu Jan 01 00:00:09 1970 +0000
  | | | | |  summary:     (9) expand
  | | | | |
  | o-----+  changeset:   8:7a0b11f71937
  | | | | |  parent:      0:e6eb3150255d
  |/ / / /   parent:      7:b632bb1b1224
  | | | |    user:        test
  | | | |    date:        Thu Jan 01 00:00:08 1970 +0000
  | | | |    summary:     (8) merge two known; one immediate left, one far right
  | | | |
  o | | |    changeset:   7:b632bb1b1224
  |\ \ \ \   parent:      2:3d9a33b8d1e1
  | | | | |  parent:      5:4409d547b708
  | | | | |  user:        test
  | | | | |  date:        Thu Jan 01 00:00:07 1970 +0000
  | | | | |  summary:     (7) expand
  | | | | |
  +---o | |  changeset:   6:b105a072e251
  | |/ / /   parent:      2:3d9a33b8d1e1
  | | | |    parent:      5:4409d547b708
  | | | |    user:        test
  | | | |    date:        Thu Jan 01 00:00:06 1970 +0000
  | | | |    summary:     (6) merge two known; one immediate left, one far left
  | | | |
  | o | |    changeset:   5:4409d547b708
  | |\ \ \   parent:      3:27eef8ed80b4
  | | | | |  parent:      4:26a8bac39d9f
  | | | | |  user:        test
  | | | | |  date:        Thu Jan 01 00:00:05 1970 +0000
  | | | | |  summary:     (5) expand
  | | | | |
  | | o | |  changeset:   4:26a8bac39d9f
  | |/|/ /   parent:      1:6db2ef61d156
  | | | |    parent:      3:27eef8ed80b4
  | | | |    user:        test
  | | | |    date:        Thu Jan 01 00:00:04 1970 +0000
  | | | |    summary:     (4) merge two known; one immediate left, one immediate right
  | | | |
  | o | |  changeset:   3:27eef8ed80b4
  |/ / /   user:        test
  | | |    date:        Thu Jan 01 00:00:03 1970 +0000
  | | |    summary:     (3) collapse
  | | |
  o | |  changeset:   2:3d9a33b8d1e1
  |/ /   user:        test
  | |    date:        Thu Jan 01 00:00:02 1970 +0000
  | |    summary:     (2) collapse
  | |
  o |  changeset:   1:6db2ef61d156
  |/   user:        test
  |    date:        Thu Jan 01 00:00:01 1970 +0000
  |    summary:     (1) collapse
  |
  o  changeset:   0:e6eb3150255d
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     (0) root
  


File glog per revset (only merges):

  $ hg log -G -r 'file("a")' -m
  o    changeset:   32:d06dffa21a31
  |\   parent:      27:886ed638191b
  | |  parent:      31:621d83e11f67
  | |  user:        test
  | |  date:        Thu Jan 01 00:00:32 1970 +0000
  | |  summary:     (32) expand
  | |
  o |  changeset:   31:621d83e11f67
  |\|  parent:      21:d42a756af44d
  | |  parent:      30:6e11cd4b648f
  | |  user:        test
  | |  date:        Thu Jan 01 00:00:31 1970 +0000
  | |  summary:     (31) expand
  | |
  o |    changeset:   30:6e11cd4b648f
  |\ \   parent:      28:44ecd0b9ae99
  | | |  parent:      29:cd9bb2be7593
  | | |  user:        test
  | | |  date:        Thu Jan 01 00:00:30 1970 +0000
  | | |  summary:     (30) expand
  | | |
  o | |    changeset:   28:44ecd0b9ae99
  |\ \ \   parent:      1:6db2ef61d156
  | | | |  parent:      26:7f25b6c2f0b9
  | | | |  user:        test
  | | | |  date:        Thu Jan 01 00:00:28 1970 +0000
  | | | |  summary:     (28) merge zero known
  | | | |
  o | | |    changeset:   26:7f25b6c2f0b9
  |\ \ \ \   parent:      18:1aa84d96232a
  | | | | |  parent:      25:91da8ed57247
  | | | | |  user:        test
  | | | | |  date:        Thu Jan 01 00:00:26 1970 +0000
  | | | | |  summary:     (26) merge one known; far right
  | | | | |
  | o-----+  changeset:   25:91da8ed57247
  | | | | |  parent:      21:d42a756af44d
  | | | | |  parent:      24:a9c19a3d96b7
  | | | | |  user:        test
  | | | | |  date:        Thu Jan 01 00:00:25 1970 +0000
  | | | | |  summary:     (25) merge one known; far left
  | | | | |
  | o | | |    changeset:   24:a9c19a3d96b7
  | |\ \ \ \   parent:      0:e6eb3150255d
  | | | | | |  parent:      23:a01cddf0766d
  | | | | | |  user:        test
  | | | | | |  date:        Thu Jan 01 00:00:24 1970 +0000
  | | | | | |  summary:     (24) merge one known; immediate right
  | | | | | |
  | o---+ | |  changeset:   23:a01cddf0766d
  | | | | | |  parent:      1:6db2ef61d156
  | | | | | |  parent:      22:e0d9cccacb5d
  | | | | | |  user:        test
  | | | | | |  date:        Thu Jan 01 00:00:23 1970 +0000
  | | | | | |  summary:     (23) merge one known; immediate left
  | | | | | |
  | o-------+  changeset:   22:e0d9cccacb5d
  | | | | | |  parent:      18:1aa84d96232a
  |/ / / / /   parent:      21:d42a756af44d
  | | | | |    user:        test
  | | | | |    date:        Thu Jan 01 00:00:22 1970 +0000
  | | | | |    summary:     (22) merge two known; one far left, one far right
  | | | | |
  | | | | o    changeset:   21:d42a756af44d
  | | | | |\   parent:      19:31ddc2c1573b
  | | | | | |  parent:      20:d30ed6450e32
  | | | | | |  user:        test
  | | | | | |  date:        Thu Jan 01 00:00:21 1970 +0000
  | | | | | |  summary:     (21) expand
  | | | | | |
  +-+-------o  changeset:   20:d30ed6450e32
  | | | | |    parent:      0:e6eb3150255d
  | | | | |    parent:      18:1aa84d96232a
  | | | | |    user:        test
  | | | | |    date:        Thu Jan 01 00:00:20 1970 +0000
  | | | | |    summary:     (20) merge two known; two far right
  | | | | |
  | | | | o    changeset:   19:31ddc2c1573b
  | | | | |\   parent:      15:1dda3f72782d
  | | | | | |  parent:      17:44765d7c06e0
  | | | | | |  user:        test
  | | | | | |  date:        Thu Jan 01 00:00:19 1970 +0000
  | | | | | |  summary:     (19) expand
  | | | | | |
  o---+---+ |  changeset:   18:1aa84d96232a
    | | | | |  parent:      1:6db2ef61d156
   / / / / /   parent:      15:1dda3f72782d
  | | | | |    user:        test
  | | | | |    date:        Thu Jan 01 00:00:18 1970 +0000
  | | | | |    summary:     (18) merge two known; two far left
  | | | | |
  | | | | o    changeset:   17:44765d7c06e0
  | | | | |\   parent:      12:86b91144a6e9
  | | | | | |  parent:      16:3677d192927d
  | | | | | |  user:        test
  | | | | | |  date:        Thu Jan 01 00:00:17 1970 +0000
  | | | | | |  summary:     (17) expand
  | | | | | |
  +-+-------o  changeset:   16:3677d192927d
  | | | | |    parent:      0:e6eb3150255d
  | | | | |    parent:      1:6db2ef61d156
  | | | | |    user:        test
  | | | | |    date:        Thu Jan 01 00:00:16 1970 +0000
  | | | | |    summary:     (16) merge two known; one immediate right, one near right
  | | | | |
  | | | o |    changeset:   15:1dda3f72782d
  | | | |\ \   parent:      13:22d8966a97e3
  | | | | | |  parent:      14:8eac370358ef
  | | | | | |  user:        test
  | | | | | |  date:        Thu Jan 01 00:00:15 1970 +0000
  | | | | | |  summary:     (15) expand
  | | | | | |
  +-------o |  changeset:   14:8eac370358ef
  | | | | |/   parent:      0:e6eb3150255d
  | | | | |    parent:      12:86b91144a6e9
  | | | | |    user:        test
  | | | | |    date:        Thu Jan 01 00:00:14 1970 +0000
  | | | | |    summary:     (14) merge two known; one immediate right, one far right
  | | | | |
  | | | o |    changeset:   13:22d8966a97e3
  | | | |\ \   parent:      9:7010c0af0a35
  | | | | | |  parent:      11:832d76e6bdf2
  | | | | | |  user:        test
  | | | | | |  date:        Thu Jan 01 00:00:13 1970 +0000
  | | | | | |  summary:     (13) expand
  | | | | | |
  | +---+---o  changeset:   12:86b91144a6e9
  | | | | |    parent:      1:6db2ef61d156
  | | | | |    parent:      9:7010c0af0a35
  | | | | |    user:        test
  | | | | |    date:        Thu Jan 01 00:00:12 1970 +0000
  | | | | |    summary:     (12) merge two known; one immediate right, one far left
  | | | | |
  | | | | o    changeset:   11:832d76e6bdf2
  | | | | |\   parent:      6:b105a072e251
  | | | | | |  parent:      10:74c64d036d72
  | | | | | |  user:        test
  | | | | | |  date:        Thu Jan 01 00:00:11 1970 +0000
  | | | | | |  summary:     (11) expand
  | | | | | |
  +---------o  changeset:   10:74c64d036d72
  | | | | |/   parent:      0:e6eb3150255d
  | | | | |    parent:      6:b105a072e251
  | | | | |    user:        test
  | | | | |    date:        Thu Jan 01 00:00:10 1970 +0000
  | | | | |    summary:     (10) merge two known; one immediate left, one near right
  | | | | |
  | | | o |    changeset:   9:7010c0af0a35
  | | | |\ \   parent:      7:b632bb1b1224
  | | | | | |  parent:      8:7a0b11f71937
  | | | | | |  user:        test
  | | | | | |  date:        Thu Jan 01 00:00:09 1970 +0000
  | | | | | |  summary:     (9) expand
  | | | | | |
  +-------o |  changeset:   8:7a0b11f71937
  | | | |/ /   parent:      0:e6eb3150255d
  | | | | |    parent:      7:b632bb1b1224
  | | | | |    user:        test
  | | | | |    date:        Thu Jan 01 00:00:08 1970 +0000
  | | | | |    summary:     (8) merge two known; one immediate left, one far right
  | | | | |
  | | | o |    changeset:   7:b632bb1b1224
  | | | |\ \   parent:      2:3d9a33b8d1e1
  | | | | | |  parent:      5:4409d547b708
  | | | | | |  user:        test
  | | | | | |  date:        Thu Jan 01 00:00:07 1970 +0000
  | | | | | |  summary:     (7) expand
  | | | | | |
  | | | +---o  changeset:   6:b105a072e251
  | | | | |/   parent:      2:3d9a33b8d1e1
  | | | | |    parent:      5:4409d547b708
  | | | | |    user:        test
  | | | | |    date:        Thu Jan 01 00:00:06 1970 +0000
  | | | | |    summary:     (6) merge two known; one immediate left, one far left
  | | | | |
  | | | o |    changeset:   5:4409d547b708
  | | | |\ \   parent:      3:27eef8ed80b4
  | | | | | |  parent:      4:26a8bac39d9f
  | | | | | |  user:        test
  | | | | | |  date:        Thu Jan 01 00:00:05 1970 +0000
  | | | | | |  summary:     (5) expand
  | | | | | |
  | +---o | |  changeset:   4:26a8bac39d9f
  | | | |/ /   parent:      1:6db2ef61d156
  | | | | |    parent:      3:27eef8ed80b4
  | | | | |    user:        test
  | | | | |    date:        Thu Jan 01 00:00:04 1970 +0000
  | | | | |    summary:     (4) merge two known; one immediate left, one immediate right
  | | | | |


Empty revision range - display nothing:
  $ hg log -G -r 1..0

  $ cd ..

#if no-outer-repo

From outer space:
  $ hg log -G -l1 repo
  @  changeset:   34:fea3ac5810e0
  |  tag:         tip
  |  parent:      32:d06dffa21a31
  |  user:        test
  |  date:        Thu Jan 01 00:00:34 1970 +0000
  |  summary:     (34) head
  |
  $ hg log -G -l1 repo/a
  @  changeset:   34:fea3ac5810e0
  |  tag:         tip
  |  parent:      32:d06dffa21a31
  |  user:        test
  |  date:        Thu Jan 01 00:00:34 1970 +0000
  |  summary:     (34) head
  |
  $ hg log -G -l1 repo/missing

#endif

File log with revs != cset revs:
  $ hg init flog
  $ cd flog
  $ echo one >one
  $ hg add one
  $ hg commit -mone
  $ echo two >two
  $ hg add two
  $ hg commit -mtwo
  $ echo more >two
  $ hg commit -mmore
  $ hg log -G two
  @  changeset:   2:12c28321755b
  |  tag:         tip
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     more
  |
  o  changeset:   1:5ac72c0599bf
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     two
  |

Issue1896: File log with explicit style
  $ hg log -G --style=default one
  o  changeset:   0:3d578b4a1f53
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     one
  
Issue2395: glog --style header and footer
  $ hg log -G --style=xml one
  <?xml version="1.0"?>
  <log>
  o  <logentry revision="0" node="3d578b4a1f537d5fcf7301bfa9c0b97adfaa6fb1">
     <author email="test">test</author>
     <date>1970-01-01T00:00:00+00:00</date>
     <msg xml:space="preserve">one</msg>
     </logentry>
  </log>

  $ cd ..

Incoming and outgoing:

  $ hg clone -U -r31 repo repo2
  adding changesets
  adding manifests
  adding file changes
  added 31 changesets with 31 changes to 1 files
  $ cd repo2

  $ hg incoming --graph ../repo
  comparing with ../repo
  searching for changes
  o  changeset:   34:fea3ac5810e0
  |  tag:         tip
  |  parent:      32:d06dffa21a31
  |  user:        test
  |  date:        Thu Jan 01 00:00:34 1970 +0000
  |  summary:     (34) head
  |
  | o  changeset:   33:68608f5145f9
  |    parent:      18:1aa84d96232a
  |    user:        test
  |    date:        Thu Jan 01 00:00:33 1970 +0000
  |    summary:     (33) head
  |
  o  changeset:   32:d06dffa21a31
  |  parent:      27:886ed638191b
  |  parent:      31:621d83e11f67
  |  user:        test
  |  date:        Thu Jan 01 00:00:32 1970 +0000
  |  summary:     (32) expand
  |
  o  changeset:   27:886ed638191b
     parent:      21:d42a756af44d
     user:        test
     date:        Thu Jan 01 00:00:27 1970 +0000
     summary:     (27) collapse
  
  $ cd ..

  $ hg -R repo outgoing --graph repo2
  comparing with repo2
  searching for changes
  @  changeset:   34:fea3ac5810e0
  |  tag:         tip
  |  parent:      32:d06dffa21a31
  |  user:        test
  |  date:        Thu Jan 01 00:00:34 1970 +0000
  |  summary:     (34) head
  |
  | o  changeset:   33:68608f5145f9
  |    parent:      18:1aa84d96232a
  |    user:        test
  |    date:        Thu Jan 01 00:00:33 1970 +0000
  |    summary:     (33) head
  |
  o  changeset:   32:d06dffa21a31
  |  parent:      27:886ed638191b
  |  parent:      31:621d83e11f67
  |  user:        test
  |  date:        Thu Jan 01 00:00:32 1970 +0000
  |  summary:     (32) expand
  |
  o  changeset:   27:886ed638191b
     parent:      21:d42a756af44d
     user:        test
     date:        Thu Jan 01 00:00:27 1970 +0000
     summary:     (27) collapse
  

File + limit with revs != cset revs:
  $ cd repo
  $ touch b
  $ hg ci -Aqm0
  $ hg log -G -l2 a
  o  changeset:   34:fea3ac5810e0
  |  parent:      32:d06dffa21a31
  |  user:        test
  |  date:        Thu Jan 01 00:00:34 1970 +0000
  |  summary:     (34) head
  |
  | o  changeset:   33:68608f5145f9
  | |  parent:      18:1aa84d96232a
  | |  user:        test
  | |  date:        Thu Jan 01 00:00:33 1970 +0000
  | |  summary:     (33) head
  | |

File + limit + -ra:b, (b - a) < limit:
  $ hg log -G -l3000 -r32:tip a
  o  changeset:   34:fea3ac5810e0
  |  parent:      32:d06dffa21a31
  |  user:        test
  |  date:        Thu Jan 01 00:00:34 1970 +0000
  |  summary:     (34) head
  |
  | o  changeset:   33:68608f5145f9
  | |  parent:      18:1aa84d96232a
  | |  user:        test
  | |  date:        Thu Jan 01 00:00:33 1970 +0000
  | |  summary:     (33) head
  | |
  o |    changeset:   32:d06dffa21a31
  |\ \   parent:      27:886ed638191b
  | | |  parent:      31:621d83e11f67
  | | |  user:        test
  | | |  date:        Thu Jan 01 00:00:32 1970 +0000
  | | |  summary:     (32) expand
  | | |

Point out a common and an uncommon unshown parent

  $ hg log -G -r 'rev(8) or rev(9)'
  o    changeset:   9:7010c0af0a35
  |\   parent:      7:b632bb1b1224
  | |  parent:      8:7a0b11f71937
  | |  user:        test
  | |  date:        Thu Jan 01 00:00:09 1970 +0000
  | |  summary:     (9) expand
  | |
  o |  changeset:   8:7a0b11f71937
  |\|  parent:      0:e6eb3150255d
  | |  parent:      7:b632bb1b1224
  | |  user:        test
  | |  date:        Thu Jan 01 00:00:08 1970 +0000
  | |  summary:     (8) merge two known; one immediate left, one far right
  | |

File + limit + -ra:b, b < tip:

  $ hg log -G -l1 -r32:34 a
  o  changeset:   34:fea3ac5810e0
  |  parent:      32:d06dffa21a31
  |  user:        test
  |  date:        Thu Jan 01 00:00:34 1970 +0000
  |  summary:     (34) head
  |

file(File) + limit + -ra:b, b < tip:

  $ hg log -G -l1 -r32:34 -r 'file("a")'
  o  changeset:   34:fea3ac5810e0
  |  parent:      32:d06dffa21a31
  |  user:        test
  |  date:        Thu Jan 01 00:00:34 1970 +0000
  |  summary:     (34) head
  |

limit(file(File) and a::b), b < tip:

  $ hg log -G -r 'limit(file("a") and 32::34, 1)'
  o    changeset:   32:d06dffa21a31
  |\   parent:      27:886ed638191b
  | |  parent:      31:621d83e11f67
  | |  user:        test
  | |  date:        Thu Jan 01 00:00:32 1970 +0000
  | |  summary:     (32) expand
  | |

File + limit + -ra:b, b < tip:

  $ hg log -G -r 'limit(file("a") and 34::32, 1)'

File + limit + -ra:b, b < tip, (b - a) < limit:

  $ hg log -G -l10 -r33:34 a
  o  changeset:   34:fea3ac5810e0
  |  parent:      32:d06dffa21a31
  |  user:        test
  |  date:        Thu Jan 01 00:00:34 1970 +0000
  |  summary:     (34) head
  |
  | o  changeset:   33:68608f5145f9
  | |  parent:      18:1aa84d96232a
  | |  user:        test
  | |  date:        Thu Jan 01 00:00:33 1970 +0000
  | |  summary:     (33) head
  | |

Do not crash or produce strange graphs if history is buggy

  $ hg branch branch
  marked working directory as branch branch
  (branches are permanent and global, did you want a bookmark?)
  $ commit 36 "buggy merge: identical parents" 35 35
  $ hg log -G -l5
  @  changeset:   36:08a19a744424
  |  branch:      branch
  |  tag:         tip
  |  parent:      35:9159c3644c5e
  |  parent:      35:9159c3644c5e
  |  user:        test
  |  date:        Thu Jan 01 00:00:36 1970 +0000
  |  summary:     (36) buggy merge: identical parents
  |
  o  changeset:   35:9159c3644c5e
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     0
  |
  o  changeset:   34:fea3ac5810e0
  |  parent:      32:d06dffa21a31
  |  user:        test
  |  date:        Thu Jan 01 00:00:34 1970 +0000
  |  summary:     (34) head
  |
  | o  changeset:   33:68608f5145f9
  | |  parent:      18:1aa84d96232a
  | |  user:        test
  | |  date:        Thu Jan 01 00:00:33 1970 +0000
  | |  summary:     (33) head
  | |
  o |    changeset:   32:d06dffa21a31
  |\ \   parent:      27:886ed638191b
  | | |  parent:      31:621d83e11f67
  | | |  user:        test
  | | |  date:        Thu Jan 01 00:00:32 1970 +0000
  | | |  summary:     (32) expand
  | | |

Test log -G options

  $ testlog() {
  >   hg log -G --print-revset "$@"
  >   hg log --template 'nodetag {rev}\n' "$@" | grep nodetag \
  >     | sed 's/.*nodetag/nodetag/' > log.nodes
  >   hg log -G --template 'nodetag {rev}\n' "$@" | grep nodetag \
  >     | sed 's/.*nodetag/nodetag/' > glog.nodes
  >   diff -u log.nodes glog.nodes | grep '^[-+@ ]' || :
  > }

glog always reorders nodes which explains the difference with log

  $ testlog -r 27 -r 25 -r 21 -r 34 -r 32 -r 31
  ['27', '25', '21', '34', '32', '31']
  []
  --- log.nodes	* (glob)
  +++ glog.nodes	* (glob)
  @@ -1,6 +1,6 @@
  -nodetag 27
  -nodetag 25
  -nodetag 21
   nodetag 34
   nodetag 32
   nodetag 31
  +nodetag 27
  +nodetag 25
  +nodetag 21
  $ testlog -u test -u not-a-user
  []
  (group
    (group
      (or
        (func
          ('symbol', 'user')
          ('string', 'test'))
        (func
          ('symbol', 'user')
          ('string', 'not-a-user')))))
  $ testlog -b not-a-branch
  abort: unknown revision 'not-a-branch'!
  abort: unknown revision 'not-a-branch'!
  abort: unknown revision 'not-a-branch'!
  $ testlog -b 35 -b 36 --only-branch branch
  []
  (group
    (group
      (or
        (func
          ('symbol', 'branch')
          ('string', 'default'))
        (func
          ('symbol', 'branch')
          ('string', 'branch'))
        (func
          ('symbol', 'branch')
          ('string', 'branch')))))
  $ testlog -k expand -k merge
  []
  (group
    (group
      (or
        (func
          ('symbol', 'keyword')
          ('string', 'expand'))
        (func
          ('symbol', 'keyword')
          ('string', 'merge')))))
  $ testlog --only-merges
  []
  (group
    (func
      ('symbol', 'merge')
      None))
  $ testlog --no-merges
  []
  (group
    (not
      (func
        ('symbol', 'merge')
        None)))
  $ testlog --date '2 0 to 4 0'
  []
  (group
    (func
      ('symbol', 'date')
      ('string', '2 0 to 4 0')))
  $ hg log -G -d 'brace ) in a date'
  abort: invalid date: 'brace ) in a date'
  [255]
  $ testlog --prune 31 --prune 32
  []
  (group
    (group
      (and
        (not
          (group
            (or
              ('string', '31')
              (func
                ('symbol', 'ancestors')
                ('string', '31')))))
        (not
          (group
            (or
              ('string', '32')
              (func
                ('symbol', 'ancestors')
                ('string', '32'))))))))

Dedicated repo for --follow and paths filtering. The g is crafted to
have 2 filelog topological heads in a linear changeset graph.

  $ cd ..
  $ hg init follow
  $ cd follow
  $ testlog --follow
  []
  []
  $ testlog -rnull
  ['null']
  []
  $ echo a > a
  $ echo aa > aa
  $ echo f > f
  $ hg ci -Am "add a" a aa f
  $ hg cp a b
  $ hg cp f g
  $ hg ci -m "copy a b"
  $ mkdir dir
  $ hg mv b dir
  $ echo g >> g
  $ echo f >> f
  $ hg ci -m "mv b dir/b"
  $ hg mv a b
  $ hg cp -f f g
  $ echo a > d
  $ hg add d
  $ hg ci -m "mv a b; add d"
  $ hg mv dir/b e
  $ hg ci -m "mv dir/b e"
  $ hg log -G --template '({rev}) {desc|firstline}\n'
  @  (4) mv dir/b e
  |
  o  (3) mv a b; add d
  |
  o  (2) mv b dir/b
  |
  o  (1) copy a b
  |
  o  (0) add a
  

  $ testlog a
  []
  (group
    (group
      (func
        ('symbol', 'filelog')
        ('string', 'a'))))
  $ testlog a b
  []
  (group
    (group
      (or
        (func
          ('symbol', 'filelog')
          ('string', 'a'))
        (func
          ('symbol', 'filelog')
          ('string', 'b')))))

Test falling back to slow path for non-existing files

  $ testlog a c
  []
  (group
    (func
      ('symbol', '_matchfiles')
      (list
        (list
          (list
            ('string', 'r:')
            ('string', 'd:relpath'))
          ('string', 'p:a'))
        ('string', 'p:c'))))

Test multiple --include/--exclude/paths

  $ testlog --include a --include e --exclude b --exclude e a e
  []
  (group
    (func
      ('symbol', '_matchfiles')
      (list
        (list
          (list
            (list
              (list
                (list
                  (list
                    ('string', 'r:')
                    ('string', 'd:relpath'))
                  ('string', 'p:a'))
                ('string', 'p:e'))
              ('string', 'i:a'))
            ('string', 'i:e'))
          ('string', 'x:b'))
        ('string', 'x:e'))))

Test glob expansion of pats

  $ expandglobs=`$PYTHON -c "import mercurial.util; \
  >   print mercurial.util.expandglobs and 'true' or 'false'"`
  $ if [ $expandglobs = "true" ]; then
  >    testlog 'a*';
  > else
  >    testlog a*;
  > fi;
  []
  (group
    (group
      (func
        ('symbol', 'filelog')
        ('string', 'aa'))))

Test --follow on a non-existent directory

  $ testlog -f dir
  abort: cannot follow file not in parent revision: "dir"
  abort: cannot follow file not in parent revision: "dir"
  abort: cannot follow file not in parent revision: "dir"

Test --follow on a directory

  $ hg up -q '.^'
  $ testlog -f dir
  []
  (group
    (and
      (func
        ('symbol', 'ancestors')
        ('symbol', '.'))
      (func
        ('symbol', '_matchfiles')
        (list
          (list
            ('string', 'r:')
            ('string', 'd:relpath'))
          ('string', 'p:dir')))))
  $ hg up -q tip

Test --follow on file not in parent revision

  $ testlog -f a
  abort: cannot follow file not in parent revision: "a"
  abort: cannot follow file not in parent revision: "a"
  abort: cannot follow file not in parent revision: "a"

Test --follow and patterns

  $ testlog -f 'glob:*'
  []
  (group
    (and
      (func
        ('symbol', 'ancestors')
        ('symbol', '.'))
      (func
        ('symbol', '_matchfiles')
        (list
          (list
            ('string', 'r:')
            ('string', 'd:relpath'))
          ('string', 'p:glob:*')))))

Test --follow on a single rename

  $ hg up -q 2
  $ testlog -f a
  []
  (group
    (group
      (func
        ('symbol', 'follow')
        ('string', 'a'))))

Test --follow and multiple renames

  $ hg up -q tip
  $ testlog -f e
  []
  (group
    (group
      (func
        ('symbol', 'follow')
        ('string', 'e'))))

Test --follow and multiple filelog heads

  $ hg up -q 2
  $ testlog -f g
  []
  (group
    (group
      (func
        ('symbol', 'follow')
        ('string', 'g'))))
  $ cat log.nodes
  nodetag 2
  nodetag 1
  nodetag 0
  $ hg up -q tip
  $ testlog -f g
  []
  (group
    (group
      (func
        ('symbol', 'follow')
        ('string', 'g'))))
  $ cat log.nodes
  nodetag 3
  nodetag 2
  nodetag 0

Test --follow and multiple files

  $ testlog -f g e
  []
  (group
    (group
      (or
        (func
          ('symbol', 'follow')
          ('string', 'g'))
        (func
          ('symbol', 'follow')
          ('string', 'e')))))
  $ cat log.nodes
  nodetag 4
  nodetag 3
  nodetag 2
  nodetag 1
  nodetag 0

Test --follow null parent

  $ hg up -q null
  $ testlog -f
  []
  []

Test --follow-first

  $ hg up -q 3
  $ echo ee > e
  $ hg ci -Am "add another e" e
  created new head
  $ hg merge --tool internal:other 4
  0 files updated, 1 files merged, 1 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ echo merge > e
  $ hg ci -m "merge 5 and 4"
  $ testlog --follow-first
  []
  (group
    (func
      ('symbol', '_firstancestors')
      (func
        ('symbol', 'rev')
        ('symbol', '6'))))

Cannot compare with log --follow-first FILE as it never worked

  $ hg log -G --print-revset --follow-first e
  []
  (group
    (group
      (func
        ('symbol', '_followfirst')
        ('string', 'e'))))
  $ hg log -G --follow-first e --template '{rev} {desc|firstline}\n'
  @    6 merge 5 and 4
  |\
  o |  5 add another e
  | |

Test --copies

  $ hg log -G --copies --template "{rev} {desc|firstline} \
  >   copies: {file_copies_switch}\n"
  @    6 merge 5 and 4   copies:
  |\
  | o  5 add another e   copies:
  | |
  o |  4 mv dir/b e   copies: e (dir/b)
  |/
  o  3 mv a b; add d   copies: b (a)g (f)
  |
  o  2 mv b dir/b   copies: dir/b (b)
  |
  o  1 copy a b   copies: b (a)g (f)
  |
  o  0 add a   copies:
  
Test "set:..." and parent revision

  $ hg up -q 4
  $ testlog "set:copied()"
  []
  (group
    (func
      ('symbol', '_matchfiles')
      (list
        (list
          ('string', 'r:')
          ('string', 'd:relpath'))
        ('string', 'p:set:copied()'))))
  $ testlog --include "set:copied()"
  []
  (group
    (func
      ('symbol', '_matchfiles')
      (list
        (list
          ('string', 'r:')
          ('string', 'd:relpath'))
        ('string', 'i:set:copied()'))))
  $ testlog -r "sort(file('set:copied()'), -rev)"
  ["sort(file('set:copied()'), -rev)"]
  []

Test --removed

  $ testlog --removed
  []
  []
  $ testlog --removed a
  []
  (group
    (func
      ('symbol', '_matchfiles')
      (list
        (list
          ('string', 'r:')
          ('string', 'd:relpath'))
        ('string', 'p:a'))))
  $ testlog --removed --follow a
  []
  (group
    (and
      (func
        ('symbol', 'ancestors')
        ('symbol', '.'))
      (func
        ('symbol', '_matchfiles')
        (list
          (list
            ('string', 'r:')
            ('string', 'd:relpath'))
          ('string', 'p:a')))))

Test --patch and --stat with --follow and --follow-first

  $ hg up -q 3
  $ hg log -G --git --patch b
  o  changeset:   1:216d4c92cf98
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     copy a b
  |
  |  diff --git a/a b/b
  |  copy from a
  |  copy to b
  |

  $ hg log -G --git --stat b
  o  changeset:   1:216d4c92cf98
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     copy a b
  |
  |   b |  0
  |   1 files changed, 0 insertions(+), 0 deletions(-)
  |

  $ hg log -G --git --patch --follow b
  o  changeset:   1:216d4c92cf98
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     copy a b
  |
  |  diff --git a/a b/b
  |  copy from a
  |  copy to b
  |
  o  changeset:   0:f8035bb17114
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     add a
  
     diff --git a/a b/a
     new file mode 100644
     --- /dev/null
     +++ b/a
     @@ -0,0 +1,1 @@
     +a
  

  $ hg log -G --git --stat --follow b
  o  changeset:   1:216d4c92cf98
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     copy a b
  |
  |   b |  0
  |   1 files changed, 0 insertions(+), 0 deletions(-)
  |
  o  changeset:   0:f8035bb17114
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     add a
  
      a |  1 +
      1 files changed, 1 insertions(+), 0 deletions(-)
  

  $ hg up -q 6
  $ hg log -G --git --patch --follow-first e
  @    changeset:   6:fc281d8ff18d
  |\   tag:         tip
  | |  parent:      5:99b31f1c2782
  | |  parent:      4:17d952250a9d
  | |  user:        test
  | |  date:        Thu Jan 01 00:00:00 1970 +0000
  | |  summary:     merge 5 and 4
  | |
  | |  diff --git a/e b/e
  | |  --- a/e
  | |  +++ b/e
  | |  @@ -1,1 +1,1 @@
  | |  -ee
  | |  +merge
  | |
  o |  changeset:   5:99b31f1c2782
  | |  parent:      3:5918b8d165d1
  | |  user:        test
  | |  date:        Thu Jan 01 00:00:00 1970 +0000
  | |  summary:     add another e
  | |
  | |  diff --git a/e b/e
  | |  new file mode 100644
  | |  --- /dev/null
  | |  +++ b/e
  | |  @@ -0,0 +1,1 @@
  | |  +ee
  | |

Test old-style --rev

  $ hg tag 'foo-bar'
  $ testlog -r 'foo-bar'
  ['foo-bar']
  []

Test --follow and forward --rev

  $ hg up -q 6
  $ echo g > g
  $ hg ci -Am 'add g' g
  created new head
  $ hg up -q 2
  $ hg log -G --template "{rev} {desc|firstline}\n"
  o  8 add g
  |
  | o  7 Added tag foo-bar for changeset fc281d8ff18d
  |/
  o    6 merge 5 and 4
  |\
  | o  5 add another e
  | |
  o |  4 mv dir/b e
  |/
  o  3 mv a b; add d
  |
  @  2 mv b dir/b
  |
  o  1 copy a b
  |
  o  0 add a
  
  $ hg archive -r 7 archive
  $ grep changessincelatesttag archive/.hg_archival.txt
  changessincelatesttag: 1
  $ rm -r archive

changessincelatesttag with no prior tag
  $ hg archive -r 4 archive
  $ grep changessincelatesttag archive/.hg_archival.txt
  changessincelatesttag: 5

  $ hg export 'all()'
  # HG changeset patch
  # User test
  # Date 0 0
  #      Thu Jan 01 00:00:00 1970 +0000
  # Node ID f8035bb17114da16215af3436ec5222428ace8ee
  # Parent  0000000000000000000000000000000000000000
  add a
  
  diff -r 000000000000 -r f8035bb17114 a
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +a
  diff -r 000000000000 -r f8035bb17114 aa
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/aa	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +aa
  diff -r 000000000000 -r f8035bb17114 f
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/f	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +f
  # HG changeset patch
  # User test
  # Date 0 0
  #      Thu Jan 01 00:00:00 1970 +0000
  # Node ID 216d4c92cf98ff2b4641d508b76b529f3d424c92
  # Parent  f8035bb17114da16215af3436ec5222428ace8ee
  copy a b
  
  diff -r f8035bb17114 -r 216d4c92cf98 b
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/b	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +a
  diff -r f8035bb17114 -r 216d4c92cf98 g
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/g	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +f
  # HG changeset patch
  # User test
  # Date 0 0
  #      Thu Jan 01 00:00:00 1970 +0000
  # Node ID bb573313a9e8349099b6ea2b2fb1fc7f424446f3
  # Parent  216d4c92cf98ff2b4641d508b76b529f3d424c92
  mv b dir/b
  
  diff -r 216d4c92cf98 -r bb573313a9e8 b
  --- a/b	Thu Jan 01 00:00:00 1970 +0000
  +++ /dev/null	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +0,0 @@
  -a
  diff -r 216d4c92cf98 -r bb573313a9e8 dir/b
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/dir/b	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +a
  diff -r 216d4c92cf98 -r bb573313a9e8 f
  --- a/f	Thu Jan 01 00:00:00 1970 +0000
  +++ b/f	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +1,2 @@
   f
  +f
  diff -r 216d4c92cf98 -r bb573313a9e8 g
  --- a/g	Thu Jan 01 00:00:00 1970 +0000
  +++ b/g	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +1,2 @@
   f
  +g
  # HG changeset patch
  # User test
  # Date 0 0
  #      Thu Jan 01 00:00:00 1970 +0000
  # Node ID 5918b8d165d1364e78a66d02e66caa0133c5d1ed
  # Parent  bb573313a9e8349099b6ea2b2fb1fc7f424446f3
  mv a b; add d
  
  diff -r bb573313a9e8 -r 5918b8d165d1 a
  --- a/a	Thu Jan 01 00:00:00 1970 +0000
  +++ /dev/null	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +0,0 @@
  -a
  diff -r bb573313a9e8 -r 5918b8d165d1 b
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/b	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +a
  diff -r bb573313a9e8 -r 5918b8d165d1 d
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/d	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +a
  diff -r bb573313a9e8 -r 5918b8d165d1 g
  --- a/g	Thu Jan 01 00:00:00 1970 +0000
  +++ b/g	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,2 +1,2 @@
   f
  -g
  +f
  # HG changeset patch
  # User test
  # Date 0 0
  #      Thu Jan 01 00:00:00 1970 +0000
  # Node ID 17d952250a9d03cc3dc77b199ab60e959b9b0260
  # Parent  5918b8d165d1364e78a66d02e66caa0133c5d1ed
  mv dir/b e
  
  diff -r 5918b8d165d1 -r 17d952250a9d dir/b
  --- a/dir/b	Thu Jan 01 00:00:00 1970 +0000
  +++ /dev/null	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +0,0 @@
  -a
  diff -r 5918b8d165d1 -r 17d952250a9d e
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/e	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +a
  # HG changeset patch
  # User test
  # Date 0 0
  #      Thu Jan 01 00:00:00 1970 +0000
  # Node ID 99b31f1c2782e2deb1723cef08930f70fc84b37b
  # Parent  5918b8d165d1364e78a66d02e66caa0133c5d1ed
  add another e
  
  diff -r 5918b8d165d1 -r 99b31f1c2782 e
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/e	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +ee
  # HG changeset patch
  # User test
  # Date 0 0
  #      Thu Jan 01 00:00:00 1970 +0000
  # Node ID fc281d8ff18d999ad6497b3d27390bcd695dcc73
  # Parent  99b31f1c2782e2deb1723cef08930f70fc84b37b
  # Parent  17d952250a9d03cc3dc77b199ab60e959b9b0260
  merge 5 and 4
  
  diff -r 99b31f1c2782 -r fc281d8ff18d dir/b
  --- a/dir/b	Thu Jan 01 00:00:00 1970 +0000
  +++ /dev/null	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +0,0 @@
  -a
  diff -r 99b31f1c2782 -r fc281d8ff18d e
  --- a/e	Thu Jan 01 00:00:00 1970 +0000
  +++ b/e	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +1,1 @@
  -ee
  +merge
  # HG changeset patch
  # User test
  # Date 0 0
  #      Thu Jan 01 00:00:00 1970 +0000
  # Node ID 02dbb8e276b8ab7abfd07cab50c901647e75c2dd
  # Parent  fc281d8ff18d999ad6497b3d27390bcd695dcc73
  Added tag foo-bar for changeset fc281d8ff18d
  
  diff -r fc281d8ff18d -r 02dbb8e276b8 .hgtags
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/.hgtags	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +fc281d8ff18d999ad6497b3d27390bcd695dcc73 foo-bar
  # HG changeset patch
  # User test
  # Date 0 0
  #      Thu Jan 01 00:00:00 1970 +0000
  # Node ID 24c2e826ddebf80f9dcd60b856bdb8e6715c5449
  # Parent  fc281d8ff18d999ad6497b3d27390bcd695dcc73
  add g
  
  diff -r fc281d8ff18d -r 24c2e826ddeb g
  --- a/g	Thu Jan 01 00:00:00 1970 +0000
  +++ b/g	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,2 +1,1 @@
  -f
  -f
  +g
  $ testlog --follow -r6 -r8 -r5 -r7 -r4
  ['6', '8', '5', '7', '4']
  (group
    (func
      ('symbol', 'descendants')
      (func
        ('symbol', 'rev')
        ('symbol', '6'))))

Test --follow-first and forward --rev

  $ testlog --follow-first -r6 -r8 -r5 -r7 -r4
  ['6', '8', '5', '7', '4']
  (group
    (func
      ('symbol', '_firstdescendants')
      (func
        ('symbol', 'rev')
        ('symbol', '6'))))
  --- log.nodes	* (glob)
  +++ glog.nodes	* (glob)
  @@ -1,3 +1,3 @@
  -nodetag 6
   nodetag 8
   nodetag 7
  +nodetag 6

Test --follow and backward --rev

  $ testlog --follow -r6 -r5 -r7 -r8 -r4
  ['6', '5', '7', '8', '4']
  (group
    (func
      ('symbol', 'ancestors')
      (func
        ('symbol', 'rev')
        ('symbol', '6'))))

Test --follow-first and backward --rev

  $ testlog --follow-first -r6 -r5 -r7 -r8 -r4
  ['6', '5', '7', '8', '4']
  (group
    (func
      ('symbol', '_firstancestors')
      (func
        ('symbol', 'rev')
        ('symbol', '6'))))

Test --follow with --rev of graphlog extension

  $ hg --config extensions.graphlog= glog -qfr1
  o  1:216d4c92cf98
  |
  o  0:f8035bb17114
  

Test subdir

  $ hg up -q 3
  $ cd dir
  $ testlog .
  []
  (group
    (func
      ('symbol', '_matchfiles')
      (list
        (list
          ('string', 'r:')
          ('string', 'd:relpath'))
        ('string', 'p:.'))))
  $ testlog ../b
  []
  (group
    (group
      (func
        ('symbol', 'filelog')
        ('string', '../b'))))
  $ testlog -f ../b
  []
  (group
    (group
      (func
        ('symbol', 'follow')
        ('string', 'b'))))
  $ cd ..

Test --hidden
 (enable obsolete)

  $ cat >> $HGRCPATH << EOF
  > [experimental]
  > evolution=createmarkers
  > EOF

  $ hg debugobsolete `hg id --debug -i -r 8`
  $ testlog
  []
  []
  $ testlog --hidden
  []
  []
  $ hg log -G --template '{rev} {desc}\n'
  o  7 Added tag foo-bar for changeset fc281d8ff18d
  |
  o    6 merge 5 and 4
  |\
  | o  5 add another e
  | |
  o |  4 mv dir/b e
  |/
  @  3 mv a b; add d
  |
  o  2 mv b dir/b
  |
  o  1 copy a b
  |
  o  0 add a
  

A template without trailing newline should do something sane

  $ hg log -G -r ::2 --template '{rev} {desc}'
  o  2 mv b dir/b
  |
  o  1 copy a b
  |
  o  0 add a
  

Extra newlines must be preserved

  $ hg log -G -r ::2 --template '\n{rev} {desc}\n\n'
  o
  |  2 mv b dir/b
  |
  o
  |  1 copy a b
  |
  o
     0 add a
  

The almost-empty template should do something sane too ...

  $ hg log -G -r ::2 --template '\n'
  o
  |
  o
  |
  o
  

issue3772

  $ hg log -G -r :null
  o  changeset:   0:f8035bb17114
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     add a
  |
  o  changeset:   -1:000000000000
     user:
     date:        Thu Jan 01 00:00:00 1970 +0000
  
  $ hg log -G -r null:null
  o  changeset:   -1:000000000000
     user:
     date:        Thu Jan 01 00:00:00 1970 +0000
  

should not draw line down to null due to the magic of fullreposet

  $ hg log -G -r 'all()' | tail -6
  |
  o  changeset:   0:f8035bb17114
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     add a
  

  $ hg log -G -r 'branch(default)' | tail -6
  |
  o  changeset:   0:f8035bb17114
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     add a
  

working-directory revision

  $ hg log -G -qr '. + wdir()'
  o  2147483647:ffffffffffff
  |
  @  3:5918b8d165d1
  |

  $ cd ..
