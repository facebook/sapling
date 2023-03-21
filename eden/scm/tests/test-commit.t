#chg-compatible
#debugruntest-compatible

  $ setconfig status.use-rust=false workingcopy.ruststatus=false

commit date test

  $ hg init test
  $ cd test
  $ echo foo > foo
  $ hg add foo
  $ cat > $TESTTMP/checkeditform.sh <<EOF
  > env | grep HGEDITFORM
  > true
  > EOF
  $ HGEDITOR="sh $TESTTMP/checkeditform.sh" hg commit -m ""
  HGEDITFORM=commit.normal.normal
  abort: empty commit message
  [255]
  $ hg commit -d '0 0' -m commit-1
  $ echo foo >> foo
  $ hg commit -d '1 4444444' -m commit-3
  hg: parse error: invalid date: '1 4444444'
  [255]
  $ hg commit -d '1	15.1' -m commit-4
  hg: parse error: invalid date: '1\t15.1'
  [255]
  $ hg commit -d 'foo bar' -m commit-5
  hg: parse error: invalid date: 'foo bar'
  [255]
#if linuxormacos
  $ echo commit-6 > $TESTTMP/commit-msg
  $ hg commit -d ' 1 4444' -l $TESTTMP/commit-msg
#else
  $ hg commit -d ' 1 4444' -m commit-6
#endif
  $ hg commit -d '1111111111111 0' -m commit-7
  hg: parse error: invalid date: '1111111111111 0'
  [255]
  $ hg commit -d '-111111111111 0' -m commit-7
  hg: parse error: invalid date: '-111111111111 0'
  [255]
  $ echo foo >> foo
  $ hg commit -d '1901-12-13 20:45:52 +0000' -m commit-7-2
  $ echo foo >> foo
  $ hg commit -d '-2147483648 0' -m commit-7-3
  $ hg log -T '{date|isodatesec}\n' -l2
  1901-12-13 20:45:52 +0000
  1901-12-13 20:45:52 +0000
  $ hg commit -d '1899-12-13 20:45:51 +0000' -m commit-7
  hg: parse error: invalid date: '1899-12-13 20:45:51 +0000'
  [255]
  $ hg commit -d '-3147483649 0' -m commit-7
  hg: parse error: invalid date: '-3147483649 0'
  [255]

commit added file that has been deleted

  $ echo bar > bar
  $ hg add bar
  $ rm bar
  $ hg commit -m commit-8
  nothing changed (1 missing files, see 'hg status')
  [1]
  $ hg commit -m commit-8-2 bar
  abort: bar: file not found!
  [255]

  $ hg -q revert -a --no-backup

  $ mkdir dir
  $ echo boo > dir/file
  $ hg add
  adding dir/file
  $ hg -v commit -m commit-9 dir
  committing files:
  dir/file
  committing manifest
  committing changelog
  committed * (glob)

  $ echo > dir.file
  $ hg add
  adding dir.file
  $ hg commit -m commit-10 dir dir.file
  abort: dir: no match under directory!
  [255]

  $ echo >> dir/file
  $ mkdir bleh
  $ mkdir dir2
  $ cd bleh
  $ hg commit -m commit-11 .
  abort: bleh: no match under directory!
  [255]
  $ hg commit -m commit-12 ../dir ../dir2
  abort: dir2: no match under directory!
  [255]
  $ hg -v commit -m commit-13 ../dir
  committing files:
  dir/file
  committing manifest
  committing changelog
  committed * (glob)
  $ cd ..

  $ hg commit -m commit-14 does-not-exist
  abort: does-not-exist: * (glob)
  [255]

#if symlink
  $ ln -s foo baz
  $ hg commit -m commit-15 baz
  abort: baz: file not tracked!
  [255]
  $ rm baz
#endif

  $ touch quux
  $ hg commit -m commit-16 quux
  abort: quux: file not tracked!
  [255]
  $ echo >> dir/file
  $ hg -v commit -m commit-17 dir/file
  committing files:
  dir/file
  committing manifest
  committing changelog
  committed * (glob)

An empty date was interpreted as epoch origin

  $ echo foo >> foo
  $ hg commit -d '' -m commit-no-date --config devel.default-date=
  $ hg tip --template '{date|isodate}\n' | grep '1970'
  [1]

Using the advanced --extra flag

  $ echo "[extensions]" >> $HGRCPATH
  $ echo "commitextras=" >> $HGRCPATH
  $ hg status
  ? quux
  $ hg add quux
  $ hg commit -m "adding internal used extras" --extra amend_source=hash
  abort: key 'amend_source' is used internally, can't be set manually
  [255]
  $ hg commit -m "special chars in extra" --extra id@phab=214
  abort: keys can only contain ascii letters, digits, '_' and '-'
  [255]
  $ hg commit -m "empty key" --extra =value
  abort: unable to parse '=value', keys can't be empty
  [255]
  $ hg commit -m "adding extras" --extra sourcehash=foo --extra oldhash=bar
  $ hg log -r . -T '{extras % "{extra}\n"}'
  branch=default
  oldhash=bar
  sourcehash=foo

Failed commit with --addremove should not update dirstate

  $ echo foo > newfile
  $ hg status
  ? newfile
  $ HGEDITOR=false hg ci --addremove
  adding newfile
  abort: edit failed: false exited with status 1
  [255]
  $ hg status
  ? newfile

Make sure we do not obscure unknown requires file entries (issue2649)

  $ echo foo >> foo
  $ echo fake >> .hg/requires
  $ hg commit -m bla
  abort: repository requires unknown features: fake
  (see https://mercurial-scm.org/wiki/MissingRequirement for more information)
  [255]

  $ cd ..


partial subdir commit test

  $ hg init test2
  $ cd test2
  $ mkdir foo
  $ echo foo > foo/foo
  $ mkdir bar
  $ echo bar > bar/bar
  $ hg add
  adding bar/bar
  adding foo/foo
  $ HGEDITOR=cat hg ci -e -m commit-subdir-1 foo
  commit-subdir-1
  
  
  HG: Enter commit message.  Lines beginning with 'HG:' are removed.
  HG: Leave message empty to abort commit.
  HG: --
  HG: user: test
  HG: branch 'default'
  HG: added foo/foo


  $ hg ci -m commit-subdir-2 bar

subdir log 1

  $ hg log -v foo
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       foo/foo
  description:
  commit-subdir-1
  
  

subdir log 2

  $ hg log -v bar
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       bar/bar
  description:
  commit-subdir-2
  
  

full log

  $ hg log -v
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       bar/bar
  description:
  commit-subdir-2
  
  
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       foo/foo
  description:
  commit-subdir-1
  
  
  $ cd ..


dot and subdir commit test

  $ hg init test3
  $ echo commit-foo-subdir > commit-log-test
  $ cd test3
  $ mkdir foo
  $ echo foo content > foo/plain-file
  $ hg add foo/plain-file
  $ HGEDITOR=cat hg ci --edit -l ../commit-log-test foo
  commit-foo-subdir
  
  
  HG: Enter commit message.  Lines beginning with 'HG:' are removed.
  HG: Leave message empty to abort commit.
  HG: --
  HG: user: test
  HG: branch 'default'
  HG: added foo/plain-file


  $ echo modified foo content > foo/plain-file
  $ hg ci -m commit-foo-dot .

full log

  $ hg log -v
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       foo/plain-file
  description:
  commit-foo-dot
  
  
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       foo/plain-file
  description:
  commit-foo-subdir
  
  

subdir log

  $ cd foo
  $ hg log .
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     commit-foo-dot
  
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     commit-foo-subdir
  
  $ cd ..
  $ cd ..

Issue1049: Hg permits partial commit of merge without warning

  $ hg init issue1049
  $ cd issue1049
  $ echo a > a
  $ hg ci -Ama
  adding a
  $ echo a >> a
  $ hg ci -mb
  $ hg up 'desc(a)'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo b >> a
  $ hg ci -mc
  $ HGMERGE=true hg merge
  merging a
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)

should fail because we are specifying a file name

  $ hg ci -mmerge a
  abort: cannot partially commit a merge (do not specify files or patterns)
  [255]

should fail because we are specifying a pattern

  $ hg ci -mmerge -I a
  abort: cannot partially commit a merge (do not specify files or patterns)
  [255]

should succeed

  $ HGEDITOR="sh $TESTTMP/checkeditform.sh" hg ci -mmerge --edit
  HGEDITFORM=commit.normal.merge
  $ cd ..


test commit message content

  $ hg init commitmsg
  $ cd commitmsg
  $ echo changed > changed
  $ echo removed > removed
  $ hg book activebookmark
  $ hg ci -qAm init

  $ hg rm removed
  $ echo changed >> changed
  $ echo added > added
  $ hg add added
  $ HGEDITOR=cat hg ci -A
  
  
  HG: Enter commit message.  Lines beginning with 'HG:' are removed.
  HG: Leave message empty to abort commit.
  HG: --
  HG: user: test
  HG: branch 'default'
  HG: bookmark 'activebookmark'
  HG: added added
  HG: changed changed
  HG: removed removed
  abort: empty commit message
  [255]

test saving last-message.txt

  $ hg init sub
  $ echo a > sub/a
  $ hg -R sub add sub/a
  $ cat > .hg/hgrc <<EOF
  > [hooks]
  > precommit.test-saving-last-message = false
  > EOF

  $ echo 'sub = sub' > .hgsub
  $ hg add .hgsub

  $ cat > $TESTTMP/editor.sh <<EOF
  > echo "==== before editing:"
  > cat \$1
  > echo "===="
  > echo "test saving last-message.txt" >> \$1
  > EOF

  $ rm -f .hg/last-message.txt
  $ HGEDITOR="sh $TESTTMP/editor.sh" hg commit -q
  ==== before editing:
  
  
  HG: Enter commit message.  Lines beginning with 'HG:' are removed.
  HG: Leave message empty to abort commit.
  HG: --
  HG: user: test
  HG: branch 'default'
  HG: bookmark 'activebookmark'
  HG: added .hgsub
  HG: added added
  HG: changed changed
  HG: removed removed
  ====
  note: commit message saved in .hg/last-message.txt
  abort: precommit.test-saving-last-message hook exited with status 1
  [255]
  $ cat .hg/last-message.txt
  
  
  test saving last-message.txt

test that '[committemplate] changeset' definition and commit log
specific template keywords work well

  $ cat >> .hg/hgrc <<EOF
  > [committemplate]
  > changeset.commit.normal = 'HG: this is "commit.normal" template
  >     HG: {extramsg}
  >     {if(activebookmark,
  >    "HG: bookmark '{activebookmark}' is activated\n",
  >    "HG: no bookmark is activated\n")}{subrepos %
  >    "HG: subrepo '{subrepo}' is changed\n"}'
  > 
  > changeset.commit = HG: this is "commit" template
  >     HG: {extramsg}
  >     {if(activebookmark,
  >    "HG: bookmark '{activebookmark}' is activated\n",
  >    "HG: no bookmark is activated\n")}{subrepos %
  >    "HG: subrepo '{subrepo}' is changed\n"}
  > 
  > changeset = HG: this is customized commit template
  >     HG: {extramsg}
  >     {if(activebookmark,
  >    "HG: bookmark '{activebookmark}' is activated\n",
  >    "HG: no bookmark is activated\n")}{subrepos %
  >    "HG: subrepo '{subrepo}' is changed\n"}
  > EOF

  $ hg init sub2
  $ echo a > sub2/a
  $ hg -R sub2 add sub2/a
  $ echo 'sub2 = sub2' >> .hgsub

  $ HGEDITOR=cat hg commit -q
  HG: this is "commit.normal" template
  HG: Leave message empty to abort commit.
  HG: bookmark 'activebookmark' is activated
  abort: empty commit message
  [255]

  $ cat >> .hg/hgrc <<EOF
  > [committemplate]
  > changeset.commit.normal =
  > # now, "changeset.commit" should be chosen for "hg commit"
  > EOF

  $ hg bookmark --inactive activebookmark
  $ hg forget .hgsub
  $ HGEDITOR=cat hg commit -q
  HG: this is "commit" template
  HG: Leave message empty to abort commit.
  HG: no bookmark is activated
  abort: empty commit message
  [255]

  $ cat >> .hg/hgrc <<EOF
  > [committemplate]
  > changeset.commit =
  > # now, "changeset" should be chosen for "hg commit"
  > EOF

  $ HGEDITOR=cat hg commit -q
  HG: this is customized commit template
  HG: Leave message empty to abort commit.
  HG: no bookmark is activated
  abort: empty commit message
  [255]

  $ cat >> .hg/hgrc <<EOF
  > [committemplate]
  > changeset = {desc}
  >     HG: mods={file_mods}
  >     HG: adds={file_adds}
  >     HG: dels={file_dels}
  >     HG: files={files}
  >     HG:
  >     {splitlines(diff()) % 'HG: {line}\n'
  >    }HG:
  >     HG: mods={file_mods}
  >     HG: adds={file_adds}
  >     HG: dels={file_dels}
  >     HG: files={files}\n
  > EOF
  $ hg status -amr
  M changed
  A added
  R removed
  $ HGEDITOR=cat hg commit -q -e -m "foo bar" changed
  foo bar
  HG: mods=changed
  HG: adds=
  HG: dels=
  HG: files=changed
  HG:
  HG: --- a/changed	Thu Jan 01 00:00:00 1970 +0000
  HG: +++ b/changed	Thu Jan 01 00:00:00 1970 +0000
  HG: @@ -1,1 +1,2 @@
  HG:  changed
  HG: +changed
  HG:
  HG: mods=changed
  HG: adds=
  HG: dels=
  HG: files=changed
  note: commit message saved in .hg/last-message.txt
  abort: precommit.test-saving-last-message hook exited with status 1
  [255]
  $ hg status -amr
  M changed
  A added
  R removed
  $ hg parents --template "M {file_mods}\nA {file_adds}\nR {file_dels}\n"
  M 
  A changed removed
  R 

  $ cat >> .hg/hgrc <<EOF
  > [committemplate]
  > changeset = {desc}
  >     HG: mods={file_mods}
  >     HG: adds={file_adds}
  >     HG: dels={file_dels}
  >     HG: files={files}
  >     HG:
  >     {splitlines(diff("changed")) % 'HG: {line}\n'
  >    }HG:
  >     HG: mods={file_mods}
  >     HG: adds={file_adds}
  >     HG: dels={file_dels}
  >     HG: files={files}
  >     HG:
  >     {splitlines(diff("added")) % 'HG: {line}\n'
  >    }HG:
  >     HG: mods={file_mods}
  >     HG: adds={file_adds}
  >     HG: dels={file_dels}
  >     HG: files={files}
  >     HG:
  >     {splitlines(diff("removed")) % 'HG: {line}\n'
  >    }HG:
  >     HG: mods={file_mods}
  >     HG: adds={file_adds}
  >     HG: dels={file_dels}
  >     HG: files={files}\n
  > EOF
  $ HGEDITOR=cat hg commit -q -e -m "foo bar" added removed
  foo bar
  HG: mods=
  HG: adds=added
  HG: dels=removed
  HG: files=added removed
  HG:
  HG:
  HG: mods=
  HG: adds=added
  HG: dels=removed
  HG: files=added removed
  HG:
  HG: --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  HG: +++ b/added	Thu Jan 01 00:00:00 1970 +0000
  HG: @@ -0,0 +1,1 @@
  HG: +added
  HG:
  HG: mods=
  HG: adds=added
  HG: dels=removed
  HG: files=added removed
  HG:
  HG: --- a/removed	Thu Jan 01 00:00:00 1970 +0000
  HG: +++ /dev/null	Thu Jan 01 00:00:00 1970 +0000
  HG: @@ -1,1 +0,0 @@
  HG: -removed
  HG:
  HG: mods=
  HG: adds=added
  HG: dels=removed
  HG: files=added removed
  note: commit message saved in .hg/last-message.txt
  abort: precommit.test-saving-last-message hook exited with status 1
  [255]
  $ hg status -amr
  M changed
  A added
  R removed
  $ hg parents --template "M {file_mods}\nA {file_adds}\nR {file_dels}\n"
  M 
  A changed removed
  R 

  $ cat >> .hg/hgrc <<EOF
  > # disable customizing for subsequent tests
  > [committemplate]
  > changeset =
  > EOF

  $ cd ..


commit copy

  $ hg init dir2
  $ cd dir2
  $ echo bleh > bar
  $ hg add bar
  $ hg ci -m 'add bar'

  $ hg cp bar foo
  $ echo >> bar
  $ hg ci -m 'cp bar foo; change bar'

  $ hg debugrename foo
  foo renamed from bar:26d3ca0dfd18e44d796b564e38dd173c9668d3a9
  $ hg debugindex bar
     rev    offset  length  ..... linkrev nodeid       p1           p2 (re)
       0         0       6  .....       0 26d3ca0dfd18 000000000000 000000000000 (re)
       1         6       7  .....       1 d267bddd54f7 26d3ca0dfd18 000000000000 (re)

Test making empty commits
  $ hg commit --config ui.allowemptycommit=True -m "empty commit"
  $ hg log -r . -v --stat
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  description:
  empty commit
  
  
  
verify pathauditor blocks evil filepaths
  $ cp -R . $TESTTMP/audit2
  $ cp -R . $TESTTMP/audit3
  $ cat > evil-commit.py <<EOF
  > from __future__ import absolute_import
  > from edenscm import context, hg, node, pycompat, ui as uimod
  > notrc = pycompat.ensurestr(u".h\u200cg/hgrc")
  > u = uimod.ui.load()
  > r = hg.repository(u, '.')
  > def filectxfn(repo, memctx, path):
  >     return context.memfilectx(repo, memctx, path,
  >         b'[hooks]\nupdate = echo owned')
  > c = context.memctx(r, [r['tip']],
  >                    'evil', [notrc], filectxfn, 0)
  > r.commitctx(c)
  > EOF
  $ hg debugpython -- evil-commit.py
#if windows
  $ hg co --clean tip
  abort: path contains illegal component: .h\xe2\x80\x8cg\\hgrc (esc)
  [255]
#else
  $ hg co --clean tip
  abort: path contains illegal component: .h\xe2\x80\x8cg/hgrc (esc)
  [255]
#endif

  $ cd $TESTTMP/audit2
  $ cat > evil-commit.py <<EOF
  > from __future__ import absolute_import
  > from edenscm import context, hg, node, ui as uimod
  > notrc = "HG~1/hgrc"
  > u = uimod.ui.load()
  > r = hg.repository(u, '.')
  > def filectxfn(repo, memctx, path):
  >     return context.memfilectx(repo, memctx, path,
  >         b'[hooks]\nupdate = echo owned')
  > c = context.memctx(r, [r['tip']],
  >                    'evil', [notrc], filectxfn, 0)
  > r.commitctx(c)
  > EOF
  $ hg debugpython -- evil-commit.py
  $ hg co --clean tip
  abort: path contains illegal component: HG~1/hgrc
  [255]

  $ cd $TESTTMP/audit3
  $ cat > evil-commit.py <<EOF
  > from __future__ import absolute_import
  > from edenscm import context, hg, node, ui as uimod
  > notrc = "HG8B6C~2/hgrc"
  > u = uimod.ui.load()
  > r = hg.repository(u, '.')
  > def filectxfn(repo, memctx, path):
  >     return context.memfilectx(repo, memctx, path,
  >         b'[hooks]\nupdate = echo owned')
  > c = context.memctx(r, [r['tip']],
  >                    'evil', [notrc], filectxfn, 0)
  > r.commitctx(c)
  > EOF
  $ hg debugpython -- evil-commit.py
  $ hg co --clean tip
  abort: path contains illegal component: HG8B6C~2/hgrc
  [255]

# test that an unmodified commit template message aborts

  $ hg init unmodified_commit_template
  $ cd unmodified_commit_template
  $ echo foo > foo
  $ hg add foo
  $ hg commit -m "foo"
  $ cat >> .hg/hgrc <<EOF
  > [committemplate]
  > changeset.commit = HI THIS IS NOT STRIPPED
  >     HG: this is customized commit template
  >     HG: {extramsg}
  >     {if(activebookmark,
  >    "HG: bookmark '{activebookmark}' is activated\n",
  >    "HG: no bookmark is activated\n")}
  > EOF
  $ cat > $TESTTMP/notouching.sh <<EOF
  > true
  > EOF
  $ echo foo2 > foo2
  $ hg add foo2
  $ HGEDITOR="sh $TESTTMP/notouching.sh" hg commit
  abort: commit message unchanged
  [255]

test that text below the --- >8 --- special string is ignored

  $ cat <<'EOF' > $TESTTMP/lowercaseline.sh
  > cat $1 | sed s/LINE/line/ | tee $1.new
  > mv $1.new $1
  > EOF

  $ hg init ignore_below_special_string
  $ cd ignore_below_special_string
  $ echo foo > foo
  $ hg add foo
  $ hg commit -m "foo"
  $ cat >> .hg/hgrc <<EOF
  > [committemplate]
  > changeset.commit = first LINE
  >     HG: this is customized commit template
  >     HG: {extramsg}
  >     HG: ------------------------ >8 ------------------------
  >     {diff()}
  > EOF
  $ echo foo2 > foo2
  $ hg add foo2
  $ HGEDITOR="sh $TESTTMP/notouching.sh" hg ci
  abort: commit message unchanged
  [255]
  $ HGEDITOR="sh $TESTTMP/lowercaseline.sh" hg ci
  first line
  HG: this is customized commit template
  HG: Leave message empty to abort commit.
  HG: ------------------------ >8 ------------------------
  diff -r e63c23eaa88a foo2
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/foo2	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +foo2
  $ hg log -T '{desc}\n' -r .
  first line

test that the special string --- >8 --- isn't used when not at the beginning of
a line

  $ cat >> .hg/hgrc <<EOF
  > [committemplate]
  > changeset.commit = first LINE2
  >     another line HG: ------------------------ >8 ------------------------
  >     HG: this is customized commit template
  >     HG: {extramsg}
  >     HG: ------------------------ >8 ------------------------
  >     {diff()}
  > EOF
  $ echo foo >> foo
  $ HGEDITOR="sh $TESTTMP/lowercaseline.sh" hg ci
  first line2
  another line HG: ------------------------ >8 ------------------------
  HG: this is customized commit template
  HG: Leave message empty to abort commit.
  HG: ------------------------ >8 ------------------------
  diff -r 3661b22b0702 foo
  --- a/foo	Thu Jan 01 00:00:00 1970 +0000
  +++ b/foo	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +1,2 @@
   foo
  +foo
  $ hg log -T '{desc}\n' -r .
  first line2
  another line HG: ------------------------ >8 ------------------------

also test that this special string isn't accepted when there is some extra text
at the end

  $ cat >> .hg/hgrc <<EOF
  > [committemplate]
  > changeset.commit = first LINE3
  >     HG: ------------------------ >8 ------------------------foobar
  >     second line
  >     HG: this is customized commit template
  >     HG: {extramsg}
  >     HG: ------------------------ >8 ------------------------
  >     {diff()}
  > EOF
  $ echo foo >> foo
  $ HGEDITOR="sh $TESTTMP/lowercaseline.sh" hg ci
  first line3
  HG: ------------------------ >8 ------------------------foobar
  second line
  HG: this is customized commit template
  HG: Leave message empty to abort commit.
  HG: ------------------------ >8 ------------------------
  diff -r ce648f5f066f foo
  --- a/foo	Thu Jan 01 00:00:00 1970 +0000
  +++ b/foo	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,2 +1,3 @@
   foo
   foo
  +foo
  $ hg log -T '{desc}\n' -r .
  first line3
  second line

  $ cd ..

