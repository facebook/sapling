
#require no-eden


  $ eagerepo
This is needed to avoid filelog() revset in "log", which isn't compatible w/ eagerepo.
  $ setconfig experimental.pathhistory=true
  $ setconfig checkout.use-rust=true

commit date test

  $ sl init test
  $ cd test
  $ echo foo > foo
  $ sl add foo
  $ cat > $TESTTMP/checkeditform.sh <<EOF
  > env | grep HGEDITFORM
  > true
  > EOF
  $ HGEDITOR="sh $TESTTMP/checkeditform.sh" sl commit -m ""
  HGEDITFORM=commit.normal.normal
  abort: empty commit message
  [255]
  $ sl commit -d '0 0' -m commit-1
  $ echo foo >> foo
  $ sl commit -d '1 4444444' -m commit-3
  sl: parse error: invalid date: '1 4444444'
  [255]
  $ sl commit -d '1	15.1' -m commit-4
  sl: parse error: invalid date: '1\t15.1'
  [255]
  $ sl commit -d 'foo bar' -m commit-5
  sl: parse error: invalid date: 'foo bar'
  [255]
#if linuxormacos
  $ echo commit-6 > $TESTTMP/commit-msg
  $ sl commit -d ' 1 4444' -l $TESTTMP/commit-msg
#else
  $ sl commit -d ' 1 4444' -m commit-6
#endif
  $ sl commit -d '1111111111111 0' -m commit-7
  sl: parse error: invalid date: '1111111111111 0'
  [255]
  $ sl commit -d '-111111111111 0' -m commit-7
  sl: parse error: invalid date: '-111111111111 0'
  [255]
  $ echo foo >> foo
  $ sl commit -d '1901-12-13 20:45:52 +0000' -m commit-7-2
  $ echo foo >> foo
  $ sl commit -d '-2147483648 0' -m commit-7-3
  $ sl log -T '{date|isodatesec}\n' -l2
  1901-12-13 20:45:52 +0000
  1901-12-13 20:45:52 +0000
  $ sl commit -d '1899-12-13 20:45:51 +0000' -m commit-7
  sl: parse error: invalid date: '1899-12-13 20:45:51 +0000'
  [255]
  $ sl commit -d '-3147483649 0' -m commit-7
  sl: parse error: invalid date: '-3147483649 0'
  [255]

multiple commit messages are separated by blank lines

  $ echo foo >> foo
  $ sl commit -m title -m body -m body2
  $ sl log -r . -T '{desc}\n'
  title
  
  body
  
  body2

commit added file that has been deleted

  $ echo bar > bar
  $ sl add bar
  $ rm bar
  $ sl commit -m commit-8
  nothing changed (1 missing files, see 'sl status')
  [1]
  $ sl commit -m commit-8-2 bar
  abort: bar: file not found!
  [255]

  $ sl -q revert -a --no-backup

  $ mkdir dir
  $ echo boo > dir/file
  $ sl add
  adding dir/file
  $ sl -v commit -m commit-9 dir
  committing files:
  dir/file
  committing manifest
  committing changelog
  committed * (glob)

  $ echo > dir.file
  $ sl add
  adding dir.file
  $ sl commit -m commit-10 dir dir.file
  abort: dir: no match under directory!
  [255]

  $ echo >> dir/file
  $ mkdir bleh
  $ mkdir dir2
  $ cd bleh
  $ sl commit -m commit-11 .
  abort: bleh: no match under directory!
  [255]
  $ sl commit -m commit-12 ../dir ../dir2
  abort: dir2: no match under directory!
  [255]
  $ sl -v commit -m commit-13 ../dir
  committing files:
  dir/file
  committing manifest
  committing changelog
  committed * (glob)
  $ cd ..

  $ sl commit -m commit-14 does-not-exist
  does-not-exist: $ENOENT$
  abort: does-not-exist: $ENOENT$
  [255]

#if symlink
  $ ln -s foo baz
  $ sl commit -m commit-15 baz
  abort: baz: file not tracked!
  [255]
  $ rm baz
#endif

  $ touch quux
  $ sl commit -m commit-16 quux
  abort: quux: file not tracked!
  [255]
  $ echo >> dir/file
  $ sl -v commit -m commit-17 dir/file
  committing files:
  dir/file
  committing manifest
  committing changelog
  committed * (glob)

An empty date was interpreted as epoch origin

  $ echo foo >> foo
  $ sl commit -d '' -m commit-no-date --config devel.default-date=
  $ sl tip --template '{date|isodate}\n' | grep '1970'
  [1]

Using the advanced --extra flag

  $ echo "[extensions]" >> $HGRCPATH
  $ echo "commitextras=" >> $HGRCPATH
  $ sl status
  ? quux
  $ sl add quux
  $ sl commit -m "adding internal used extras" --extra amend_source=hash
  abort: key 'amend_source' is used internally, can't be set manually
  [255]
  $ sl commit -m "special chars in extra" --extra id@phab=214
  abort: keys can only contain ascii letters, digits, '_' and '-'
  [255]
  $ sl commit -m "empty key" --extra =value
  abort: unable to parse '=value', keys can't be empty
  [255]
  $ sl commit -m "adding extras" --extra sourcehash=foo --extra oldhash=bar
  $ sl log -r . -T '{extras % "{extra}\n"}'
  branch=default
  oldhash=bar
  sourcehash=foo

Failed commit with --addremove should not update dirstate

  $ echo foo > newfile
  $ sl status
  ? newfile
  $ HGEDITOR=false sl ci --addremove
  adding newfile
  abort: edit failed: false exited with status 1
  [255]
  $ sl status
  ? newfile

Make sure we do not obscure unknown requires file entries (issue2649)

  $ echo foo >> foo
  $ echo fake >> .sl/requires
  $ sl commit -m bla
  abort: repository requires unknown features: fake
  (consider upgrading Sapling)
  [255]

  $ cd ..


partial subdir commit test

  $ sl init test2
  $ cd test2
  $ mkdir foo
  $ echo foo > foo/foo
  $ mkdir bar
  $ echo bar > bar/bar
  $ sl add
  adding bar/bar
  adding foo/foo
  $ HGEDITOR=cat sl ci -e -m commit-subdir-1 foo
  commit-subdir-1
  
  
  SL: Enter commit message.  Lines beginning with 'SL:' are removed.
  SL: Leave message empty to abort commit.
  SL: --
  SL: user: test
  SL: added foo/foo


  $ sl ci -m commit-subdir-2 bar

subdir log 1

  $ sl log -v foo
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       foo/foo
  description:
  commit-subdir-1
  
  

subdir log 2

  $ sl log -v bar
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       bar/bar
  description:
  commit-subdir-2
  
  

full log

  $ sl log -v
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

  $ sl init test3
  $ echo commit-foo-subdir > commit-log-test
  $ cd test3
  $ mkdir foo
  $ echo foo content > foo/plain-file
  $ sl add foo/plain-file
  $ HGEDITOR=cat sl ci --edit -l ../commit-log-test foo
  commit-foo-subdir
  
  
  SL: Enter commit message.  Lines beginning with 'SL:' are removed.
  SL: Leave message empty to abort commit.
  SL: --
  SL: user: test
  SL: added foo/plain-file


  $ echo modified foo content > foo/plain-file
  $ sl ci -m commit-foo-dot .

full log

  $ sl log -v
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
  $ sl log .
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

  $ sl init issue1049
  $ cd issue1049
  $ echo a > a
  $ sl ci -Ama
  adding a
  $ echo a >> a
  $ sl ci -mb
  $ sl up 'desc(a)'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo b >> a
  $ sl ci -mc
  $ HGMERGE=true sl merge
  merging a
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)

should fail because we are specifying a file name

  $ sl ci -mmerge a
  abort: cannot partially commit a merge (do not specify files or patterns)
  [255]

should fail because we are specifying a pattern

  $ sl ci -mmerge -I a
  abort: cannot partially commit a merge (do not specify files or patterns)
  [255]

should succeed

  $ HGEDITOR="sh $TESTTMP/checkeditform.sh" sl ci -mmerge --edit
  HGEDITFORM=commit.normal.merge
  $ cd ..


test commit message content

  $ sl init commitmsg
  $ cd commitmsg
  $ echo changed > changed
  $ echo removed > removed
  $ sl book activebookmark
  $ sl ci -qAm init

  $ sl rm removed
  $ echo changed >> changed
  $ echo added > added
  $ sl add added
  $ HGEDITOR=cat sl ci -A
  
  
  SL: Enter commit message.  Lines beginning with 'SL:' are removed.
  SL: Leave message empty to abort commit.
  SL: --
  SL: user: test
  SL: bookmark 'activebookmark'
  SL: added added
  SL: changed changed
  SL: removed removed
  abort: empty commit message
  [255]

test saving last-message.txt

  $ sl init sub
  $ echo a > sub/a
  $ sl -R sub add sub/a
  $ cat > .sl/config <<EOF
  > [hooks]
  > precommit.test-saving-last-message = false
  > EOF

  $ echo 'sub = sub' > .hgsub
  $ sl add .hgsub

  $ cat > $TESTTMP/editor.sh <<EOF
  > echo "==== before editing:"
  > cat \$1
  > echo "===="
  > echo "test saving last-message.txt" >> \$1
  > EOF

  $ rm -f .sl/last-message.txt
  $ HGEDITOR="sh $TESTTMP/editor.sh" sl commit -q
  ==== before editing:
  
  
  SL: Enter commit message.  Lines beginning with 'SL:' are removed.
  SL: Leave message empty to abort commit.
  SL: --
  SL: user: test
  SL: bookmark 'activebookmark'
  SL: added .hgsub
  SL: added added
  SL: changed changed
  SL: removed removed
  ====
  note: commit message saved in .sl/last-message.txt
  abort: precommit.test-saving-last-message hook exited with status 1
  [255]
  $ cat .sl/last-message.txt
  
  
  test saving last-message.txt

test that '[committemplate] changeset' definition and commit log
specific template keywords work well

  $ cat >> .sl/config <<EOF
  > [committemplate]
  > changeset.commit.normal = '{slprefix}: this is "commit.normal" template
  >     {slprefix}: Leave message empty to abort commit.
  >     {if(activebookmark,
  >    "{slprefix}: bookmark '{activebookmark}' is activated\n",
  >    "{slprefix}: no bookmark is activated\n")}{subrepos %
  >    "{slprefix}: subrepo '{subrepo}' is changed\n"}'
  > 
  > changeset.commit = {slprefix}: this is "commit" template
  >     {slprefix}: Leave message empty to abort commit.
  >     {if(activebookmark,
  >    "{slprefix}: bookmark '{activebookmark}' is activated\n",
  >    "{slprefix}: no bookmark is activated\n")}{subrepos %
  >    "{slprefix}: subrepo '{subrepo}' is changed\n"}
  > 
  > changeset = {slprefix}: this is customized commit template
  >     {slprefix}: Leave message empty to abort commit.
  >     {if(activebookmark,
  >    "{slprefix}: bookmark '{activebookmark}' is activated\n",
  >    "{slprefix}: no bookmark is activated\n")}{subrepos %
  >    "{slprefix}: subrepo '{subrepo}' is changed\n"}
  > EOF

  $ sl init sub2
  $ echo a > sub2/a
  $ sl -R sub2 add sub2/a
  $ echo 'sub2 = sub2' >> .hgsub

  $ HGEDITOR=cat sl commit -q
  SL: this is "commit.normal" template
  SL: Leave message empty to abort commit.
  SL: bookmark 'activebookmark' is activated
  abort: empty commit message
  [255]

  $ cat >> .sl/config <<EOF
  > [committemplate]
  > changeset.commit.normal =
  > # now, "changeset.commit" should be chosen for "sl commit"
  > EOF

  $ sl bookmark --inactive activebookmark
  $ sl forget .hgsub
  $ HGEDITOR=cat sl commit -q
  SL: this is "commit" template
  SL: Leave message empty to abort commit.
  SL: no bookmark is activated
  abort: empty commit message
  [255]

  $ cat >> .sl/config <<EOF
  > [committemplate]
  > changeset.commit =
  > # now, "changeset" should be chosen for "sl commit"
  > EOF

  $ HGEDITOR=cat sl commit -q
  SL: this is customized commit template
  SL: Leave message empty to abort commit.
  SL: no bookmark is activated
  abort: empty commit message
  [255]

  $ cat >> .sl/config <<EOF
  > [committemplate]
  > changeset = {desc}
  >     {slprefix}: mods={file_mods}
  >     {slprefix}: adds={file_adds}
  >     {slprefix}: dels={file_dels}
  >     {slprefix}: files={files}
  >     {slprefix}:
  >     {splitlines(diff()) % '{slprefix}: {line}\n'
  >    }{slprefix}:
  >     {slprefix}: mods={file_mods}
  >     {slprefix}: adds={file_adds}
  >     {slprefix}: dels={file_dels}
  >     {slprefix}: files={files}\n
  > EOF
  $ sl status -amr
  M changed
  A added
  R removed
  $ HGEDITOR=cat sl commit -q -e -m "foo bar" changed
  foo bar
  SL: mods=changed
  SL: adds=
  SL: dels=
  SL: files=changed
  SL:
  SL: --- a/changed	Thu Jan 01 00:00:00 1970 +0000
  SL: +++ b/changed	Thu Jan 01 00:00:00 1970 +0000
  SL: @@ -1,1 +1,2 @@
  SL:  changed
  SL: +changed
  SL:
  SL: mods=changed
  SL: adds=
  SL: dels=
  SL: files=changed
  note: commit message saved in .sl/last-message.txt
  abort: precommit.test-saving-last-message hook exited with status 1
  [255]
  $ sl status -amr
  M changed
  A added
  R removed
  $ sl parents --template "M {file_mods}\nA {file_adds}\nR {file_dels}\n"
  M 
  A changed removed
  R 

  $ cat >> .sl/config <<EOF
  > [committemplate]
  > changeset = {desc}
  >     {slprefix}: mods={file_mods}
  >     {slprefix}: adds={file_adds}
  >     {slprefix}: dels={file_dels}
  >     {slprefix}: files={files}
  >     {slprefix}:
  >     {splitlines(diff("changed")) % '{slprefix}: {line}\n'
  >    }{slprefix}:
  >     {slprefix}: mods={file_mods}
  >     {slprefix}: adds={file_adds}
  >     {slprefix}: dels={file_dels}
  >     {slprefix}: files={files}
  >     {slprefix}:
  >     {splitlines(diff("added")) % '{slprefix}: {line}\n'
  >    }{slprefix}:
  >     {slprefix}: mods={file_mods}
  >     {slprefix}: adds={file_adds}
  >     {slprefix}: dels={file_dels}
  >     {slprefix}: files={files}
  >     {slprefix}:
  >     {splitlines(diff("removed")) % '{slprefix}: {line}\n'
  >    }{slprefix}:
  >     {slprefix}: mods={file_mods}
  >     {slprefix}: adds={file_adds}
  >     {slprefix}: dels={file_dels}
  >     {slprefix}: files={files}\n
  > EOF
  $ HGEDITOR=cat sl commit -q -e -m "foo bar" added removed
  foo bar
  SL: mods=
  SL: adds=added
  SL: dels=removed
  SL: files=added removed
  SL:
  SL:
  SL: mods=
  SL: adds=added
  SL: dels=removed
  SL: files=added removed
  SL:
  SL: --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  SL: +++ b/added	Thu Jan 01 00:00:00 1970 +0000
  SL: @@ -0,0 +1,1 @@
  SL: +added
  SL:
  SL: mods=
  SL: adds=added
  SL: dels=removed
  SL: files=added removed
  SL:
  SL: --- a/removed	Thu Jan 01 00:00:00 1970 +0000
  SL: +++ /dev/null	Thu Jan 01 00:00:00 1970 +0000
  SL: @@ -1,1 +0,0 @@
  SL: -removed
  SL:
  SL: mods=
  SL: adds=added
  SL: dels=removed
  SL: files=added removed
  note: commit message saved in .sl/last-message.txt
  abort: precommit.test-saving-last-message hook exited with status 1
  [255]
  $ sl status -amr
  M changed
  A added
  R removed
  $ sl parents --template "M {file_mods}\nA {file_adds}\nR {file_dels}\n"
  M 
  A changed removed
  R 

  $ cat >> .sl/config <<EOF
  > # disable customizing for subsequent tests
  > [committemplate]
  > changeset =
  > EOF

  $ cd ..


commit copy

  $ sl init dir2
  $ cd dir2
  $ echo bleh > bar
  $ sl add bar
  $ sl ci -m 'add bar'

  $ sl cp bar foo
  $ echo >> bar
  $ sl ci -m 'cp bar foo; change bar'

  $ sl debugrename foo
  foo renamed from bar:26d3ca0dfd18e44d796b564e38dd173c9668d3a9

Test making empty commits
  $ sl commit --config ui.allowemptycommit=True -m "empty commit"
  $ sl log -r . -v --stat
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
  > from sapling import context, hg, node, ui as uimod
  > notrc = u".h\u200cg/hgrc"
  > u = uimod.ui.load()
  > r = hg.repository(u, '.')
  > def filectxfn(repo, memctx, path):
  >     return context.memfilectx(repo, memctx, path,
  >         b'[hooks]\nupdate = echo owned')
  > c = context.memctx(r, [r['tip']],
  >                    'evil', [notrc], filectxfn, 0)
  > r.commitctx(c)
  > EOF
  $ sl debugpython -- evil-commit.py
#if windows
  $ sl co --clean tip
  abort: path contains illegal component: .h\xe2\x80\x8cg\\hgrc (esc)
  [255]
#endif
#if osx
  $ sl co --clean tip
  abort: error writing files:
   .h‌g/hgrc: path contains illegal component '.h‌g': .h‌g/hgrc
  [255]
#endif

#if windows
  $ cd $TESTTMP/audit2
  $ cat > evil-commit.py <<EOF
  > from __future__ import absolute_import
  > from sapling import context, hg, node, ui as uimod
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
  $ sl debugpython -- evil-commit.py
  $ sl co --clean tip
  abort: path contains illegal component: HG~1/hgrc
  [255]

  $ cd $TESTTMP/audit3
  $ cat > evil-commit.py <<EOF
  > from __future__ import absolute_import
  > from sapling import context, hg, node, ui as uimod
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
  $ sl debugpython -- evil-commit.py
  $ sl co --clean tip
  abort: path contains illegal component: HG8B6C~2/hgrc
  [255]
#endif

# test that an unmodified commit template message aborts

  $ sl init unmodified_commit_template
  $ cd unmodified_commit_template
  $ echo foo > foo
  $ sl add foo
  $ sl commit -m "foo"
  $ cat >> .sl/config <<EOF
  > [committemplate]
  > changeset.commit = HI THIS IS NOT STRIPPED
  >     {slprefix}: this is customized commit template
  >     {slprefix}: Leave message empty to abort commit.
  >     {if(activebookmark,
  >    "{slprefix}: bookmark '{activebookmark}' is activated\n",
  >    "{slprefix}: no bookmark is activated\n")}
  > EOF
  $ cat > $TESTTMP/notouching.sh <<EOF
  > true
  > EOF
  $ echo foo2 > foo2
  $ sl add foo2
  $ HGEDITOR="sh $TESTTMP/notouching.sh" sl commit
  abort: commit message unchanged
  [255]

test that text below the --- >8 --- special string is ignored

  $ cat <<'EOF' > $TESTTMP/lowercaseline.sh
  > cat $1 | sed s/LINE/line/ | tee $1.new
  > mv $1.new $1
  > EOF

  $ sl init ignore_below_special_string
  $ cd ignore_below_special_string
  $ echo foo > foo
  $ sl add foo
  $ sl commit -m "foo"
  $ cat >> .sl/config <<EOF
  > [committemplate]
  > changeset.commit = first LINE
  >     {slprefix}: this is customized commit template
  >     {slprefix}: Leave message empty to abort commit.
  >     {slprefix}: ------------------------ >8 ------------------------
  >     {diff()}
  > EOF
  $ echo foo2 > foo2
  $ sl add foo2
  $ HGEDITOR="sh $TESTTMP/notouching.sh" sl ci
  abort: commit message unchanged
  [255]
  $ HGEDITOR="sh $TESTTMP/lowercaseline.sh" sl ci
  first line
  SL: this is customized commit template
  SL: Leave message empty to abort commit.
  SL: ------------------------ >8 ------------------------
  diff -r e63c23eaa88a foo2
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/foo2	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +foo2
  $ sl log -T '{desc}\n' -r .
  first line

test that the special string --- >8 --- isn't used when not at the beginning of
a line

  $ cat >> .sl/config <<EOF
  > [committemplate]
  > changeset.commit = first LINE2
  >     another line {slprefix}: ------------------------ >8 ------------------------
  >     {slprefix}: this is customized commit template
  >     {slprefix}: Leave message empty to abort commit.
  >     {slprefix}: ------------------------ >8 ------------------------
  >     {diff()}
  > EOF
  $ echo foo >> foo
  $ HGEDITOR="sh $TESTTMP/lowercaseline.sh" sl ci
  first line2
  another line SL: ------------------------ >8 ------------------------
  SL: this is customized commit template
  SL: Leave message empty to abort commit.
  SL: ------------------------ >8 ------------------------
  diff -r 3661b22b0702 foo
  --- a/foo	Thu Jan 01 00:00:00 1970 +0000
  +++ b/foo	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +1,2 @@
   foo
  +foo
  $ sl log -T '{desc}\n' -r .
  first line2
  another line SL: ------------------------ >8 ------------------------

also test that this special string isn't accepted when there is some extra text
at the end

  $ cat >> .sl/config <<EOF
  > [committemplate]
  > changeset.commit = first LINE3
  >     {slprefix}: ------------------------ >8 ------------------------foobar
  >     second line
  >     {slprefix}: this is customized commit template
  >     {slprefix}: Leave message empty to abort commit.
  >     {slprefix}: ------------------------ >8 ------------------------
  >     {diff()}
  > EOF
  $ echo foo >> foo
  $ HGEDITOR="sh $TESTTMP/lowercaseline.sh" sl ci
  first line3
  SL: ------------------------ >8 ------------------------foobar
  second line
  SL: this is customized commit template
  SL: Leave message empty to abort commit.
  SL: ------------------------ >8 ------------------------
  diff -r 4e01d9d4e7f8 foo
  --- a/foo	Thu Jan 01 00:00:00 1970 +0000
  +++ b/foo	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,2 +1,3 @@
   foo
   foo
  +foo
  $ sl log -T '{desc}\n' -r .
  first line3
  second line

  $ cd ..
