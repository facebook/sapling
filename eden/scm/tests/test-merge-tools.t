#chg-compatible

  $ setconfig status.use-rust=False workingcopy.use-rust=False
  $ setconfig devel.segmented-changelog-rev-compat=true
  $ setconfig workingcopy.ruststatus=False
  $ disable treemanifest

test merge-tools configuration - mostly exercising filemerge.py

  $ unset HGMERGE # make sure HGMERGE doesn't interfere with the test
  $ hg init repo
  $ cd repo

revision 0

  $ echo "revision 0" > f
  $ echo "space" >> f
  $ hg commit -Am "revision 0"
  adding f

revision 1

  $ echo "revision 1" > f
  $ echo "space" >> f
  $ hg commit -Am "revision 1"
  $ hg goto ffd2bda21d6ef8cb02e27e3d7f96f8ac8d196821 > /dev/null

revision 2

  $ echo "revision 2" > f
  $ echo "space" >> f
  $ hg commit -Am "revision 2"
  $ hg goto ffd2bda21d6ef8cb02e27e3d7f96f8ac8d196821 > /dev/null

revision 3 - simple to merge

  $ echo "revision 3" >> f
  $ hg commit -Am "revision 3"

revision 4 - hard to merge

  $ hg goto ffd2bda21d6ef8cb02e27e3d7f96f8ac8d196821 > /dev/null
  $ echo "revision 4" > f
  $ hg commit -Am "revision 4"

  $ echo "[merge-tools]" > .hg/hgrc

  $ beforemerge() {
  >   cat .hg/hgrc
  >   echo "# hg goto -C 1"
  >   hg goto -C 1 > /dev/null
  > }
  $ aftermerge() {
  >   echo "# cat f"
  >   cat f
  >   echo "# hg stat"
  >   hg stat
  >   echo "# hg resolve --list"
  >   hg resolve --list
  >   rm -f f.orig
  > }

Tool selection

default is internal merge:

  $ beforemerge
  [merge-tools]
  # hg goto -C 1

hg merge -r 2
override $PATH to ensure hgmerge not visible

  $ PATH="$TMPBINDIR:$BINDIR:/usr/sbin:/usr/bin" hg merge -r 0185f4e0cf024bb0ed9694d1cbdea347ecce96d9
  merging f
  warning: 1 conflicts while merging f! (edit, then use 'hg resolve --mark')
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg goto -C .' to abandon
  [1]
  $ aftermerge
  # cat f
  <<<<<<< working copy: ef83787e2614 - test: revision 1
  revision 1
  =======
  revision 2
  >>>>>>> merge rev:    0185f4e0cf02 - test: revision 2
  space
  # hg stat
  M f
  ? f.orig
  # hg resolve --list
  U f

simplest hgrc using false for merge:

  $ echo "false.whatever=" >> .hg/hgrc
  $ beforemerge
  [merge-tools]
  false.whatever=
  # hg goto -C 1
  $ hg merge -r 0185f4e0cf024bb0ed9694d1cbdea347ecce96d9
  merging f
  merging f failed!
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg goto -C .' to abandon
  [1]
  $ aftermerge
  # cat f
  revision 1
  space
  # hg stat
  M f
  ? f.orig
  # hg resolve --list
  U f

#if unix-permissions

unexecutable file in $PATH shouldn't be found:
- Replace false with false2 so we dont fight with the system false
  $ cat > .hg/hgrc <<EOF
  > [merge-tools]
  > false2.whatever=
  > EOF
  $ echo "echo fail" > false2
  $ hg up -qC ef83787e2614c6fef8e58c9739cd5b46240ad4f0
  $ PATH="`pwd`:$TMPBINDIR:$BINDIR:/usr/sbin:/usr/bin" hg merge -r 0185f4e0cf024bb0ed9694d1cbdea347ecce96d9
  merging f
  warning: 1 conflicts while merging f! (edit, then use 'hg resolve --mark')
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg goto -C .' to abandon
  [1]
  $ rm false2

#endif

executable directory in $PATH shouldn't be found:

  $ mkdir false2
  $ hg up -qC ef83787e2614c6fef8e58c9739cd5b46240ad4f0
  $ PATH="`pwd`:$TMPBINDIR:$BINDIR:/usr/sbin:/usr/bin" hg merge -r 0185f4e0cf024bb0ed9694d1cbdea347ecce96d9
  merging f
  warning: 1 conflicts while merging f! (edit, then use 'hg resolve --mark')
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg goto -C .' to abandon
  [1]
  $ rmdir false2

  $ cat > .hg/hgrc <<EOF
  > [merge-tools]
  > false.whatever=
  > EOF

true with higher .priority gets precedence:

  $ echo "true.priority=1" >> .hg/hgrc
  $ beforemerge
  [merge-tools]
  false.whatever=
  true.priority=1
  # hg goto -C 1
  $ hg merge -r 0185f4e0cf024bb0ed9694d1cbdea347ecce96d9
  merging f
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ aftermerge
  # cat f
  revision 1
  space
  # hg stat
  M f
  # hg resolve --list
  R f

unless lowered on command line:

  $ beforemerge
  [merge-tools]
  false.whatever=
  true.priority=1
  # hg goto -C 1
  $ hg merge -r 0185f4e0cf024bb0ed9694d1cbdea347ecce96d9 --config merge-tools.true.priority=-7
  merging f
  merging f failed!
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg goto -C .' to abandon
  [1]
  $ aftermerge
  # cat f
  revision 1
  space
  # hg stat
  M f
  ? f.orig
  # hg resolve --list
  U f

or false set higher on command line:

  $ beforemerge
  [merge-tools]
  false.whatever=
  true.priority=1
  # hg goto -C 1
  $ hg merge -r 0185f4e0cf024bb0ed9694d1cbdea347ecce96d9 --config merge-tools.false.priority=117
  merging f
  merging f failed!
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg goto -C .' to abandon
  [1]
  $ aftermerge
  # cat f
  revision 1
  space
  # hg stat
  M f
  ? f.orig
  # hg resolve --list
  U f

or true set to disabled:
  $ beforemerge
  [merge-tools]
  false.whatever=
  true.priority=1
  # hg goto -C 1
  $ hg merge -r 0185f4e0cf024bb0ed9694d1cbdea347ecce96d9 --config merge-tools.true.disabled=yes
  merging f
  merging f failed!
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg goto -C .' to abandon
  [1]
  $ aftermerge
  # cat f
  revision 1
  space
  # hg stat
  M f
  ? f.orig
  # hg resolve --list
  U f

or true.executable not found in PATH:

  $ beforemerge
  [merge-tools]
  false.whatever=
  true.priority=1
  # hg goto -C 1
  $ hg merge -r 0185f4e0cf024bb0ed9694d1cbdea347ecce96d9 --config merge-tools.true.executable=nonexistentmergetool
  merging f
  merging f failed!
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg goto -C .' to abandon
  [1]
  $ aftermerge
  # cat f
  revision 1
  space
  # hg stat
  M f
  ? f.orig
  # hg resolve --list
  U f

or true.executable with bogus path:

  $ beforemerge
  [merge-tools]
  false.whatever=
  true.priority=1
  # hg goto -C 1
  $ hg merge -r 0185f4e0cf024bb0ed9694d1cbdea347ecce96d9 --config merge-tools.true.executable=/nonexistent/mergetool
  merging f
  merging f failed!
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg goto -C .' to abandon
  [1]
  $ aftermerge
  # cat f
  revision 1
  space
  # hg stat
  M f
  ? f.orig
  # hg resolve --list
  U f

but true.executable set to cat found in PATH works:

  $ echo "true.executable=cat" >> .hg/hgrc
  $ beforemerge
  [merge-tools]
  false.whatever=
  true.priority=1
  true.executable=cat
  # hg goto -C 1
  $ hg merge -r 0185f4e0cf024bb0ed9694d1cbdea347ecce96d9
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
  # hg resolve --list
  R f

and true.executable set to cat with path works:

  $ beforemerge
  [merge-tools]
  false.whatever=
  true.priority=1
  true.executable=cat
  # hg goto -C 1
  $ hg merge -r 0185f4e0cf024bb0ed9694d1cbdea347ecce96d9 --config merge-tools.true.executable=cat
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
  # hg resolve --list
  R f

#if unix-permissions

environment variables in true.executable are handled:

  $ echo 'echo "custom merge tool"' > .hg/merge.sh
  $ beforemerge
  [merge-tools]
  false.whatever=
  true.priority=1
  true.executable=cat
  # hg goto -C 1
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
  # hg resolve --list
  R f

#endif

Tool selection and merge-patterns

merge-patterns specifies new tool false:

  $ beforemerge
  [merge-tools]
  false.whatever=
  true.priority=1
  true.executable=cat
  # hg goto -C 1
  $ hg merge -r 0185f4e0cf024bb0ed9694d1cbdea347ecce96d9 --config merge-patterns.f=false
  merging f
  merging f failed!
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg goto -C .' to abandon
  [1]
  $ aftermerge
  # cat f
  revision 1
  space
  # hg stat
  M f
  ? f.orig
  # hg resolve --list
  U f

merge-patterns specifies executable not found in PATH and gets warning:

  $ beforemerge
  [merge-tools]
  false.whatever=
  true.priority=1
  true.executable=cat
  # hg goto -C 1
  $ hg merge -r 0185f4e0cf024bb0ed9694d1cbdea347ecce96d9 --config merge-patterns.f=true --config merge-tools.true.executable=nonexistentmergetool
  couldn't find merge tool true (for pattern f)
  merging f
  couldn't find merge tool true (for pattern f)
  merging f failed!
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg goto -C .' to abandon
  [1]
  $ aftermerge
  # cat f
  revision 1
  space
  # hg stat
  M f
  ? f.orig
  # hg resolve --list
  U f

merge-patterns specifies executable with bogus path and gets warning:

  $ beforemerge
  [merge-tools]
  false.whatever=
  true.priority=1
  true.executable=cat
  # hg goto -C 1
  $ hg merge -r 0185f4e0cf024bb0ed9694d1cbdea347ecce96d9 --config merge-patterns.f=true --config merge-tools.true.executable=/nonexistent/mergetool
  couldn't find merge tool true (for pattern f)
  merging f
  couldn't find merge tool true (for pattern f)
  merging f failed!
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg goto -C .' to abandon
  [1]
  $ aftermerge
  # cat f
  revision 1
  space
  # hg stat
  M f
  ? f.orig
  # hg resolve --list
  U f

ui.merge overrules priority

ui.merge specifies false:

  $ beforemerge
  [merge-tools]
  false.whatever=
  true.priority=1
  true.executable=cat
  # hg goto -C 1
  $ hg merge -r 0185f4e0cf024bb0ed9694d1cbdea347ecce96d9 --config ui.merge=false
  merging f
  merging f failed!
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg goto -C .' to abandon
  [1]
  $ aftermerge
  # cat f
  revision 1
  space
  # hg stat
  M f
  ? f.orig
  # hg resolve --list
  U f

ui.merge specifies internal:fail:

  $ beforemerge
  [merge-tools]
  false.whatever=
  true.priority=1
  true.executable=cat
  # hg goto -C 1
  $ hg merge -r 0185f4e0cf024bb0ed9694d1cbdea347ecce96d9 --config ui.merge=internal:fail
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg goto -C .' to abandon
  [1]
  $ aftermerge
  # cat f
  revision 1
  space
  # hg stat
  M f
  # hg resolve --list
  U f

ui.merge specifies :local (without internal prefix):

  $ beforemerge
  [merge-tools]
  false.whatever=
  true.priority=1
  true.executable=cat
  # hg goto -C 1
  $ hg merge -r 0185f4e0cf024bb0ed9694d1cbdea347ecce96d9 --config ui.merge=:local
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ aftermerge
  # cat f
  revision 1
  space
  # hg stat
  M f
  # hg resolve --list
  R f

ui.merge specifies internal:other:

  $ beforemerge
  [merge-tools]
  false.whatever=
  true.priority=1
  true.executable=cat
  # hg goto -C 1
  $ hg merge -r 0185f4e0cf024bb0ed9694d1cbdea347ecce96d9 --config ui.merge=internal:other
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ aftermerge
  # cat f
  revision 2
  space
  # hg stat
  M f
  # hg resolve --list
  R f

ui.merge specifies internal:prompt:

  $ beforemerge
  [merge-tools]
  false.whatever=
  true.priority=1
  true.executable=cat
  # hg goto -C 1
  $ hg merge -r 0185f4e0cf024bb0ed9694d1cbdea347ecce96d9 --config ui.merge=internal:prompt
  keep (l)ocal [working copy], take (o)ther [merge rev], or leave (u)nresolved for f? u
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg goto -C .' to abandon
  [1]
  $ aftermerge
  # cat f
  revision 1
  space
  # hg stat
  M f
  # hg resolve --list
  U f

ui.merge specifies :prompt, with 'leave unresolved' chosen

  $ beforemerge
  [merge-tools]
  false.whatever=
  true.priority=1
  true.executable=cat
  # hg goto -C 1
  $ hg merge -r 0185f4e0cf024bb0ed9694d1cbdea347ecce96d9 --config ui.merge=:prompt --config ui.interactive=True << EOF
  > u
  > EOF
  keep (l)ocal [working copy], take (o)ther [merge rev], or leave (u)nresolved for f? u
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg goto -C .' to abandon
  [1]
  $ aftermerge
  # cat f
  revision 1
  space
  # hg stat
  M f
  # hg resolve --list
  U f

prompt with EOF

  $ beforemerge
  [merge-tools]
  false.whatever=
  true.priority=1
  true.executable=cat
  # hg goto -C 1
  $ hg merge -r 0185f4e0cf024bb0ed9694d1cbdea347ecce96d9 --config ui.merge=internal:prompt --config ui.interactive=true
  keep (l)ocal [working copy], take (o)ther [merge rev], or leave (u)nresolved for f? 
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg goto -C .' to abandon
  [1]
  $ aftermerge
  # cat f
  revision 1
  space
  # hg stat
  M f
  # hg resolve --list
  U f
  $ hg resolve --all --config ui.merge=internal:prompt --config ui.interactive=true
  keep (l)ocal [working copy], take (o)ther [merge rev], or leave (u)nresolved for f? 
  [1]
  $ aftermerge
  # cat f
  revision 1
  space
  # hg stat
  M f
  ? f.orig
  # hg resolve --list
  U f
  $ rm f
  $ hg resolve --all --config ui.merge=internal:prompt --config ui.interactive=true
  keep (l)ocal [working copy], take (o)ther [merge rev], or leave (u)nresolved for f? 
  [1]
  $ aftermerge
  # cat f
  revision 1
  space
  # hg stat
  M f
  # hg resolve --list
  U f
  $ hg resolve --all --config ui.merge=internal:prompt
  keep (l)ocal [working copy], take (o)ther [merge rev], or leave (u)nresolved for f? u
  [1]
  $ aftermerge
  # cat f
  revision 1
  space
  # hg stat
  M f
  ? f.orig
  # hg resolve --list
  U f

ui.merge specifies internal:dump:

  $ beforemerge
  [merge-tools]
  false.whatever=
  true.priority=1
  true.executable=cat
  # hg goto -C 1
  $ hg merge -r 0185f4e0cf024bb0ed9694d1cbdea347ecce96d9 --config ui.merge=internal:dump
  merging f
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg goto -C .' to abandon
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
  # hg resolve --list
  U f

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

check that internal:dump doesn't dump files if premerge runs
successfully

  $ beforemerge
  [merge-tools]
  false.whatever=
  true.priority=1
  true.executable=cat
  # hg goto -C 1
  $ hg merge -r edce46a272756e7e892533c95e8e6fe3f064939a --config ui.merge=internal:dump
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
  # hg resolve --list
  R f

check that internal:forcedump dumps files, even if local and other can
be merged easily

  $ beforemerge
  [merge-tools]
  false.whatever=
  true.priority=1
  true.executable=cat
  # hg goto -C 1
  $ hg merge -r edce46a272756e7e892533c95e8e6fe3f064939a --config ui.merge=internal:forcedump
  merging f
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg goto -C .' to abandon
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
  # hg resolve --list
  U f

  $ cat f.base
  revision 0
  space

  $ cat f.local
  revision 1
  space

  $ cat f.other
  revision 0
  space
  revision 3

  $ rm -f f.base f.local f.other

ui.merge specifies internal:other but is overruled by pattern for false:

  $ beforemerge
  [merge-tools]
  false.whatever=
  true.priority=1
  true.executable=cat
  # hg goto -C 1
  $ hg merge -r 0185f4e0cf024bb0ed9694d1cbdea347ecce96d9 --config ui.merge=internal:other --config merge-patterns.f=false
  merging f
  merging f failed!
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg goto -C .' to abandon
  [1]
  $ aftermerge
  # cat f
  revision 1
  space
  # hg stat
  M f
  ? f.orig
  # hg resolve --list
  U f

Premerge

ui.merge specifies internal:other but is overruled by --tool=false

  $ beforemerge
  [merge-tools]
  false.whatever=
  true.priority=1
  true.executable=cat
  # hg goto -C 1
  $ hg merge -r 0185f4e0cf024bb0ed9694d1cbdea347ecce96d9 --config ui.merge=internal:other --tool=false
  merging f
  merging f failed!
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg goto -C .' to abandon
  [1]
  $ aftermerge
  # cat f
  revision 1
  space
  # hg stat
  M f
  ? f.orig
  # hg resolve --list
  U f

HGMERGE specifies internal:other but is overruled by --tool=false

  $ HGMERGE=internal:other ; export HGMERGE
  $ beforemerge
  [merge-tools]
  false.whatever=
  true.priority=1
  true.executable=cat
  # hg goto -C 1
  $ hg merge -r 0185f4e0cf024bb0ed9694d1cbdea347ecce96d9 --tool=false
  merging f
  merging f failed!
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg goto -C .' to abandon
  [1]
  $ aftermerge
  # cat f
  revision 1
  space
  # hg stat
  M f
  ? f.orig
  # hg resolve --list
  U f

  $ unset HGMERGE # make sure HGMERGE doesn't interfere with remaining tests

update is a merge ...

(this also tests that files reverted with '--rev REV' are treated as
"modified", even if none of mode, size and timestamp of them isn't
changed on the filesystem (see also issue4583))

  $ cat >> $HGRCPATH <<EOF
  > [fakedirstatewritetime]
  > # emulate invoking dirstate.write() via repo.status()
  > # at 2000-01-01 00:00
  > fakenow = 2000-01-01 00:00:00
  > EOF

  $ beforemerge
  [merge-tools]
  false.whatever=
  true.priority=1
  true.executable=cat
  # hg goto -C 1
  $ hg goto -q ffd2bda21d6ef8cb02e27e3d7f96f8ac8d196821
  $ f -s f
  f: size=17
  $ touch -t 200001010000 f
  $ hg debugrebuildstate
  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > fakedirstatewritetime = $TESTDIR/fakedirstatewritetime.py
  > EOF
  $ hg revert -q -r ef83787e2614c6fef8e58c9739cd5b46240ad4f0 .
  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > fakedirstatewritetime = !
  > EOF
  $ f -s f
  f: size=17
  $ touch -t 200001010000 f
  $ hg status f
  M f
  $ hg goto -r 0185f4e0cf024bb0ed9694d1cbdea347ecce96d9
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
  # hg resolve --list
  R f

update should also have --tool

  $ beforemerge
  [merge-tools]
  false.whatever=
  true.priority=1
  true.executable=cat
  # hg goto -C 1
  $ hg goto -q ffd2bda21d6ef8cb02e27e3d7f96f8ac8d196821
  $ f -s f
  f: size=17
  $ touch -t 200001010000 f
  $ hg debugrebuildstate
  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > fakedirstatewritetime = $TESTDIR/fakedirstatewritetime.py
  > EOF
  $ hg revert -q -r ef83787e2614c6fef8e58c9739cd5b46240ad4f0 .
  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > fakedirstatewritetime = !
  > EOF
  $ f -s f
  f: size=17
  $ touch -t 200001010000 f
  $ hg status f
  M f
  $ hg goto -r 0185f4e0cf024bb0ed9694d1cbdea347ecce96d9 --tool false
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
  # hg resolve --list
  U f

Default is silent simplemerge:

  $ beforemerge
  [merge-tools]
  false.whatever=
  true.priority=1
  true.executable=cat
  # hg goto -C 1
  $ hg merge -r edce46a272756e7e892533c95e8e6fe3f064939a
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
  # hg resolve --list
  R f

.premerge=True is same:

  $ beforemerge
  [merge-tools]
  false.whatever=
  true.priority=1
  true.executable=cat
  # hg goto -C 1
  $ hg merge -r edce46a272756e7e892533c95e8e6fe3f064939a --config merge-tools.true.premerge=True
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
  # hg resolve --list
  R f

.premerge=False executes merge-tool:

  $ beforemerge
  [merge-tools]
  false.whatever=
  true.priority=1
  true.executable=cat
  # hg goto -C 1
  $ hg merge -r edce46a272756e7e892533c95e8e6fe3f064939a --config merge-tools.true.premerge=False
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
  # hg resolve --list
  R f

premerge=keep keeps conflict markers in:

  $ beforemerge
  [merge-tools]
  false.whatever=
  true.priority=1
  true.executable=cat
  # hg goto -C 1
  $ hg merge -r 'max(desc(revision))' --config merge-tools.true.premerge=keep
  merging f
  <<<<<<< working copy: ef83787e2614 - test: revision 1
  revision 1
  space
  =======
  revision 4
  >>>>>>> merge rev:    81448d39c9a0 - test: revision 4
  revision 0
  space
  revision 4
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ aftermerge
  # cat f
  <<<<<<< working copy: ef83787e2614 - test: revision 1
  revision 1
  space
  =======
  revision 4
  >>>>>>> merge rev:    81448d39c9a0 - test: revision 4
  # hg stat
  M f
  # hg resolve --list
  R f

premerge=keep-merge3 keeps conflict markers with base content:

  $ beforemerge
  [merge-tools]
  false.whatever=
  true.priority=1
  true.executable=cat
  # hg goto -C 1
  $ hg merge -r 'max(desc(revision))' --config merge-tools.true.premerge=keep-merge3
  merging f
  <<<<<<< working copy: ef83787e2614 - test: revision 1
  revision 1
  space
  ||||||| base
  revision 0
  space
  =======
  revision 4
  >>>>>>> merge rev:    81448d39c9a0 - test: revision 4
  revision 0
  space
  revision 4
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ aftermerge
  # cat f
  <<<<<<< working copy: ef83787e2614 - test: revision 1
  revision 1
  space
  ||||||| base
  revision 0
  space
  =======
  revision 4
  >>>>>>> merge rev:    81448d39c9a0 - test: revision 4
  # hg stat
  M f
  # hg resolve --list
  R f


Tool execution

set tools.args explicit to include $base $local $other $output:

  $ beforemerge
  [merge-tools]
  false.whatever=
  true.priority=1
  true.executable=cat
  # hg goto -C 1
  $ hg merge -r 0185f4e0cf024bb0ed9694d1cbdea347ecce96d9 --config merge-tools.true.executable=head --config merge-tools.true.args='$base $local $other $output' \
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
  # hg resolve --list
  R f

Merge with "echo mergeresult > $local":

  $ beforemerge
  [merge-tools]
  false.whatever=
  true.priority=1
  true.executable=cat
  # hg goto -C 1
  $ hg merge -r 0185f4e0cf024bb0ed9694d1cbdea347ecce96d9 --config merge-tools.true.executable=echo --config merge-tools.true.args='mergeresult > $local'
  merging f
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ aftermerge
  # cat f
  mergeresult
  # hg stat
  M f
  # hg resolve --list
  R f

- and $local is the file f:

  $ beforemerge
  [merge-tools]
  false.whatever=
  true.priority=1
  true.executable=cat
  # hg goto -C 1
  $ hg merge -r 0185f4e0cf024bb0ed9694d1cbdea347ecce96d9 --config merge-tools.true.executable=echo --config merge-tools.true.args='mergeresult > f'
  merging f
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ aftermerge
  # cat f
  mergeresult
  # hg stat
  M f
  # hg resolve --list
  R f

Relative path in merge tool executable searches for tool inside repo before looking outside

  $ beforemerge
  [merge-tools]
  false.whatever=
  true.priority=1
  true.executable=cat
  # hg goto -C 1
  $ echo "echo hello, world!" > hello_world.sh
  $ chmod +x hello_world.sh
  $ hg stat
  ? hello_world.sh
  $ hg merge -r 0185f4e0cf024bb0ed9694d1cbdea347ecce96d9 --config merge-tools.true.executable='hello_world.sh' --config merge-tools.true.args='> f'
  merging f
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ rm -f hello_world.sh
  $ aftermerge
  # cat f
  hello, world!
  # hg stat
  M f
  # hg resolve --list
  R f

Merge with "echo mergeresult > $output" - the variable is a bit magic:

  $ beforemerge
  [merge-tools]
  false.whatever=
  true.priority=1
  true.executable=cat
  # hg goto -C 1
  $ hg merge -r 0185f4e0cf024bb0ed9694d1cbdea347ecce96d9 --config merge-tools.true.executable=echo --config merge-tools.true.args='mergeresult > $output'
  merging f
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ aftermerge
  # cat f
  mergeresult
  # hg stat
  M f
  # hg resolve --list
  R f

Merge using tool with a path that must be quoted:

  $ beforemerge
  [merge-tools]
  false.whatever=
  true.priority=1
  true.executable=cat
  # hg goto -C 1
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
  # hg resolve --list
  R f

Issue3581: Merging a filename that needs to be quoted
(This test doesn't work on Windows filesystems even on Linux, so check
for Unix-like permission)

#if unix-permissions
  $ beforemerge
  [merge-tools]
  false.whatever=
  true.priority=1
  true.executable=cat
  # hg goto -C 1
  $ echo "revision 5" > '"; exit 1; echo "'
  $ hg commit -Am "revision 5"
  adding "; exit 1; echo "
  warning: filename contains '"', which is reserved on Windows: '"; exit 1; echo "'
  $ hg goto -C ef83787e2614c6fef8e58c9739cd5b46240ad4f0 > /dev/null
  $ echo "revision 6" > '"; exit 1; echo "'
  $ hg commit -Am "revision 6"
  adding "; exit 1; echo "
  warning: filename contains '"', which is reserved on Windows: '"; exit 1; echo "'
  $ hg merge --config merge-tools.true.executable="true" -r 10f0eb7f76ea9770c110b7161c9ac2b0cc7f7846
  merging "; exit 1; echo "
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg goto -C ef83787e2614c6fef8e58c9739cd5b46240ad4f0 > /dev/null
#endif

Merge post-processing

cat is a bad merge-tool and doesn't change:

  $ beforemerge
  [merge-tools]
  false.whatever=
  true.priority=1
  true.executable=cat
  # hg goto -C 1
  $ hg merge -y -r 0185f4e0cf024bb0ed9694d1cbdea347ecce96d9 --config merge-tools.true.checkchanged=1
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
  use 'hg resolve' to retry unresolved file merges or 'hg goto -C .' to abandon
  [1]
  $ aftermerge
  # cat f
  revision 1
  space
  # hg stat
  M f
  ? f.orig
  # hg resolve --list
  U f

#if symlink

internal merge cannot handle symlinks and shouldn't try:

  $ hg goto -q -C ef83787e2614c6fef8e58c9739cd5b46240ad4f0
  $ rm f
  $ ln -s symlink f
  $ hg commit -qm 'f is symlink'
  $ hg merge -r 0185f4e0cf024bb0ed9694d1cbdea347ecce96d9 --tool internal:merge
  merging f
  warning: internal :merge cannot merge symlinks for f
  warning: 1 conflicts while merging f! (edit, then use 'hg resolve --mark')
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg goto -C .' to abandon
  [1]

#endif

Verify naming of temporary files and that extension is preserved:

  $ hg goto -q -C ef83787e2614c6fef8e58c9739cd5b46240ad4f0
  $ hg mv f f.txt
  $ hg ci -qm "f.txt"
  $ hg goto -q -C 0185f4e0cf024bb0ed9694d1cbdea347ecce96d9
  $ hg merge -y -r tip --tool echo --config merge-tools.echo.args='$base $local $other $output'
  merging f and f.txt to f.txt
  */f~base* $TESTTMP/repo/f.txt.orig */f~other.*txt $TESTTMP/repo/f.txt (glob)
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)

Check that debugpicktool examines which merge tool is chosen for
specified file as expected

  $ beforemerge
  [merge-tools]
  false.whatever=
  true.priority=1
  true.executable=cat
  # hg goto -C 1

(default behavior: checking files in the working parent context)

  $ hg manifest
  f
  $ hg debugpickmergetool
  f = true

(-X/-I and file patterns limmit examination targets)

  $ hg debugpickmergetool -X f
  $ hg debugpickmergetool unknown
  unknown: no such file in rev ef83787e2614

(--changedelete emulates merging change and delete)

  $ hg debugpickmergetool --changedelete
  f = :prompt

(-r REV causes checking files in specified revision)

  $ hg manifest -r tip
  f.txt
  $ hg debugpickmergetool -r tip
  f.txt = true

#if symlink

(symlink causes chosing :prompt)

  $ hg debugpickmergetool -r 6d00b3726f6e
  f = :prompt

#endif

(--verbose shows some configurations)

  $ hg debugpickmergetool --tool foobar -v
  with --tool 'foobar'
  f = foobar

  $ HGMERGE=false hg debugpickmergetool -v
  with HGMERGE='false'
  f = false

  $ hg debugpickmergetool --config ui.merge=false -v
  with ui.merge='false'
  f = false

(--debug shows errors detected intermediately)

  $ hg debugpickmergetool --config merge-patterns.f=true --config merge-tools.true.executable=nonexistentmergetool --debug f
  couldn't find merge tool true (for pattern f)
  picktool() interactive=False plain=False
  couldn't find merge tool true
  picktool() tools
  f = false

test ui.merge:interactive

  $ hg debugpickmergetool --config ui.formatted=false --config ui.interactive=false --config ui.merge=nonint --config ui.merge:interactive=int f
  f = nonint
  $ HGPLAIN=1 hg debugpickmergetool --config ui.interactive=true  --config ui.merge=nonint --config ui.merge:interactive=int f
  f = nonint
  $ HGPLAIN=1 HGPLAINEXCEPT=mergetool hg debugpickmergetool --config ui.interactive=true  --config ui.merge=nonint --config ui.merge:interactive=int f
  f = int
  $ hg debugpickmergetool --config ui.formatted=true  --config ui.interactive=false --config ui.merge=nonint --config ui.merge:interactive=int f
  f = nonint
  $ hg debugpickmergetool --config ui.formatted=true  --config ui.interactive=true  --config ui.merge=nonint --config ui.merge:interactive=int f
  f = int
