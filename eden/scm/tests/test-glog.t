#debugruntest-compatible
# Copyright (c) Meta Platforms, Inc. and affiliates.
# Copyright (c) Mercurial Contributors.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.


# @  (34) head
# |
# | o  (33) head
# | |
# o |    (32) expand
# |\ \
# | o \    (31) expand
# | |\ \
# | | o \    (30) expand
# | | |\ \
# | | | o |  (29) regular commit
# | | | | |
# | | o | |    (28) merge zero known
# | | |\ \ \
# o | | | | |  (27) collapse
# |/ / / / /
# | | o---+  (26) merge one known; far right
# | | | | |
# +---o | |  (25) merge one known; far left
# | | | | |
# | | o | |  (24) merge one known; immediate right
# | | |\| |
# | | o | |  (23) merge one known; immediate left
# | |/| | |
# +---o---+  (22) merge two known; one far left, one far right
# | |  / /
# o | | |    (21) expand
# |\ \ \ \
# | o---+-+  (20) merge two known; two far right
# |  / / /
# o | | |    (19) expand
# |\ \ \ \
# +---+---o  (18) merge two known; two far left
# | | | |
# | o | |    (17) expand
# | |\ \ \
# | | o---+  (16) merge two known; one immediate right, one near right
# | | |/ /
# o | | |    (15) expand
# |\ \ \ \
# | o-----+  (14) merge two known; one immediate right, one far right
# | |/ / /
# o | | |    (13) expand
# |\ \ \ \
# +---o | |  (12) merge two known; one immediate right, one far left
# | | |/ /
# | o | |    (11) expand
# | |\ \ \
# | | o---+  (10) merge two known; one immediate left, one near right
# | |/ / /
# o | | |    (9) expand
# |\ \ \ \
# | o-----+  (8) merge two known; one immediate left, one far right
# |/ / / /
# o | | |    (7) expand
# |\ \ \ \
# +---o | |  (6) merge two known; one immediate left, one far left
# | |/ / /
# | o | |    (5) expand
# | |\ \ \
# | | o | |  (4) merge two known; one immediate left, one immediate right
# | |/|/ /
# | o / /  (3) collapse
# |/ / /
# o / /  (2) collapse
# |/ /
# o /  (1) collapse
# |/
# o  (0) root

  $ setconfig format.use-segmented-changelog=true
  $ setconfig devel.segmented-changelog-rev-compat=true
  $ commit() {
  >   rev=$1
  >   msg=$2
  >   shift 2
  >   if [ "$#" -gt 0 ]; then
  >       hg debugsetparents "$@"
  >   fi
  >   echo $rev > a
  >   hg commit -Aqd "$rev 0" -m "($rev) $msg"
  > }

  $ cat > printrevset.py << 'EOF'
  > from __future__ import absolute_import
  > from edenscm import (
  >   cmdutil,
  >   commands,
  >   extensions,
  >   revsetlang,
  > )
  > def uisetup(ui):
  >     def printrevset(orig, ui, repo, *pats, **opts):
  >         if opts.get('print_revset'):
  >             expr = cmdutil.getgraphlogrevs(repo, pats, opts)[1]
  >             if expr:
  >                 tree = revsetlang.parse(expr)
  >             else:
  >                 tree = []
  >             ui.write('%r\n' % (opts.get('rev', []),))
  >             ui.write(revsetlang.prettyformat(tree) + '\n')
  >             return 0
  >         return orig(ui, repo, *pats, **opts)
  >     entry = extensions.wrapcommand(commands.table, 'log', printrevset)
  >     entry[1].append(('', 'print-revset', False,
  >                      'print generated revset and exit (DEPRECATED)'))
  > EOF

  $ echo '[extensions]' >> $HGRCPATH
  $ echo "printrevset=$TESTTMP/printrevset.py" >> $HGRCPATH

  $ hg init repo
  $ cd repo

# Empty repo:

  $ hg log -G

# Building DAG:

  $ commit 0 root
  $ commit 1 collapse 0
  $ commit 2 collapse 1
  $ commit 3 collapse 2
  $ commit 4 'merge two known; one immediate left, one immediate right' 1 3
  $ commit 5 expand 3 4
  $ commit 6 'merge two known; one immediate left, one far left' 2 5
  $ commit 7 expand 2 5
  $ commit 8 'merge two known; one immediate left, one far right' 0 7
  $ commit 9 expand 7 8
  $ commit 10 'merge two known; one immediate left, one near right' 0 6
  $ commit 11 expand 6 10
  $ commit 12 'merge two known; one immediate right, one far left' 1 9
  $ commit 13 expand 9 11
  $ commit 14 'merge two known; one immediate right, one far right' 0 12
  $ commit 15 expand 13 14
  $ commit 16 'merge two known; one immediate right, one near right' 0 1
  $ commit 17 expand 12 16
  $ commit 18 'merge two known; two far left' 1 15
  $ commit 19 expand 15 17
  $ commit 20 'merge two known; two far right' 0 18
  $ commit 21 expand 19 20
  $ commit 22 'merge two known; one far left, one far right' 18 21
  $ commit 23 'merge one known; immediate left' 1 22
  $ commit 24 'merge one known; immediate right' 0 23
  $ commit 25 'merge one known; far left' 21 24
  $ commit 26 'merge one known; far right' 18 25
  $ commit 27 collapse 21
  $ commit 28 'merge zero known' 1 26
  $ commit 29 'regular commit' 0
  $ commit 30 expand 28 29
  $ commit 31 expand 21 30
  $ commit 32 expand 27 31
  $ commit 33 head 18
  $ commit 34 head 32

  $ hg log -G -q
  @  fea3ac5810e0
  │
  │ o  68608f5145f9
  │ │
  o │    d06dffa21a31
  ├───╮
  │ │ o    621d83e11f67
  │ │ ├─╮
  │ │ │ o    6e11cd4b648f
  │ │ │ ├─╮
  │ │ │ │ o  cd9bb2be7593
  │ │ │ │ │
  │ │ │ o │    44ecd0b9ae99
  │ │ │ ├───╮
  o │ │ │ │ │  886ed638191b
  ├───╯ │ │ │
  │ │   │ │ o  7f25b6c2f0b9
  │ ╭───────┤
  │ │   │ │ o  91da8ed57247
  ╭─────────┤
  │ │   │ │ o  a9c19a3d96b7
  │ │   │ ╭─┤
  │ │   │ │ o  a01cddf0766d
  │ │   ╭───┤
  │ │   │ │ o  e0d9cccacb5d
  ╭─┬───────╯
  o │   │ │  d42a756af44d
  ├───╮ │ │
  │ │ o │ │  d30ed6450e32
  │ ╭─┴───╮
  o │   │ │  31ddc2c1573b
  ├───╮ │ │
  │ o │ │ │  1aa84d96232a
  ╭─┴───╮ │
  │   o │ │  44765d7c06e0
  │ ╭─┤ │ │
  │ o │ │ │  3677d192927d
  │ ╰───┬─╮
  o   │ │ │  1dda3f72782d
  ├─╮ │ │ │
  │ o │ │ │  8eac370358ef
  │ ╰─┬───╮
  o   │ │ │  22d8966a97e3
  ├─╮ │ │ │
  │ │ o │ │  86b91144a6e9
  ╭───┴─╮ │
  │ o   │ │  832d76e6bdf2
  │ ├─╮ │ │
  │ │ o │ │  74c64d036d72
  │ ╭─┴───╮
  o │   │ │  7010c0af0a35
  ├───╮ │ │
  │ │ o │ │  7a0b11f71937
  ╭───┴───╮
  o │   │ │  b632bb1b1224
  ├───╮ │ │
  │ o │ │ │  b105a072e251
  ╭─┴─╮ │ │
  │   o │ │  4409d547b708
  │ ╭─┤ │ │
  │ o │ │ │  26a8bac39d9f
  │ ╰─┬─╮ │
  │   o │ │  27eef8ed80b4
  ├───╯ │ │
  o     │ │  3d9a33b8d1e1
  ├─────╯ │
  o       │  6db2ef61d156
  ├───────╯
  o  e6eb3150255d

  $ hg log -G
  @  commit:      fea3ac5810e0
  │  user:        test
  │  date:        Thu Jan 01 00:00:34 1970 +0000
  │  summary:     (34) head
  │
  │ o  commit:      68608f5145f9
  │ │  user:        test
  │ │  date:        Thu Jan 01 00:00:33 1970 +0000
  │ │  summary:     (33) head
  │ │
  o │    commit:      d06dffa21a31
  ├───╮  user:        test
  │ │ │  date:        Thu Jan 01 00:00:32 1970 +0000
  │ │ │  summary:     (32) expand
  │ │ │
  │ │ o    commit:      621d83e11f67
  │ │ ├─╮  user:        test
  │ │ │ │  date:        Thu Jan 01 00:00:31 1970 +0000
  │ │ │ │  summary:     (31) expand
  │ │ │ │
  │ │ │ o    commit:      6e11cd4b648f
  │ │ │ ├─╮  user:        test
  │ │ │ │ │  date:        Thu Jan 01 00:00:30 1970 +0000
  │ │ │ │ │  summary:     (30) expand
  │ │ │ │ │
  │ │ │ │ o  commit:      cd9bb2be7593
  │ │ │ │ │  user:        test
  │ │ │ │ │  date:        Thu Jan 01 00:00:29 1970 +0000
  │ │ │ │ │  summary:     (29) regular commit
  │ │ │ │ │
  │ │ │ o │    commit:      44ecd0b9ae99
  │ │ │ ├───╮  user:        test
  │ │ │ │ │ │  date:        Thu Jan 01 00:00:28 1970 +0000
  │ │ │ │ │ │  summary:     (28) merge zero known
  │ │ │ │ │ │
  o │ │ │ │ │  commit:      886ed638191b
  ├───╯ │ │ │  user:        test
  │ │   │ │ │  date:        Thu Jan 01 00:00:27 1970 +0000
  │ │   │ │ │  summary:     (27) collapse
  │ │   │ │ │
  │ │   │ │ o  commit:      7f25b6c2f0b9
  │ ╭───────┤  user:        test
  │ │   │ │ │  date:        Thu Jan 01 00:00:26 1970 +0000
  │ │   │ │ │  summary:     (26) merge one known; far right
  │ │   │ │ │
  │ │   │ │ o  commit:      91da8ed57247
  ╭─────────┤  user:        test
  │ │   │ │ │  date:        Thu Jan 01 00:00:25 1970 +0000
  │ │   │ │ │  summary:     (25) merge one known; far left
  │ │   │ │ │
  │ │   │ │ o  commit:      a9c19a3d96b7
  │ │   │ ╭─┤  user:        test
  │ │   │ │ │  date:        Thu Jan 01 00:00:24 1970 +0000
  │ │   │ │ │  summary:     (24) merge one known; immediate right
  │ │   │ │ │
  │ │   │ │ o  commit:      a01cddf0766d
  │ │   ╭───┤  user:        test
  │ │   │ │ │  date:        Thu Jan 01 00:00:23 1970 +0000
  │ │   │ │ │  summary:     (23) merge one known; immediate left
  │ │   │ │ │
  │ │   │ │ o  commit:      e0d9cccacb5d
  ╭─┬───────╯  user:        test
  │ │   │ │    date:        Thu Jan 01 00:00:22 1970 +0000
  │ │   │ │    summary:     (22) merge two known; one far left, one far right
  │ │   │ │
  o │   │ │  commit:      d42a756af44d
  ├───╮ │ │  user:        test
  │ │ │ │ │  date:        Thu Jan 01 00:00:21 1970 +0000
  │ │ │ │ │  summary:     (21) expand
  │ │ │ │ │
  │ │ o │ │  commit:      d30ed6450e32
  │ ╭─┴───╮  user:        test
  │ │   │ │  date:        Thu Jan 01 00:00:20 1970 +0000
  │ │   │ │  summary:     (20) merge two known; two far right
  │ │   │ │
  o │   │ │  commit:      31ddc2c1573b
  ├───╮ │ │  user:        test
  │ │ │ │ │  date:        Thu Jan 01 00:00:19 1970 +0000
  │ │ │ │ │  summary:     (19) expand
  │ │ │ │ │
  │ o │ │ │  commit:      1aa84d96232a
  ╭─┴───╮ │  user:        test
  │   │ │ │  date:        Thu Jan 01 00:00:18 1970 +0000
  │   │ │ │  summary:     (18) merge two known; two far left
  │   │ │ │
  │   o │ │  commit:      44765d7c06e0
  │ ╭─┤ │ │  user:        test
  │ │ │ │ │  date:        Thu Jan 01 00:00:17 1970 +0000
  │ │ │ │ │  summary:     (17) expand
  │ │ │ │ │
  │ o │ │ │  commit:      3677d192927d
  │ ╰───┬─╮  user:        test
  │   │ │ │  date:        Thu Jan 01 00:00:16 1970 +0000
  │   │ │ │  summary:     (16) merge two known; one immediate right, one near right
  │   │ │ │
  o   │ │ │  commit:      1dda3f72782d
  ├─╮ │ │ │  user:        test
  │ │ │ │ │  date:        Thu Jan 01 00:00:15 1970 +0000
  │ │ │ │ │  summary:     (15) expand
  │ │ │ │ │
  │ o │ │ │  commit:      8eac370358ef
  │ ╰─┬───╮  user:        test
  │   │ │ │  date:        Thu Jan 01 00:00:14 1970 +0000
  │   │ │ │  summary:     (14) merge two known; one immediate right, one far right
  │   │ │ │
  o   │ │ │  commit:      22d8966a97e3
  ├─╮ │ │ │  user:        test
  │ │ │ │ │  date:        Thu Jan 01 00:00:13 1970 +0000
  │ │ │ │ │  summary:     (13) expand
  │ │ │ │ │
  │ │ o │ │  commit:      86b91144a6e9
  ╭───┴─╮ │  user:        test
  │ │   │ │  date:        Thu Jan 01 00:00:12 1970 +0000
  │ │   │ │  summary:     (12) merge two known; one immediate right, one far left
  │ │   │ │
  │ o   │ │  commit:      832d76e6bdf2
  │ ├─╮ │ │  user:        test
  │ │ │ │ │  date:        Thu Jan 01 00:00:11 1970 +0000
  │ │ │ │ │  summary:     (11) expand
  │ │ │ │ │
  │ │ o │ │  commit:      74c64d036d72
  │ ╭─┴───╮  user:        test
  │ │   │ │  date:        Thu Jan 01 00:00:10 1970 +0000
  │ │   │ │  summary:     (10) merge two known; one immediate left, one near right
  │ │   │ │
  o │   │ │  commit:      7010c0af0a35
  ├───╮ │ │  user:        test
  │ │ │ │ │  date:        Thu Jan 01 00:00:09 1970 +0000
  │ │ │ │ │  summary:     (9) expand
  │ │ │ │ │
  │ │ o │ │  commit:      7a0b11f71937
  ╭───┴───╮  user:        test
  │ │   │ │  date:        Thu Jan 01 00:00:08 1970 +0000
  │ │   │ │  summary:     (8) merge two known; one immediate left, one far right
  │ │   │ │
  o │   │ │  commit:      b632bb1b1224
  ├───╮ │ │  user:        test
  │ │ │ │ │  date:        Thu Jan 01 00:00:07 1970 +0000
  │ │ │ │ │  summary:     (7) expand
  │ │ │ │ │
  │ o │ │ │  commit:      b105a072e251
  ╭─┴─╮ │ │  user:        test
  │   │ │ │  date:        Thu Jan 01 00:00:06 1970 +0000
  │   │ │ │  summary:     (6) merge two known; one immediate left, one far left
  │   │ │ │
  │   o │ │  commit:      4409d547b708
  │ ╭─┤ │ │  user:        test
  │ │ │ │ │  date:        Thu Jan 01 00:00:05 1970 +0000
  │ │ │ │ │  summary:     (5) expand
  │ │ │ │ │
  │ o │ │ │  commit:      26a8bac39d9f
  │ ╰─┬─╮ │  user:        test
  │   │ │ │  date:        Thu Jan 01 00:00:04 1970 +0000
  │   │ │ │  summary:     (4) merge two known; one immediate left, one immediate right
  │   │ │ │
  │   o │ │  commit:      27eef8ed80b4
  ├───╯ │ │  user:        test
  │     │ │  date:        Thu Jan 01 00:00:03 1970 +0000
  │     │ │  summary:     (3) collapse
  │     │ │
  o     │ │  commit:      3d9a33b8d1e1
  ├─────╯ │  user:        test
  │       │  date:        Thu Jan 01 00:00:02 1970 +0000
  │       │  summary:     (2) collapse
  │       │
  o       │  commit:      6db2ef61d156
  ├───────╯  user:        test
  │          date:        Thu Jan 01 00:00:01 1970 +0000
  │          summary:     (1) collapse
  │
  o  commit:      e6eb3150255d
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     (0) root

# File glog:

  $ hg log -G a
  @  commit:      fea3ac5810e0
  │  user:        test
  │  date:        Thu Jan 01 00:00:34 1970 +0000
  │  summary:     (34) head
  │
  │ o  commit:      68608f5145f9
  │ │  user:        test
  │ │  date:        Thu Jan 01 00:00:33 1970 +0000
  │ │  summary:     (33) head
  │ │
  o │    commit:      d06dffa21a31
  ├───╮  user:        test
  │ │ │  date:        Thu Jan 01 00:00:32 1970 +0000
  │ │ │  summary:     (32) expand
  │ │ │
  │ │ o    commit:      621d83e11f67
  │ │ ├─╮  user:        test
  │ │ │ │  date:        Thu Jan 01 00:00:31 1970 +0000
  │ │ │ │  summary:     (31) expand
  │ │ │ │
  │ │ │ o    commit:      6e11cd4b648f
  │ │ │ ├─╮  user:        test
  │ │ │ │ │  date:        Thu Jan 01 00:00:30 1970 +0000
  │ │ │ │ │  summary:     (30) expand
  │ │ │ │ │
  │ │ │ │ o  commit:      cd9bb2be7593
  │ │ │ │ │  user:        test
  │ │ │ │ │  date:        Thu Jan 01 00:00:29 1970 +0000
  │ │ │ │ │  summary:     (29) regular commit
  │ │ │ │ │
  │ │ │ o │    commit:      44ecd0b9ae99
  │ │ │ ├───╮  user:        test
  │ │ │ │ │ │  date:        Thu Jan 01 00:00:28 1970 +0000
  │ │ │ │ │ │  summary:     (28) merge zero known
  │ │ │ │ │ │
  o │ │ │ │ │  commit:      886ed638191b
  ├───╯ │ │ │  user:        test
  │ │   │ │ │  date:        Thu Jan 01 00:00:27 1970 +0000
  │ │   │ │ │  summary:     (27) collapse
  │ │   │ │ │
  │ │   │ │ o  commit:      7f25b6c2f0b9
  │ ╭───────┤  user:        test
  │ │   │ │ │  date:        Thu Jan 01 00:00:26 1970 +0000
  │ │   │ │ │  summary:     (26) merge one known; far right
  │ │   │ │ │
  │ │   │ │ o  commit:      91da8ed57247
  ╭─────────┤  user:        test
  │ │   │ │ │  date:        Thu Jan 01 00:00:25 1970 +0000
  │ │   │ │ │  summary:     (25) merge one known; far left
  │ │   │ │ │
  │ │   │ │ o  commit:      a9c19a3d96b7
  │ │   │ ╭─┤  user:        test
  │ │   │ │ │  date:        Thu Jan 01 00:00:24 1970 +0000
  │ │   │ │ │  summary:     (24) merge one known; immediate right
  │ │   │ │ │
  │ │   │ │ o  commit:      a01cddf0766d
  │ │   ╭───┤  user:        test
  │ │   │ │ │  date:        Thu Jan 01 00:00:23 1970 +0000
  │ │   │ │ │  summary:     (23) merge one known; immediate left
  │ │   │ │ │
  │ │   │ │ o  commit:      e0d9cccacb5d
  ╭─┬───────╯  user:        test
  │ │   │ │    date:        Thu Jan 01 00:00:22 1970 +0000
  │ │   │ │    summary:     (22) merge two known; one far left, one far right
  │ │   │ │
  o │   │ │  commit:      d42a756af44d
  ├───╮ │ │  user:        test
  │ │ │ │ │  date:        Thu Jan 01 00:00:21 1970 +0000
  │ │ │ │ │  summary:     (21) expand
  │ │ │ │ │
  │ │ o │ │  commit:      d30ed6450e32
  │ ╭─┴───╮  user:        test
  │ │   │ │  date:        Thu Jan 01 00:00:20 1970 +0000
  │ │   │ │  summary:     (20) merge two known; two far right
  │ │   │ │
  o │   │ │  commit:      31ddc2c1573b
  ├───╮ │ │  user:        test
  │ │ │ │ │  date:        Thu Jan 01 00:00:19 1970 +0000
  │ │ │ │ │  summary:     (19) expand
  │ │ │ │ │
  │ o │ │ │  commit:      1aa84d96232a
  ╭─┴───╮ │  user:        test
  │   │ │ │  date:        Thu Jan 01 00:00:18 1970 +0000
  │   │ │ │  summary:     (18) merge two known; two far left
  │   │ │ │
  │   o │ │  commit:      44765d7c06e0
  │ ╭─┤ │ │  user:        test
  │ │ │ │ │  date:        Thu Jan 01 00:00:17 1970 +0000
  │ │ │ │ │  summary:     (17) expand
  │ │ │ │ │
  │ o │ │ │  commit:      3677d192927d
  │ ╰───┬─╮  user:        test
  │   │ │ │  date:        Thu Jan 01 00:00:16 1970 +0000
  │   │ │ │  summary:     (16) merge two known; one immediate right, one near right
  │   │ │ │
  o   │ │ │  commit:      1dda3f72782d
  ├─╮ │ │ │  user:        test
  │ │ │ │ │  date:        Thu Jan 01 00:00:15 1970 +0000
  │ │ │ │ │  summary:     (15) expand
  │ │ │ │ │
  │ o │ │ │  commit:      8eac370358ef
  │ ╰─┬───╮  user:        test
  │   │ │ │  date:        Thu Jan 01 00:00:14 1970 +0000
  │   │ │ │  summary:     (14) merge two known; one immediate right, one far right
  │   │ │ │
  o   │ │ │  commit:      22d8966a97e3
  ├─╮ │ │ │  user:        test
  │ │ │ │ │  date:        Thu Jan 01 00:00:13 1970 +0000
  │ │ │ │ │  summary:     (13) expand
  │ │ │ │ │
  │ │ o │ │  commit:      86b91144a6e9
  ╭───┴─╮ │  user:        test
  │ │   │ │  date:        Thu Jan 01 00:00:12 1970 +0000
  │ │   │ │  summary:     (12) merge two known; one immediate right, one far left
  │ │   │ │
  │ o   │ │  commit:      832d76e6bdf2
  │ ├─╮ │ │  user:        test
  │ │ │ │ │  date:        Thu Jan 01 00:00:11 1970 +0000
  │ │ │ │ │  summary:     (11) expand
  │ │ │ │ │
  │ │ o │ │  commit:      74c64d036d72
  │ ╭─┴───╮  user:        test
  │ │   │ │  date:        Thu Jan 01 00:00:10 1970 +0000
  │ │   │ │  summary:     (10) merge two known; one immediate left, one near right
  │ │   │ │
  o │   │ │  commit:      7010c0af0a35
  ├───╮ │ │  user:        test
  │ │ │ │ │  date:        Thu Jan 01 00:00:09 1970 +0000
  │ │ │ │ │  summary:     (9) expand
  │ │ │ │ │
  │ │ o │ │  commit:      7a0b11f71937
  ╭───┴───╮  user:        test
  │ │   │ │  date:        Thu Jan 01 00:00:08 1970 +0000
  │ │   │ │  summary:     (8) merge two known; one immediate left, one far right
  │ │   │ │
  o │   │ │  commit:      b632bb1b1224
  ├───╮ │ │  user:        test
  │ │ │ │ │  date:        Thu Jan 01 00:00:07 1970 +0000
  │ │ │ │ │  summary:     (7) expand
  │ │ │ │ │
  │ o │ │ │  commit:      b105a072e251
  ╭─┴─╮ │ │  user:        test
  │   │ │ │  date:        Thu Jan 01 00:00:06 1970 +0000
  │   │ │ │  summary:     (6) merge two known; one immediate left, one far left
  │   │ │ │
  │   o │ │  commit:      4409d547b708
  │ ╭─┤ │ │  user:        test
  │ │ │ │ │  date:        Thu Jan 01 00:00:05 1970 +0000
  │ │ │ │ │  summary:     (5) expand
  │ │ │ │ │
  │ o │ │ │  commit:      26a8bac39d9f
  │ ╰─┬─╮ │  user:        test
  │   │ │ │  date:        Thu Jan 01 00:00:04 1970 +0000
  │   │ │ │  summary:     (4) merge two known; one immediate left, one immediate right
  │   │ │ │
  │   o │ │  commit:      27eef8ed80b4
  ├───╯ │ │  user:        test
  │     │ │  date:        Thu Jan 01 00:00:03 1970 +0000
  │     │ │  summary:     (3) collapse
  │     │ │
  o     │ │  commit:      3d9a33b8d1e1
  ├─────╯ │  user:        test
  │       │  date:        Thu Jan 01 00:00:02 1970 +0000
  │       │  summary:     (2) collapse
  │       │
  o       │  commit:      6db2ef61d156
  ├───────╯  user:        test
  │          date:        Thu Jan 01 00:00:01 1970 +0000
  │          summary:     (1) collapse
  │
  o  commit:      e6eb3150255d
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     (0) root

# File glog per revset:

  $ hg log -G -r 'file("a")'
  @  commit:      fea3ac5810e0
  │  user:        test
  │  date:        Thu Jan 01 00:00:34 1970 +0000
  │  summary:     (34) head
  │
  │ o  commit:      68608f5145f9
  │ │  user:        test
  │ │  date:        Thu Jan 01 00:00:33 1970 +0000
  │ │  summary:     (33) head
  │ │
  o │    commit:      d06dffa21a31
  ├───╮  user:        test
  │ │ │  date:        Thu Jan 01 00:00:32 1970 +0000
  │ │ │  summary:     (32) expand
  │ │ │
  │ │ o    commit:      621d83e11f67
  │ │ ├─╮  user:        test
  │ │ │ │  date:        Thu Jan 01 00:00:31 1970 +0000
  │ │ │ │  summary:     (31) expand
  │ │ │ │
  │ │ │ o    commit:      6e11cd4b648f
  │ │ │ ├─╮  user:        test
  │ │ │ │ │  date:        Thu Jan 01 00:00:30 1970 +0000
  │ │ │ │ │  summary:     (30) expand
  │ │ │ │ │
  │ │ │ │ o  commit:      cd9bb2be7593
  │ │ │ │ │  user:        test
  │ │ │ │ │  date:        Thu Jan 01 00:00:29 1970 +0000
  │ │ │ │ │  summary:     (29) regular commit
  │ │ │ │ │
  │ │ │ o │    commit:      44ecd0b9ae99
  │ │ │ ├───╮  user:        test
  │ │ │ │ │ │  date:        Thu Jan 01 00:00:28 1970 +0000
  │ │ │ │ │ │  summary:     (28) merge zero known
  │ │ │ │ │ │
  o │ │ │ │ │  commit:      886ed638191b
  ├───╯ │ │ │  user:        test
  │ │   │ │ │  date:        Thu Jan 01 00:00:27 1970 +0000
  │ │   │ │ │  summary:     (27) collapse
  │ │   │ │ │
  │ │   │ │ o  commit:      7f25b6c2f0b9
  │ ╭───────┤  user:        test
  │ │   │ │ │  date:        Thu Jan 01 00:00:26 1970 +0000
  │ │   │ │ │  summary:     (26) merge one known; far right
  │ │   │ │ │
  │ │   │ │ o  commit:      91da8ed57247
  ╭─────────┤  user:        test
  │ │   │ │ │  date:        Thu Jan 01 00:00:25 1970 +0000
  │ │   │ │ │  summary:     (25) merge one known; far left
  │ │   │ │ │
  │ │   │ │ o  commit:      a9c19a3d96b7
  │ │   │ ╭─┤  user:        test
  │ │   │ │ │  date:        Thu Jan 01 00:00:24 1970 +0000
  │ │   │ │ │  summary:     (24) merge one known; immediate right
  │ │   │ │ │
  │ │   │ │ o  commit:      a01cddf0766d
  │ │   ╭───┤  user:        test
  │ │   │ │ │  date:        Thu Jan 01 00:00:23 1970 +0000
  │ │   │ │ │  summary:     (23) merge one known; immediate left
  │ │   │ │ │
  │ │   │ │ o  commit:      e0d9cccacb5d
  ╭─┬───────╯  user:        test
  │ │   │ │    date:        Thu Jan 01 00:00:22 1970 +0000
  │ │   │ │    summary:     (22) merge two known; one far left, one far right
  │ │   │ │
  o │   │ │  commit:      d42a756af44d
  ├───╮ │ │  user:        test
  │ │ │ │ │  date:        Thu Jan 01 00:00:21 1970 +0000
  │ │ │ │ │  summary:     (21) expand
  │ │ │ │ │
  │ │ o │ │  commit:      d30ed6450e32
  │ ╭─┴───╮  user:        test
  │ │   │ │  date:        Thu Jan 01 00:00:20 1970 +0000
  │ │   │ │  summary:     (20) merge two known; two far right
  │ │   │ │
  o │   │ │  commit:      31ddc2c1573b
  ├───╮ │ │  user:        test
  │ │ │ │ │  date:        Thu Jan 01 00:00:19 1970 +0000
  │ │ │ │ │  summary:     (19) expand
  │ │ │ │ │
  │ o │ │ │  commit:      1aa84d96232a
  ╭─┴───╮ │  user:        test
  │   │ │ │  date:        Thu Jan 01 00:00:18 1970 +0000
  │   │ │ │  summary:     (18) merge two known; two far left
  │   │ │ │
  │   o │ │  commit:      44765d7c06e0
  │ ╭─┤ │ │  user:        test
  │ │ │ │ │  date:        Thu Jan 01 00:00:17 1970 +0000
  │ │ │ │ │  summary:     (17) expand
  │ │ │ │ │
  │ o │ │ │  commit:      3677d192927d
  │ ╰───┬─╮  user:        test
  │   │ │ │  date:        Thu Jan 01 00:00:16 1970 +0000
  │   │ │ │  summary:     (16) merge two known; one immediate right, one near right
  │   │ │ │
  o   │ │ │  commit:      1dda3f72782d
  ├─╮ │ │ │  user:        test
  │ │ │ │ │  date:        Thu Jan 01 00:00:15 1970 +0000
  │ │ │ │ │  summary:     (15) expand
  │ │ │ │ │
  │ o │ │ │  commit:      8eac370358ef
  │ ╰─┬───╮  user:        test
  │   │ │ │  date:        Thu Jan 01 00:00:14 1970 +0000
  │   │ │ │  summary:     (14) merge two known; one immediate right, one far right
  │   │ │ │
  o   │ │ │  commit:      22d8966a97e3
  ├─╮ │ │ │  user:        test
  │ │ │ │ │  date:        Thu Jan 01 00:00:13 1970 +0000
  │ │ │ │ │  summary:     (13) expand
  │ │ │ │ │
  │ │ o │ │  commit:      86b91144a6e9
  ╭───┴─╮ │  user:        test
  │ │   │ │  date:        Thu Jan 01 00:00:12 1970 +0000
  │ │   │ │  summary:     (12) merge two known; one immediate right, one far left
  │ │   │ │
  │ o   │ │  commit:      832d76e6bdf2
  │ ├─╮ │ │  user:        test
  │ │ │ │ │  date:        Thu Jan 01 00:00:11 1970 +0000
  │ │ │ │ │  summary:     (11) expand
  │ │ │ │ │
  │ │ o │ │  commit:      74c64d036d72
  │ ╭─┴───╮  user:        test
  │ │   │ │  date:        Thu Jan 01 00:00:10 1970 +0000
  │ │   │ │  summary:     (10) merge two known; one immediate left, one near right
  │ │   │ │
  o │   │ │  commit:      7010c0af0a35
  ├───╮ │ │  user:        test
  │ │ │ │ │  date:        Thu Jan 01 00:00:09 1970 +0000
  │ │ │ │ │  summary:     (9) expand
  │ │ │ │ │
  │ │ o │ │  commit:      7a0b11f71937
  ╭───┴───╮  user:        test
  │ │   │ │  date:        Thu Jan 01 00:00:08 1970 +0000
  │ │   │ │  summary:     (8) merge two known; one immediate left, one far right
  │ │   │ │
  o │   │ │  commit:      b632bb1b1224
  ├───╮ │ │  user:        test
  │ │ │ │ │  date:        Thu Jan 01 00:00:07 1970 +0000
  │ │ │ │ │  summary:     (7) expand
  │ │ │ │ │
  │ o │ │ │  commit:      b105a072e251
  ╭─┴─╮ │ │  user:        test
  │   │ │ │  date:        Thu Jan 01 00:00:06 1970 +0000
  │   │ │ │  summary:     (6) merge two known; one immediate left, one far left
  │   │ │ │
  │   o │ │  commit:      4409d547b708
  │ ╭─┤ │ │  user:        test
  │ │ │ │ │  date:        Thu Jan 01 00:00:05 1970 +0000
  │ │ │ │ │  summary:     (5) expand
  │ │ │ │ │
  │ o │ │ │  commit:      26a8bac39d9f
  │ ╰─┬─╮ │  user:        test
  │   │ │ │  date:        Thu Jan 01 00:00:04 1970 +0000
  │   │ │ │  summary:     (4) merge two known; one immediate left, one immediate right
  │   │ │ │
  │   o │ │  commit:      27eef8ed80b4
  ├───╯ │ │  user:        test
  │     │ │  date:        Thu Jan 01 00:00:03 1970 +0000
  │     │ │  summary:     (3) collapse
  │     │ │
  o     │ │  commit:      3d9a33b8d1e1
  ├─────╯ │  user:        test
  │       │  date:        Thu Jan 01 00:00:02 1970 +0000
  │       │  summary:     (2) collapse
  │       │
  o       │  commit:      6db2ef61d156
  ├───────╯  user:        test
  │          date:        Thu Jan 01 00:00:01 1970 +0000
  │          summary:     (1) collapse
  │
  o  commit:      e6eb3150255d
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     (0) root

# File glog per revset (only merges):

  $ hg log -G -r 'file("a")' -m
  o    commit:      d06dffa21a31
  ├─╮  user:        test
  │ ╷  date:        Thu Jan 01 00:00:32 1970 +0000
  │ ╷  summary:     (32) expand
  │ ╷
  o ╷  commit:      621d83e11f67
  ├─╮  user:        test
  │ │  date:        Thu Jan 01 00:00:31 1970 +0000
  │ │  summary:     (31) expand
  │ │
  o │    commit:      6e11cd4b648f
  ├───╮  user:        test
  │ │ │  date:        Thu Jan 01 00:00:30 1970 +0000
  │ │ ~  summary:     (30) expand
  │ │
  o │    commit:      44ecd0b9ae99
  ├───╮  user:        test
  │ │ │  date:        Thu Jan 01 00:00:28 1970 +0000
  │ │ ~  summary:     (28) merge zero known
  │ │
  o │    commit:      7f25b6c2f0b9
  ├───╮  user:        test
  │ │ │  date:        Thu Jan 01 00:00:26 1970 +0000
  │ │ │  summary:     (26) merge one known; far right
  │ │ │
  │ │ o  commit:      91da8ed57247
  │ ╭─┤  user:        test
  │ │ │  date:        Thu Jan 01 00:00:25 1970 +0000
  │ │ │  summary:     (25) merge one known; far left
  │ │ │
  │ │ o    commit:      a9c19a3d96b7
  │ │ ├─╮  user:        test
  │ │ │ │  date:        Thu Jan 01 00:00:24 1970 +0000
  │ │ │ ~  summary:     (24) merge one known; immediate right
  │ │ │
  │ │ o    commit:      a01cddf0766d
  │ │ ├─╮  user:        test
  │ │ │ │  date:        Thu Jan 01 00:00:23 1970 +0000
  │ │ │ ~  summary:     (23) merge one known; immediate left
  │ │ │
  │ │ o  commit:      e0d9cccacb5d
  ╭─┬─╯  user:        test
  │ │    date:        Thu Jan 01 00:00:22 1970 +0000
  │ │    summary:     (22) merge two known; one far left, one far right
  │ │
  │ o    commit:      d42a756af44d
  │ ├─╮  user:        test
  │ │ │  date:        Thu Jan 01 00:00:21 1970 +0000
  │ │ │  summary:     (21) expand
  │ │ │
  │ │ o  commit:      d30ed6450e32
  ╭───┤  user:        test
  │ │ │  date:        Thu Jan 01 00:00:20 1970 +0000
  │ │ ~  summary:     (20) merge two known; two far right
  │ │
  │ o    commit:      31ddc2c1573b
  │ ├─╮  user:        test
  │ │ │  date:        Thu Jan 01 00:00:19 1970 +0000
  │ │ │  summary:     (19) expand
  │ │ │
  o │ │  commit:      1aa84d96232a
  ├─╮ │  user:        test
  │ │ │  date:        Thu Jan 01 00:00:18 1970 +0000
  ~ │ │  summary:     (18) merge two known; two far left
    │ │
    │ o  commit:      44765d7c06e0
  ╭───┤  user:        test
  │ │ │  date:        Thu Jan 01 00:00:17 1970 +0000
  │ │ │  summary:     (17) expand
  │ │ │
  o │ │    commit:      3677d192927d
  ├─────╮  user:        test
  │ │ │ │  date:        Thu Jan 01 00:00:16 1970 +0000
  ~ │ │ ~  summary:     (16) merge two known; one immediate right, one near right
    │ │
    o │  commit:      1dda3f72782d
  ╭─┤ │  user:        test
  │ │ │  date:        Thu Jan 01 00:00:15 1970 +0000
  │ │ │  summary:     (15) expand
  │ │ │
  o │ │  commit:      8eac370358ef
  ├───╮  user:        test
  │ │ │  date:        Thu Jan 01 00:00:14 1970 +0000
  ~ │ │  summary:     (14) merge two known; one immediate right, one far right
    │ │
    o │  commit:      22d8966a97e3
  ╭─┤ │  user:        test
  │ │ │  date:        Thu Jan 01 00:00:13 1970 +0000
  │ │ │  summary:     (13) expand
  │ │ │
  │ │ o  commit:      86b91144a6e9
  │ ╭─┤  user:        test
  │ │ │  date:        Thu Jan 01 00:00:12 1970 +0000
  │ │ ~  summary:     (12) merge two known; one immediate right, one far left
  │ │
  o │    commit:      832d76e6bdf2
  ├───╮  user:        test
  │ │ │  date:        Thu Jan 01 00:00:11 1970 +0000
  │ │ │  summary:     (11) expand
  │ │ │
  │ │ o  commit:      74c64d036d72
  ╭───┤  user:        test
  │ │ │  date:        Thu Jan 01 00:00:10 1970 +0000
  │ │ ~  summary:     (10) merge two known; one immediate left, one near right
  │ │
  │ o    commit:      7010c0af0a35
  │ ├─╮  user:        test
  │ │ │  date:        Thu Jan 01 00:00:09 1970 +0000
  │ │ │  summary:     (9) expand
  │ │ │
  │ │ o  commit:      7a0b11f71937
  │ ╭─┤  user:        test
  │ │ │  date:        Thu Jan 01 00:00:08 1970 +0000
  │ │ ~  summary:     (8) merge two known; one immediate left, one far right
  │ │
  │ o    commit:      b632bb1b1224
  │ ├─╮  user:        test
  │ │ │  date:        Thu Jan 01 00:00:07 1970 +0000
  │ │ ~  summary:     (7) expand
  │ │
  o │  commit:      b105a072e251
  ├─╮  user:        test
  │ │  date:        Thu Jan 01 00:00:06 1970 +0000
  ~ │  summary:     (6) merge two known; one immediate left, one far left
    │
    o  commit:      4409d547b708
  ╭─┤  user:        test
  │ │  date:        Thu Jan 01 00:00:05 1970 +0000
  ~ │  summary:     (5) expand
    │
    o  commit:      26a8bac39d9f
  ╭─┤  user:        test
  │ │  date:        Thu Jan 01 00:00:04 1970 +0000
  ~ ~  summary:     (4) merge two known; one immediate left, one immediate right

# Empty revision range - display nothing:

  $ hg log -G -r 1..0

  $ cd ..

#if no-outer-repo
# From outer space:
  $ hg log -G -l1 repo
  @  changeset:   34:fea3ac5810e0
  ~  parent:      32:d06dffa21a31
     user:        test
     date:        Thu Jan 01 00:00:34 1970 +0000
     summary:     (34) head
  $ hg log -G -l1 repo/a
  @  changeset:   34:fea3ac5810e0
  ~  parent:      32:d06dffa21a31
     user:        test
     date:        Thu Jan 01 00:00:34 1970 +0000
     summary:     (34) head
  $ hg log -G -l1 repo/missing
#endif

# File log with revs != cset revs:

  $ hg init flog
  $ cd flog
  $ echo one > one
  $ hg add one
  $ hg commit -mone
  $ echo two > two
  $ hg add two
  $ hg commit -mtwo
  $ echo more > two
  $ hg commit -mmore
  $ hg log -G two
  @  commit:      12c28321755b
  │  user:        test
  │  date:        Thu Jan 01 00:00:00 1970 +0000
  │  summary:     more
  │
  o  commit:      5ac72c0599bf
  │  user:        test
  ~  date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     two

# Issue1896: File log with explicit style

  $ hg log -G '--style=default' one
  o  commit:      3d578b4a1f53
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     one

# Issue2395: glog --style header and footer

  $ hg log -G '--style=xml' one
  <?xml version="1.0"?>
  <log>
  o  <logentry node="3d578b4a1f537d5fcf7301bfa9c0b97adfaa6fb1">
     <author email="test">test</author>
     <date>1970-01-01T00:00:00+00:00</date>
     <msg xml:space="preserve">one</msg>
     </logentry>
  </log>

  $ cd ..

# File + limit with revs != cset revs:

  $ cd repo
  $ touch b
  $ hg ci -Aqm0
  $ hg log -G -l2 a
  o  commit:      fea3ac5810e0
  │  user:        test
  ~  date:        Thu Jan 01 00:00:34 1970 +0000
     summary:     (34) head
  
  o  commit:      68608f5145f9
  │  user:        test
  ~  date:        Thu Jan 01 00:00:33 1970 +0000
     summary:     (33) head

# File + limit + -ra:b, (b - a) < limit:

  $ hg log -G -l3000 '-r32:tip' a
  o  commit:      fea3ac5810e0
  │  user:        test
  │  date:        Thu Jan 01 00:00:34 1970 +0000
  │  summary:     (34) head
  │
  │ o  commit:      68608f5145f9
  │ │  user:        test
  │ ~  date:        Thu Jan 01 00:00:33 1970 +0000
  │    summary:     (33) head
  │
  o    commit:      d06dffa21a31
  ├─╮  user:        test
  │ │  date:        Thu Jan 01 00:00:32 1970 +0000
  ~ ~  summary:     (32) expand

# Point out a common and an uncommon unshown parent

  $ hg log -G -r 'rev(8) or rev(9)'
  o    commit:      7010c0af0a35
  ├─╮  user:        test
  │ │  date:        Thu Jan 01 00:00:09 1970 +0000
  │ ~  summary:     (9) expand
  │
  o    commit:      7a0b11f71937
  ├─╮  user:        test
  │ │  date:        Thu Jan 01 00:00:08 1970 +0000
  ~ ~  summary:     (8) merge two known; one immediate left, one far right

# File + limit + -ra:b, b < tip:

  $ hg log -G -l1 '-r32:34' a
  o  commit:      fea3ac5810e0
  │  user:        test
  ~  date:        Thu Jan 01 00:00:34 1970 +0000
     summary:     (34) head

# file(File) + limit + -ra:b, b < tip:

  $ hg log -G -l1 '-r32:34' -r 'file("a")'
  o  commit:      fea3ac5810e0
  │  user:        test
  ~  date:        Thu Jan 01 00:00:34 1970 +0000
     summary:     (34) head

# limit(file(File) and a::b), b < tip:

  $ hg log -G -r 'limit(file("a") and 32::34, 1)'
  o    commit:      d06dffa21a31
  ├─╮  user:        test
  │ │  date:        Thu Jan 01 00:00:32 1970 +0000
  ~ ~  summary:     (32) expand

# File + limit + -ra:b, b < tip:

  $ hg log -G -r 'limit(file("a") and 34::32, 1)'

# File + limit + -ra:b, b < tip, (b - a) < limit:

  $ hg log -G -l10 '-r33:34' a
  o  commit:      fea3ac5810e0
  │  user:        test
  ~  date:        Thu Jan 01 00:00:34 1970 +0000
     summary:     (34) head
  
  o  commit:      68608f5145f9
  │  user:        test
  ~  date:        Thu Jan 01 00:00:33 1970 +0000
     summary:     (33) head

# Do not crash or produce strange graphs if history is buggy

  $ commit 36 'buggy merge: identical parents' 35 35
  $ hg log -G -l5
  @  commit:      95fa8febd08a
  │  user:        test
  │  date:        Thu Jan 01 00:00:36 1970 +0000
  │  summary:     (36) buggy merge: identical parents
  │
  o  commit:      9159c3644c5e
  │  user:        test
  │  date:        Thu Jan 01 00:00:00 1970 +0000
  │  summary:     0
  │
  o  commit:      fea3ac5810e0
  │  user:        test
  │  date:        Thu Jan 01 00:00:34 1970 +0000
  │  summary:     (34) head
  │
  │ o  commit:      68608f5145f9
  │ │  user:        test
  │ ~  date:        Thu Jan 01 00:00:33 1970 +0000
  │    summary:     (33) head
  │
  o    commit:      d06dffa21a31
  ├─╮  user:        test
  │ │  date:        Thu Jan 01 00:00:32 1970 +0000
  ~ ~  summary:     (32) expand

# Test log -G options
# glog always reorders nodes which explains the difference with log

  $ hg log -G --print-revset -r 27 -r 25 -r 21 -r 34 -r 32 -r 31
  ['27', '25', '21', '34', '32', '31']
  []
  $ hg log -G --print-revset -u test -u not-a-user
  []
  (group
    (group
      (or
        (list
          (func
            (symbol 'user')
            (string 'test'))
          (func
            (symbol 'user')
            (string 'not-a-user'))))))
  $ hg log -G --print-revset -k expand -k merge
  []
  (group
    (group
      (or
        (list
          (func
            (symbol 'keyword')
            (string 'expand'))
          (func
            (symbol 'keyword')
            (string 'merge'))))))
  $ hg log -G --print-revset --only-merges
  []
  (group
    (func
      (symbol 'merge')
      None))
  $ hg log -G --print-revset --no-merges
  []
  (group
    (not
      (func
        (symbol 'merge')
        None)))
  $ hg log -G --print-revset --date '2 0 to 4 0'
  []
  (group
    (func
      (symbol 'date')
      (string '2 0 to 4 0')))
  $ hg log -G -d 'brace ) in a date'
  hg: parse error: invalid date: 'brace ) in a date'
  [255]
  $ hg log -G --print-revset --prune 31 --prune 32
  []
  (group
    (group
      (and
        (not
          (group
            (or
              (list
                (string '31')
                (func
                  (symbol 'ancestors')
                  (string '31'))))))
        (not
          (group
            (or
              (list
                (string '32')
                (func
                  (symbol 'ancestors')
                  (string '32')))))))))

# Dedicated repo for --follow and paths filtering. The g is crafted to
# have 2 filelog topological heads in a linear changeset graph.

  $ cd ..
  $ hg init follow
  $ cd follow
  $ hg log -G --print-revset --follow
  []
  []
  $ hg log -G --print-revset -rnull
  ['null']
  []
  $ echo a > a
  $ echo aa > aa
  $ echo f > f
  $ hg ci -Am 'add a' a aa f
  $ hg cp a b
  $ hg cp f g
  $ hg ci -m 'copy a b'
  $ mkdir dir
  $ hg mv b dir
  $ echo g >> g
  $ echo f >> f
  $ hg ci -m 'mv b dir/b'
  $ hg mv a b
  $ hg cp -f f g
  $ echo a > d
  $ hg add d
  $ hg ci -m 'mv a b; add d'
  $ hg mv dir/b e
  $ hg ci -m 'mv dir/b e'
  $ hg log -G --template '({rev}) {desc|firstline}\n'
  @  (4) mv dir/b e
  │
  o  (3) mv a b; add d
  │
  o  (2) mv b dir/b
  │
  o  (1) copy a b
  │
  o  (0) add a

  $ hg log -G --print-revset a
  []
  (group
    (group
      (func
        (symbol 'filelog')
        (string 'a'))))
  $ hg log -G --print-revset a b
  []
  (group
    (group
      (or
        (list
          (func
            (symbol 'filelog')
            (string 'a'))
          (func
            (symbol 'filelog')
            (string 'b'))))))

# Test falling back to slow path for non-existing files

  $ hg log -G --print-revset a c
  []
  (group
    (func
      (symbol '_matchfiles')
      (list
        (string 'r:')
        (string 'd:relpath')
        (string 'p:a')
        (string 'p:c'))))

# Test multiple --include/--exclude/paths

  $ hg log -G --print-revset --include a --include e --exclude b --exclude e a e
  []
  (group
    (func
      (symbol '_matchfiles')
      (list
        (string 'r:')
        (string 'd:relpath')
        (string 'p:a')
        (string 'p:e')
        (string 'i:a')
        (string 'i:e')
        (string 'x:b')
        (string 'x:e'))))

#if false
  $ hg log -G --print-revset 'a*'
  []
  (group
    (group
      (func
        (symbol 'filelog')
        (string 'aa'))))
#endif

# Test --follow on a non-existent directory

  $ hg log -G --print-revset -f dir
  abort: cannot follow file not in parent revision: "dir"
  [255]

# Test --follow on a directory

  $ hg up -q '.^'
  $ hg log -G --print-revset -f dir
  []
  (group
    (group
      (func
        (symbol 'follow')
        (string 'dir'))))
  $ hg up -q tip

# Test --follow on file not in parent revision

  $ hg log -G --print-revset -f a
  abort: cannot follow file not in parent revision: "a"
  [255]

# Test --follow and patterns

  $ hg log -G --print-revset -f 'glob:*'
  []
  (group
    (and
      (func
        (symbol 'ancestors')
        (symbol '.'))
      (func
        (symbol '_matchfiles')
        (list
          (string 'r:')
          (string 'd:relpath')
          (string 'p:glob:*')))))

# Test --follow on a single rename

  $ hg up -q 2
  $ hg log -G --print-revset -f a
  []
  (group
    (group
      (func
        (symbol 'follow')
        (string 'a'))))

# Test --follow and multiple renames

  $ hg up -q tip
  $ hg log -G --print-revset -f e
  []
  (group
    (group
      (func
        (symbol 'follow')
        (string 'e'))))

# Test --follow and multiple filelog heads

  $ hg up -q 2
  $ hg log -G --print-revset -f g
  []
  (group
    (group
      (func
        (symbol 'follow')
        (string 'g'))))
  $ hg up -q tip
  $ hg log -G --print-revset -f g
  []
  (group
    (group
      (func
        (symbol 'follow')
        (string 'g'))))

# Test --follow and multiple files

  $ hg log -G --print-revset -f g e
  []
  (group
    (group
      (or
        (list
          (func
            (symbol 'follow')
            (string 'g'))
          (func
            (symbol 'follow')
            (string 'e'))))))

# Test --follow null parent

  $ hg up -q null
  $ hg log -G --print-revset -f
  []
  []

# Test --follow-first

  $ hg up -q 3
  $ echo ee > e
  $ hg ci -Am 'add another e' e
  $ hg merge --tool 'internal:other' 4
  0 files updated, 1 files merged, 1 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ echo merge > e
  $ hg ci -m 'merge 5 and 4'
  $ hg log -G --print-revset --follow-first
  []
  (group
    (func
      (symbol '_firstancestors')
      (func
        (symbol 'rev')
        (symbol '6'))))

# Cannot compare with log --follow-first FILE as it never worked

  $ hg log -G --print-revset --follow-first e
  []
  (group
    (group
      (func
        (symbol '_followfirst')
        (string 'e'))))
  $ hg log -G --follow-first e --template '{rev} {desc|firstline}\n'
  @    6 merge 5 and 4
  ├─╮
  │ │
  │ ~
  │
  o  5 add another e
  │
  ~

# Test --copies

  $ hg log -G --copies --template '{rev} {desc|firstline}   copies: {file_copies_switch}\n'
  @    6 merge 5 and 4   copies:
  ├─╮
  │ o  5 add another e   copies:
  │ │
  o │  4 mv dir/b e   copies: e (dir/b)
  ├─╯
  o  3 mv a b; add d   copies: b (a)g (f)
  │
  o  2 mv b dir/b   copies: dir/b (b)
  │
  o  1 copy a b   copies: b (a)g (f)
  │
  o  0 add a   copies:

# Test "set:..." and parent revision

  $ hg up -q 4
  $ hg log -G --print-revset 'set:copied()'
  []
  (group
    (func
      (symbol '_matchfiles')
      (list
        (string 'r:')
        (string 'd:relpath')
        (string 'p:set:copied()'))))
  $ hg log -G --print-revset --include 'set:copied()'
  []
  (group
    (func
      (symbol '_matchfiles')
      (list
        (string 'r:')
        (string 'd:relpath')
        (string 'i:set:copied()'))))
  $ hg log -G --print-revset -r 'sort(file('\''set:copied()'\''), -rev)'
  ["sort(file('set:copied()'), -rev)"]
  []

# Test --removed

  $ hg log -G --print-revset --removed
  []
  []
  $ hg log -G --print-revset --removed a
  []
  (group
    (func
      (symbol '_matchfiles')
      (list
        (string 'r:')
        (string 'd:relpath')
        (string 'p:a'))))
  $ hg log -G --print-revset --removed --follow a
  []
  (group
    (and
      (func
        (symbol 'ancestors')
        (symbol '.'))
      (func
        (symbol '_matchfiles')
        (list
          (string 'r:')
          (string 'd:relpath')
          (string 'p:a')))))

# Test --patch and --stat with --follow and --follow-first

  $ hg up -q 3
  $ hg log -G --git --patch b
  o  commit:      216d4c92cf98
  │  user:        test
  ~  date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     copy a b
  
     diff --git a/a b/b
     copy from a
     copy to b

  $ hg log -G --git --stat b
  o  commit:      216d4c92cf98
  │  user:        test
  ~  date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     copy a b
  
      b |  0
      1 files changed, 0 insertions(+), 0 deletions(-)

  $ hg log -G --git --patch --follow b
  @  commit:      4e4494cd467d
  ╷  user:        test
  ╷  date:        Thu Jan 01 00:00:00 1970 +0000
  ╷  summary:     mv a b; add d
  ╷
  ╷  diff --git a/a b/b
  ╷  copy from a
  ╷  copy to b
  ╷
  o  commit:      f8035bb17114
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
  @  commit:      4e4494cd467d
  ╷  user:        test
  ╷  date:        Thu Jan 01 00:00:00 1970 +0000
  ╷  summary:     mv a b; add d
  ╷
  ╷   b |  0
  ╷   1 files changed, 0 insertions(+), 0 deletions(-)
  ╷
  o  commit:      f8035bb17114
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     add a
  
      a |  1 +
      1 files changed, 1 insertions(+), 0 deletions(-)

  $ hg up -q 6
  $ hg log -G --git --patch --follow-first e
  @    commit:      36921220a3d9
  ├─╮  user:        test
  │ │  date:        Thu Jan 01 00:00:00 1970 +0000
  │ ~  summary:     merge 5 and 4
  │
  │    diff --git a/e b/e
  │    --- a/e
  │    +++ b/e
  │    @@ -1,1 +1,1 @@
  │    -ee
  │    +merge
  │
  o  commit:      303e395907af
  │  user:        test
  ~  date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     add another e
  
     diff --git a/e b/e
     new file mode 100644
     --- /dev/null
     +++ b/e
     @@ -0,0 +1,1 @@
     +ee

# Test old-style --rev

  $ echo 'fc281d8ff18d999ad6497b3d27390bcd695dcc73 foo-bar' >> .hgtags
  $ hg commit -Aqm 'Added tag foo-bar for changeset fc281d8ff18d'
  $ hg book foo-bar
  $ hg log -G --print-revset -r foo-bar
  ['foo-bar']
  []

# Test --follow and forward --rev

  $ hg up -q 6
  $ echo g > g
  $ hg ci -Am 'add g' g
  $ hg up -q 2
  $ hg log -G --template '{rev} {desc|firstline}\n'
  o  8 add g
  │
  │ o  7 Added tag foo-bar for changeset fc281d8ff18d
  ├─╯
  o    6 merge 5 and 4
  ├─╮
  │ o  5 add another e
  │ │
  o │  4 mv dir/b e
  ├─╯
  o  3 mv a b; add d
  │
  @  2 mv b dir/b
  │
  o  1 copy a b
  │
  o  0 add a
  $ hg archive -r 7 archive
  $ rm -r archive

# changessincelatesttag with no prior tag

  $ hg archive -r 4 archive

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
  # Node ID 8e28f3456c5ac21c5b24c91870ed63027905fa0c
  # Parent  216d4c92cf98ff2b4641d508b76b529f3d424c92
  mv b dir/b
  
  diff -r 216d4c92cf98 -r 8e28f3456c5a b
  --- a/b	Thu Jan 01 00:00:00 1970 +0000
  +++ /dev/null	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +0,0 @@
  -a
  diff -r 216d4c92cf98 -r 8e28f3456c5a dir/b
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/dir/b	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +a
  diff -r 216d4c92cf98 -r 8e28f3456c5a f
  --- a/f	Thu Jan 01 00:00:00 1970 +0000
  +++ b/f	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +1,2 @@
   f
  +f
  diff -r 216d4c92cf98 -r 8e28f3456c5a g
  --- a/g	Thu Jan 01 00:00:00 1970 +0000
  +++ b/g	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +1,2 @@
   f
  +g
  # HG changeset patch
  # User test
  # Date 0 0
  #      Thu Jan 01 00:00:00 1970 +0000
  # Node ID 4e4494cd467d419979bf7f82059773308b21e260
  # Parent  8e28f3456c5ac21c5b24c91870ed63027905fa0c
  mv a b; add d
  
  diff -r 8e28f3456c5a -r 4e4494cd467d a
  --- a/a	Thu Jan 01 00:00:00 1970 +0000
  +++ /dev/null	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +0,0 @@
  -a
  diff -r 8e28f3456c5a -r 4e4494cd467d b
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/b	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +a
  diff -r 8e28f3456c5a -r 4e4494cd467d d
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/d	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +a
  diff -r 8e28f3456c5a -r 4e4494cd467d g
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
  # Node ID e44ebe41bd2f734e8515763f44049e838d74d725
  # Parent  4e4494cd467d419979bf7f82059773308b21e260
  mv dir/b e
  
  diff -r 4e4494cd467d -r e44ebe41bd2f dir/b
  --- a/dir/b	Thu Jan 01 00:00:00 1970 +0000
  +++ /dev/null	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +0,0 @@
  -a
  diff -r 4e4494cd467d -r e44ebe41bd2f e
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/e	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +a
  # HG changeset patch
  # User test
  # Date 0 0
  #      Thu Jan 01 00:00:00 1970 +0000
  # Node ID 303e395907af014f67745883c70049a6f69a707d
  # Parent  4e4494cd467d419979bf7f82059773308b21e260
  add another e
  
  diff -r 4e4494cd467d -r 303e395907af e
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/e	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +ee
  # HG changeset patch
  # User test
  # Date 0 0
  #      Thu Jan 01 00:00:00 1970 +0000
  # Node ID 36921220a3d9820aae9def32a3372a87a6c866e5
  # Parent  303e395907af014f67745883c70049a6f69a707d
  # Parent  e44ebe41bd2f734e8515763f44049e838d74d725
  merge 5 and 4
  
  diff -r 303e395907af -r 36921220a3d9 dir/b
  --- a/dir/b	Thu Jan 01 00:00:00 1970 +0000
  +++ /dev/null	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +0,0 @@
  -a
  diff -r 303e395907af -r 36921220a3d9 e
  --- a/e	Thu Jan 01 00:00:00 1970 +0000
  +++ b/e	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +1,1 @@
  -ee
  +merge
  # HG changeset patch
  # User test
  # Date 0 0
  #      Thu Jan 01 00:00:00 1970 +0000
  # Node ID 71adb5c4f02f067755963d8087e45153bb0b466f
  # Parent  36921220a3d9820aae9def32a3372a87a6c866e5
  Added tag foo-bar for changeset fc281d8ff18d
  
  diff -r 36921220a3d9 -r 71adb5c4f02f .hgtags
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/.hgtags	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +fc281d8ff18d999ad6497b3d27390bcd695dcc73 foo-bar
  # HG changeset patch
  # User test
  # Date 0 0
  #      Thu Jan 01 00:00:00 1970 +0000
  # Node ID 6f6118f7da5a0ca1b1f17f12fa70f17c5960d3c8
  # Parent  36921220a3d9820aae9def32a3372a87a6c866e5
  add g
  
  diff -r 36921220a3d9 -r 6f6118f7da5a g
  --- a/g	Thu Jan 01 00:00:00 1970 +0000
  +++ b/g	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,2 +1,1 @@
  -f
  -f
  +g
  $ hg log -G --print-revset --follow -r6 -r8 -r5 -r7 -r4
  ['6', '8', '5', '7', '4']
  (group
    (func
      (symbol 'descendants')
      (func
        (symbol 'rev')
        (symbol '6'))))

# Test --follow-first and forward --rev

  $ hg log -G --print-revset --follow-first -r6 -r8 -r5 -r7 -r4
  ['6', '8', '5', '7', '4']
  (group
    (func
      (symbol '_firstdescendants')
      (func
        (symbol 'rev')
        (symbol '6'))))

# Test --follow and backward --rev

  $ hg log -G --print-revset --follow -r6 -r5 -r7 -r8 -r4
  ['6', '5', '7', '8', '4']
  (group
    (func
      (symbol 'ancestors')
      (func
        (symbol 'rev')
        (symbol '6'))))

# Test --follow-first and backward --rev

  $ hg log -G --print-revset --follow-first -r6 -r5 -r7 -r8 -r4
  ['6', '5', '7', '8', '4']
  (group
    (func
      (symbol '_firstancestors')
      (func
        (symbol 'rev')
        (symbol '6'))))

# Test subdir

  $ hg up -q 3
  $ cd dir
  $ hg log -G --print-revset .
  []
  (group
    (func
      (symbol '_matchfiles')
      (list
        (string 'r:')
        (string 'd:relpath')
        (string 'p:.'))))
  $ hg log -G --print-revset ../b
  []
  (group
    (group
      (func
        (symbol 'filelog')
        (string '../b'))))
  $ hg log -G --print-revset -f ../b
  []
  (group
    (group
      (func
        (symbol 'follow')
        (string 'b'))))
  $ cd ..

# A template without trailing newline should do something sane

  $ hg log -G -r '::2' --template '{rev} {desc}'
  o  2 mv b dir/b
  │
  o  1 copy a b
  │
  o  0 add a

# Extra newlines must be preserved

  $ hg log -G -r '::2' --template '\n{rev} {desc}\n\n'
  o
  │  2 mv b dir/b
  │
  o
  │  1 copy a b
  │
  o
     0 add a

# The almost-empty template should do something sane too ...

  $ hg log -G -r '::2' --template '\n'
  o
  │
  o
  │
  o

# issue3772

  $ hg log -G -r ':null'
  o  commit:      f8035bb17114
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     add a
  $ hg log -G -r 'null:null'
  o  commit:      000000000000
     user:
     date:        Thu Jan 01 00:00:00 1970 +0000

# should not draw line down to null due to the magic of fullreposet

  $ hg log -G -r 'all()' | tail -5
  o  commit:      f8035bb17114
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     add a

# working-directory revision
# XXX: Currently not working.
# sh % "hg log -G -qr '. + wdir()'"
# node template with changeset_printer:

  $ hg log -Gqr '5:7' --config 'ui.graphnodetemplate="{rev}"'
  7  71adb5c4f02f
  │
  6    36921220a3d9
  ├─╮
  │ │
  │ ~
  │
  5  303e395907af
  │
  ~

# label() should just work in node template:

  $ hg log -Gqr 7 --config 'extensions.color=' '--color=debug' --config 'ui.graphnodetemplate={label("branch.{branch}", rev)}'
  [branch.default|7]  [log.node|71adb5c4f02f]
  │
  ~

  $ cd ..

# change graph edge styling

  $ cd repo
  $ cat >> $HGRCPATH << 'EOF'
  > [experimental]
  > graphstyle.parent = |
  > graphstyle.grandparent = :
  > graphstyle.missing =
  > EOF
  $ hg log -G -r 'file("a")' -m
  @  commit:      95fa8febd08a
  ╷  user:        test
  ╷  date:        Thu Jan 01 00:00:36 1970 +0000
  ╷  summary:     (36) buggy merge: identical parents
  ╷
  o    commit:      d06dffa21a31
  ├─╮  user:        test
  │ ╷  date:        Thu Jan 01 00:00:32 1970 +0000
  │ ╷  summary:     (32) expand
  │ ╷
  o ╷  commit:      621d83e11f67
  ├─╮  user:        test
  │ │  date:        Thu Jan 01 00:00:31 1970 +0000
  │ │  summary:     (31) expand
  │ │
  o │    commit:      6e11cd4b648f
  ├───╮  user:        test
  │ │ │  date:        Thu Jan 01 00:00:30 1970 +0000
  │ │ ~  summary:     (30) expand
  │ │
  o │    commit:      44ecd0b9ae99
  ├───╮  user:        test
  │ │ │  date:        Thu Jan 01 00:00:28 1970 +0000
  │ │ ~  summary:     (28) merge zero known
  │ │
  o │    commit:      7f25b6c2f0b9
  ├───╮  user:        test
  │ │ │  date:        Thu Jan 01 00:00:26 1970 +0000
  │ │ │  summary:     (26) merge one known; far right
  │ │ │
  │ │ o  commit:      91da8ed57247
  │ ╭─┤  user:        test
  │ │ │  date:        Thu Jan 01 00:00:25 1970 +0000
  │ │ │  summary:     (25) merge one known; far left
  │ │ │
  │ │ o    commit:      a9c19a3d96b7
  │ │ ├─╮  user:        test
  │ │ │ │  date:        Thu Jan 01 00:00:24 1970 +0000
  │ │ │ ~  summary:     (24) merge one known; immediate right
  │ │ │
  │ │ o    commit:      a01cddf0766d
  │ │ ├─╮  user:        test
  │ │ │ │  date:        Thu Jan 01 00:00:23 1970 +0000
  │ │ │ ~  summary:     (23) merge one known; immediate left
  │ │ │
  │ │ o  commit:      e0d9cccacb5d
  ╭─┬─╯  user:        test
  │ │    date:        Thu Jan 01 00:00:22 1970 +0000
  │ │    summary:     (22) merge two known; one far left, one far right
  │ │
  │ o    commit:      d42a756af44d
  │ ├─╮  user:        test
  │ │ │  date:        Thu Jan 01 00:00:21 1970 +0000
  │ │ │  summary:     (21) expand
  │ │ │
  │ │ o  commit:      d30ed6450e32
  ╭───┤  user:        test
  │ │ │  date:        Thu Jan 01 00:00:20 1970 +0000
  │ │ ~  summary:     (20) merge two known; two far right
  │ │
  │ o    commit:      31ddc2c1573b
  │ ├─╮  user:        test
  │ │ │  date:        Thu Jan 01 00:00:19 1970 +0000
  │ │ │  summary:     (19) expand
  │ │ │
  o │ │  commit:      1aa84d96232a
  ├─╮ │  user:        test
  │ │ │  date:        Thu Jan 01 00:00:18 1970 +0000
  ~ │ │  summary:     (18) merge two known; two far left
    │ │
    │ o  commit:      44765d7c06e0
  ╭───┤  user:        test
  │ │ │  date:        Thu Jan 01 00:00:17 1970 +0000
  │ │ │  summary:     (17) expand
  │ │ │
  o │ │    commit:      3677d192927d
  ├─────╮  user:        test
  │ │ │ │  date:        Thu Jan 01 00:00:16 1970 +0000
  ~ │ │ ~  summary:     (16) merge two known; one immediate right, one near right
    │ │
    o │  commit:      1dda3f72782d
  ╭─┤ │  user:        test
  │ │ │  date:        Thu Jan 01 00:00:15 1970 +0000
  │ │ │  summary:     (15) expand
  │ │ │
  o │ │  commit:      8eac370358ef
  ├───╮  user:        test
  │ │ │  date:        Thu Jan 01 00:00:14 1970 +0000
  ~ │ │  summary:     (14) merge two known; one immediate right, one far right
    │ │
    o │  commit:      22d8966a97e3
  ╭─┤ │  user:        test
  │ │ │  date:        Thu Jan 01 00:00:13 1970 +0000
  │ │ │  summary:     (13) expand
  │ │ │
  │ │ o  commit:      86b91144a6e9
  │ ╭─┤  user:        test
  │ │ │  date:        Thu Jan 01 00:00:12 1970 +0000
  │ │ ~  summary:     (12) merge two known; one immediate right, one far left
  │ │
  o │    commit:      832d76e6bdf2
  ├───╮  user:        test
  │ │ │  date:        Thu Jan 01 00:00:11 1970 +0000
  │ │ │  summary:     (11) expand
  │ │ │
  │ │ o  commit:      74c64d036d72
  ╭───┤  user:        test
  │ │ │  date:        Thu Jan 01 00:00:10 1970 +0000
  │ │ ~  summary:     (10) merge two known; one immediate left, one near right
  │ │
  │ o    commit:      7010c0af0a35
  │ ├─╮  user:        test
  │ │ │  date:        Thu Jan 01 00:00:09 1970 +0000
  │ │ │  summary:     (9) expand
  │ │ │
  │ │ o  commit:      7a0b11f71937
  │ ╭─┤  user:        test
  │ │ │  date:        Thu Jan 01 00:00:08 1970 +0000
  │ │ ~  summary:     (8) merge two known; one immediate left, one far right
  │ │
  │ o    commit:      b632bb1b1224
  │ ├─╮  user:        test
  │ │ │  date:        Thu Jan 01 00:00:07 1970 +0000
  │ │ ~  summary:     (7) expand
  │ │
  o │  commit:      b105a072e251
  ├─╮  user:        test
  │ │  date:        Thu Jan 01 00:00:06 1970 +0000
  ~ │  summary:     (6) merge two known; one immediate left, one far left
    │
    o  commit:      4409d547b708
  ╭─┤  user:        test
  │ │  date:        Thu Jan 01 00:00:05 1970 +0000
  ~ │  summary:     (5) expand
    │
    o  commit:      26a8bac39d9f
  ╭─┤  user:        test
  │ │  date:        Thu Jan 01 00:00:04 1970 +0000
  ~ ~  summary:     (4) merge two known; one immediate left, one immediate right

# Setting HGPLAIN sets graphmod styling to ASCII:

  $ HGPLAIN=1 hg log -G -r 'file("a")' -m
  @  commit:      95fa8febd08a
  .  user:        test
  .  date:        Thu Jan 01 00:00:36 1970 +0000
  .  summary:     (36) buggy merge: identical parents
  .
  o    commit:      d06dffa21a31
  |\   user:        test
  | .  date:        Thu Jan 01 00:00:32 1970 +0000
  | .  summary:     (32) expand
  | .
  o .  commit:      621d83e11f67
  |\.  user:        test
  | |  date:        Thu Jan 01 00:00:31 1970 +0000
  | |  summary:     (31) expand
  | |
  o |    commit:      6e11cd4b648f
  +---.  user:        test
  | | |  date:        Thu Jan 01 00:00:30 1970 +0000
  | | ~  summary:     (30) expand
  | |
  o |    commit:      44ecd0b9ae99
  +---.  user:        test
  | | |  date:        Thu Jan 01 00:00:28 1970 +0000
  | | ~  summary:     (28) merge zero known
  | |
  o |    commit:      7f25b6c2f0b9
  +---.  user:        test
  | | |  date:        Thu Jan 01 00:00:26 1970 +0000
  | | |  summary:     (26) merge one known; far right
  | | |
  | | o  commit:      91da8ed57247
  | |/|  user:        test
  | | |  date:        Thu Jan 01 00:00:25 1970 +0000
  | | |  summary:     (25) merge one known; far left
  | | |
  | | o    commit:      a9c19a3d96b7
  | | |\   user:        test
  | | | |  date:        Thu Jan 01 00:00:24 1970 +0000
  | | | ~  summary:     (24) merge one known; immediate right
  | | |
  | | o    commit:      a01cddf0766d
  | | |\   user:        test
  | | | |  date:        Thu Jan 01 00:00:23 1970 +0000
  | | | ~  summary:     (23) merge one known; immediate left
  | | |
  | | o  commit:      e0d9cccacb5d
  +-+-'  user:        test
  | |    date:        Thu Jan 01 00:00:22 1970 +0000
  | |    summary:     (22) merge two known; one far left, one far right
  | |
  | o    commit:      d42a756af44d
  | |\   user:        test
  | | |  date:        Thu Jan 01 00:00:21 1970 +0000
  | | |  summary:     (21) expand
  | | |
  | | o  commit:      d30ed6450e32
  +---+  user:        test
  | | |  date:        Thu Jan 01 00:00:20 1970 +0000
  | | ~  summary:     (20) merge two known; two far right
  | |
  | o    commit:      31ddc2c1573b
  | |\   user:        test
  | | |  date:        Thu Jan 01 00:00:19 1970 +0000
  | | |  summary:     (19) expand
  | | |
  o | |  commit:      1aa84d96232a
  |\| |  user:        test
  | | |  date:        Thu Jan 01 00:00:18 1970 +0000
  ~ | |  summary:     (18) merge two known; two far left
    | |
    | o  commit:      44765d7c06e0
  .---+  user:        test
  | | |  date:        Thu Jan 01 00:00:17 1970 +0000
  | | |  summary:     (17) expand
  | | |
  o | |    commit:      3677d192927d
  +-----.  user:        test
  | | | |  date:        Thu Jan 01 00:00:16 1970 +0000
  ~ | | ~  summary:     (16) merge two known; one immediate right, one near right
    | |
    o |  commit:      1dda3f72782d
   /| |  user:        test
  | | |  date:        Thu Jan 01 00:00:15 1970 +0000
  | | |  summary:     (15) expand
  | | |
  o | |  commit:      8eac370358ef
  +---+  user:        test
  | | |  date:        Thu Jan 01 00:00:14 1970 +0000
  ~ | |  summary:     (14) merge two known; one immediate right, one far right
    | |
    o |  commit:      22d8966a97e3
   /| |  user:        test
  | | |  date:        Thu Jan 01 00:00:13 1970 +0000
  | | |  summary:     (13) expand
  | | |
  | | o  commit:      86b91144a6e9
  | |/|  user:        test
  | | |  date:        Thu Jan 01 00:00:12 1970 +0000
  | | ~  summary:     (12) merge two known; one immediate right, one far left
  | |
  o |    commit:      832d76e6bdf2
  +---.  user:        test
  | | |  date:        Thu Jan 01 00:00:11 1970 +0000
  | | |  summary:     (11) expand
  | | |
  | | o  commit:      74c64d036d72
  +---+  user:        test
  | | |  date:        Thu Jan 01 00:00:10 1970 +0000
  | | ~  summary:     (10) merge two known; one immediate left, one near right
  | |
  | o    commit:      7010c0af0a35
  | |\   user:        test
  | | |  date:        Thu Jan 01 00:00:09 1970 +0000
  | | |  summary:     (9) expand
  | | |
  | | o  commit:      7a0b11f71937
  | |/|  user:        test
  | | |  date:        Thu Jan 01 00:00:08 1970 +0000
  | | ~  summary:     (8) merge two known; one immediate left, one far right
  | |
  | o    commit:      b632bb1b1224
  | |\   user:        test
  | | |  date:        Thu Jan 01 00:00:07 1970 +0000
  | | ~  summary:     (7) expand
  | |
  o |  commit:      b105a072e251
  |\|  user:        test
  | |  date:        Thu Jan 01 00:00:06 1970 +0000
  ~ |  summary:     (6) merge two known; one immediate left, one far left
    |
    o  commit:      4409d547b708
   /|  user:        test
  | |  date:        Thu Jan 01 00:00:05 1970 +0000
  ~ |  summary:     (5) expand
    |
    o  commit:      26a8bac39d9f
   /|  user:        test
  | |  date:        Thu Jan 01 00:00:04 1970 +0000
  ~ ~  summary:     (4) merge two known; one immediate left, one immediate right

# .. unless HGPLAINEXCEPT=graph is set:

  $ HGPLAIN=1 HGPLAINEXCEPT=graph hg log -G -r 'file("a")' -m
  @  commit:      95fa8febd08a
  ╷  user:        test
  ╷  date:        Thu Jan 01 00:00:36 1970 +0000
  ╷  summary:     (36) buggy merge: identical parents
  ╷
  o    commit:      d06dffa21a31
  ├─╮  user:        test
  │ ╷  date:        Thu Jan 01 00:00:32 1970 +0000
  │ ╷  summary:     (32) expand
  │ ╷
  o ╷  commit:      621d83e11f67
  ├─╮  user:        test
  │ │  date:        Thu Jan 01 00:00:31 1970 +0000
  │ │  summary:     (31) expand
  │ │
  o │    commit:      6e11cd4b648f
  ├───╮  user:        test
  │ │ │  date:        Thu Jan 01 00:00:30 1970 +0000
  │ │ ~  summary:     (30) expand
  │ │
  o │    commit:      44ecd0b9ae99
  ├───╮  user:        test
  │ │ │  date:        Thu Jan 01 00:00:28 1970 +0000
  │ │ ~  summary:     (28) merge zero known
  │ │
  o │    commit:      7f25b6c2f0b9
  ├───╮  user:        test
  │ │ │  date:        Thu Jan 01 00:00:26 1970 +0000
  │ │ │  summary:     (26) merge one known; far right
  │ │ │
  │ │ o  commit:      91da8ed57247
  │ ╭─┤  user:        test
  │ │ │  date:        Thu Jan 01 00:00:25 1970 +0000
  │ │ │  summary:     (25) merge one known; far left
  │ │ │
  │ │ o    commit:      a9c19a3d96b7
  │ │ ├─╮  user:        test
  │ │ │ │  date:        Thu Jan 01 00:00:24 1970 +0000
  │ │ │ ~  summary:     (24) merge one known; immediate right
  │ │ │
  │ │ o    commit:      a01cddf0766d
  │ │ ├─╮  user:        test
  │ │ │ │  date:        Thu Jan 01 00:00:23 1970 +0000
  │ │ │ ~  summary:     (23) merge one known; immediate left
  │ │ │
  │ │ o  commit:      e0d9cccacb5d
  ╭─┬─╯  user:        test
  │ │    date:        Thu Jan 01 00:00:22 1970 +0000
  │ │    summary:     (22) merge two known; one far left, one far right
  │ │
  │ o    commit:      d42a756af44d
  │ ├─╮  user:        test
  │ │ │  date:        Thu Jan 01 00:00:21 1970 +0000
  │ │ │  summary:     (21) expand
  │ │ │
  │ │ o  commit:      d30ed6450e32
  ╭───┤  user:        test
  │ │ │  date:        Thu Jan 01 00:00:20 1970 +0000
  │ │ ~  summary:     (20) merge two known; two far right
  │ │
  │ o    commit:      31ddc2c1573b
  │ ├─╮  user:        test
  │ │ │  date:        Thu Jan 01 00:00:19 1970 +0000
  │ │ │  summary:     (19) expand
  │ │ │
  o │ │  commit:      1aa84d96232a
  ├─╮ │  user:        test
  │ │ │  date:        Thu Jan 01 00:00:18 1970 +0000
  ~ │ │  summary:     (18) merge two known; two far left
    │ │
    │ o  commit:      44765d7c06e0
  ╭───┤  user:        test
  │ │ │  date:        Thu Jan 01 00:00:17 1970 +0000
  │ │ │  summary:     (17) expand
  │ │ │
  o │ │    commit:      3677d192927d
  ├─────╮  user:        test
  │ │ │ │  date:        Thu Jan 01 00:00:16 1970 +0000
  ~ │ │ ~  summary:     (16) merge two known; one immediate right, one near right
    │ │
    o │  commit:      1dda3f72782d
  ╭─┤ │  user:        test
  │ │ │  date:        Thu Jan 01 00:00:15 1970 +0000
  │ │ │  summary:     (15) expand
  │ │ │
  o │ │  commit:      8eac370358ef
  ├───╮  user:        test
  │ │ │  date:        Thu Jan 01 00:00:14 1970 +0000
  ~ │ │  summary:     (14) merge two known; one immediate right, one far right
    │ │
    o │  commit:      22d8966a97e3
  ╭─┤ │  user:        test
  │ │ │  date:        Thu Jan 01 00:00:13 1970 +0000
  │ │ │  summary:     (13) expand
  │ │ │
  │ │ o  commit:      86b91144a6e9
  │ ╭─┤  user:        test
  │ │ │  date:        Thu Jan 01 00:00:12 1970 +0000
  │ │ ~  summary:     (12) merge two known; one immediate right, one far left
  │ │
  o │    commit:      832d76e6bdf2
  ├───╮  user:        test
  │ │ │  date:        Thu Jan 01 00:00:11 1970 +0000
  │ │ │  summary:     (11) expand
  │ │ │
  │ │ o  commit:      74c64d036d72
  ╭───┤  user:        test
  │ │ │  date:        Thu Jan 01 00:00:10 1970 +0000
  │ │ ~  summary:     (10) merge two known; one immediate left, one near right
  │ │
  │ o    commit:      7010c0af0a35
  │ ├─╮  user:        test
  │ │ │  date:        Thu Jan 01 00:00:09 1970 +0000
  │ │ │  summary:     (9) expand
  │ │ │
  │ │ o  commit:      7a0b11f71937
  │ ╭─┤  user:        test
  │ │ │  date:        Thu Jan 01 00:00:08 1970 +0000
  │ │ ~  summary:     (8) merge two known; one immediate left, one far right
  │ │
  │ o    commit:      b632bb1b1224
  │ ├─╮  user:        test
  │ │ │  date:        Thu Jan 01 00:00:07 1970 +0000
  │ │ ~  summary:     (7) expand
  │ │
  o │  commit:      b105a072e251
  ├─╮  user:        test
  │ │  date:        Thu Jan 01 00:00:06 1970 +0000
  ~ │  summary:     (6) merge two known; one immediate left, one far left
    │
    o  commit:      4409d547b708
  ╭─┤  user:        test
  │ │  date:        Thu Jan 01 00:00:05 1970 +0000
  ~ │  summary:     (5) expand
    │
    o  commit:      26a8bac39d9f
  ╭─┤  user:        test
  │ │  date:        Thu Jan 01 00:00:04 1970 +0000
  ~ ~  summary:     (4) merge two known; one immediate left, one immediate right

# Draw only part of a grandparent line differently with "<N><char>"; only the
# last N lines (for positive N) or everything but the first N lines (for
# negative N) along the current node use the style, the rest of the edge uses
# the parent edge styling.
# Last 3 lines:

  $ cat >> $HGRCPATH << 'EOF'
  > [experimental]
  > graphstyle.parent = !
  > graphstyle.grandparent = 3.
  > graphstyle.missing =
  > EOF
  $ hg log -G -r '36:18 & file("a")' -m
  @  commit:      95fa8febd08a
  ╷  user:        test
  ╷  date:        Thu Jan 01 00:00:36 1970 +0000
  ╷  summary:     (36) buggy merge: identical parents
  ╷
  o    commit:      d06dffa21a31
  ├─╮  user:        test
  │ ╷  date:        Thu Jan 01 00:00:32 1970 +0000
  │ ╷  summary:     (32) expand
  │ ╷
  o ╷  commit:      621d83e11f67
  ├─╮  user:        test
  │ │  date:        Thu Jan 01 00:00:31 1970 +0000
  │ │  summary:     (31) expand
  │ │
  o │    commit:      6e11cd4b648f
  ├───╮  user:        test
  │ │ │  date:        Thu Jan 01 00:00:30 1970 +0000
  │ │ ~  summary:     (30) expand
  │ │
  o │    commit:      44ecd0b9ae99
  ├───╮  user:        test
  │ │ │  date:        Thu Jan 01 00:00:28 1970 +0000
  │ │ ~  summary:     (28) merge zero known
  │ │
  o │    commit:      7f25b6c2f0b9
  ├───╮  user:        test
  │ │ │  date:        Thu Jan 01 00:00:26 1970 +0000
  │ │ │  summary:     (26) merge one known; far right
  │ │ │
  │ │ o  commit:      91da8ed57247
  │ ╭─┤  user:        test
  │ │ │  date:        Thu Jan 01 00:00:25 1970 +0000
  │ │ │  summary:     (25) merge one known; far left
  │ │ │
  │ │ o    commit:      a9c19a3d96b7
  │ │ ├─╮  user:        test
  │ │ │ │  date:        Thu Jan 01 00:00:24 1970 +0000
  │ │ │ ~  summary:     (24) merge one known; immediate right
  │ │ │
  │ │ o    commit:      a01cddf0766d
  │ │ ├─╮  user:        test
  │ │ │ │  date:        Thu Jan 01 00:00:23 1970 +0000
  │ │ │ ~  summary:     (23) merge one known; immediate left
  │ │ │
  │ │ o  commit:      e0d9cccacb5d
  ╭─┬─╯  user:        test
  │ │    date:        Thu Jan 01 00:00:22 1970 +0000
  │ │    summary:     (22) merge two known; one far left, one far right
  │ │
  │ o    commit:      d42a756af44d
  │ ├─╮  user:        test
  │ │ │  date:        Thu Jan 01 00:00:21 1970 +0000
  │ │ │  summary:     (21) expand
  │ │ │
  │ │ o  commit:      d30ed6450e32
  ╭───┤  user:        test
  │ │ │  date:        Thu Jan 01 00:00:20 1970 +0000
  │ │ ~  summary:     (20) merge two known; two far right
  │ │
  │ o    commit:      31ddc2c1573b
  │ ├─╮  user:        test
  │ │ │  date:        Thu Jan 01 00:00:19 1970 +0000
  │ ~ ~  summary:     (19) expand
  │
  o    commit:      1aa84d96232a
  ├─╮  user:        test
  │ │  date:        Thu Jan 01 00:00:18 1970 +0000
  ~ ~  summary:     (18) merge two known; two far left

# All but the first 3 lines:

  $ cat >> $HGRCPATH << 'EOF'
  > [experimental]
  > graphstyle.parent = !
  > graphstyle.grandparent = -3.
  > graphstyle.missing =
  > EOF
  $ hg log -G -r '36:18 & file("a")' -m
  @  commit:      95fa8febd08a
  ╷  user:        test
  ╷  date:        Thu Jan 01 00:00:36 1970 +0000
  ╷  summary:     (36) buggy merge: identical parents
  ╷
  o    commit:      d06dffa21a31
  ├─╮  user:        test
  │ ╷  date:        Thu Jan 01 00:00:32 1970 +0000
  │ ╷  summary:     (32) expand
  │ ╷
  o ╷  commit:      621d83e11f67
  ├─╮  user:        test
  │ │  date:        Thu Jan 01 00:00:31 1970 +0000
  │ │  summary:     (31) expand
  │ │
  o │    commit:      6e11cd4b648f
  ├───╮  user:        test
  │ │ │  date:        Thu Jan 01 00:00:30 1970 +0000
  │ │ ~  summary:     (30) expand
  │ │
  o │    commit:      44ecd0b9ae99
  ├───╮  user:        test
  │ │ │  date:        Thu Jan 01 00:00:28 1970 +0000
  │ │ ~  summary:     (28) merge zero known
  │ │
  o │    commit:      7f25b6c2f0b9
  ├───╮  user:        test
  │ │ │  date:        Thu Jan 01 00:00:26 1970 +0000
  │ │ │  summary:     (26) merge one known; far right
  │ │ │
  │ │ o  commit:      91da8ed57247
  │ ╭─┤  user:        test
  │ │ │  date:        Thu Jan 01 00:00:25 1970 +0000
  │ │ │  summary:     (25) merge one known; far left
  │ │ │
  │ │ o    commit:      a9c19a3d96b7
  │ │ ├─╮  user:        test
  │ │ │ │  date:        Thu Jan 01 00:00:24 1970 +0000
  │ │ │ ~  summary:     (24) merge one known; immediate right
  │ │ │
  │ │ o    commit:      a01cddf0766d
  │ │ ├─╮  user:        test
  │ │ │ │  date:        Thu Jan 01 00:00:23 1970 +0000
  │ │ │ ~  summary:     (23) merge one known; immediate left
  │ │ │
  │ │ o  commit:      e0d9cccacb5d
  ╭─┬─╯  user:        test
  │ │    date:        Thu Jan 01 00:00:22 1970 +0000
  │ │    summary:     (22) merge two known; one far left, one far right
  │ │
  │ o    commit:      d42a756af44d
  │ ├─╮  user:        test
  │ │ │  date:        Thu Jan 01 00:00:21 1970 +0000
  │ │ │  summary:     (21) expand
  │ │ │
  │ │ o  commit:      d30ed6450e32
  ╭───┤  user:        test
  │ │ │  date:        Thu Jan 01 00:00:20 1970 +0000
  │ │ ~  summary:     (20) merge two known; two far right
  │ │
  │ o    commit:      31ddc2c1573b
  │ ├─╮  user:        test
  │ │ │  date:        Thu Jan 01 00:00:19 1970 +0000
  │ ~ ~  summary:     (19) expand
  │
  o    commit:      1aa84d96232a
  ├─╮  user:        test
  │ │  date:        Thu Jan 01 00:00:18 1970 +0000
  ~ ~  summary:     (18) merge two known; two far left
  $ cd ..

# Change graph shorten, test better with graphstyle.missing not none

  $ cd repo
  $ cat >> $HGRCPATH << 'EOF'
  > [experimental]
  > graphstyle.parent = |
  > graphstyle.grandparent = :
  > graphstyle.missing = '
  > graphshorten = true
  > EOF
  $ hg log -G -r 'file("a")' -m -T '{rev} {desc}'
  @  36 (36) buggy merge: identical parents
  o    32 (32) expand
  ├─╮
  o ╷  31 (31) expand
  ├─╮
  o │    30 (30) expand
  ├───╮
  │ │ │
  │ │ ~
  │ │
  o │    28 (28) merge zero known
  ├───╮
  │ │ │
  │ │ ~
  │ │
  o │    26 (26) merge one known; far right
  ├───╮
  │ │ o  25 (25) merge one known; far left
  │ ╭─┤
  │ │ o    24 (24) merge one known; immediate right
  │ │ ├─╮
  │ │ │ │
  │ │ │ ~
  │ │ │
  │ │ o    23 (23) merge one known; immediate left
  │ │ ├─╮
  │ │ │ │
  │ │ │ ~
  │ │ │
  │ │ o  22 (22) merge two known; one far left, one far right
  ╭─┬─╯
  │ o    21 (21) expand
  │ ├─╮
  │ │ o  20 (20) merge two known; two far right
  ╭───┤
  │ │ │
  │ │ ~
  │ │
  │ o    19 (19) expand
  │ ├─╮
  o │ │  18 (18) merge two known; two far left
  ├─╮ │
  │ │ │
  ~ │ │
    │ │
    │ o  17 (17) expand
  ╭───┤
  o │ │    16 (16) merge two known; one immediate right, one near right
  ├─────╮
  │ │ │ │
  ~ │ │ ~
    │ │
    o │  15 (15) expand
  ╭─┤ │
  o │ │  14 (14) merge two known; one immediate right, one far right
  ├───╮
  │ │ │
  ~ │ │
    │ │
    o │  13 (13) expand
  ╭─┤ │
  │ │ o  12 (12) merge two known; one immediate right, one far left
  │ ╭─┤
  │ │ │
  │ │ ~
  │ │
  o │    11 (11) expand
  ├───╮
  │ │ o  10 (10) merge two known; one immediate left, one near right
  ╭───┤
  │ │ │
  │ │ ~
  │ │
  │ o    9 (9) expand
  │ ├─╮
  │ │ o  8 (8) merge two known; one immediate left, one far right
  │ ╭─┤
  │ │ │
  │ │ ~
  │ │
  │ o    7 (7) expand
  │ ├─╮
  │ │ │
  │ │ ~
  │ │
  o │  6 (6) merge two known; one immediate left, one far left
  ├─╮
  │ │
  ~ │
    │
    o  5 (5) expand
  ╭─┤
  │ │
  ~ │
    │
    o  4 (4) merge two known; one immediate left, one immediate right
  ╭─┤
  │ │
  ~ ~

# behavior with newlines

  $ hg log -G -r '::2' -T '{rev} {desc}'
  o  2 (2) collapse
  o  1 (1) collapse
  o  0 (0) root

  $ hg log -G -r '::2' -T '{rev} {desc}\n'
  o  2 (2) collapse
  o  1 (1) collapse
  o  0 (0) root

  $ hg log -G -r '::2' -T '{rev} {desc}\n\n'
  o  2 (2) collapse
  │
  o  1 (1) collapse
  │
  o  0 (0) root

  $ hg log -G -r '::2' -T '\n{rev} {desc}'
  o
  │  2 (2) collapse
  o
  │  1 (1) collapse
  o
     0 (0) root

  $ hg log -G -r '::2' -T '{rev} {desc}\n\n\n'
  o  2 (2) collapse
  │
  │
  o  1 (1) collapse
  │
  │
  o  0 (0) root
  $ cd ..

# When inserting extra line nodes to handle more than 2 parents, ensure that
# the right node styles are used (issue5174):

  $ hg init repo-issue5174
  $ cd repo-issue5174
  $ echo a > f0
  $ hg ci -Aqm 0
  $ echo a > f1
  $ hg ci -Aqm 1
  $ echo a > f2
  $ hg ci -Aqm 2
  $ hg co '.^'
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo a > f3
  $ hg ci -Aqm 3
  $ hg co '.^^'
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ echo a > f4
  $ hg ci -Aqm 4
  $ hg merge -r 2
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg ci -qm 5
  $ hg merge -r 3
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg ci -qm 6
  $ hg log -G -r '0 | 1 | 2 | 6'
  @    commit:      851fe89689ad
  ├─╮  user:        test
  ╷ ╷  date:        Thu Jan 01 00:00:00 1970 +0000
  ╷ ╷  summary:     6
  ╷ ╷
  o ╷  commit:      3e6599df4cce
  ├─╯  user:        test
  │    date:        Thu Jan 01 00:00:00 1970 +0000
  │    summary:     2
  │
  o  commit:      bd9a55143933
  │  user:        test
  │  date:        Thu Jan 01 00:00:00 1970 +0000
  │  summary:     1
  │
  o  commit:      870a5edc339c
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     0

  $ cd ..

# Multiple roots (issue5440):

  $ hg init multiroots
  $ cd multiroots
  $ cat > .hg/hgrc << 'EOF'
  > [ui]
  > logtemplate = '{rev} {desc}\n\n'
  > EOF

  $ touch foo
  $ hg ci -Aqm foo
  $ hg co -q null
  $ touch bar
  $ hg ci -Aqm bar

  $ hg log -Gr 'null:'
  @  1 bar
  
  o  0 foo
  
  o  -1
  $ hg log -Gr null+0
  o  0 foo
  
  o  -1
  $ hg log -Gr null+1
  @  1 bar
  
  o  -1

  $ cd ..
