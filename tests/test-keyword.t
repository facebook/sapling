  $ cat <<EOF >> $HGRCPATH
  > [extensions]
  > keyword =
  > mq =
  > notify =
  > record =
  > transplant =
  > [ui]
  > interactive = true
  > EOF

hide outer repo
  $ hg init

Run kwdemo before [keyword] files are set up
as it would succeed without uisetup otherwise

  $ hg --quiet kwdemo
  [extensions]
  keyword =
  [keyword]
  demo.txt = 
  [keywordset]
  svn = False
  [keywordmaps]
  Author = {author|user}
  Date = {date|utcdate}
  Header = {root}/{file},v {node|short} {date|utcdate} {author|user}
  Id = {file|basename},v {node|short} {date|utcdate} {author|user}
  RCSFile = {file|basename},v
  RCSfile = {file|basename},v
  Revision = {node|short}
  Source = {root}/{file},v
  $Author: test $
  $Date: ????/??/?? ??:??:?? $ (glob)
  $Header: */demo.txt,v ???????????? ????/??/?? ??:??:?? test $ (glob)
  $Id: demo.txt,v ???????????? ????/??/?? ??:??:?? test $ (glob)
  $RCSFile: demo.txt,v $
  $RCSfile: demo.txt,v $
  $Revision: ???????????? $ (glob)
  $Source: */demo.txt,v $ (glob)

  $ hg --quiet kwdemo "Branch = {branches}"
  [extensions]
  keyword =
  [keyword]
  demo.txt = 
  [keywordset]
  svn = False
  [keywordmaps]
  Branch = {branches}
  $Branch: demobranch $

  $ cat <<EOF >> $HGRCPATH
  > [keyword]
  > ** =
  > b = ignore
  > i = ignore
  > [hooks]
  > EOF
  $ cp $HGRCPATH $HGRCPATH.nohooks
  > cat <<EOF >> $HGRCPATH
  > commit=
  > commit.test=cp a hooktest
  > EOF

  $ hg init Test-bndl
  $ cd Test-bndl

kwshrink should exit silently in empty/invalid repo

  $ hg kwshrink

Symlinks cannot be created on Windows.
A bundle to test this was made with:
 hg init t
 cd t
 echo a > a
 ln -s a sym
 hg add sym
 hg ci -m addsym -u mercurial
 hg bundle --base null ../test-keyword.hg

  $ hg pull -u "$TESTDIR"/bundles/test-keyword.hg
  pulling from *test-keyword.hg (glob)
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ echo 'expand $Id$' > a
  $ echo 'do not process $Id:' >> a
  $ echo 'xxx $' >> a
  $ echo 'ignore $Id$' > b

Output files as they were created

  $ cat a b
  expand $Id$
  do not process $Id:
  xxx $
  ignore $Id$

no kwfiles

  $ hg kwfiles

untracked candidates

  $ hg -v kwfiles --unknown
  k a

Add files and check status

  $ hg addremove
  adding a
  adding b
  $ hg status
  A a
  A b


Default keyword expansion including commit hook
Interrupted commit should not change state or run commit hook

  $ hg --debug commit
  abort: empty commit message
  [255]
  $ hg status
  A a
  A b

Commit with several checks

  $ hg --debug commit -mabsym -u 'User Name <user@example.com>'
  committing files:
  a
  b
  committing manifest
  committing changelog
  overwriting a expanding keywords
  running hook commit.test: cp a hooktest
  committed changeset 1:ef63ca68695bc9495032c6fda1350c71e6d256e9
  $ hg status
  ? hooktest
  $ hg debugrebuildstate
  $ hg --quiet identify
  ef63ca68695b

cat files in working directory with keywords expanded

  $ cat a b
  expand $Id: a,v ef63ca68695b 1970/01/01 00:00:00 user $
  do not process $Id:
  xxx $
  ignore $Id$

hg cat files and symlink, no expansion

  $ hg cat sym a b && echo
  expand $Id: a,v ef63ca68695b 1970/01/01 00:00:00 user $
  do not process $Id:
  xxx $
  ignore $Id$
  a

  $ diff a hooktest

  $ cp $HGRCPATH.nohooks $HGRCPATH
  $ rm hooktest

hg status of kw-ignored binary file starting with '\1\n'

  >>> open("i", "wb").write("\1\nfoo")
  $ hg -q commit -Am metasep i
  $ hg status
  >>> open("i", "wb").write("\1\nbar")
  $ hg status
  M i
  $ hg -q commit -m "modify metasep" i
  $ hg status --rev 2:3
  M i
  $ touch empty
  $ hg -q commit -A -m "another file"
  $ hg status -A --rev 3:4 i
  C i

  $ hg -q strip --no-backup 2

Test hook execution

bundle

  $ hg bundle --base null ../kw.hg
  2 changesets found
  $ cd ..
  $ hg init Test
  $ cd Test

Notify on pull to check whether keywords stay as is in email
ie. if patch.diff wrapper acts as it should

  $ cat <<EOF >> $HGRCPATH
  > [hooks]
  > incoming.notify = python:hgext.notify.hook
  > [notify]
  > sources = pull
  > diffstat = False
  > maxsubject = 15
  > [reposubs]
  > * = Test
  > EOF

Pull from bundle and trigger notify

  $ hg pull -u ../kw.hg
  pulling from ../kw.hg
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 3 changes to 3 files
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Date: * (glob)
  Subject: changeset in...
  From: mercurial
  X-Hg-Notification: changeset a2392c293916
  Message-Id: <hg.a2392c293916*> (glob)
  To: Test
  
  changeset a2392c293916 in $TESTTMP/Test (glob)
  details: $TESTTMP/Test?cmd=changeset;node=a2392c293916
  description:
  	addsym
  
  diffs (6 lines):
  
  diff -r 000000000000 -r a2392c293916 sym
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/sym	Sat Feb 09 20:25:47 2008 +0100
  @@ -0,0 +1,1 @@
  +a
  \ No newline at end of file
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Date:* (glob)
  Subject: changeset in...
  From: User Name <user@example.com>
  X-Hg-Notification: changeset ef63ca68695b
  Message-Id: <hg.ef63ca68695b*> (glob)
  To: Test
  
  changeset ef63ca68695b in $TESTTMP/Test (glob)
  details: $TESTTMP/Test?cmd=changeset;node=ef63ca68695b
  description:
  	absym
  
  diffs (12 lines):
  
  diff -r a2392c293916 -r ef63ca68695b a
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,3 @@
  +expand $Id$
  +do not process $Id:
  +xxx $
  diff -r a2392c293916 -r ef63ca68695b b
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/b	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +ignore $Id$
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ cp $HGRCPATH.nohooks $HGRCPATH

Touch files and check with status

  $ touch a b
  $ hg status

Update and expand

  $ rm sym a b
  $ hg update -C
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cat a b
  expand $Id: a,v ef63ca68695b 1970/01/01 00:00:00 user $
  do not process $Id:
  xxx $
  ignore $Id$

Check whether expansion is filewise and file mode is preserved

  $ echo '$Id$' > c
  $ echo 'tests for different changenodes' >> c
#if unix-permissions
  $ chmod 600 c
  $ ls -l c | cut -b 1-10
  -rw-------
#endif

commit file c

  $ hg commit -A -mcndiff -d '1 0' -u 'User Name <user@example.com>'
  adding c
#if unix-permissions
  $ ls -l c | cut -b 1-10
  -rw-------
#endif

force expansion

  $ hg -v kwexpand
  overwriting a expanding keywords
  overwriting c expanding keywords

compare changenodes in a and c

  $ cat a c
  expand $Id: a,v ef63ca68695b 1970/01/01 00:00:00 user $
  do not process $Id:
  xxx $
  $Id: c,v 40a904bbbe4c 1970/01/01 00:00:01 user $
  tests for different changenodes

record

  $ echo '$Id$' > r
  $ hg add r

record chunk

  >>> lines = open('a', 'rb').readlines()
  >>> lines.insert(1, 'foo\n')
  >>> lines.append('bar\n')
  >>> open('a', 'wb').writelines(lines)
  $ hg record -d '10 1' -m rectest a<<EOF
  > y
  > y
  > n
  > EOF
  diff --git a/a b/a
  2 hunks, 2 lines changed
  examine changes to 'a'? [Ynesfdaq?] y
  
  @@ -1,3 +1,4 @@
   expand $Id$
  +foo
   do not process $Id:
   xxx $
  record change 1/2 to 'a'? [Ynesfdaq?] y
  
  @@ -2,2 +3,3 @@
   do not process $Id:
   xxx $
  +bar
  record change 2/2 to 'a'? [Ynesfdaq?] n
  

  $ hg identify
  5f5eb23505c3+ tip
  $ hg status
  M a
  A r

Cat modified file a

  $ cat a
  expand $Id: a,v 5f5eb23505c3 1970/01/01 00:00:10 test $
  foo
  do not process $Id:
  xxx $
  bar

Diff remaining chunk

  $ hg diff a
  diff -r 5f5eb23505c3 a
  --- a/a	Thu Jan 01 00:00:09 1970 -0000
  +++ b/a	* (glob)
  @@ -2,3 +2,4 @@
   foo
   do not process $Id:
   xxx $
  +bar

  $ hg rollback
  repository tip rolled back to revision 2 (undo commit)
  working directory now based on revision 2

Record all chunks in file a

  $ echo foo > msg

 - do not use "hg record -m" here!

  $ hg record -l msg -d '11 1' a<<EOF
  > y
  > y
  > y
  > EOF
  diff --git a/a b/a
  2 hunks, 2 lines changed
  examine changes to 'a'? [Ynesfdaq?] y
  
  @@ -1,3 +1,4 @@
   expand $Id$
  +foo
   do not process $Id:
   xxx $
  record change 1/2 to 'a'? [Ynesfdaq?] y
  
  @@ -2,2 +3,3 @@
   do not process $Id:
   xxx $
  +bar
  record change 2/2 to 'a'? [Ynesfdaq?] y
  

File a should be clean

  $ hg status -A a
  C a

rollback and revert expansion

  $ cat a
  expand $Id: a,v 78e0a02d76aa 1970/01/01 00:00:11 test $
  foo
  do not process $Id:
  xxx $
  bar
  $ hg --verbose rollback
  repository tip rolled back to revision 2 (undo commit)
  working directory now based on revision 2
  overwriting a expanding keywords
  $ hg status a
  M a
  $ cat a
  expand $Id: a,v ef63ca68695b 1970/01/01 00:00:00 user $
  foo
  do not process $Id:
  xxx $
  bar
  $ echo '$Id$' > y
  $ echo '$Id$' > z
  $ hg add y
  $ hg commit -Am "rollback only" z
  $ cat z
  $Id: z,v 45a5d3adce53 1970/01/01 00:00:00 test $
  $ hg --verbose rollback
  repository tip rolled back to revision 2 (undo commit)
  working directory now based on revision 2
  overwriting z shrinking keywords

Only z should be overwritten

  $ hg status a y z
  M a
  A y
  A z
  $ cat z
  $Id$
  $ hg forget y z
  $ rm y z

record added file alone

  $ hg -v record -l msg -d '12 2' r<<EOF
  > y
  > y
  > EOF
  diff --git a/r b/r
  new file mode 100644
  examine changes to 'r'? [Ynesfdaq?] y
  
  @@ -0,0 +1,1 @@
  +$Id$
  record this change to 'r'? [Ynesfdaq?] y
  
  resolving manifests
  patching file r
  committing files:
  r
  committing manifest
  committing changelog
  committed changeset 3:82a2f715724d
  overwriting r expanding keywords
  $ hg status r
  $ hg --verbose rollback
  repository tip rolled back to revision 2 (undo commit)
  working directory now based on revision 2
  overwriting r shrinking keywords
  $ hg forget r
  $ rm msg r
  $ hg update -C
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

record added keyword ignored file

  $ echo '$Id$' > i
  $ hg add i
  $ hg --verbose record -d '13 1' -m recignored<<EOF
  > y
  > y
  > EOF
  diff --git a/i b/i
  new file mode 100644
  examine changes to 'i'? [Ynesfdaq?] y
  
  @@ -0,0 +1,1 @@
  +$Id$
  record this change to 'i'? [Ynesfdaq?] y
  
  resolving manifests
  patching file i
  committing files:
  i
  committing manifest
  committing changelog
  committed changeset 3:9f40ceb5a072
  $ cat i
  $Id$
  $ hg -q rollback
  $ hg forget i
  $ rm i

amend

  $ echo amend >> a
  $ echo amend >> b
  $ hg -q commit -d '14 1' -m 'prepare amend'

  $ hg --debug commit --amend -d '15 1' -m 'amend without changes' | grep keywords
  overwriting a expanding keywords
  $ hg -q id
  67d8c481a6be
  $ head -1 a
  expand $Id: a,v 67d8c481a6be 1970/01/01 00:00:15 test $

  $ hg -q strip --no-backup tip

Test patch queue repo

  $ hg init --mq
  $ hg qimport -r tip -n mqtest.diff
  $ hg commit --mq -m mqtest

Keywords should not be expanded in patch

  $ cat .hg/patches/mqtest.diff
  # HG changeset patch
  # User User Name <user@example.com>
  # Date 1 0
  #      Thu Jan 01 00:00:01 1970 +0000
  # Node ID 40a904bbbe4cd4ab0a1f28411e35db26341a40ad
  # Parent  ef63ca68695bc9495032c6fda1350c71e6d256e9
  cndiff
  
  diff -r ef63ca68695b -r 40a904bbbe4c c
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/c	Thu Jan 01 00:00:01 1970 +0000
  @@ -0,0 +1,2 @@
  +$Id$
  +tests for different changenodes

  $ hg qpop
  popping mqtest.diff
  patch queue now empty

qgoto, implying qpush, should expand

  $ hg qgoto mqtest.diff
  applying mqtest.diff
  now at: mqtest.diff
  $ cat c
  $Id: c,v 40a904bbbe4c 1970/01/01 00:00:01 user $
  tests for different changenodes
  $ hg cat c
  $Id: c,v 40a904bbbe4c 1970/01/01 00:00:01 user $
  tests for different changenodes

Keywords should not be expanded in filelog

  $ hg --config 'extensions.keyword=!' cat c
  $Id$
  tests for different changenodes

qpop and move on

  $ hg qpop
  popping mqtest.diff
  patch queue now empty

Copy and show added kwfiles

  $ hg cp a c
  $ hg kwfiles
  a
  c

Commit and show expansion in original and copy

  $ hg --debug commit -ma2c -d '1 0' -u 'User Name <user@example.com>'
  committing files:
  c
   c: copy a:0045e12f6c5791aac80ca6cbfd97709a88307292
  committing manifest
  committing changelog
  overwriting c expanding keywords
  committed changeset 2:25736cf2f5cbe41f6be4e6784ef6ecf9f3bbcc7d
  $ cat a c
  expand $Id: a,v ef63ca68695b 1970/01/01 00:00:00 user $
  do not process $Id:
  xxx $
  expand $Id: c,v 25736cf2f5cb 1970/01/01 00:00:01 user $
  do not process $Id:
  xxx $

Touch copied c and check its status

  $ touch c
  $ hg status

Copy kwfile to keyword ignored file unexpanding keywords

  $ hg --verbose copy a i
  copying a to i
  overwriting i shrinking keywords
  $ head -n 1 i
  expand $Id$
  $ hg forget i
  $ rm i

Copy ignored file to ignored file: no overwriting

  $ hg --verbose copy b i
  copying b to i
  $ hg forget i
  $ rm i

cp symlink file; hg cp -A symlink file (part1)
- copied symlink points to kwfile: overwrite

#if symlink
  $ cp sym i
  $ ls -l i
  -rw-r--r--* (glob)
  $ head -1 i
  expand $Id: a,v ef63ca68695b 1970/01/01 00:00:00 user $
  $ hg copy --after --verbose sym i
  copying sym to i
  overwriting i shrinking keywords
  $ head -1 i
  expand $Id$
  $ hg forget i
  $ rm i
#endif

Test different options of hg kwfiles

  $ hg kwfiles
  a
  c
  $ hg -v kwfiles --ignore
  I b
  I sym
  $ hg kwfiles --all
  K a
  K c
  I b
  I sym

Diff specific revision

  $ hg diff --rev 1
  diff -r ef63ca68695b c
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/c	* (glob)
  @@ -0,0 +1,3 @@
  +expand $Id$
  +do not process $Id:
  +xxx $

Status after rollback:

  $ hg rollback
  repository tip rolled back to revision 1 (undo commit)
  working directory now based on revision 1
  $ hg status
  A c
  $ hg update --clean
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved

#if symlink

cp symlink file; hg cp -A symlink file (part2)
- copied symlink points to kw ignored file: do not overwrite

  $ cat a > i
  $ ln -s i symignored
  $ hg commit -Am 'fake expansion in ignored and symlink' i symignored
  $ cp symignored x
  $ hg copy --after --verbose symignored x
  copying symignored to x
  $ head -n 1 x
  expand $Id: a,v ef63ca68695b 1970/01/01 00:00:00 user $
  $ hg forget x
  $ rm x

  $ hg rollback
  repository tip rolled back to revision 1 (undo commit)
  working directory now based on revision 1
  $ hg update --clean
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ rm i symignored

#endif

Custom keywordmaps as argument to kwdemo

  $ hg --quiet kwdemo "Xinfo = {author}: {desc}"
  [extensions]
  keyword =
  [keyword]
  ** = 
  b = ignore
  demo.txt = 
  i = ignore
  [keywordset]
  svn = False
  [keywordmaps]
  Xinfo = {author}: {desc}
  $Xinfo: test: hg keyword configuration and expansion example $

Configure custom keywordmaps

  $ cat <<EOF >>$HGRCPATH
  > [keywordmaps]
  > Id = {file} {node|short} {date|rfc822date} {author|user}
  > Xinfo = {author}: {desc}
  > EOF

Cat and hg cat files before custom expansion

  $ cat a b
  expand $Id: a,v ef63ca68695b 1970/01/01 00:00:00 user $
  do not process $Id:
  xxx $
  ignore $Id$
  $ hg cat sym a b && echo
  expand $Id: a ef63ca68695b Thu, 01 Jan 1970 00:00:00 +0000 user $
  do not process $Id:
  xxx $
  ignore $Id$
  a

Write custom keyword and prepare multi-line commit message

  $ echo '$Xinfo$' >> a
  $ cat <<EOF >> log
  > firstline
  > secondline
  > EOF

Interrupted commit should not change state

  $ hg commit
  abort: empty commit message
  [255]
  $ hg status
  M a
  ? c
  ? log

Commit with multi-line message and custom expansion

  $ hg --debug commit -l log -d '2 0' -u 'User Name <user@example.com>'
  committing files:
  a
  committing manifest
  committing changelog
  overwriting a expanding keywords
  committed changeset 2:bb948857c743469b22bbf51f7ec8112279ca5d83
  $ rm log

Stat, verify and show custom expansion (firstline)

  $ hg status
  ? c
  $ hg verify
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  3 files, 3 changesets, 4 total revisions
  $ cat a b
  expand $Id: a bb948857c743 Thu, 01 Jan 1970 00:00:02 +0000 user $
  do not process $Id:
  xxx $
  $Xinfo: User Name <user@example.com>: firstline $
  ignore $Id$
  $ hg cat sym a b && echo
  expand $Id: a bb948857c743 Thu, 01 Jan 1970 00:00:02 +0000 user $
  do not process $Id:
  xxx $
  $Xinfo: User Name <user@example.com>: firstline $
  ignore $Id$
  a

annotate

  $ hg annotate a
  1: expand $Id$
  1: do not process $Id:
  1: xxx $
  2: $Xinfo$

remove with status checks

  $ hg debugrebuildstate
  $ hg remove a
  $ hg --debug commit -m rma
  committing files:
  committing manifest
  committing changelog
  committed changeset 3:d14c712653769de926994cf7fbb06c8fbd68f012
  $ hg status
  ? c

Rollback, revert, and check expansion

  $ hg rollback
  repository tip rolled back to revision 2 (undo commit)
  working directory now based on revision 2
  $ hg status
  R a
  ? c
  $ hg revert --no-backup --rev tip a
  $ cat a
  expand $Id: a bb948857c743 Thu, 01 Jan 1970 00:00:02 +0000 user $
  do not process $Id:
  xxx $
  $Xinfo: User Name <user@example.com>: firstline $

Clone to test global and local configurations

  $ cd ..

Expansion in destination with global configuration

  $ hg --quiet clone Test globalconf
  $ cat globalconf/a
  expand $Id: a bb948857c743 Thu, 01 Jan 1970 00:00:02 +0000 user $
  do not process $Id:
  xxx $
  $Xinfo: User Name <user@example.com>: firstline $

No expansion in destination with local configuration in origin only

  $ hg --quiet --config 'keyword.**=ignore' clone Test localconf
  $ cat localconf/a
  expand $Id$
  do not process $Id:
  xxx $
  $Xinfo$

Clone to test incoming

  $ hg clone -r1 Test Test-a
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 3 changes to 3 files
  updating to branch default
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd Test-a
  $ cat <<EOF >> .hg/hgrc
  > [paths]
  > default = ../Test
  > EOF
  $ hg incoming
  comparing with $TESTTMP/Test (glob)
  searching for changes
  changeset:   2:bb948857c743
  tag:         tip
  user:        User Name <user@example.com>
  date:        Thu Jan 01 00:00:02 1970 +0000
  summary:     firstline
  
Imported patch should not be rejected

  >>> import re
  >>> text = re.sub(r'(Id.*)', r'\1 rejecttest', open('a').read())
  >>> open('a', 'wb').write(text)
  $ hg --debug commit -m'rejects?' -d '3 0' -u 'User Name <user@example.com>'
  committing files:
  a
  committing manifest
  committing changelog
  overwriting a expanding keywords
  committed changeset 2:85e279d709ffc28c9fdd1b868570985fc3d87082
  $ hg export -o ../rejecttest.diff tip
  $ cd ../Test
  $ hg import ../rejecttest.diff
  applying ../rejecttest.diff
  $ cat a b
  expand $Id: a 4e0994474d25 Thu, 01 Jan 1970 00:00:03 +0000 user $ rejecttest
  do not process $Id: rejecttest
  xxx $
  $Xinfo: User Name <user@example.com>: rejects? $
  ignore $Id$

  $ hg rollback
  repository tip rolled back to revision 2 (undo import)
  working directory now based on revision 2
  $ hg update --clean
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

kwexpand/kwshrink on selected files

  $ mkdir x
  $ hg copy a x/a
  $ hg --verbose kwshrink a
  overwriting a shrinking keywords
 - sleep required for dirstate.normal() check
  $ sleep 1
  $ hg status a
  $ hg --verbose kwexpand a
  overwriting a expanding keywords
  $ hg status a

kwexpand x/a should abort

  $ hg --verbose kwexpand x/a
  abort: outstanding uncommitted changes
  [255]
  $ cd x
  $ hg --debug commit -m xa -d '3 0' -u 'User Name <user@example.com>'
  committing files:
  x/a
   x/a: copy a:779c764182ce5d43e2b1eb66ce06d7b47bfe342e
  committing manifest
  committing changelog
  overwriting x/a expanding keywords
  committed changeset 3:b4560182a3f9a358179fd2d835c15e9da379c1e4
  $ cat a
  expand $Id: x/a b4560182a3f9 Thu, 01 Jan 1970 00:00:03 +0000 user $
  do not process $Id:
  xxx $
  $Xinfo: User Name <user@example.com>: xa $

kwshrink a inside directory x

  $ hg --verbose kwshrink a
  overwriting x/a shrinking keywords
  $ cat a
  expand $Id$
  do not process $Id:
  xxx $
  $Xinfo$
  $ cd ..

kwexpand nonexistent

  $ hg kwexpand nonexistent
  nonexistent:* (glob)


#if serve
hg serve
 - expand with hgweb file
 - no expansion with hgweb annotate/changeset/filediff
 - check errors

  $ hg serve -p $HGPORT -d --pid-file=hg.pid -A access.log -E errors.log
  $ cat hg.pid >> $DAEMON_PIDS
  $ get-with-headers.py localhost:$HGPORT 'file/tip/a/?style=raw'
  200 Script output follows
  
  expand $Id: a bb948857c743 Thu, 01 Jan 1970 00:00:02 +0000 user $
  do not process $Id:
  xxx $
  $Xinfo: User Name <user@example.com>: firstline $
  $ get-with-headers.py localhost:$HGPORT 'annotate/tip/a/?style=raw'
  200 Script output follows
  
  
  user@1: expand $Id$
  user@1: do not process $Id:
  user@1: xxx $
  user@2: $Xinfo$
  
  
  
  
  $ get-with-headers.py localhost:$HGPORT 'rev/tip/?style=raw'
  200 Script output follows
  
  
  # HG changeset patch
  # User User Name <user@example.com>
  # Date 3 0
  # Node ID b4560182a3f9a358179fd2d835c15e9da379c1e4
  # Parent  bb948857c743469b22bbf51f7ec8112279ca5d83
  xa
  
  diff -r bb948857c743 -r b4560182a3f9 x/a
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/x/a	Thu Jan 01 00:00:03 1970 +0000
  @@ -0,0 +1,4 @@
  +expand $Id$
  +do not process $Id:
  +xxx $
  +$Xinfo$
  
  $ get-with-headers.py localhost:$HGPORT 'diff/bb948857c743/a?style=raw'
  200 Script output follows
  
  
  diff -r ef63ca68695b -r bb948857c743 a
  --- a/a	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a	Thu Jan 01 00:00:02 1970 +0000
  @@ -1,3 +1,4 @@
   expand $Id$
   do not process $Id:
   xxx $
  +$Xinfo$
  
  
  
  
  $ cat errors.log
#endif

Prepare merge and resolve tests

  $ echo '$Id$' > m
  $ hg add m
  $ hg commit -m 4kw
  $ echo foo >> m
  $ hg commit -m 5foo

simplemerge

  $ hg update 4
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo foo >> m
  $ hg commit -m 6foo
  created new head
  $ hg merge
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg commit -m simplemerge
  $ cat m
  $Id: m 27d48ee14f67 Thu, 01 Jan 1970 00:00:00 +0000 test $
  foo

conflict: keyword should stay outside conflict zone

  $ hg update 4
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo bar >> m
  $ hg commit -m 8bar
  created new head
  $ hg merge
  merging m
  warning: conflicts during merge.
  merging m incomplete! (edit conflicts, then use 'hg resolve --mark')
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  [1]
  $ cat m
  $Id$
  <<<<<<< local: 88a80c8d172e - test: 8bar
  bar
  =======
  foo
  >>>>>>> other: 85d2d2d732a5  - test: simplemerge

resolve to local, m must contain hash of last change (local parent)

  $ hg resolve -t internal:local -a
  (no more unresolved files)
  $ hg commit -m localresolve
  $ cat m
  $Id: m 88a80c8d172e Thu, 01 Jan 1970 00:00:00 +0000 test $
  bar

Test restricted mode with transplant -b

  $ hg update 6
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg branch foo
  marked working directory as branch foo
  (branches are permanent and global, did you want a bookmark?)
  $ mv a a.bak
  $ echo foobranch > a
  $ cat a.bak >> a
  $ rm a.bak
  $ hg commit -m 9foobranch
  $ hg update default
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg -y transplant -b foo tip
  applying 4aa30d025d50
  4aa30d025d50 transplanted to e00abbf63521

Expansion in changeset but not in file

  $ hg tip -p
  changeset:   11:e00abbf63521
  tag:         tip
  parent:      9:800511b3a22d
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     9foobranch
  
  diff -r 800511b3a22d -r e00abbf63521 a
  --- a/a	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,3 +1,4 @@
  +foobranch
   expand $Id$
   do not process $Id:
   xxx $
  
  $ head -n 2 a
  foobranch
  expand $Id: a e00abbf63521 Thu, 01 Jan 1970 00:00:00 +0000 test $

Turn off expansion

  $ hg -q rollback
  $ hg -q update -C

kwshrink with unknown file u

  $ cp a u
  $ hg --verbose kwshrink
  overwriting a shrinking keywords
  overwriting m shrinking keywords
  overwriting x/a shrinking keywords

Keywords shrunk in working directory, but not yet disabled
 - cat shows unexpanded keywords
 - hg cat shows expanded keywords

  $ cat a b
  expand $Id$
  do not process $Id:
  xxx $
  $Xinfo$
  ignore $Id$
  $ hg cat sym a b && echo
  expand $Id: a bb948857c743 Thu, 01 Jan 1970 00:00:02 +0000 user $
  do not process $Id:
  xxx $
  $Xinfo: User Name <user@example.com>: firstline $
  ignore $Id$
  a

Now disable keyword expansion

  $ cp $HGRCPATH $HGRCPATH.backup
  $ rm "$HGRCPATH"
  $ cat a b
  expand $Id$
  do not process $Id:
  xxx $
  $Xinfo$
  ignore $Id$
  $ hg cat sym a b && echo
  expand $Id$
  do not process $Id:
  xxx $
  $Xinfo$
  ignore $Id$
  a

enable keyword expansion again

  $ cat $HGRCPATH.backup >> $HGRCPATH

Test restricted mode with unshelve

  $ cat <<EOF >> $HGRCPATH
  > [extensions]
  > shelve =
  > EOF

  $ echo xxxx >> a
  $ hg diff
  diff -r 800511b3a22d a
  --- a/a	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a	* (glob)
  @@ -2,3 +2,4 @@
   do not process $Id:
   xxx $
   $Xinfo$
  +xxxx
  $ hg shelve -q --name tmp
  $ hg shelve --list --patch
  tmp             (*)    changes to 'localresolve' (glob)
  
  diff --git a/a b/a
  --- a/a
  +++ b/a
  @@ -2,3 +2,4 @@
   do not process $Id:
   xxx $
   $Xinfo$
  +xxxx

  $ hg update -q -C 10
  $ hg unshelve -q tmp
  $ hg diff
  diff -r 4aa30d025d50 a
  --- a/a	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a	* (glob)
  @@ -3,3 +3,4 @@
   do not process $Id:
   xxx $
   $Xinfo$
  +xxxx

Test restricted mode with rebase

  $ cat <<EOF >> $HGRCPATH
  > [extensions]
  > rebase =
  > EOF

  $ hg update -q -C 9

  $ echo xxxx >> a
  $ hg commit -m '#11'
  $ hg diff -c 11
  diff -r 800511b3a22d -r b07670694489 a
  --- a/a	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a	Thu Jan 01 00:00:00 1970 +0000
  @@ -2,3 +2,4 @@
   do not process $Id:
   xxx $
   $Xinfo$
  +xxxx

  $ hg diff -c 10
  diff -r 27d48ee14f67 -r 4aa30d025d50 a
  --- a/a	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,3 +1,4 @@
  +foobranch
   expand $Id$
   do not process $Id:
   xxx $

  $ hg rebase -q -s 10 -d 11 --keep
  $ hg diff -r 9 -r 12 a
  diff -r 800511b3a22d -r 1939b927726c a
  --- a/a	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,4 +1,6 @@
  +foobranch
   expand $Id$
   do not process $Id:
   xxx $
   $Xinfo$
  +xxxx

Test restricted mode with graft

  $ hg graft -q 10
  $ hg diff -r 9 -r 13 a
  diff -r 800511b3a22d -r 01a68de1003a a
  --- a/a	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,4 +1,6 @@
  +foobranch
   expand $Id$
   do not process $Id:
   xxx $
   $Xinfo$
  +xxxx

Test restricted mode with backout

  $ hg backout -q 11
  $ hg diff a
  diff -r 01a68de1003a a
  --- a/a	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a	* (glob)
  @@ -3,4 +3,3 @@
   do not process $Id:
   xxx $
   $Xinfo$
  -xxxx

Test restricted mode with histedit

  $ cat <<EOF >> $HGRCPATH
  > [extensions]
  > histedit =
  > EOF

  $ hg commit -m 'backout #11'
  $ hg histedit -q --command - 13 <<EOF
  > pick 49f5f2d940c3 14 backout #11
  > pick 01a68de1003a 13 9foobranch
  > EOF

Test restricted mode with fetch (with merge)

  $ cat <<EOF >> $HGRCPATH
  > [extensions]
  > fetch =
  > EOF

  $ hg clone -q -r 9 . ../fetch-merge
  $ cd ../fetch-merge
  $ hg -R ../Test export 10 | hg import -q -
  $ hg fetch -q -r 11
  $ hg diff -r 9 a
  diff -r 800511b3a22d a
  --- a/a	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a	* (glob)
  @@ -1,4 +1,6 @@
  +foobranch
   expand $Id$
   do not process $Id:
   xxx $
   $Xinfo$
  +xxxx

  $ cd ..
