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


  $ "$TESTDIR/hghave" no-outer-repo || exit 80

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

  $ echo "[extensions]" >> $HGRCPATH
  $ echo "graphlog=" >> $HGRCPATH

  $ hg init repo
  $ cd repo

Empty repo:

  $ hg glog


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


  $ hg glog -q
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
  

  $ hg glog
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
  $ hg glog a
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
  | o |  changeset:   31:621d83e11f67
  | | |  parent:      21:d42a756af44d
  | | |  parent:      30:6e11cd4b648f
  | | |  user:        test
  | | |  date:        Thu Jan 01 00:00:31 1970 +0000
  | | |  summary:     (31) expand
  | | |
  | o |    changeset:   30:6e11cd4b648f
  | |\ \   parent:      28:44ecd0b9ae99
  | | | |  parent:      29:cd9bb2be7593
  | | | |  user:        test
  | | | |  date:        Thu Jan 01 00:00:30 1970 +0000
  | | | |  summary:     (30) expand
  | | | |
  | | o |  changeset:   29:cd9bb2be7593
  | | | |  parent:      0:e6eb3150255d
  | | | |  user:        test
  | | | |  date:        Thu Jan 01 00:00:29 1970 +0000
  | | | |  summary:     (29) regular commit
  | | | |
  | o | |  changeset:   28:44ecd0b9ae99
  | | | |  parent:      1:6db2ef61d156
  | | | |  parent:      26:7f25b6c2f0b9
  | | | |  user:        test
  | | | |  date:        Thu Jan 01 00:00:28 1970 +0000
  | | | |  summary:     (28) merge zero known
  | | | |
  o | | |  changeset:   27:886ed638191b
  | | | |  parent:      21:d42a756af44d
  | | | |  user:        test
  | | | |  date:        Thu Jan 01 00:00:27 1970 +0000
  | | | |  summary:     (27) collapse
  | | | |
  | o | |  changeset:   26:7f25b6c2f0b9
  | | | |  parent:      18:1aa84d96232a
  | | | |  parent:      25:91da8ed57247
  | | | |  user:        test
  | | | |  date:        Thu Jan 01 00:00:26 1970 +0000
  | | | |  summary:     (26) merge one known; far right
  | | | |
  | o | |  changeset:   25:91da8ed57247
  | | | |  parent:      21:d42a756af44d
  | | | |  parent:      24:a9c19a3d96b7
  | | | |  user:        test
  | | | |  date:        Thu Jan 01 00:00:25 1970 +0000
  | | | |  summary:     (25) merge one known; far left
  | | | |
  | o | |  changeset:   24:a9c19a3d96b7
  | | | |  parent:      0:e6eb3150255d
  | | | |  parent:      23:a01cddf0766d
  | | | |  user:        test
  | | | |  date:        Thu Jan 01 00:00:24 1970 +0000
  | | | |  summary:     (24) merge one known; immediate right
  | | | |
  | o | |  changeset:   23:a01cddf0766d
  | | | |  parent:      1:6db2ef61d156
  | | | |  parent:      22:e0d9cccacb5d
  | | | |  user:        test
  | | | |  date:        Thu Jan 01 00:00:23 1970 +0000
  | | | |  summary:     (23) merge one known; immediate left
  | | | |
  | o | |  changeset:   22:e0d9cccacb5d
  |/ / /   parent:      18:1aa84d96232a
  | | |    parent:      21:d42a756af44d
  | | |    user:        test
  | | |    date:        Thu Jan 01 00:00:22 1970 +0000
  | | |    summary:     (22) merge two known; one far left, one far right
  | | |
  o | |    changeset:   21:d42a756af44d
  |\ \ \   parent:      19:31ddc2c1573b
  | | | |  parent:      20:d30ed6450e32
  | | | |  user:        test
  | | | |  date:        Thu Jan 01 00:00:21 1970 +0000
  | | | |  summary:     (21) expand
  | | | |
  | o---+  changeset:   20:d30ed6450e32
  |   | |  parent:      0:e6eb3150255d
  |  / /   parent:      18:1aa84d96232a
  | | |    user:        test
  | | |    date:        Thu Jan 01 00:00:20 1970 +0000
  | | |    summary:     (20) merge two known; two far right
  | | |
  o | |    changeset:   19:31ddc2c1573b
  |\ \ \   parent:      15:1dda3f72782d
  | | | |  parent:      17:44765d7c06e0
  | | | |  user:        test
  | | | |  date:        Thu Jan 01 00:00:19 1970 +0000
  | | | |  summary:     (19) expand
  | | | |
  +-----o  changeset:   18:1aa84d96232a
  | | |    parent:      1:6db2ef61d156
  | | |    parent:      15:1dda3f72782d
  | | |    user:        test
  | | |    date:        Thu Jan 01 00:00:18 1970 +0000
  | | |    summary:     (18) merge two known; two far left
  | | |
  | o |    changeset:   17:44765d7c06e0
  | |\ \   parent:      12:86b91144a6e9
  | | | |  parent:      16:3677d192927d
  | | | |  user:        test
  | | | |  date:        Thu Jan 01 00:00:17 1970 +0000
  | | | |  summary:     (17) expand
  | | | |
  | | o |  changeset:   16:3677d192927d
  | | | |  parent:      0:e6eb3150255d
  | | | |  parent:      1:6db2ef61d156
  | | | |  user:        test
  | | | |  date:        Thu Jan 01 00:00:16 1970 +0000
  | | | |  summary:     (16) merge two known; one immediate right, one near right
  | | | |
  o | | |    changeset:   15:1dda3f72782d
  |\ \ \ \   parent:      13:22d8966a97e3
  | | | | |  parent:      14:8eac370358ef
  | | | | |  user:        test
  | | | | |  date:        Thu Jan 01 00:00:15 1970 +0000
  | | | | |  summary:     (15) expand
  | | | | |
  | o | | |  changeset:   14:8eac370358ef
  | |/ / /   parent:      0:e6eb3150255d
  | | | |    parent:      12:86b91144a6e9
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
  | |  / /   parent:      1:6db2ef61d156
  | | | |    parent:      9:7010c0af0a35
  | | | |    user:        test
  | | | |    date:        Thu Jan 01 00:00:12 1970 +0000
  | | | |    summary:     (12) merge two known; one immediate right, one far left
  | | | |
  | o | |  changeset:   11:832d76e6bdf2
  | | | |  parent:      6:b105a072e251
  | | | |  parent:      10:74c64d036d72
  | | | |  user:        test
  | | | |  date:        Thu Jan 01 00:00:11 1970 +0000
  | | | |  summary:     (11) expand
  | | | |
  | o | |  changeset:   10:74c64d036d72
  | | | |  parent:      0:e6eb3150255d
  | | | |  parent:      6:b105a072e251
  | | | |  user:        test
  | | | |  date:        Thu Jan 01 00:00:10 1970 +0000
  | | | |  summary:     (10) merge two known; one immediate left, one near right
  | | | |
  o | | |  changeset:   9:7010c0af0a35
  | | | |  parent:      7:b632bb1b1224
  | | | |  parent:      8:7a0b11f71937
  | | | |  user:        test
  | | | |  date:        Thu Jan 01 00:00:09 1970 +0000
  | | | |  summary:     (9) expand
  | | | |
  o | | |  changeset:   8:7a0b11f71937
  | | | |  parent:      0:e6eb3150255d
  | | | |  parent:      7:b632bb1b1224
  | | | |  user:        test
  | | | |  date:        Thu Jan 01 00:00:08 1970 +0000
  | | | |  summary:     (8) merge two known; one immediate left, one far right
  | | | |
  o | | |  changeset:   7:b632bb1b1224
  | | | |  parent:      2:3d9a33b8d1e1
  | | | |  parent:      5:4409d547b708
  | | | |  user:        test
  | | | |  date:        Thu Jan 01 00:00:07 1970 +0000
  | | | |  summary:     (7) expand
  | | | |
  | o | |  changeset:   6:b105a072e251
  |/ / /   parent:      2:3d9a33b8d1e1
  | | |    parent:      5:4409d547b708
  | | |    user:        test
  | | |    date:        Thu Jan 01 00:00:06 1970 +0000
  | | |    summary:     (6) merge two known; one immediate left, one far left
  | | |
  o | |  changeset:   5:4409d547b708
  | | |  parent:      3:27eef8ed80b4
  | | |  parent:      4:26a8bac39d9f
  | | |  user:        test
  | | |  date:        Thu Jan 01 00:00:05 1970 +0000
  | | |  summary:     (5) expand
  | | |
  o | |  changeset:   4:26a8bac39d9f
  | | |  parent:      1:6db2ef61d156
  | | |  parent:      3:27eef8ed80b4
  | | |  user:        test
  | | |  date:        Thu Jan 01 00:00:04 1970 +0000
  | | |  summary:     (4) merge two known; one immediate left, one immediate right
  | | |
  o | |  changeset:   3:27eef8ed80b4
  | | |  user:        test
  | | |  date:        Thu Jan 01 00:00:03 1970 +0000
  | | |  summary:     (3) collapse
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
  

Unused arguments:
  $ hg glog -q foo bar
  hg glog: invalid arguments
  hg glog [OPTION]... [FILE]
  
  show revision history alongside an ASCII revision graph
  [255]

Empty revision range - display nothing:
  $ hg glog -r 1..0

From outer space:
  $ cd ..
  $ hg glog -l1 repo
  @  changeset:   34:fea3ac5810e0
  |  tag:         tip
  |  parent:      32:d06dffa21a31
  |  user:        test
  |  date:        Thu Jan 01 00:00:34 1970 +0000
  |  summary:     (34) head
  |
  $ hg glog -l1 repo/a
  @  changeset:   34:fea3ac5810e0
  |  tag:         tip
  |  parent:      32:d06dffa21a31
  |  user:        test
  |  date:        Thu Jan 01 00:00:34 1970 +0000
  |  summary:     (34) head
  |
  $ hg glog -l1 repo/missing

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
  $ hg glog two
  @  changeset:   2:12c28321755b
  |  tag:         tip
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     more
  |
  o  changeset:   1:5ac72c0599bf
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     two
  

Issue1896: File log with explicit style
  $ hg glog --style=default one
  o  changeset:   0:3d578b4a1f53
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     one
  
Issue2395: glog --style header and footer
  $ hg glog --style=xml one
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
  $ hg glog -l2 a
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
  $ hg glog -l3000 -r32:tip a
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

File + limit + -ra:b, b < tip:
  $ hg glog -l1 -r32:34 a
  o  changeset:   34:fea3ac5810e0
  |  parent:      32:d06dffa21a31
  |  user:        test
  |  date:        Thu Jan 01 00:00:34 1970 +0000
  |  summary:     (34) head
  |

File + limit + -ra:b, b < tip, (b - a) < limit:
  $ hg glog -l10 -r33:34 a
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

  $ commit 36 "buggy merge: identical parents" 35 35
  $ hg glog -l5
  @  changeset:   36:95fa8febd08a
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
