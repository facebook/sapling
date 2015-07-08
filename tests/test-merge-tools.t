test merge-tools configuration - mostly exercising filemerge.py

  $ unset HGMERGE # make sure HGMERGE doesn't interfere with the test
  $ hg init

revision 0

  $ echo "revision 0" > f
  $ echo "space" >> f
  $ hg commit -Am "revision 0"
  adding f

revision 1

  $ echo "revision 1" > f
  $ echo "space" >> f
  $ hg commit -Am "revision 1"
  $ hg update 0 > /dev/null

revision 2

  $ echo "revision 2" > f
  $ echo "space" >> f
  $ hg commit -Am "revision 2"
  created new head
  $ hg update 0 > /dev/null

revision 3 - simple to merge

  $ echo "revision 3" >> f
  $ hg commit -Am "revision 3"
  created new head

revision 4 - hard to merge

  $ hg update 0 > /dev/null
  $ echo "revision 4" > f
  $ hg commit -Am "revision 4"
  created new head

  $ echo "[merge-tools]" > .hg/hgrc

  $ beforemerge() {
  >   cat .hg/hgrc
  >   echo "# hg update -C 1"
  >   hg update -C 1 > /dev/null
  > }
  $ aftermerge() {
  >   echo "# cat f"
  >   cat f
  >   echo "# hg stat"
  >   hg stat
  >   rm -f f.orig
  > }

Tool selection

default is internal merge:

  $ beforemerge
  [merge-tools]
  # hg update -C 1

hg merge -r 2
override $PATH to ensure hgmerge not visible; use $PYTHON in case we're
running from a devel copy, not a temp installation

  $ PATH="$BINDIR" $PYTHON "$BINDIR"/hg merge -r 2
  merging f
  warning: conflicts during merge.
  merging f incomplete! (edit conflicts, then use 'hg resolve --mark')
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  [1]
  $ aftermerge
  # cat f
  <<<<<<< local: ef83787e2614  - test: revision 1
  revision 1
  =======
  revision 2
  >>>>>>> other: 0185f4e0cf02  - test: revision 2
  space
  # hg stat
  M f
  ? f.orig

simplest hgrc using false for merge:

  $ echo "false.whatever=" >> .hg/hgrc
  $ beforemerge
  [merge-tools]
  false.whatever=
  # hg update -C 1
  $ hg merge -r 2
  merging f
  merging f failed!
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  [1]
  $ aftermerge
  # cat f
  revision 1
  space
  # hg stat
  M f
  ? f.orig

#if unix-permissions

unexecutable file in $PATH shouldn't be found:

  $ echo "echo fail" > false
  $ hg up -qC 1
  $ PATH="`pwd`:$BINDIR" $PYTHON "$BINDIR"/hg merge -r 2
  merging f
  warning: conflicts during merge.
  merging f incomplete! (edit conflicts, then use 'hg resolve --mark')
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  [1]
  $ rm false

#endif

executable directory in $PATH shouldn't be found:

  $ mkdir false
  $ hg up -qC 1
  $ PATH="`pwd`:$BINDIR" $PYTHON "$BINDIR"/hg merge -r 2
  merging f
  warning: conflicts during merge.
  merging f incomplete! (edit conflicts, then use 'hg resolve --mark')
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  [1]
  $ rmdir false

true with higher .priority gets precedence:

  $ echo "true.priority=1" >> .hg/hgrc
  $ beforemerge
  [merge-tools]
  false.whatever=
  true.priority=1
  # hg update -C 1
  $ hg merge -r 2
  merging f
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ aftermerge
  # cat f
  revision 1
  space
  # hg stat
  M f

unless lowered on command line:

  $ beforemerge
  [merge-tools]
  false.whatever=
  true.priority=1
  # hg update -C 1
  $ hg merge -r 2 --config merge-tools.true.priority=-7
  merging f
  merging f failed!
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  [1]
  $ aftermerge
  # cat f
  revision 1
  space
  # hg stat
  M f
  ? f.orig

or false set higher on command line:

  $ beforemerge
  [merge-tools]
  false.whatever=
  true.priority=1
  # hg update -C 1
  $ hg merge -r 2 --config merge-tools.false.priority=117
  merging f
  merging f failed!
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  [1]
  $ aftermerge
  # cat f
  revision 1
  space
  # hg stat
  M f
  ? f.orig

or true.executable not found in PATH:

  $ beforemerge
  [merge-tools]
  false.whatever=
  true.priority=1
  # hg update -C 1
  $ hg merge -r 2 --config merge-tools.true.executable=nonexistentmergetool
  merging f
  merging f failed!
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  [1]
  $ aftermerge
  # cat f
  revision 1
  space
  # hg stat
  M f
  ? f.orig

or true.executable with bogus path:

  $ beforemerge
  [merge-tools]
  false.whatever=
  true.priority=1
  # hg update -C 1
  $ hg merge -r 2 --config merge-tools.true.executable=/nonexistent/mergetool
  merging f
  merging f failed!
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  [1]
  $ aftermerge
  # cat f
  revision 1
  space
  # hg stat
  M f
  ? f.orig

but true.executable set to cat found in PATH works:

  $ echo "true.executable=cat" >> .hg/hgrc
  $ beforemerge
  [merge-tools]
  false.whatever=
  true.priority=1
  true.executable=cat
  # hg update -C 1
  $ hg merge -r 2
  merging f
  revision 1
  space
  revision 0
  space
  revision 2
  space
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ aftermerge
  # cat f
  revision 1
  space
  # hg stat
  M f

and true.executable set to cat with path works:

  $ beforemerge
  [merge-tools]
  false.whatever=
  true.priority=1
  true.executable=cat
  # hg update -C 1
  $ hg merge -r 2 --config merge-tools.true.executable=cat
  merging f
  revision 1
  space
  revision 0
  space
  revision 2
  space
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ aftermerge
  # cat f
  revision 1
  space
  # hg stat
  M f

#if unix-permissions

environment variables in true.executable are handled:

  $ echo 'echo "custom merge tool"' > .hg/merge.sh
  $ beforemerge
  [merge-tools]
  false.whatever=
  true.priority=1
  true.executable=cat
  # hg update -C 1
  $ hg --config merge-tools.true.executable='sh' \
  >    --config merge-tools.true.args=.hg/merge.sh \
  >    merge -r 2
  merging f
  custom merge tool
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ aftermerge
  # cat f
  revision 1
  space
  # hg stat
  M f

#endif

Tool selection and merge-patterns

merge-patterns specifies new tool false:

  $ beforemerge
  [merge-tools]
  false.whatever=
  true.priority=1
  true.executable=cat
  # hg update -C 1
  $ hg merge -r 2 --config merge-patterns.f=false
  merging f
  merging f failed!
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  [1]
  $ aftermerge
  # cat f
  revision 1
  space
  # hg stat
  M f
  ? f.orig

merge-patterns specifies executable not found in PATH and gets warning:

  $ beforemerge
  [merge-tools]
  false.whatever=
  true.priority=1
  true.executable=cat
  # hg update -C 1
  $ hg merge -r 2 --config merge-patterns.f=true --config merge-tools.true.executable=nonexistentmergetool
  couldn't find merge tool true specified for f
  merging f
  merging f failed!
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  [1]
  $ aftermerge
  # cat f
  revision 1
  space
  # hg stat
  M f
  ? f.orig

merge-patterns specifies executable with bogus path and gets warning:

  $ beforemerge
  [merge-tools]
  false.whatever=
  true.priority=1
  true.executable=cat
  # hg update -C 1
  $ hg merge -r 2 --config merge-patterns.f=true --config merge-tools.true.executable=/nonexistent/mergetool
  couldn't find merge tool true specified for f
  merging f
  merging f failed!
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  [1]
  $ aftermerge
  # cat f
  revision 1
  space
  # hg stat
  M f
  ? f.orig

ui.merge overrules priority

ui.merge specifies false:

  $ beforemerge
  [merge-tools]
  false.whatever=
  true.priority=1
  true.executable=cat
  # hg update -C 1
  $ hg merge -r 2 --config ui.merge=false
  merging f
  merging f failed!
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  [1]
  $ aftermerge
  # cat f
  revision 1
  space
  # hg stat
  M f
  ? f.orig

ui.merge specifies internal:fail:

  $ beforemerge
  [merge-tools]
  false.whatever=
  true.priority=1
  true.executable=cat
  # hg update -C 1
  $ hg merge -r 2 --config ui.merge=internal:fail
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  [1]
  $ aftermerge
  # cat f
  revision 1
  space
  # hg stat
  M f

ui.merge specifies :local (without internal prefix):

  $ beforemerge
  [merge-tools]
  false.whatever=
  true.priority=1
  true.executable=cat
  # hg update -C 1
  $ hg merge -r 2 --config ui.merge=:local
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ aftermerge
  # cat f
  revision 1
  space
  # hg stat
  M f

ui.merge specifies internal:other:

  $ beforemerge
  [merge-tools]
  false.whatever=
  true.priority=1
  true.executable=cat
  # hg update -C 1
  $ hg merge -r 2 --config ui.merge=internal:other
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ aftermerge
  # cat f
  revision 2
  space
  # hg stat
  M f

ui.merge specifies internal:prompt:

  $ beforemerge
  [merge-tools]
  false.whatever=
  true.priority=1
  true.executable=cat
  # hg update -C 1
  $ hg merge -r 2 --config ui.merge=internal:prompt
   no tool found to merge f
  keep (l)ocal or take (o)ther? l
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ aftermerge
  # cat f
  revision 1
  space
  # hg stat
  M f

ui.merge specifies internal:dump:

  $ beforemerge
  [merge-tools]
  false.whatever=
  true.priority=1
  true.executable=cat
  # hg update -C 1
  $ hg merge -r 2 --config ui.merge=internal:dump
  merging f
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  [1]
  $ aftermerge
  # cat f
  revision 1
  space
  # hg stat
  M f
  ? f.base
  ? f.local
  ? f.orig
  ? f.other

f.base:

  $ cat f.base
  revision 0
  space

f.local:

  $ cat f.local
  revision 1
  space

f.other:

  $ cat f.other
  revision 2
  space
  $ rm f.base f.local f.other

ui.merge specifies internal:other but is overruled by pattern for false:

  $ beforemerge
  [merge-tools]
  false.whatever=
  true.priority=1
  true.executable=cat
  # hg update -C 1
  $ hg merge -r 2 --config ui.merge=internal:other --config merge-patterns.f=false
  merging f
  merging f failed!
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  [1]
  $ aftermerge
  # cat f
  revision 1
  space
  # hg stat
  M f
  ? f.orig

Premerge

ui.merge specifies internal:other but is overruled by --tool=false

  $ beforemerge
  [merge-tools]
  false.whatever=
  true.priority=1
  true.executable=cat
  # hg update -C 1
  $ hg merge -r 2 --config ui.merge=internal:other --tool=false
  merging f
  merging f failed!
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  [1]
  $ aftermerge
  # cat f
  revision 1
  space
  # hg stat
  M f
  ? f.orig

HGMERGE specifies internal:other but is overruled by --tool=false

  $ HGMERGE=internal:other ; export HGMERGE
  $ beforemerge
  [merge-tools]
  false.whatever=
  true.priority=1
  true.executable=cat
  # hg update -C 1
  $ hg merge -r 2 --tool=false
  merging f
  merging f failed!
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  [1]
  $ aftermerge
  # cat f
  revision 1
  space
  # hg stat
  M f
  ? f.orig

  $ unset HGMERGE # make sure HGMERGE doesn't interfere with remaining tests

update is a merge ...

(this also tests that files reverted with '--rev REV' are treated as
"modified", even if none of mode, size and timestamp of them isn't
changed on the filesystem (see also issue4583))

  $ cat >> $HGRCPATH <<EOF
  > [fakedirstatewritetime]
  > # emulate invoking dirstate.write() via repo.status()
  > # at 2000-01-01 00:00
  > fakenow = 200001010000
  > EOF

  $ beforemerge
  [merge-tools]
  false.whatever=
  true.priority=1
  true.executable=cat
  # hg update -C 1
  $ hg update -q 0
  $ f -s f
  f: size=17
  $ touch -t 200001010000 f
  $ hg debugrebuildstate
  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > fakedirstatewritetime = $TESTDIR/fakedirstatewritetime.py
  > EOF
  $ hg revert -q -r 1 .
  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > fakedirstatewritetime = !
  > EOF
  $ f -s f
  f: size=17
  $ touch -t 200001010000 f
  $ hg status f
  M f
  $ hg update -r 2
  merging f
  revision 1
  space
  revision 0
  space
  revision 2
  space
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  $ aftermerge
  # cat f
  revision 1
  space
  # hg stat
  M f

update should also have --tool

  $ beforemerge
  [merge-tools]
  false.whatever=
  true.priority=1
  true.executable=cat
  # hg update -C 1
  $ hg update -q 0
  $ f -s f
  f: size=17
  $ touch -t 200001010000 f
  $ hg debugrebuildstate
  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > fakedirstatewritetime = $TESTDIR/fakedirstatewritetime.py
  > EOF
  $ hg revert -q -r 1 .
  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > fakedirstatewritetime = !
  > EOF
  $ f -s f
  f: size=17
  $ touch -t 200001010000 f
  $ hg status f
  M f
  $ hg update -r 2 --tool false
  merging f
  merging f failed!
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges
  [1]
  $ aftermerge
  # cat f
  revision 1
  space
  # hg stat
  M f
  ? f.orig

Default is silent simplemerge:

  $ beforemerge
  [merge-tools]
  false.whatever=
  true.priority=1
  true.executable=cat
  # hg update -C 1
  $ hg merge -r 3
  merging f
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ aftermerge
  # cat f
  revision 1
  space
  revision 3
  # hg stat
  M f

.premerge=True is same:

  $ beforemerge
  [merge-tools]
  false.whatever=
  true.priority=1
  true.executable=cat
  # hg update -C 1
  $ hg merge -r 3 --config merge-tools.true.premerge=True
  merging f
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ aftermerge
  # cat f
  revision 1
  space
  revision 3
  # hg stat
  M f

.premerge=False executes merge-tool:

  $ beforemerge
  [merge-tools]
  false.whatever=
  true.priority=1
  true.executable=cat
  # hg update -C 1
  $ hg merge -r 3 --config merge-tools.true.premerge=False
  merging f
  revision 1
  space
  revision 0
  space
  revision 0
  space
  revision 3
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ aftermerge
  # cat f
  revision 1
  space
  # hg stat
  M f

premerge=keep keeps conflict markers in:

  $ beforemerge
  [merge-tools]
  false.whatever=
  true.priority=1
  true.executable=cat
  # hg update -C 1
  $ hg merge -r 4 --config merge-tools.true.premerge=keep
  merging f
  <<<<<<< local: ef83787e2614  - test: revision 1
  revision 1
  space
  =======
  revision 4
  >>>>>>> other: 81448d39c9a0 - test: revision 4
  revision 0
  space
  revision 4
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ aftermerge
  # cat f
  <<<<<<< local: ef83787e2614  - test: revision 1
  revision 1
  space
  =======
  revision 4
  >>>>>>> other: 81448d39c9a0 - test: revision 4
  # hg stat
  M f

premerge=keep-merge3 keeps conflict markers with base content:

  $ beforemerge
  [merge-tools]
  false.whatever=
  true.priority=1
  true.executable=cat
  # hg update -C 1
  $ hg merge -r 4 --config merge-tools.true.premerge=keep-merge3
  merging f
  <<<<<<< local: ef83787e2614  - test: revision 1
  revision 1
  space
  ||||||| base
  revision 0
  space
  =======
  revision 4
  >>>>>>> other: 81448d39c9a0 - test: revision 4
  revision 0
  space
  revision 4
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ aftermerge
  # cat f
  <<<<<<< local: ef83787e2614  - test: revision 1
  revision 1
  space
  ||||||| base
  revision 0
  space
  =======
  revision 4
  >>>>>>> other: 81448d39c9a0 - test: revision 4
  # hg stat
  M f


Tool execution

set tools.args explicit to include $base $local $other $output:

  $ beforemerge
  [merge-tools]
  false.whatever=
  true.priority=1
  true.executable=cat
  # hg update -C 1
  $ hg merge -r 2 --config merge-tools.true.executable=head --config merge-tools.true.args='$base $local $other $output' \
  >   | sed 's,==> .* <==,==> ... <==,g'
  merging f
  ==> ... <==
  revision 0
  space
  
  ==> ... <==
  revision 1
  space
  
  ==> ... <==
  revision 2
  space
  
  ==> ... <==
  revision 1
  space
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ aftermerge
  # cat f
  revision 1
  space
  # hg stat
  M f

Merge with "echo mergeresult > $local":

  $ beforemerge
  [merge-tools]
  false.whatever=
  true.priority=1
  true.executable=cat
  # hg update -C 1
  $ hg merge -r 2 --config merge-tools.true.executable=echo --config merge-tools.true.args='mergeresult > $local'
  merging f
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ aftermerge
  # cat f
  mergeresult
  # hg stat
  M f

- and $local is the file f:

  $ beforemerge
  [merge-tools]
  false.whatever=
  true.priority=1
  true.executable=cat
  # hg update -C 1
  $ hg merge -r 2 --config merge-tools.true.executable=echo --config merge-tools.true.args='mergeresult > f'
  merging f
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ aftermerge
  # cat f
  mergeresult
  # hg stat
  M f

Merge with "echo mergeresult > $output" - the variable is a bit magic:

  $ beforemerge
  [merge-tools]
  false.whatever=
  true.priority=1
  true.executable=cat
  # hg update -C 1
  $ hg merge -r 2 --config merge-tools.true.executable=echo --config merge-tools.true.args='mergeresult > $output'
  merging f
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ aftermerge
  # cat f
  mergeresult
  # hg stat
  M f

Merge using tool with a path that must be quoted:

  $ beforemerge
  [merge-tools]
  false.whatever=
  true.priority=1
  true.executable=cat
  # hg update -C 1
  $ cat <<EOF > 'my merge tool'
  > cat "\$1" "\$2" "\$3" > "\$4"
  > EOF
  $ hg --config merge-tools.true.executable='sh' \
  >    --config merge-tools.true.args='"./my merge tool" $base $local $other $output' \
  >    merge -r 2
  merging f
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ rm -f 'my merge tool'
  $ aftermerge
  # cat f
  revision 0
  space
  revision 1
  space
  revision 2
  space
  # hg stat
  M f

Issue3581: Merging a filename that needs to be quoted
(This test doesn't work on Windows filesystems even on Linux, so check
for Unix-like permission)

#if unix-permissions
  $ beforemerge
  [merge-tools]
  false.whatever=
  true.priority=1
  true.executable=cat
  # hg update -C 1
  $ echo "revision 5" > '"; exit 1; echo "'
  $ hg commit -Am "revision 5"
  adding "; exit 1; echo "
  warning: filename contains '"', which is reserved on Windows: '"; exit 1; echo "'
  $ hg update -C 1 > /dev/null
  $ echo "revision 6" > '"; exit 1; echo "'
  $ hg commit -Am "revision 6"
  adding "; exit 1; echo "
  warning: filename contains '"', which is reserved on Windows: '"; exit 1; echo "'
  created new head
  $ hg merge --config merge-tools.true.executable="true" -r 5
  merging "; exit 1; echo "
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg update -C 1 > /dev/null
#endif

Merge post-processing

cat is a bad merge-tool and doesn't change:

  $ beforemerge
  [merge-tools]
  false.whatever=
  true.priority=1
  true.executable=cat
  # hg update -C 1
  $ hg merge -y -r 2 --config merge-tools.true.checkchanged=1
  merging f
  revision 1
  space
  revision 0
  space
  revision 2
  space
   output file f appears unchanged
  was merge successful (yn)? n
  merging f failed!
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  [1]
  $ aftermerge
  # cat f
  revision 1
  space
  # hg stat
  M f
  ? f.orig

#if symlink

internal merge cannot handle symlinks and shouldn't try:

  $ hg update -q -C 1
  $ rm f
  $ ln -s symlink f
  $ hg commit -qm 'f is symlink'
  $ hg merge -r 2 --tool internal:merge
  merging f
  warning: internal :merge cannot merge symlinks for f
  merging f incomplete! (edit conflicts, then use 'hg resolve --mark')
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  [1]

#endif
