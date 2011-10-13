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
  $ domerge() {
  >   beforemerge
  >   echo "# hg merge $*"
  >   hg merge $*
  >   aftermerge
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
  merging f failed!
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  [1]
  $ aftermerge
  # cat f
  <<<<<<< local
  revision 1
  =======
  revision 2
  >>>>>>> other
  space
  # hg stat
  M f
  ? f.orig

simplest hgrc using false for merge:

  $ echo "false.whatever=" >> .hg/hgrc
  $ domerge -r 2
  [merge-tools]
  false.whatever=
  # hg update -C 1
  # hg merge -r 2
  merging f
  merging f failed!
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  # cat f
  revision 1
  space
  # hg stat
  M f
  ? f.orig

true with higher .priority gets precedence:

  $ echo "true.priority=1" >> .hg/hgrc
  $ domerge -r 2
  [merge-tools]
  false.whatever=
  true.priority=1
  # hg update -C 1
  # hg merge -r 2
  merging f
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  # cat f
  revision 1
  space
  # hg stat
  M f

unless lowered on command line:

  $ domerge -r 2 --config merge-tools.true.priority=-7
  [merge-tools]
  false.whatever=
  true.priority=1
  # hg update -C 1
  # hg merge -r 2 --config merge-tools.true.priority=-7
  merging f
  merging f failed!
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  # cat f
  revision 1
  space
  # hg stat
  M f
  ? f.orig

or false set higher on command line:

  $ domerge -r 2 --config merge-tools.false.priority=117
  [merge-tools]
  false.whatever=
  true.priority=1
  # hg update -C 1
  # hg merge -r 2 --config merge-tools.false.priority=117
  merging f
  merging f failed!
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  # cat f
  revision 1
  space
  # hg stat
  M f
  ? f.orig

or true.executable not found in PATH:

  $ domerge -r 2 --config merge-tools.true.executable=nonexistingmergetool
  [merge-tools]
  false.whatever=
  true.priority=1
  # hg update -C 1
  # hg merge -r 2 --config merge-tools.true.executable=nonexistingmergetool
  merging f
  merging f failed!
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  # cat f
  revision 1
  space
  # hg stat
  M f
  ? f.orig

or true.executable with bogus path:

  $ domerge -r 2 --config merge-tools.true.executable=/nonexisting/mergetool
  [merge-tools]
  false.whatever=
  true.priority=1
  # hg update -C 1
  # hg merge -r 2 --config merge-tools.true.executable=/nonexisting/mergetool
  merging f
  merging f failed!
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  # cat f
  revision 1
  space
  # hg stat
  M f
  ? f.orig

but true.executable set to cat found in PATH works:

  $ echo "true.executable=cat" >> .hg/hgrc
  $ domerge -r 2
  [merge-tools]
  false.whatever=
  true.priority=1
  true.executable=cat
  # hg update -C 1
  # hg merge -r 2
  merging f
  revision 1
  space
  revision 0
  space
  revision 2
  space
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  # cat f
  revision 1
  space
  # hg stat
  M f

and true.executable set to cat with path works:

  $ domerge -r 2 --config merge-tools.true.executable=cat
  [merge-tools]
  false.whatever=
  true.priority=1
  true.executable=cat
  # hg update -C 1
  # hg merge -r 2 --config merge-tools.true.executable=cat
  merging f
  revision 1
  space
  revision 0
  space
  revision 2
  space
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  # cat f
  revision 1
  space
  # hg stat
  M f

environment variables in true.executable are handled:

  $ cat > $HGTMP/merge.sh <<EOF
  > #!/bin/sh
  > echo 'custom merge tool'
  > EOF
  $ chmod +x $HGTMP/merge.sh
  $ domerge -r 2 --config merge-tools.true.executable='$HGTMP/merge.sh'
  [merge-tools]
  false.whatever=
  true.priority=1
  true.executable=cat
  # hg update -C 1
  # hg merge -r 2 --config merge-tools.true.executable=$HGTMP/merge.sh
  merging f
  custom merge tool
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  # cat f
  revision 1
  space
  # hg stat
  M f

Tool selection and merge-patterns

merge-patterns specifies new tool false:

  $ domerge -r 2 --config merge-patterns.f=false
  [merge-tools]
  false.whatever=
  true.priority=1
  true.executable=cat
  # hg update -C 1
  # hg merge -r 2 --config merge-patterns.f=false
  merging f
  merging f failed!
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  # cat f
  revision 1
  space
  # hg stat
  M f
  ? f.orig

merge-patterns specifies executable not found in PATH and gets warning:

  $ domerge -r 2 --config merge-patterns.f=true --config merge-tools.true.executable=nonexistingmergetool
  [merge-tools]
  false.whatever=
  true.priority=1
  true.executable=cat
  # hg update -C 1
  # hg merge -r 2 --config merge-patterns.f=true --config merge-tools.true.executable=nonexistingmergetool
  couldn't find merge tool true specified for f
  merging f
  merging f failed!
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  # cat f
  revision 1
  space
  # hg stat
  M f
  ? f.orig

merge-patterns specifies executable with bogus path and gets warning:

  $ domerge -r 2 --config merge-patterns.f=true --config merge-tools.true.executable=/nonexisting/mergetool
  [merge-tools]
  false.whatever=
  true.priority=1
  true.executable=cat
  # hg update -C 1
  # hg merge -r 2 --config merge-patterns.f=true --config merge-tools.true.executable=/nonexisting/mergetool
  couldn't find merge tool true specified for f
  merging f
  merging f failed!
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  # cat f
  revision 1
  space
  # hg stat
  M f
  ? f.orig

ui.merge overrules priority

ui.merge specifies false:

  $ domerge -r 2 --config ui.merge=false
  [merge-tools]
  false.whatever=
  true.priority=1
  true.executable=cat
  # hg update -C 1
  # hg merge -r 2 --config ui.merge=false
  merging f
  merging f failed!
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  # cat f
  revision 1
  space
  # hg stat
  M f
  ? f.orig

ui.merge specifies internal:fail:

  $ domerge -r 2 --config ui.merge=internal:fail
  [merge-tools]
  false.whatever=
  true.priority=1
  true.executable=cat
  # hg update -C 1
  # hg merge -r 2 --config ui.merge=internal:fail
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  # cat f
  revision 1
  space
  # hg stat
  M f

ui.merge specifies internal:local:

  $ domerge -r 2 --config ui.merge=internal:local
  [merge-tools]
  false.whatever=
  true.priority=1
  true.executable=cat
  # hg update -C 1
  # hg merge -r 2 --config ui.merge=internal:local
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  # cat f
  revision 1
  space
  # hg stat
  M f

ui.merge specifies internal:other:

  $ domerge -r 2 --config ui.merge=internal:other
  [merge-tools]
  false.whatever=
  true.priority=1
  true.executable=cat
  # hg update -C 1
  # hg merge -r 2 --config ui.merge=internal:other
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  # cat f
  revision 2
  space
  # hg stat
  M f

ui.merge specifies internal:prompt:

  $ domerge -r 2 --config ui.merge=internal:prompt
  [merge-tools]
  false.whatever=
  true.priority=1
  true.executable=cat
  # hg update -C 1
  # hg merge -r 2 --config ui.merge=internal:prompt
   no tool found to merge f
  keep (l)ocal or take (o)ther? l
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  # cat f
  revision 1
  space
  # hg stat
  M f

ui.merge specifies internal:dump:

  $ domerge -r 2 --config ui.merge=internal:dump
  [merge-tools]
  false.whatever=
  true.priority=1
  true.executable=cat
  # hg update -C 1
  # hg merge -r 2 --config ui.merge=internal:dump
  merging f
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
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

  $ domerge -r 2 --config ui.merge=internal:other --config merge-patterns.f=false
  [merge-tools]
  false.whatever=
  true.priority=1
  true.executable=cat
  # hg update -C 1
  # hg merge -r 2 --config ui.merge=internal:other --config merge-patterns.f=false
  merging f
  merging f failed!
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  # cat f
  revision 1
  space
  # hg stat
  M f
  ? f.orig

Premerge

ui.merge specifies internal:other but is overruled by --tool=false

  $ domerge -r 2 --config ui.merge=internal:other --tool=false
  [merge-tools]
  false.whatever=
  true.priority=1
  true.executable=cat
  # hg update -C 1
  # hg merge -r 2 --config ui.merge=internal:other --tool=false
  merging f
  merging f failed!
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  # cat f
  revision 1
  space
  # hg stat
  M f
  ? f.orig

HGMERGE specifies internal:other but is overruled by --tool=false

  $ HGMERGE=internal:other ; export HGMERGE
  $ domerge -r 2 --tool=false
  [merge-tools]
  false.whatever=
  true.priority=1
  true.executable=cat
  # hg update -C 1
  # hg merge -r 2 --tool=false
  merging f
  merging f failed!
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  # cat f
  revision 1
  space
  # hg stat
  M f
  ? f.orig

  $ unset HGMERGE # make sure HGMERGE doesn't interfere with remaining tests

Default is silent simplemerge:

  $ domerge -r 3
  [merge-tools]
  false.whatever=
  true.priority=1
  true.executable=cat
  # hg update -C 1
  # hg merge -r 3
  merging f
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  # cat f
  revision 1
  space
  revision 3
  # hg stat
  M f

.premerge=True is same:

  $ domerge -r 3 --config merge-tools.true.premerge=True
  [merge-tools]
  false.whatever=
  true.priority=1
  true.executable=cat
  # hg update -C 1
  # hg merge -r 3 --config merge-tools.true.premerge=True
  merging f
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  # cat f
  revision 1
  space
  revision 3
  # hg stat
  M f

.premerge=False executes merge-tool:

  $ domerge -r 3 --config merge-tools.true.premerge=False
  [merge-tools]
  false.whatever=
  true.priority=1
  true.executable=cat
  # hg update -C 1
  # hg merge -r 3 --config merge-tools.true.premerge=False
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
  # cat f
  revision 1
  space
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
  > #!/bin/sh
  > cat "\$1" "\$2" "\$3" > "\$4"
  > EOF
  $ chmod +x 'my merge tool'
  $ hg merge -r 2 --config merge-tools.true.executable='./my merge tool' --config merge-tools.true.args='$base $local $other $output'
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

Merge post-processing

cat is a bad merge-tool and doesn't change:

  $ domerge -y -r 2 --config merge-tools.true.checkchanged=1
  [merge-tools]
  false.whatever=
  true.priority=1
  true.executable=cat
  # hg update -C 1
  # hg merge -y -r 2 --config merge-tools.true.checkchanged=1
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
  # cat f
  revision 1
  space
  # hg stat
  M f
  ? f.orig
