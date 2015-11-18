
  $ catpatch() {
  >     cat $1 | sed -e "s/^\(# Parent \).*/\1/"
  > }
  $ echo "[extensions]" >> $HGRCPATH
  $ echo "mq=" >> $HGRCPATH
  $ runtest() {
  >     hg init mq
  >     cd mq
  > 
  >     echo a > a
  >     hg ci -Ama
  > 
  >     echo '% qnew should refuse bad patch names'
  >     hg qnew series
  >     hg qnew status
  >     hg qnew guards
  >     hg qnew .
  >     hg qnew ..
  >     hg qnew .hgignore
  >     hg qnew .mqfoo
  >     hg qnew 'foo#bar'
  >     hg qnew 'foo:bar'
  >     hg qnew "`echo foo; echo bar`"
  > 
  >     hg qinit -c
  > 
  >     echo '% qnew with name containing slash'
  >     hg qnew foo/
  >     hg qnew foo/bar.patch
  >     hg qnew foo
  >     hg qseries
  >     hg qpop
  >     hg qdelete foo/bar.patch
  > 
  >     echo '% qnew with uncommitted changes'
  >     echo a > somefile
  >     hg add somefile
  >     hg qnew uncommitted.patch
  >     hg st
  >     hg qseries
  > 
  >     echo '% qnew implies add'
  >     hg -R .hg/patches st
  > 
  >     echo '% qnew missing'
  >     hg qnew missing.patch missing
  > 
  >     echo '% qnew -m'
  >     hg qnew -m 'foo bar' mtest.patch
  >     catpatch .hg/patches/mtest.patch
  > 
  >     echo '% qnew twice'
  >     hg qnew first.patch
  >     hg qnew first.patch
  > 
  >     touch ../first.patch
  >     hg qimport ../first.patch
  > 
  >     echo '% qnew -f from a subdirectory'
  >     hg qpop -a
  >     mkdir d
  >     cd d
  >     echo b > b
  >     hg ci -Am t
  >     echo b >> b
  >     hg st
  >     hg qnew -g -f p
  >     catpatch ../.hg/patches/p
  > 
  >     echo '% qnew -u with no username configured'
  >     HGUSER= hg qnew -u blue red
  >     catpatch ../.hg/patches/red
  > 
  >     echo '% qnew -e -u with no username configured'
  >     HGUSER= hg qnew -e -u chartreuse fucsia
  >     catpatch ../.hg/patches/fucsia
  > 
  >     echo '% fail when trying to import a merge'
  >     hg init merge
  >     cd merge
  >     touch a
  >     hg ci -Am null
  >     echo a >> a
  >     hg ci -m a
  >     hg up -r 0
  >     echo b >> a
  >     hg ci -m b
  >     hg merge -f 1
  >     hg resolve --mark a
  >     hg qnew -f merge
  > 
  >     cd ../../..
  >     rm -r mq
  > }

plain headers

  $ echo "[mq]" >> $HGRCPATH
  $ echo "plain=true" >> $HGRCPATH
  $ mkdir sandbox
  $ (cd sandbox ; runtest)
  adding a
  % qnew should refuse bad patch names
  abort: "series" cannot be used as the name of a patch
  abort: "status" cannot be used as the name of a patch
  abort: "guards" cannot be used as the name of a patch
  abort: "." cannot be used as the name of a patch
  abort: ".." cannot be used as the name of a patch
  abort: patch name cannot begin with ".hg"
  abort: patch name cannot begin with ".mq"
  abort: '#' cannot be used in the name of a patch
  abort: ':' cannot be used in the name of a patch
  abort: '\n' cannot be used in the name of a patch
  % qnew with name containing slash
  abort: path ends in directory separator: foo/ (glob)
  abort: "foo" already exists as a directory
  foo/bar.patch
  popping foo/bar.patch
  patch queue now empty
  % qnew with uncommitted changes
  uncommitted.patch
  % qnew implies add
  A .hgignore
  A series
  A uncommitted.patch
  % qnew missing
  abort: missing: * (glob)
  % qnew -m
  foo bar
  
  % qnew twice
  abort: patch "first.patch" already exists
  abort: patch "first.patch" already exists
  % qnew -f from a subdirectory
  popping first.patch
  popping mtest.patch
  popping uncommitted.patch
  patch queue now empty
  adding d/b
  M d/b
  diff --git a/d/b b/d/b
  --- a/d/b
  +++ b/d/b
  @@ -1,1 +1,2 @@
   b
  +b
  % qnew -u with no username configured
  From: blue
  
  % qnew -e -u with no username configured
  From: chartreuse
  
  % fail when trying to import a merge
  adding a
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  created new head
  merging a
  warning: conflicts while merging a! (edit, then use 'hg resolve --mark')
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  (no more unresolved files)
  abort: cannot manage merge changesets
  $ rm -r sandbox

hg headers

  $ echo "plain=false" >> $HGRCPATH
  $ mkdir sandbox
  $ (cd sandbox ; runtest)
  adding a
  % qnew should refuse bad patch names
  abort: "series" cannot be used as the name of a patch
  abort: "status" cannot be used as the name of a patch
  abort: "guards" cannot be used as the name of a patch
  abort: "." cannot be used as the name of a patch
  abort: ".." cannot be used as the name of a patch
  abort: patch name cannot begin with ".hg"
  abort: patch name cannot begin with ".mq"
  abort: '#' cannot be used in the name of a patch
  abort: ':' cannot be used in the name of a patch
  abort: '\n' cannot be used in the name of a patch
  % qnew with name containing slash
  abort: path ends in directory separator: foo/ (glob)
  abort: "foo" already exists as a directory
  foo/bar.patch
  popping foo/bar.patch
  patch queue now empty
  % qnew with uncommitted changes
  uncommitted.patch
  % qnew implies add
  A .hgignore
  A series
  A uncommitted.patch
  % qnew missing
  abort: missing: * (glob)
  % qnew -m
  # HG changeset patch
  # Parent 
  foo bar
  
  % qnew twice
  abort: patch "first.patch" already exists
  abort: patch "first.patch" already exists
  % qnew -f from a subdirectory
  popping first.patch
  popping mtest.patch
  popping uncommitted.patch
  patch queue now empty
  adding d/b
  M d/b
  # HG changeset patch
  # Parent 
  
  diff --git a/d/b b/d/b
  --- a/d/b
  +++ b/d/b
  @@ -1,1 +1,2 @@
   b
  +b
  % qnew -u with no username configured
  # HG changeset patch
  # User blue
  # Parent 
  
  % qnew -e -u with no username configured
  # HG changeset patch
  # User chartreuse
  # Parent 
  
  % fail when trying to import a merge
  adding a
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  created new head
  merging a
  warning: conflicts while merging a! (edit, then use 'hg resolve --mark')
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  (no more unresolved files)
  abort: cannot manage merge changesets
  $ rm -r sandbox

Test saving last-message.txt

  $ hg init repo
  $ cd repo

  $ cat > $TESTTMP/commitfailure.py <<EOF
  > from mercurial import error
  > def reposetup(ui, repo):
  >     class commitfailure(repo.__class__):
  >         def commit(self, *args, **kwargs):
  >             raise error.Abort('emulating unexpected abort')
  >     repo.__class__ = commitfailure
  > EOF
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > # this failure occurs before editor invocation
  > commitfailure = $TESTTMP/commitfailure.py
  > EOF

  $ cat > $TESTTMP/editor.sh << EOF
  > echo "==== before editing"
  > cat \$1
  > echo "===="
  > echo "test saving last-message.txt" >> \$1
  > EOF

(test that editor is not invoked before transaction starting)

  $ rm -f .hg/last-message.txt
  $ HGEDITOR="sh $TESTTMP/editor.sh" hg qnew -e patch
  abort: emulating unexpected abort
  [255]
  $ test -f .hg/last-message.txt
  [1]

(test that editor is invoked and commit message is saved into
"last-message.txt")

  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > commitfailure = !
  > [hooks]
  > # this failure occurs after editor invocation
  > pretxncommit.unexpectedabort = false
  > EOF

  $ rm -f .hg/last-message.txt
  $ hg status
  $ HGEDITOR="sh $TESTTMP/editor.sh" hg qnew -e patch
  ==== before editing
  
  
  HG: Enter commit message.  Lines beginning with 'HG:' are removed.
  HG: Leave message empty to use default message.
  HG: --
  HG: user: test
  HG: branch 'default'
  HG: no files changed
  ====
  note: commit message saved in .hg/last-message.txt
  transaction abort!
  rollback completed
  abort: pretxncommit.unexpectedabort hook exited with status 1
  [255]
  $ cat .hg/last-message.txt
  
  
  test saving last-message.txt

  $ cat >> .hg/hgrc <<EOF
  > [hooks]
  > pretxncommit.unexpectedabort =
  > EOF

#if unix-permissions

Test handling default message with the patch filename with tail whitespaces

  $ cat > $TESTTMP/editor.sh << EOF
  > echo "==== before editing"
  > cat \$1
  > echo "===="
  > echo "[mq]: patch        " > \$1
  > EOF

  $ rm -f .hg/last-message.txt
  $ hg status
  $ HGEDITOR="sh $TESTTMP/editor.sh" hg qnew -e "patch "
  ==== before editing
  
  
  HG: Enter commit message.  Lines beginning with 'HG:' are removed.
  HG: Leave message empty to use default message.
  HG: --
  HG: user: test
  HG: branch 'default'
  HG: no files changed
  ====
  $ cat ".hg/patches/patch "
  # HG changeset patch
  # Parent  0000000000000000000000000000000000000000
  

  $ cd ..

#endif
