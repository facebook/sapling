#debugruntest-compatible
# coding=utf-8

# coding=utf-8
# Copyright (c) Meta Platforms, Inc. and affiliates.
# Copyright (c) Mercurial Contributors.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

  $ configure modernclient
  $ enable amend

  $ newclientrepo a
  $ mkdir d1
  $ mkdir d1/d2
  $ echo line 1 > a
  $ echo line 1 > d1/d2/a
  $ hg ci -Ama
  adding a
  adding d1/d2/a
  $ hg book rev0
  $ hg book -i
  $ hg push -q -r . --to rev0 --create

  $ echo line 2 >> a
  $ hg ci -u someone -d '1 0' '-msecond change'
  $ hg book rev1
  $ hg book -i

# import with no args:

  $ hg import
  abort: need at least one patch to import
  [255]

# generate patches for the test

  $ hg export tip > ../exported-tip.patch
  $ hg diff '-r rev0:' > ../diffed-tip.patch

# import exported patch
# (this also tests that editor is not invoked, if the patch contains the
# commit message and '--edit' is not specified)

  $ newclientrepo b test:a_server rev0
  $ HGEDITOR=cat hg import ../exported-tip.patch
  applying ../exported-tip.patch

# message and committer and date should be same

  $ hg tip
  commit:      * (glob)
  user:        someone
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     second change

# import exported patch with external patcher
# (this also tests that editor is invoked, if the '--edit' is specified,
# regardless of the commit message in the patch)

  $ cat > ../dummypatch.py << 'EOF'
  > from __future__ import print_function
  > print('patching file a')
  > open('a', 'wb').write(b'line2\n')
  > EOF
  $ newclientrepo b0 test:a_server rev0
  $ HGEDITOR=cat hg --config "ui.patch=hg debugpython -- ../dummypatch.py" import --edit ../exported-tip.patch
  applying ../exported-tip.patch
  second change
  
  
  HG: Enter commit message.  Lines beginning with 'HG:' are removed.
  HG: Leave message empty to abort commit.
  HG: --
  HG: user: someone
  HG: branch 'default'
  HG: changed a
  $ cat a
  line2

# import of plain diff should fail without message
# (this also tests that editor is invoked, if the patch doesn't contain
# the commit message, regardless of '--edit')

  $ newclientrepo b1 test:a_server rev0
  $ cat > $TESTTMP/editor.sh << 'EOF'
  > env | grep HGEDITFORM
  > cat \$1
  > EOF
  $ HGEDITOR=cat hg import ../diffed-tip.patch
  applying ../diffed-tip.patch
  
  
  HG: Enter commit message.  Lines beginning with 'HG:' are removed.
  HG: Leave message empty to abort commit.
  HG: --
  HG: user: test
  HG: branch 'default'
  HG: changed a
  abort: empty commit message
  [255]

# Test avoiding editor invocation at applying the patch with --exact,
# even if commit message is empty

  $ echo a >> a
  $ hg commit -m ' '
  $ hg tip -T '{node}\n'
  e7df5eeeca3300b311991dbe19748d533edb2e8a
  $ hg export -o ../empty-log.diff .
  $ hg goto -q -C '.^1'
  $ hg hide -q -r tip
  $ HGEDITOR=cat hg import --exact ../empty-log.diff
  applying ../empty-log.diff
  $ hg tip -T '{node}\n'
  e7df5eeeca3300b311991dbe19748d533edb2e8a

# import of plain diff should be ok with message

  $ newclientrepo b2 test:a_server rev0
  $ hg import -mpatch ../diffed-tip.patch
  applying ../diffed-tip.patch

# import of plain diff with specific date and user
# (this also tests that editor is not invoked, if
# '--message'/'--logfile' is specified and '--edit' is not)

  $ newclientrepo b3 test:a_server rev0
  $ hg import -mpatch -d '1 0' -u 'user@nowhere.net' ../diffed-tip.patch
  applying ../diffed-tip.patch
  $ hg tip -pv
  commit:      * (glob)
  user:        user@nowhere.net
  date:        Thu Jan 01 00:00:01 1970 +0000
  files:       a
  description:
  patch
  
  
  diff -r * -r * a (glob)
  --- a/a	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a	Thu Jan 01 00:00:01 1970 +0000
  @@ -1,1 +1,2 @@
   line 1
  +line 2

# import of plain diff should be ok with --no-commit
# (this also tests that editor is not invoked, if '--no-commit' is
# specified, regardless of '--edit')

  $ newclientrepo b4 test:a_server rev0
  $ HGEDITOR=cat hg import --no-commit --edit ../diffed-tip.patch
  applying ../diffed-tip.patch
  $ hg diff --nodates
  diff -r * a (glob)
  --- a/a
  +++ b/a
  @@ -1,1 +1,2 @@
   line 1
  +line 2

# import of malformed plain diff should fail

  $ newclientrepo b5 test:a_server rev0

  $ sed 's/1,1/foo/' < ../diffed-tip.patch > ../broken.patch

  $ hg import -mpatch ../broken.patch
  applying ../broken.patch
  abort: bad hunk #1
  [255]

# hg -R repo import
# put the clone in a subdir - having a directory named "a"
# used to hide a bug.

  $ mkdir dir
  $ newclientrepo dir/b test:a_server rev0
  $ cd ..
  $ hg -R b import ../exported-tip.patch
  applying ../exported-tip.patch
  $ cd ..

# import from stdin

  $ newclientrepo b6 test:a_server rev0
  $ hg import - < ../exported-tip.patch
  applying patch from stdin

# import two patches in one stream

  $ newclientrepo b7
  $ cd ..
  $ hg --cwd a export 'rev0:tip' | hg --cwd b7 import -
  applying patch from stdin
  $ hg --cwd a id
  da4d12908167 rev1
  $ hg --cwd b7 id
  da4d12908167

# override commit message

  $ newclientrepo b8 test:a_server rev0
  $ cd ..
  $ hg --cwd b8 import -m override - < exported-tip.patch
  applying patch from stdin
  $ hg --cwd b8 log -r tip -T '{desc}'
  override (no-eol)

def mkmsg(path1, path2):
    import email, sys

    Message = email.message.Message

    msg = Message()
    patch = open(path1, "rb").read()
    msg.set_payload(b"email commit message\n" + patch)
    msg["Subject"] = "email patch"
    msg["From"] = "email patcher"
    open(path2, "wb").write(msg.as_string().encode("utf-8"))

# plain diff in email, subject, message body

  $ newclientrepo b9 test:a_server rev0
  $ cd ..

  >>> mkmsg("diffed-tip.patch", "msg.patch")

  $ hg --cwd b9 import ../msg.patch
  applying ../msg.patch
  $ hg --cwd b9 log -r tip -T '{author}\n{desc}'
  email patcher
  email patch
  email commit message (no-eol)

# hg export in email, should use patch header

  $ newclientrepo b10 test:a_server rev0
  $ cd ..

  >>> mkmsg("exported-tip.patch", "msg.patch")

  $ cat msg.patch | hg --cwd b10 import -
  applying patch from stdin
  $ hg --cwd b10 log -r tip -T '{desc}'
  second change (no-eol)

# subject: duplicate detection, removal of [PATCH]
# The '---' tests the gitsendmail handling without proper mail headers

def mkmsg2(path1, path2):
    import email, sys

    msg = email.message.Message()
    patch = open(path1, "rb").read()
    msg.set_payload(b"email patch\n\nnext line\n---\n" + patch)
    msg["Subject"] = "[PATCH] email patch"
    msg["From"] = "email patcher"
    open(path2, "wb").write(msg.as_string().encode("utf-8"))


# plain diff in email, [PATCH] subject, message body with subject

  $ newclientrepo b11 test:a_server rev0
  $ cd ..

  >>> mkmsg2("diffed-tip.patch", "msg.patch")

  $ cat msg.patch | hg --cwd b11 import -
  applying patch from stdin
  $ hg --cwd b11 tip --template '{desc}\n'
  email patch
  
  next line

# Issue963: Parent of working dir incorrect after import of multiple
# patches and rollback
# We weren't backing up the correct dirstate file when importing many
# patches: import patch1 patch2; rollback

  $ echo line 3 >> a/a
  $ hg --cwd a ci '-mthird change'
  $ hg --cwd a book rev2
  $ hg --cwd a book -i
  $ hg --cwd a export -o '../patch%n' rev1 rev2
  $ newclientrepo b12 test:a_server rev0
  $ cd ..
  $ hg --cwd b12 parents --template 'parent: {desc}\n'
  parent: a
  $ hg --cwd b12 import -v ../patch1 ../patch2
  applying ../patch1
  patching file a
  committing files:
  a
  committing manifest
  committing changelog
  created * (glob)
  applying ../patch2
  patching file a
  committing files:
  a
  committing manifest
  committing changelog
  created * (glob)
  $ hg --cwd b12 hide -q tip
  $ hg --cwd b12 parents --template 'parent: {desc}\n'
  parent: second change

# importing a patch in a subdirectory failed at the commit stage

  $ echo line 2 >> a/d1/d2/a
  $ hg --cwd a ci -u someoneelse -d '1 0' '-msubdir change'

# hg import in a subdirectory

  $ newclientrepo b13 test:a_server rev0
  $ cd ..
  $ hg --cwd a export tip > tmp

  $ sed 's#d1/d2##' < tmp > subdir-tip.patch

  $ cd b13/d1/d2
  $ hg import ../../../subdir-tip.patch
  applying ../../../subdir-tip.patch
  $ cd ../../..

# message should be 'subdir change'
# committer should be 'someoneelse'

  $ hg --cwd b13 tip
  commit:      * (glob)
  user:        someoneelse
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     subdir change

# should be empty

  $ hg --cwd b13 status

# Test fuzziness (ambiguous patch location, fuzz=2)

  $ newclientrepo fuzzy
  $ echo line1 > a
  $ echo line0 >> a
  $ echo line3 >> a
  $ hg ci -Am adda
  adding a
  $ hg book -i rev0
  $ echo line1 > a
  $ echo line2 >> a
  $ echo line0 >> a
  $ echo line3 >> a
  $ hg ci -m change a
  $ hg export tip > fuzzy-tip.patch
  $ hg up -C rev0 --inactive
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo line1 > a
  $ echo line0 >> a
  $ echo line1 >> a
  $ echo line0 >> a
  $ hg ci -m brancha
  $ hg import --config 'patch.fuzz=0' -v fuzzy-tip.patch
  applying fuzzy-tip.patch
  patching file a
  Hunk #1 FAILED at 0
  1 out of 1 hunks FAILED -- saving rejects to file a.rej
  abort: patch failed to apply
  [255]
  $ hg import --no-commit -v fuzzy-tip.patch
  applying fuzzy-tip.patch
  patching file a
  Hunk #1 succeeded at 2 with fuzz 1 (offset 0 lines).
  applied to working directory
  $ hg revert -a
  reverting a

# import with --no-commit should have written .hg/last-message.txt

  $ cat .hg/last-message.txt
  change (no-eol)

# test fuzziness with eol=auto

  $ hg --config 'patch.eol=auto' import --no-commit -v fuzzy-tip.patch
  applying fuzzy-tip.patch
  patching file a
  Hunk #1 succeeded at 2 with fuzz 1 (offset 0 lines).
  applied to working directory
  $ cd ..

# Test hunk touching empty files (issue906)

  $ newclientrepo empty
  $ touch a
  $ touch b1
  $ touch c1
  $ echo d > d
  $ hg ci -Am init
  adding a
  adding b1
  adding c1
  adding d
  $ hg book -i rev0
  $ echo a > a
  $ echo b > b1
  $ hg mv b1 b2
  $ echo c > c1
  $ hg copy c1 c2
  $ rm d
  $ touch d
  $ hg diff --git
  diff --git a/a b/a
  --- a/a
  +++ b/a
  @@ -0,0 +1,1 @@
  +a
  diff --git a/b1 b/b2
  rename from b1
  rename to b2
  --- a/b1
  +++ b/b2
  @@ -0,0 +1,1 @@
  +b
  diff --git a/c1 b/c1
  --- a/c1
  +++ b/c1
  @@ -0,0 +1,1 @@
  +c
  diff --git a/c1 b/c2
  copy from c1
  copy to c2
  --- a/c1
  +++ b/c2
  @@ -0,0 +1,1 @@
  +c
  diff --git a/d b/d
  --- a/d
  +++ b/d
  @@ -1,1 +0,0 @@
  -d
  $ hg ci -m empty
  $ hg export --git tip > empty.diff
  $ hg up -C rev0 --inactive
  4 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ hg import empty.diff
  applying empty.diff
  $ cd ..

# Test importing a patch ending with a binary file removal

  $ newclientrepo binaryremoval
  $ echo a > a

  >>> with open("b", "wb") as f:
  ...     f.write(b"a\0b") and None

  $ hg ci -Am addall
  adding a
  adding b
  $ hg book -i rev0
  $ hg rm a
  $ hg rm b
  $ hg st
  R a
  R b
  $ hg ci -m remove
  $ hg export --git . > remove.diff

  $ grep git remove.diff
  diff --git a/a b/a
  diff --git a/b b/b

  $ hg up -C rev0 --inactive
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg import remove.diff
  applying remove.diff
  $ hg manifest
  $ cd ..

# Issue927: test update+rename with common name

  $ newclientrepo t
  $ touch a
  $ hg ci -Am t
  adding a
  $ hg book -i rev0
  $ echo a > a

# Here, bfile.startswith(afile)

  $ hg copy a a2
  $ hg ci -m copya
  $ hg export --git tip > copy.diff
  $ hg up -C rev0 --inactive
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg import copy.diff
  applying copy.diff

# a should contain an 'a'

  $ cat a
  a

# and a2 should have duplicated it

  $ cat a2
  a
  $ cd ..

# test -p0

  $ newclientrepo p0
  $ echo a > a
  $ hg ci -Am t
  adding a
  $ hg import -p foo
  abort: invalid value 'foo' for option -p, expected int
  [255]
  $ hg import -p0 - << 'EOS'
  > foobar
  > --- a	Sat Apr 12 22:43:58 2008 -0400
  > +++ a	Sat Apr 12 22:44:05 2008 -0400
  > @@ -1,1 +1,1 @@
  > -a
  > +bb
  > EOS
  applying patch from stdin
  $ hg status
  $ cat a
  bb

# test --prefix

  $ mkdir -p dir/dir2
  $ echo b > dir/dir2/b
  $ hg ci -Am b
  adding dir/dir2/b
  $ hg import -p2 --prefix dir - << 'EOS'
  > foobar
  > --- drop1/drop2/dir2/b
  > +++ drop1/drop2/dir2/b
  > @@ -1,1 +1,1 @@
  > -b
  > +cc
  > EOS
  applying patch from stdin
  $ hg status
  $ cat dir/dir2/b
  cc
  $ cd ..

# test paths outside repo root

  $ mkdir outside
  $ touch outside/foo
  $ newclientrepo inside
  $ hg import - << 'EOS'
  > diff --git a/a b/b
  > rename from ../outside/foo
  > rename to bar
  > EOS
  applying patch from stdin
  abort: path contains illegal component: ../outside/foo
  [255]
  $ cd ..

# test import with similarity and git and strip (issue295 et al.)

  $ newclientrepo sim
  $ echo 'this is a test' > a
  $ hg ci -Ama
  adding a
  $ cat > ../rename.diff << 'EOF'
  > diff --git a/foo/a b/foo/a
  > deleted file mode 100644
  > --- a/foo/a
  > +++ /dev/null
  > @@ -1,1 +0,0 @@
  > -this is a test
  > diff --git a/foo/b b/foo/b
  > new file mode 100644
  > --- /dev/null
  > +++ b/foo/b
  > @@ -0,0 +1,2 @@
  > +this is a test
  > +foo
  > EOF
  $ hg import --no-commit -v -s 1 ../rename.diff -p2
  applying ../rename.diff
  patching file a
  patching file b
  adding b
  recording removal of a as rename to b (88% similar)
  applied to working directory
  $ echo 'mod b' > b
  $ hg st -C
  A b
    a
  R a
  $ hg revert -a
  undeleting a
  forgetting b
  $ cat b
  mod b
  $ rm b
  $ hg import --no-commit -v -s 100 ../rename.diff -p2
  applying ../rename.diff
  patching file a
  patching file b
  adding b
  applied to working directory
  $ hg st -C
  A b
  R a
  $ cd ..

# Issue1495: add empty file from the end of patch

  $ newclientrepo addemptyend
  $ touch a
  $ hg addremove
  adding a
  $ hg ci -m commit
  $ cat > a.patch << 'EOF'
  > add a, b
  > diff --git a/a b/a
  > --- a/a
  > +++ b/a
  > @@ -0,0 +1,1 @@
  > +a
  > diff --git a/b b/b
  > new file mode 100644
  > EOF
  $ hg import --no-commit a.patch
  applying a.patch

# apply a good patch followed by an empty patch (mainly to ensure
# that dirstate is *not* updated when import crashes)

  $ hg goto -q -C .
  $ rm b
  $ touch empty.patch
  $ hg import a.patch empty.patch
  applying a.patch
  applying empty.patch
  abort: empty.patch: no diffs found
  [255]
  $ hg tip --template '{desc|firstline}\n'
  commit
  $ hg -q status
  M a
  $ cd ..

# create file when source is not /dev/null

  $ cat > create.patch << 'EOF'
  > diff -Naur proj-orig/foo proj-new/foo
  > --- proj-orig/foo       1969-12-31 16:00:00.000000000 -0800
  > +++ proj-new/foo        2009-07-17 16:50:45.801368000 -0700
  > @@ -0,0 +1,1 @@
  > +a
  > EOF

# some people have patches like the following too

  $ cat > create2.patch << 'EOF'
  > diff -Naur proj-orig/foo proj-new/foo
  > --- proj-orig/foo.orig  1969-12-31 16:00:00.000000000 -0800
  > +++ proj-new/foo        2009-07-17 16:50:45.801368000 -0700
  > @@ -0,0 +1,1 @@
  > +a
  > EOF
  $ newclientrepo oddcreate
  $ hg import --no-commit ../create.patch
  applying ../create.patch
  $ cat foo
  a
  $ rm foo
  $ hg revert foo
  $ hg import --no-commit ../create2.patch
  applying ../create2.patch
  $ cat foo
  a

  $ cd ..

# Issue1859: first line mistaken for email headers

  $ newclientrepo emailconfusion
  $ cat > a.patch << 'EOF'
  > module: summary
  > 
  > description
  > 
  > 
  > diff -r 000000000000 -r 9b4c1e343b55 test.txt
  > --- /dev/null
  > +++ b/a
  > @@ -0,0 +1,1 @@
  > +a
  > EOF
  $ hg import -d '0 0' a.patch
  applying a.patch
  $ hg parents -v
  commit:      5a681217c0ad
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       a
  description:
  module: summary
  
  description
  $ cd ..

# in commit message

  $ newclientrepo commitconfusion
  $ cat > a.patch << 'EOF'
  > module: summary
  > 
  > --- description
  > 
  > diff --git a/a b/a
  > new file mode 100644
  > --- /dev/null
  > +++ b/a
  > @@ -0,0 +1,1 @@
  > +a
  > EOF

  $ hg import -d '0 0' a.patch -q
  $ hg parents -v
  commit:      f34d9187897d
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       a
  description:
  module: summary
  $ cd ..

  $ cat > trickyheaders.patch << 'EOF'
  > From: User A <user@a>
  > Subject: [PATCH] from: tricky!
  > 
  > # HG changeset patch
  > # User User B
  > # Date 1266264441 18000
  > # Branch stable
  > # Node ID f2be6a1170ac83bf31cb4ae0bad00d7678115bc0
  > # Parent  0000000000000000000000000000000000000000
  > from: tricky!
  > 
  > That is not a header.
  > 
  > diff -r 000000000000 -r f2be6a1170ac foo
  > --- /dev/null
  > +++ b/foo
  > @@ -0,0 +1,1 @@
  > +foo
  > EOF

  $ newclientrepo trickyheaders
  $ hg import -d '0 0' ../trickyheaders.patch
  applying ../trickyheaders.patch
  $ hg export --git tip
  # HG changeset patch
  # User User B
  # Date 0 0
  #      Thu Jan 01 00:00:00 1970 +0000
  # Node ID eb56ab91903632294ac504838508cb370c0901d2
  # Parent  0000000000000000000000000000000000000000
  from: tricky!
  
  That is not a header.
  
  diff --git a/foo b/foo
  new file mode 100644
  --- /dev/null
  +++ b/foo
  @@ -0,0 +1,1 @@
  +foo
  $ cd ..

# Issue2102: hg export and hg import speak different languages

  $ newclientrepo issue2102
  $ mkdir -p src/cmd/gc
  $ touch src/cmd/gc/mksys.bash
  $ hg ci -Am init
  adding src/cmd/gc/mksys.bash
  $ hg import - << 'EOS'
  > # HG changeset patch
  > # User Rob Pike
  > # Date 1216685449 25200
  > # Node ID 03aa2b206f499ad6eb50e6e207b9e710d6409c98
  > # Parent  93d10138ad8df586827ca90b4ddb5033e21a3a84
  > help management of empty pkg and lib directories in perforce
  > 
  > R=gri
  > DELTA=4  (4 added, 0 deleted, 0 changed)
  > OCL=13328
  > CL=13328
  > 
  > diff --git a/lib/place-holder b/lib/place-holder
  > new file mode 100644
  > --- /dev/null
  > +++ b/lib/place-holder
  > @@ -0,0 +1,2 @@
  > +perforce does not maintain empty directories.
  > +this file helps.
  > diff --git a/pkg/place-holder b/pkg/place-holder
  > new file mode 100644
  > --- /dev/null
  > +++ b/pkg/place-holder
  > @@ -0,0 +1,2 @@
  > +perforce does not maintain empty directories.
  > +this file helps.
  > diff --git a/src/cmd/gc/mksys.bash b/src/cmd/gc/mksys.bash
  > old mode 100644
  > new mode 100755
  > EOS
  applying patch from stdin

#if execbit
  $ hg diff --git -c tip
  diff --git a/lib/place-holder b/lib/place-holder
  new file mode 100644
  --- /dev/null
  +++ b/lib/place-holder
  @@ -0,0 +1,2 @@
  +perforce does not maintain empty directories.
  +this file helps.
  diff --git a/pkg/place-holder b/pkg/place-holder
  new file mode 100644
  --- /dev/null
  +++ b/pkg/place-holder
  @@ -0,0 +1,2 @@
  +perforce does not maintain empty directories.
  +this file helps.
  diff --git a/src/cmd/gc/mksys.bash b/src/cmd/gc/mksys.bash
  old mode 100644
  new mode 100755
#else
sh % "hg diff --git -c tip" == r"""
    diff --git a/lib/place-holder b/lib/place-holder
    new file mode 100644
    --- /dev/null
    +++ b/lib/place-holder
    @@ -0,0 +1,2 @@
    +perforce does not maintain empty directories.
    +this file helps.
    diff --git a/pkg/place-holder b/pkg/place-holder
    new file mode 100644
    --- /dev/null
    +++ b/pkg/place-holder
    @@ -0,0 +1,2 @@
    +perforce does not maintain empty directories.
    +this file helps."""

# /* The mode change for mksys.bash is missing here, because on platforms  */
# /* that don't support execbits, mode changes in patches are ignored when */
# /* they are imported. This is obviously also the reason for why the hash */
# /* in the created changeset is different to the one you see above the    */
# /* #else clause */
#endif

  $ cd ..

# diff lines looking like headers

  $ newclientrepo difflineslikeheaders
  $ echo a > a
  $ echo b > b
  $ echo c > c
  $ hg ci -Am1
  adding a
  adding b
  adding c
  $ hg book -i rev0

  $ echo 'key: value' >> a
  $ echo 'key: value' >> b
  $ echo foo >> c
  $ hg ci -m2
  $ hg book -i rev1

  $ hg up -C rev0 --inactive
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg diff --git -c 'desc(1)' > want
  $ hg diff -c rev1 | hg import --no-commit -
  applying patch from stdin
  $ hg diff --git
  diff --git a/a b/a
  --- a/a
  +++ b/a
  @@ -1,1 +1,2 @@
   a
  +key: value
  diff --git a/b b/b
  --- a/b
  +++ b/b
  @@ -1,1 +1,2 @@
   b
  +key: value
  diff --git a/c b/c
  --- a/c
  +++ b/c
  @@ -1,1 +1,2 @@
   c
  +foo
  $ cd ..

# import a unified diff with no lines of context (diff -U0)

  $ newclientrepo diffzero
  $ cat > f << 'EOF'
  > c2
  > c4
  > c5
  > EOF
  $ hg commit -Am0
  adding f

  $ hg import --no-commit - << 'EOS'
  > # HG changeset patch
  > # User test
  > # Date 0 0
  > # Node ID f4974ab632f3dee767567b0576c0ec9a4508575c
  > # Parent  8679a12a975b819fae5f7ad3853a2886d143d794
  > 1
  > diff -r 8679a12a975b -r f4974ab632f3 f
  > --- a/f	Thu Jan 01 00:00:00 1970 +0000
  > +++ b/f	Thu Jan 01 00:00:00 1970 +0000
  > @@ -0,0 +1,1 @@
  > +c1
  > @@ -1,0 +3,1 @@
  > +c3
  > @@ -3,1 +4,0 @@
  > -c5
  > EOS
  applying patch from stdin

  $ cat f
  c1
  c2
  c3
  c4

  $ cd ..

# commit message that looks like a diff header (issue1879)

  $ newclientrepo headerlikemsg
  $ touch empty
  $ echo nonempty >> nonempty
  $ hg ci -qAl - << 'EOS'
  > blah blah
  > diff blah
  > blah blah
  > EOS
  $ hg book -i rev0
  $ hg --config 'diff.git=1' log -pv
  commit:      c6ef204ef767
  bookmark:    rev0
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       empty nonempty
  description:
  blah blah
  diff blah
  blah blah
  
  
  diff --git a/empty b/empty
  new file mode 100644
  diff --git a/nonempty b/nonempty
  new file mode 100644
  --- /dev/null
  +++ b/nonempty
  @@ -0,0 +1,1 @@
  +nonempty

#  (without --git, empty file is lost, but commit message should be preserved)

  $ newclientrepo plain
  $ cd ..
  $ hg --cwd headerlikemsg export rev0 | hg -R plain import -
  applying patch from stdin
  $ hg --config 'diff.git=1' -R plain log -pv
  commit:      60a2d231e71f
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       nonempty
  description:
  blah blah
  diff blah
  blah blah
  
  
  diff --git a/nonempty b/nonempty
  new file mode 100644
  --- /dev/null
  +++ b/nonempty
  @@ -0,0 +1,1 @@
  +nonempty

#  (with --git, patch contents should be fully preserved)

  $ newclientrepo git
  $ cd ..
  $ hg --config 'diff.git=1' --cwd headerlikemsg export rev0 | hg -R git import -
  applying patch from stdin
  $ hg --config 'diff.git=1' -R git log -pv
  commit:      c6ef204ef767
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       empty nonempty
  description:
  blah blah
  diff blah
  blah blah
  
  
  diff --git a/empty b/empty
  new file mode 100644
  diff --git a/nonempty b/nonempty
  new file mode 100644
  --- /dev/null
  +++ b/nonempty
  @@ -0,0 +1,1 @@
  +nonempty

  $ cd ..

# no segfault while importing a unified diff which start line is zero but chunk
# size is non-zero

  $ newclientrepo startlinezero
  $ echo foo > foo
  $ hg commit -Amfoo
  adding foo

  $ hg import --no-commit - << 'EOS'
  > diff a/foo b/foo
  > --- a/foo
  > +++ b/foo
  > @@ -0,1 +0,1 @@
  >  foo
  > EOS
  applying patch from stdin

  $ cd ..

# Test corner case involving fuzz and skew

  $ newclientrepo morecornercases

  $ cat > 01-no-context-beginning-of-file.diff << 'EOF'
  > diff --git a/a b/a
  > --- a/a
  > +++ b/a
  > @@ -1,0 +1,1 @@
  > +line
  > EOF

  $ cat > 02-no-context-middle-of-file.diff << 'EOF'
  > diff --git a/a b/a
  > --- a/a
  > +++ b/a
  > @@ -1,1 +1,1 @@
  > -2
  > +add some skew
  > @@ -2,0 +2,1 @@
  > +line
  > EOF

  $ cat > 03-no-context-end-of-file.diff << 'EOF'
  > diff --git a/a b/a
  > --- a/a
  > +++ b/a
  > @@ -10,0 +10,1 @@
  > +line
  > EOF

  $ cat > 04-middle-of-file-completely-fuzzed.diff << 'EOF'
  > diff --git a/a b/a
  > --- a/a
  > +++ b/a
  > @@ -1,1 +1,1 @@
  > -2
  > +add some skew
  > @@ -2,2 +2,3 @@
  >  not matching, should fuzz
  >  ... a bit
  > +line
  > EOF

  $ cat > a << 'EOF'
  > 1
  > 2
  > 3
  > 4
  > EOF
  $ hg ci -Am adda a

  $ hg import -v --no-commit 01-no-context-beginning-of-file.diff
  applying 01-no-context-beginning-of-file.diff
  patching file a
  applied to working directory
  $ cat a
  1
  line
  2
  3
  4

  $ hg revert -aqC a

  $ hg import -v --no-commit 02-no-context-middle-of-file.diff
  applying 02-no-context-middle-of-file.diff
  patching file a
  Hunk #1 succeeded at 2 (offset 1 lines).
  Hunk #2 succeeded at 4 (offset 1 lines).
  applied to working directory
  $ cat a
  1
  add some skew
  3
  line
  4

  $ hg revert -aqC a

  $ hg import -v --no-commit 03-no-context-end-of-file.diff
  applying 03-no-context-end-of-file.diff
  patching file a
  Hunk #1 succeeded at 5 (offset -6 lines).
  applied to working directory
  $ cat a
  1
  2
  3
  4
  line

  $ hg revert -aqC a

  $ hg import -v --no-commit 04-middle-of-file-completely-fuzzed.diff
  applying 04-middle-of-file-completely-fuzzed.diff
  patching file a
  Hunk #1 succeeded at 2 (offset 1 lines).
  Hunk #2 succeeded at 5 with fuzz 2 (offset 1 lines).
  applied to working directory
  $ cat a
  1
  add some skew
  3
  4
  line

  $ hg revert -aqC a

  $ cd ..

# Test partial application
# ------------------------
# prepare a stack of patches depending on each other

  $ newclientrepo partial
  $ cat > a << 'EOF'
  > one
  > two
  > three
  > four
  > five
  > six
  > seven
  > EOF
  $ hg add a
  $ echo b > b
  $ hg add b
  $ hg commit -m initial -u Babar
  $ cat > a << 'EOF'
  > one
  > two
  > 3
  > four
  > five
  > six
  > seven
  > EOF
  $ hg commit -m three -u Celeste
  $ cat > a << 'EOF'
  > one
  > two
  > 3
  > 4
  > five
  > six
  > seven
  > EOF
  $ hg commit -m four -u Rataxes
  $ cat > a << 'EOF'
  > one
  > two
  > 3
  > 4
  > 5
  > six
  > seven
  > EOF
  $ echo bb >> b
  $ hg commit -m five -u Arthur
  $ echo Babar > jungle
  $ hg add jungle
  $ hg ci -m jungle -u Zephir
  $ echo Celeste >> jungle
  $ hg ci -m 'extended jungle' -u Cornelius
  $ hg log -G --template '{desc|firstline} [{author}] {diffstat}\n'
  @  extended jungle [Cornelius] 1: +1/-0
  │
  o  jungle [Zephir] 1: +1/-0
  │
  o  five [Arthur] 2: +2/-1
  │
  o  four [Rataxes] 1: +1/-1
  │
  o  three [Celeste] 1: +1/-1
  │
  o  initial [Babar] 2: +8/-0

# Adding those config options should not change the output of diffstat. Bugfix #4755.

  $ hg log -r . --template '{diffstat}\n'
  1: +1/-0
  $ hg log -r . --template '{diffstat}\n' --config 'diff.git=1' --config 'diff.noprefix=1'
  1: +1/-0

# Importing with some success and some errors:

  $ hg goto --rev 'desc(initial)'
  2 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg export --rev 'desc(five)' | hg import --partial -
  applying patch from stdin
  patching file a
  Hunk #1 FAILED at 1
  1 out of 1 hunks FAILED -- saving rejects to file a.rej
  patch applied partially
  (fix the .rej files and run `hg commit --amend`)
  [1]

  $ hg log -G --template '{desc|firstline} [{author}] {diffstat}\n'
  @  five [Arthur] 1: +1/-0
  │
  │ o  extended jungle [Cornelius] 1: +1/-0
  │ │
  │ o  jungle [Zephir] 1: +1/-0
  │ │
  │ o  five [Arthur] 2: +2/-1
  │ │
  │ o  four [Rataxes] 1: +1/-1
  │ │
  │ o  three [Celeste] 1: +1/-1
  ├─╯
  o  initial [Babar] 2: +8/-0
  $ hg export
  # HG changeset patch
  # User Arthur
  # Date 0 0
  #      Thu Jan 01 00:00:00 1970 +0000
  # Node ID 26e6446bb2526e2be1037935f5fca2b2706f1509
  # Parent  8e4f0351909eae6b9cf68c2c076cb54c42b54b2e
  five
  
  diff -r 8e4f0351909e -r 26e6446bb252 b
  --- a/b	Thu Jan 01 00:00:00 1970 +0000
  +++ b/b	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +1,2 @@
   b
  +bb
  $ hg status -c .
  C a
  C b
  $ ls
  a
  a.rej
  b

# Importing with zero success:

  $ hg goto --rev 'desc(initial)'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg export --rev 'desc(four)' | hg import --partial -
  applying patch from stdin
  patching file a
  Hunk #1 FAILED at 0
  1 out of 1 hunks FAILED -- saving rejects to file a.rej
  patch applied partially
  (fix the .rej files and run `hg commit --amend`)
  [1]

  $ hg log -G --template '{desc|firstline} [{author}] {diffstat}\n'
  @  four [Rataxes] 0: +0/-0
  │
  │ o  five [Arthur] 1: +1/-0
  ├─╯
  │ o  extended jungle [Cornelius] 1: +1/-0
  │ │
  │ o  jungle [Zephir] 1: +1/-0
  │ │
  │ o  five [Arthur] 2: +2/-1
  │ │
  │ o  four [Rataxes] 1: +1/-1
  │ │
  │ o  three [Celeste] 1: +1/-1
  ├─╯
  o  initial [Babar] 2: +8/-0
  $ hg export
  # HG changeset patch
  # User Rataxes
  # Date 0 0
  #      Thu Jan 01 00:00:00 1970 +0000
  # Node ID cb9b1847a74d9ad52e93becaf14b98dbcc274e1e
  # Parent  8e4f0351909eae6b9cf68c2c076cb54c42b54b2e
  four
  $ hg status -c .
  C a
  C b
  $ ls
  a
  a.rej
  b

# Importing with unknown file:

  $ hg goto --rev 'desc(initial)'
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg export --rev 'desc("extended jungle")' | hg import --partial -
  applying patch from stdin
  unable to find 'jungle' for patching
  (use '--prefix' to apply patch relative to the current directory)
  1 out of 1 hunks FAILED -- saving rejects to file jungle.rej
  patch applied partially
  (fix the .rej files and run `hg commit --amend`)
  [1]

  $ hg log -G --template '{desc|firstline} [{author}] {diffstat}\n'
  @  extended jungle [Cornelius] 0: +0/-0
  │
  │ o  four [Rataxes] 0: +0/-0
  ├─╯
  │ o  five [Arthur] 1: +1/-0
  ├─╯
  │ o  extended jungle [Cornelius] 1: +1/-0
  │ │
  │ o  jungle [Zephir] 1: +1/-0
  │ │
  │ o  five [Arthur] 2: +2/-1
  │ │
  │ o  four [Rataxes] 1: +1/-1
  │ │
  │ o  three [Celeste] 1: +1/-1
  ├─╯
  o  initial [Babar] 2: +8/-0
  $ hg export
  # HG changeset patch
  # User Cornelius
  # Date 0 0
  #      Thu Jan 01 00:00:00 1970 +0000
  # Node ID 1fb1f86bef43c5a75918178f8d23c29fb0a7398d
  # Parent  8e4f0351909eae6b9cf68c2c076cb54c42b54b2e
  extended jungle
  $ hg status -c .
  C a
  C b
  $ ls
  a
  a.rej
  b
  jungle.rej

# Importing multiple failing patches:

  $ hg goto --rev 'desc(initial)'
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo B > b
  $ hg commit -m 'a new base'
  $ hg export --rev 'desc("four") + desc("extended jungle")' | hg import --partial -
  applying patch from stdin
  patching file a
  Hunk #1 FAILED at 0
  1 out of 1 hunks FAILED -- saving rejects to file a.rej
  patch applied partially
  (fix the .rej files and run `hg commit --amend`)
  [1]
  $ hg log -G --template '{desc|firstline} [{author}] {diffstat}\n'
  @  four [Rataxes] 0: +0/-0
  │
  o  a new base [test] 1: +1/-1
  │
  │ o  extended jungle [Cornelius] 0: +0/-0
  ├─╯
  │ o  four [Rataxes] 0: +0/-0
  ├─╯
  │ o  five [Arthur] 1: +1/-0
  ├─╯
  │ o  extended jungle [Cornelius] 1: +1/-0
  │ │
  │ o  jungle [Zephir] 1: +1/-0
  │ │
  │ o  five [Arthur] 2: +2/-1
  │ │
  │ o  four [Rataxes] 1: +1/-1
  │ │
  │ o  three [Celeste] 1: +1/-1
  ├─╯
  o  initial [Babar] 2: +8/-0
  $ hg export
  # HG changeset patch
  # User Rataxes
  # Date 0 0
  #      Thu Jan 01 00:00:00 1970 +0000
  # Node ID a9d7b6d0ffbb4eb12b7d5939250fcd42e8930a1d
  # Parent  f59f8d2e95a8ca5b1b4ca64320140da85f3b44fd
  four
  $ hg status -c .
  C a
  C b

# Importing some extra header
# ===========================

  $ cat > $TESTTMP/parseextra.py << 'EOF'
  > import edenscm.patch
  > import edenscm.cmdutil
  > 
  > def processfoo(repo, data, extra, opts):
  >     if 'foo' in data:
  >         extra['foo'] = data['foo']
  > def postimport(ctx):
  >     if 'foo' in ctx.extra():
  >         ctx.repo().ui.write('imported-foo: %s\n' % ctx.extra()['foo'])
  > 
  > edenscm.patch.patchheadermap.append((b'Foo', 'foo'))
  > edenscm.cmdutil.extrapreimport.append('foo')
  > edenscm.cmdutil.extrapreimportmap['foo'] = processfoo
  > edenscm.cmdutil.extrapostimport.append('foo')
  > edenscm.cmdutil.extrapostimportmap['foo'] = postimport
  > EOF
  $ cat >> $HGRCPATH << 'EOF'
  > [extensions]
  > parseextra=$TESTTMP/parseextra.py
  > EOF
  $ hg up -C tip
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cat > $TESTTMP/foo.patch << 'EOF'
  > # HG changeset patch
  > # User Rataxes
  > # Date 0 0
  > #      Thu Jan 01 00:00:00 1970 +0000
  > # Foo bar
  > height
  > 
  > --- a/a	Thu Jan 01 00:00:00 1970 +0000
  > +++ b/a	Wed Oct 07 09:17:44 2015 +0000
  > @@ -5,3 +5,4 @@
  >  five
  >  six
  >  seven
  > +heigt
  > EOF
  $ hg import "$TESTTMP/foo.patch"
  applying $TESTTMP/foo.patch
  imported-foo: bar
  $ hg log --debug -r . -T '{extras}'
  branch=defaultfoo=bar (no-eol)

# Warn the user that paths are relative to the root of
# repository when file not found for patching

  $ mkdir filedir
  $ echo file1 >> filedir/file1
  $ hg add filedir/file1
  $ hg commit -m file1
  $ cd filedir
  $ hg import -p 2 - << 'EOS'
  > # HG changeset patch
  > # User test
  > # Date 0 0
  > file2
  > 
  > diff --git a/filedir/file1 b/filedir/file1
  > --- a/filedir/file1
  > +++ b/filedir/file1
  > @@ -1,1 +1,2 @@
  >  file1
  > +file2
  > EOS
  applying patch from stdin
  unable to find 'file1' for patching
  (use '--prefix' to apply patch relative to the current directory)
  1 out of 1 hunks FAILED -- saving rejects to file file1.rej
  abort: patch failed to apply
  [255]

# test import crash (issue5375)

  $ cd ..
  $ newclientrepo repo
  $ printf 'diff --git a/a b/b\nrename from a\nrename to b' | hg import -
  applying patch from stdin
  abort: source file 'a' does not exist
  [255]

  $ printf "echo 🍺" > unicode.txt

  $ hg commit -Aqm unicode
  $ hg rm unicode.txt
  $ hg commit -qm remove
  $ hg export --rev 'desc(unicode)' | hg import -
  applying patch from stdin

