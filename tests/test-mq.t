  $ checkundo()
  > {
  >     if [ -f .hg/store/undo ]; then
  >     echo ".hg/store/undo still exists after $1"
  >     fi
  > }

  $ echo "[extensions]" >> $HGRCPATH
  $ echo "mq=" >> $HGRCPATH

  $ echo "[mq]" >> $HGRCPATH
  $ echo "plain=true" >> $HGRCPATH


help

  $ hg help mq
  mq extension - manage a stack of patches
  
  This extension lets you work with a stack of patches in a Mercurial
  repository. It manages two stacks of patches - all known patches, and applied
  patches (subset of known patches).
  
  Known patches are represented as patch files in the .hg/patches directory.
  Applied patches are both patch files and changesets.
  
  Common tasks (use "hg help command" for more details):
  
    create new patch                          qnew
    import existing patch                     qimport
  
    print patch series                        qseries
    print applied patches                     qapplied
  
    add known patch to applied stack          qpush
    remove patch from applied stack           qpop
    refresh contents of top applied patch     qrefresh
  
  By default, mq will automatically use git patches when required to avoid
  losing file mode changes, copy records, binary files or empty files creations
  or deletions. This behaviour can be configured with:
  
    [mq]
    git = auto/keep/yes/no
  
  If set to 'keep', mq will obey the [diff] section configuration while
  preserving existing git patches upon qrefresh. If set to 'yes' or 'no', mq
  will override the [diff] section and always generate git or regular patches,
  possibly losing data in the second case.
  
  It may be desirable for mq changesets to be kept in the secret phase (see "hg
  help phases"), which can be enabled with the following setting:
  
    [mq]
    secret = True
  
  You will by default be managing a patch queue named "patches". You can create
  other, independent patch queues with the "hg qqueue" command.
  
  If the working directory contains uncommitted files, qpush, qpop and qgoto
  abort immediately. If -f/--force is used, the changes are discarded. Setting:
  
    [mq]
    keepchanges = True
  
  make them behave as if --keep-changes were passed, and non-conflicting local
  changes will be tolerated and preserved. If incompatible options such as
  -f/--force or --exact are passed, this setting is ignored.
  
  This extension used to provide a strip command. This command now lives in the
  strip extension.
  
  list of commands:
  
   qapplied      print the patches already applied
   qclone        clone main and patch repository at same time
   qdelete       remove patches from queue
   qdiff         diff of the current patch and subsequent modifications
   qfinish       move applied patches into repository history
   qfold         fold the named patches into the current patch
   qgoto         push or pop patches until named patch is at top of stack
   qguard        set or print guards for a patch
   qheader       print the header of the topmost or specified patch
   qimport       import a patch or existing changeset
   qnew          create a new patch
   qnext         print the name of the next pushable patch
   qpop          pop the current patch off the stack
   qprev         print the name of the preceding applied patch
   qpush         push the next patch onto the stack
   qqueue        manage multiple patch queues
   qrefresh      update the current patch
   qrename       rename a patch
   qselect       set or print guarded patches to push
   qseries       print the entire series file
   qtop          print the name of the current patch
   qunapplied    print the patches not yet applied
  
  use "hg -v help mq" to show builtin aliases and global options

  $ hg init a
  $ cd a
  $ echo a > a
  $ hg ci -Ama
  adding a

  $ hg clone . ../k
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ mkdir b
  $ echo z > b/z
  $ hg ci -Ama
  adding b/z


qinit

  $ hg qinit

  $ cd ..
  $ hg init b


-R qinit

  $ hg -R b qinit

  $ hg init c


qinit -c

  $ hg --cwd c qinit -c
  $ hg -R c/.hg/patches st
  A .hgignore
  A series


qinit; qinit -c

  $ hg init d
  $ cd d
  $ hg qinit
  $ hg qinit -c

qinit -c should create both files if they don't exist

  $ cat .hg/patches/.hgignore
  ^\.hg
  ^\.mq
  syntax: glob
  status
  guards
  $ cat .hg/patches/series
  $ hg qinit -c
  abort: repository $TESTTMP/d/.hg/patches already exists! (glob)
  [255]
  $ cd ..

  $ echo '% qinit; <stuff>; qinit -c'
  % qinit; <stuff>; qinit -c
  $ hg init e
  $ cd e
  $ hg qnew A
  $ checkundo qnew
  $ echo foo > foo
  $ hg phase -r qbase
  0: draft
  $ hg add foo
  $ hg qrefresh
  $ hg phase -r qbase
  0: draft
  $ hg qnew B
  $ echo >> foo
  $ hg qrefresh
  $ echo status >> .hg/patches/.hgignore
  $ echo bleh >> .hg/patches/.hgignore
  $ hg qinit -c
  adding .hg/patches/A (glob)
  adding .hg/patches/B (glob)
  $ hg -R .hg/patches status
  A .hgignore
  A A
  A B
  A series

qinit -c shouldn't touch these files if they already exist

  $ cat .hg/patches/.hgignore
  status
  bleh
  $ cat .hg/patches/series
  A
  B

add an untracked file

  $ echo >> .hg/patches/flaf

status --mq with color (issue2096)

  $ hg status --mq --config extensions.color= --config color.mode=ansi --color=always
  \x1b[0;32;1mA \x1b[0m\x1b[0;32;1m.hgignore\x1b[0m (esc)
  \x1b[0;32;1mA \x1b[0m\x1b[0;32;1mA\x1b[0m (esc)
  \x1b[0;32;1mA \x1b[0m\x1b[0;32;1mB\x1b[0m (esc)
  \x1b[0;32;1mA \x1b[0m\x1b[0;32;1mseries\x1b[0m (esc)
  \x1b[0;35;1;4m? \x1b[0m\x1b[0;35;1;4mflaf\x1b[0m (esc)

try the --mq option on a command provided by an extension

  $ hg purge --mq --verbose --config extensions.purge=
  removing file flaf

  $ cd ..

#if no-outer-repo

init --mq without repo

  $ mkdir f
  $ cd f
  $ hg init --mq
  abort: there is no Mercurial repository here (.hg not found)
  [255]
  $ cd ..

#endif

init --mq with repo path

  $ hg init g
  $ hg init --mq g
  $ test -d g/.hg/patches/.hg

init --mq with nonexistent directory

  $ hg init --mq nonexistentdir
  abort: repository nonexistentdir not found!
  [255]


init --mq with bundle (non "local")

  $ hg -R a bundle --all a.bundle >/dev/null
  $ hg init --mq a.bundle
  abort: only a local queue repository may be initialized
  [255]

  $ cd a

  $ hg qnew -m 'foo bar' test.patch

  $ echo '# comment' > .hg/patches/series.tmp
  $ echo >> .hg/patches/series.tmp # empty line
  $ cat .hg/patches/series >> .hg/patches/series.tmp
  $ mv .hg/patches/series.tmp .hg/patches/series


qrefresh

  $ echo a >> a
  $ hg qrefresh
  $ cat .hg/patches/test.patch
  foo bar
  
  diff -r [a-f0-9]* a (re)
  --- a/a\t(?P<date>.*) (re)
  \+\+\+ b/a\t(?P<date2>.*) (re)
  @@ -1,1 +1,2 @@
   a
  +a

empty qrefresh

  $ hg qrefresh -X a

revision:

  $ hg diff -r -2 -r -1

patch:

  $ cat .hg/patches/test.patch
  foo bar
  

working dir diff:

  $ hg diff --nodates -q
  --- a/a
  +++ b/a
  @@ -1,1 +1,2 @@
   a
  +a

restore things

  $ hg qrefresh
  $ checkundo qrefresh


qpop

  $ hg qpop
  popping test.patch
  patch queue now empty
  $ checkundo qpop


qpush with dump of tag cache
Dump the tag cache to ensure that it has exactly one head after qpush.

  $ rm -f .hg/cache/tags
  $ hg tags > /dev/null

.hg/cache/tags (pre qpush):

  $ cat .hg/cache/tags
  1 [\da-f]{40} (re)
  
  $ hg qpush
  applying test.patch
  now at: test.patch
  $ hg phase -r qbase
  2: draft
  $ hg tags > /dev/null

.hg/cache/tags (post qpush):

  $ cat .hg/cache/tags
  2 [\da-f]{40} (re)
  
  $ checkundo qpush
  $ cd ..


pop/push outside repo
  $ hg -R a qpop
  popping test.patch
  patch queue now empty
  $ hg -R a qpush
  applying test.patch
  now at: test.patch

  $ cd a
  $ hg qnew test2.patch

qrefresh in subdir

  $ cd b
  $ echo a > a
  $ hg add a
  $ hg qrefresh

pop/push -a in subdir

  $ hg qpop -a
  popping test2.patch
  popping test.patch
  patch queue now empty
  $ hg --traceback qpush -a
  applying test.patch
  applying test2.patch
  now at: test2.patch


setting columns & formatted tests truncating (issue1912)

  $ COLUMNS=4 hg qseries --config ui.formatted=true
  test.patch
  test2.patch
  $ COLUMNS=20 hg qseries --config ui.formatted=true -vs
  0 A test.patch: f...
  1 A test2.patch: 
  $ hg qpop
  popping test2.patch
  now at: test.patch
  $ hg qseries -vs
  0 A test.patch: foo bar
  1 U test2.patch: 
  $ hg sum | grep mq
  mq:     1 applied, 1 unapplied
  $ hg qpush
  applying test2.patch
  now at: test2.patch
  $ hg sum | grep mq
  mq:     2 applied
  $ hg qapplied
  test.patch
  test2.patch
  $ hg qtop
  test2.patch


prev

  $ hg qapp -1
  test.patch

next

  $ hg qunapp -1
  all patches applied
  [1]

  $ hg qpop
  popping test2.patch
  now at: test.patch

commit should fail

  $ hg commit
  abort: cannot commit over an applied mq patch
  [255]

push should fail if draft

  $ hg push ../../k
  pushing to ../../k
  abort: source has mq patches applied
  [255]


import should fail

  $ hg st .
  $ echo foo >> ../a
  $ hg diff > ../../import.diff
  $ hg revert --no-backup ../a
  $ hg import ../../import.diff
  abort: cannot import over an applied patch
  [255]
  $ hg st

import --no-commit should succeed

  $ hg import --no-commit ../../import.diff
  applying ../../import.diff
  $ hg st
  M a
  $ hg revert --no-backup ../a


qunapplied

  $ hg qunapplied
  test2.patch


qpush/qpop with index

  $ hg qnew test1b.patch
  $ echo 1b > 1b
  $ hg add 1b
  $ hg qrefresh
  $ hg qpush 2
  applying test2.patch
  now at: test2.patch
  $ hg qpop 0
  popping test2.patch
  popping test1b.patch
  now at: test.patch
  $ hg qpush test.patch+1
  applying test1b.patch
  now at: test1b.patch
  $ hg qpush test.patch+2
  applying test2.patch
  now at: test2.patch
  $ hg qpop test2.patch-1
  popping test2.patch
  now at: test1b.patch
  $ hg qpop test2.patch-2
  popping test1b.patch
  now at: test.patch
  $ hg qpush test1b.patch+1
  applying test1b.patch
  applying test2.patch
  now at: test2.patch


qpush --move

  $ hg qpop -a
  popping test2.patch
  popping test1b.patch
  popping test.patch
  patch queue now empty
  $ hg qguard test1b.patch -- -negguard
  $ hg qguard test2.patch -- +posguard
  $ hg qpush --move test2.patch # can't move guarded patch
  cannot push 'test2.patch' - guarded by '+posguard'
  [1]
  $ hg qselect posguard
  number of unguarded, unapplied patches has changed from 2 to 3
  $ hg qpush --move test2.patch # move to front
  applying test2.patch
  now at: test2.patch
  $ hg qpush --move test1b.patch # negative guard unselected
  applying test1b.patch
  now at: test1b.patch
  $ hg qpush --move test.patch # noop move
  applying test.patch
  now at: test.patch
  $ hg qseries -v
  0 A test2.patch
  1 A test1b.patch
  2 A test.patch
  $ hg qpop -a
  popping test.patch
  popping test1b.patch
  popping test2.patch
  patch queue now empty

cleaning up

  $ hg qselect --none
  guards deactivated
  number of unguarded, unapplied patches has changed from 3 to 2
  $ hg qguard --none test1b.patch
  $ hg qguard --none test2.patch
  $ hg qpush --move test.patch
  applying test.patch
  now at: test.patch
  $ hg qpush --move test1b.patch
  applying test1b.patch
  now at: test1b.patch
  $ hg qpush --move bogus # nonexistent patch
  abort: patch bogus not in series
  [255]
  $ hg qpush --move # no patch
  abort: please specify the patch to move
  [255]
  $ hg qpush --move test.patch # already applied
  abort: cannot push to a previous patch: test.patch
  [255]
  $ sed '2i\
  > # make qtip index different in series and fullseries
  > ' `hg root`/.hg/patches/series > $TESTTMP/sedtmp
  $ cp $TESTTMP/sedtmp `hg root`/.hg/patches/series
  $ cat `hg root`/.hg/patches/series
  # comment
  # make qtip index different in series and fullseries
  
  test.patch
  test1b.patch
  test2.patch
  $ hg qpush --move test2.patch
  applying test2.patch
  now at: test2.patch


series after move

  $ cat `hg root`/.hg/patches/series
  # comment
  # make qtip index different in series and fullseries
  
  test.patch
  test1b.patch
  test2.patch


pop, qapplied, qunapplied

  $ hg qseries -v
  0 A test.patch
  1 A test1b.patch
  2 A test2.patch

qapplied -1 test.patch

  $ hg qapplied -1 test.patch
  only one patch applied
  [1]

qapplied -1 test1b.patch

  $ hg qapplied -1 test1b.patch
  test.patch

qapplied -1 test2.patch

  $ hg qapplied -1 test2.patch
  test1b.patch

qapplied -1

  $ hg qapplied -1
  test1b.patch

qapplied

  $ hg qapplied
  test.patch
  test1b.patch
  test2.patch

qapplied test1b.patch

  $ hg qapplied test1b.patch
  test.patch
  test1b.patch

qunapplied -1

  $ hg qunapplied -1
  all patches applied
  [1]

qunapplied

  $ hg qunapplied

popping

  $ hg qpop
  popping test2.patch
  now at: test1b.patch

qunapplied -1

  $ hg qunapplied -1
  test2.patch

qunapplied

  $ hg qunapplied
  test2.patch

qunapplied test2.patch

  $ hg qunapplied test2.patch

qunapplied -1 test2.patch

  $ hg qunapplied -1 test2.patch
  all patches applied
  [1]

popping -a

  $ hg qpop -a
  popping test1b.patch
  popping test.patch
  patch queue now empty

qapplied

  $ hg qapplied

qapplied -1

  $ hg qapplied -1
  no patches applied
  [1]
  $ hg qpush
  applying test.patch
  now at: test.patch


push should succeed

  $ hg qpop -a
  popping test.patch
  patch queue now empty
  $ hg push ../../k
  pushing to ../../k
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files


we want to start with some patches applied

  $ hg qpush -a
  applying test.patch
  applying test1b.patch
  applying test2.patch
  now at: test2.patch

% pops all patches and succeeds

  $ hg qpop -a
  popping test2.patch
  popping test1b.patch
  popping test.patch
  patch queue now empty

% does nothing and succeeds

  $ hg qpop -a
  no patches applied

% fails - nothing else to pop

  $ hg qpop
  no patches applied
  [1]

% pushes a patch and succeeds

  $ hg qpush
  applying test.patch
  now at: test.patch

% pops a patch and succeeds

  $ hg qpop
  popping test.patch
  patch queue now empty

% pushes up to test1b.patch and succeeds

  $ hg qpush test1b.patch
  applying test.patch
  applying test1b.patch
  now at: test1b.patch

% does nothing and succeeds

  $ hg qpush test1b.patch
  qpush: test1b.patch is already at the top

% does nothing and succeeds

  $ hg qpop test1b.patch
  qpop: test1b.patch is already at the top

% fails - can't push to this patch

  $ hg qpush test.patch
  abort: cannot push to a previous patch: test.patch
  [255]

% fails - can't pop to this patch

  $ hg qpop test2.patch
  abort: patch test2.patch is not applied
  [255]

% pops up to test.patch and succeeds

  $ hg qpop test.patch
  popping test1b.patch
  now at: test.patch

% pushes all patches and succeeds

  $ hg qpush -a
  applying test1b.patch
  applying test2.patch
  now at: test2.patch

% does nothing and succeeds

  $ hg qpush -a
  all patches are currently applied

% fails - nothing else to push

  $ hg qpush
  patch series already fully applied
  [1]

% does nothing and succeeds

  $ hg qpush test2.patch
  qpush: test2.patch is already at the top

strip

  $ cd ../../b
  $ echo x>x
  $ hg ci -Ama
  adding x
  $ hg strip tip
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  saved backup bundle to $TESTTMP/b/.hg/strip-backup/*-backup.hg (glob)
  $ hg unbundle .hg/strip-backup/*
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  (run 'hg update' to get a working copy)


strip with local changes, should complain

  $ hg up
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo y>y
  $ hg add y
  $ hg strip tip
  abort: local changes found
  [255]

--force strip with local changes

  $ hg strip -f tip
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  saved backup bundle to $TESTTMP/b/.hg/strip-backup/*-backup.hg (glob)
  $ cd ..


cd b; hg qrefresh

  $ hg init refresh
  $ cd refresh
  $ echo a > a
  $ hg ci -Ama
  adding a
  $ hg qnew -mfoo foo
  $ echo a >> a
  $ hg qrefresh
  $ mkdir b
  $ cd b
  $ echo f > f
  $ hg add f
  $ hg qrefresh
  $ cat ../.hg/patches/foo
  foo
  
  diff -r cb9a9f314b8b a
  --- a/a\t(?P<date>.*) (re)
  \+\+\+ b/a\t(?P<date>.*) (re)
  @@ -1,1 +1,2 @@
   a
  +a
  diff -r cb9a9f314b8b b/f
  --- /dev/null\t(?P<date>.*) (re)
  \+\+\+ b/b/f\t(?P<date>.*) (re)
  @@ -0,0 +1,1 @@
  +f

hg qrefresh .

  $ hg qrefresh .
  $ cat ../.hg/patches/foo
  foo
  
  diff -r cb9a9f314b8b b/f
  --- /dev/null\t(?P<date>.*) (re)
  \+\+\+ b/b/f\t(?P<date>.*) (re)
  @@ -0,0 +1,1 @@
  +f
  $ hg status
  M a


qpush failure

  $ cd ..
  $ hg qrefresh
  $ hg qnew -mbar bar
  $ echo foo > foo
  $ echo bar > bar
  $ hg add foo bar
  $ hg qrefresh
  $ hg qpop -a
  popping bar
  popping foo
  patch queue now empty
  $ echo bar > foo
  $ hg qpush -a
  applying foo
  applying bar
  file foo already exists
  1 out of 1 hunks FAILED -- saving rejects to file foo.rej
  patch failed, unable to continue (try -v)
  patch failed, rejects left in working dir
  errors during apply, please fix and refresh bar
  [2]
  $ hg st
  ? foo
  ? foo.rej


mq tags

  $ hg log --template '{rev} {tags}\n' -r qparent:qtip
  0 qparent
  1 foo qbase
  2 bar qtip tip

mq revset

  $ hg log -r 'mq()' --template '{rev}\n'
  1
  2
  $ hg help revsets | grep -i mq
      "mq()"
        Changesets managed by MQ.

bad node in status

  $ hg qpop
  popping bar
  now at: foo
  $ hg strip -qn tip
  $ hg tip
  changeset:   0:cb9a9f314b8b
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     a
  
  $ hg branches
  default                        0:cb9a9f314b8b
  $ hg qpop
  no patches applied
  [1]

  $ cd ..


git patches

  $ cat >>$HGRCPATH <<EOF
  > [diff]
  > git = True
  > EOF
  $ hg init git
  $ cd git
  $ hg qinit

  $ hg qnew -m'new file' new
  $ echo foo > new
#if execbit
  $ chmod +x new
#endif
  $ hg add new
  $ hg qrefresh
#if execbit
  $ cat .hg/patches/new
  new file
  
  diff --git a/new b/new
  new file mode 100755
  --- /dev/null
  +++ b/new
  @@ -0,0 +1,1 @@
  +foo
#else
  $ cat .hg/patches/new
  new file
  
  diff --git a/new b/new
  new file mode 100644
  --- /dev/null
  +++ b/new
  @@ -0,0 +1,1 @@
  +foo
#endif

  $ hg qnew -m'copy file' copy
  $ hg cp new copy
  $ hg qrefresh
  $ cat .hg/patches/copy
  copy file
  
  diff --git a/new b/copy
  copy from new
  copy to copy

  $ hg qpop
  popping copy
  now at: new
  $ hg qpush
  applying copy
  now at: copy
  $ hg qdiff
  diff --git a/new b/copy
  copy from new
  copy to copy
  $ cat >>$HGRCPATH <<EOF
  > [diff]
  > git = False
  > EOF
  $ hg qdiff --git
  diff --git a/new b/copy
  copy from new
  copy to copy
  $ cd ..

empty lines in status

  $ hg init emptystatus
  $ cd emptystatus
  $ hg qinit
  $ printf '\n\n' > .hg/patches/status
  $ hg qser
  $ cd ..

bad line in status (without ":")

  $ hg init badstatus
  $ cd badstatus
  $ hg qinit
  $ printf 'babar has no colon in this line\n' > .hg/patches/status
  $ hg qser
  malformated mq status line: ['babar has no colon in this line']
  $ cd ..


test file addition in slow path

  $ hg init slow
  $ cd slow
  $ hg qinit
  $ echo foo > foo
  $ hg add foo
  $ hg ci -m 'add foo'
  $ hg qnew bar
  $ echo bar > bar
  $ hg add bar
  $ hg mv foo baz
  $ hg qrefresh --git
  $ hg up -C 0
  1 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ echo >> foo
  $ hg ci -m 'change foo'
  created new head
  $ hg up -C 1
  2 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg qrefresh --git
  $ cat .hg/patches/bar
  diff --git a/bar b/bar
  new file mode 100644
  --- /dev/null
  +++ b/bar
  @@ -0,0 +1,1 @@
  +bar
  diff --git a/foo b/baz
  rename from foo
  rename to baz
  $ hg log -v --template '{rev} {file_copies}\n' -r .
  2 baz (foo)
  $ hg qrefresh --git
  $ cat .hg/patches/bar
  diff --git a/bar b/bar
  new file mode 100644
  --- /dev/null
  +++ b/bar
  @@ -0,0 +1,1 @@
  +bar
  diff --git a/foo b/baz
  rename from foo
  rename to baz
  $ hg log -v --template '{rev} {file_copies}\n' -r .
  2 baz (foo)
  $ hg qrefresh
  $ grep 'diff --git' .hg/patches/bar
  diff --git a/bar b/bar
  diff --git a/foo b/baz


test file move chains in the slow path

  $ hg up -C 1
  1 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ echo >> foo
  $ hg ci -m 'change foo again'
  $ hg up -C 2
  2 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg mv bar quux
  $ hg mv baz bleh
  $ hg qrefresh --git
  $ cat .hg/patches/bar
  diff --git a/foo b/bleh
  rename from foo
  rename to bleh
  diff --git a/quux b/quux
  new file mode 100644
  --- /dev/null
  +++ b/quux
  @@ -0,0 +1,1 @@
  +bar
  $ hg log -v --template '{rev} {file_copies}\n' -r .
  3 bleh (foo)
  $ hg mv quux fred
  $ hg mv bleh barney
  $ hg qrefresh --git
  $ cat .hg/patches/bar
  diff --git a/foo b/barney
  rename from foo
  rename to barney
  diff --git a/fred b/fred
  new file mode 100644
  --- /dev/null
  +++ b/fred
  @@ -0,0 +1,1 @@
  +bar
  $ hg log -v --template '{rev} {file_copies}\n' -r .
  3 barney (foo)


refresh omitting an added file

  $ hg qnew baz
  $ echo newfile > newfile
  $ hg add newfile
  $ hg qrefresh
  $ hg st -A newfile
  C newfile
  $ hg qrefresh -X newfile
  $ hg st -A newfile
  A newfile
  $ hg revert newfile
  $ rm newfile
  $ hg qpop
  popping baz
  now at: bar

test qdel/qrm

  $ hg qdel baz
  $ echo p >> .hg/patches/series
  $ hg qrm p
  $ hg qser
  bar

create a git patch

  $ echo a > alexander
  $ hg add alexander
  $ hg qnew -f --git addalexander
  $ grep diff .hg/patches/addalexander
  diff --git a/alexander b/alexander


create a git binary patch

  $ cat > writebin.py <<EOF
  > import sys
  > path = sys.argv[1]
  > open(path, 'wb').write('BIN\x00ARY')
  > EOF
  $ python writebin.py bucephalus

  $ python "$TESTDIR/md5sum.py" bucephalus
  8ba2a2f3e77b55d03051ff9c24ad65e7  bucephalus
  $ hg add bucephalus
  $ hg qnew -f --git addbucephalus
  $ grep diff .hg/patches/addbucephalus
  diff --git a/bucephalus b/bucephalus


check binary patches can be popped and pushed

  $ hg qpop
  popping addbucephalus
  now at: addalexander
  $ test -f bucephalus && echo % bucephalus should not be there
  [1]
  $ hg qpush
  applying addbucephalus
  now at: addbucephalus
  $ test -f bucephalus
  $ python "$TESTDIR/md5sum.py" bucephalus
  8ba2a2f3e77b55d03051ff9c24ad65e7  bucephalus



strip again

  $ cd ..
  $ hg init strip
  $ cd strip
  $ touch foo
  $ hg add foo
  $ hg ci -m 'add foo'
  $ echo >> foo
  $ hg ci -m 'change foo 1'
  $ hg up -C 0
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo 1 >> foo
  $ hg ci -m 'change foo 2'
  created new head
  $ HGMERGE=true hg merge
  merging foo
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg ci -m merge
  $ hg log
  changeset:   3:99615015637b
  tag:         tip
  parent:      2:20cbbe65cff7
  parent:      1:d2871fc282d4
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     merge
  
  changeset:   2:20cbbe65cff7
  parent:      0:53245c60e682
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     change foo 2
  
  changeset:   1:d2871fc282d4
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     change foo 1
  
  changeset:   0:53245c60e682
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add foo
  
  $ hg strip 1
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  saved backup bundle to $TESTTMP/strip/.hg/strip-backup/*-backup.hg (glob)
  $ checkundo strip
  $ hg log
  changeset:   1:20cbbe65cff7
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     change foo 2
  
  changeset:   0:53245c60e682
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add foo
  
  $ cd ..


qclone

  $ qlog()
  > {
  >     echo 'main repo:'
  >     hg log --template '    rev {rev}: {desc}\n'
  >     echo 'patch repo:'
  >     hg -R .hg/patches log --template '    rev {rev}: {desc}\n'
  > }
  $ hg init qclonesource
  $ cd qclonesource
  $ echo foo > foo
  $ hg add foo
  $ hg ci -m 'add foo'
  $ hg qinit
  $ hg qnew patch1
  $ echo bar >> foo
  $ hg qrefresh -m 'change foo'
  $ cd ..


repo with unversioned patch dir

  $ hg qclone qclonesource failure
  abort: versioned patch repository not found (see init --mq)
  [255]

  $ cd qclonesource
  $ hg qinit -c
  adding .hg/patches/patch1 (glob)
  $ hg qci -m checkpoint
  $ qlog
  main repo:
      rev 1: change foo
      rev 0: add foo
  patch repo:
      rev 0: checkpoint
  $ cd ..


repo with patches applied

  $ hg qclone qclonesource qclonedest
  updating to branch default
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd qclonedest
  $ qlog
  main repo:
      rev 0: add foo
  patch repo:
      rev 0: checkpoint
  $ cd ..


repo with patches unapplied

  $ cd qclonesource
  $ hg qpop -a
  popping patch1
  patch queue now empty
  $ qlog
  main repo:
      rev 0: add foo
  patch repo:
      rev 0: checkpoint
  $ cd ..
  $ hg qclone qclonesource qclonedest2
  updating to branch default
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd qclonedest2
  $ qlog
  main repo:
      rev 0: add foo
  patch repo:
      rev 0: checkpoint
  $ cd ..


Issue1033: test applying on an empty file

  $ hg init empty
  $ cd empty
  $ touch a
  $ hg ci -Am addempty
  adding a
  $ echo a > a
  $ hg qnew -f -e changea
  $ hg qpop
  popping changea
  patch queue now empty
  $ hg qpush
  applying changea
  now at: changea
  $ cd ..

test qpush with --force, issue1087

  $ hg init forcepush
  $ cd forcepush
  $ echo hello > hello.txt
  $ echo bye > bye.txt
  $ hg ci -Ama
  adding bye.txt
  adding hello.txt
  $ hg qnew -d '0 0' empty
  $ hg qpop
  popping empty
  patch queue now empty
  $ echo world >> hello.txt


qpush should fail, local changes

  $ hg qpush
  abort: local changes found
  [255]


apply force, should not discard changes with empty patch

  $ hg qpush -f
  applying empty
  patch empty is empty
  now at: empty
  $ hg diff --config diff.nodates=True
  diff -r d58265112590 hello.txt
  --- a/hello.txt
  +++ b/hello.txt
  @@ -1,1 +1,2 @@
   hello
  +world
  $ hg qdiff --config diff.nodates=True
  diff -r 9ecee4f634e3 hello.txt
  --- a/hello.txt
  +++ b/hello.txt
  @@ -1,1 +1,2 @@
   hello
  +world
  $ hg log -l1 -p
  changeset:   1:d58265112590
  tag:         empty
  tag:         qbase
  tag:         qtip
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     imported patch empty
  
  
  $ hg qref -d '0 0'
  $ hg qpop
  popping empty
  patch queue now empty
  $ echo universe >> hello.txt
  $ echo universe >> bye.txt


qpush should fail, local changes

  $ hg qpush
  abort: local changes found
  [255]


apply force, should discard changes in hello, but not bye

  $ hg qpush -f --verbose
  applying empty
  saving current version of hello.txt as hello.txt.orig
  patching file hello.txt
  hello.txt
  now at: empty
  $ hg st
  M bye.txt
  ? hello.txt.orig
  $ hg diff --config diff.nodates=True
  diff -r ba252371dbc1 bye.txt
  --- a/bye.txt
  +++ b/bye.txt
  @@ -1,1 +1,2 @@
   bye
  +universe
  $ hg qdiff --config diff.nodates=True
  diff -r 9ecee4f634e3 bye.txt
  --- a/bye.txt
  +++ b/bye.txt
  @@ -1,1 +1,2 @@
   bye
  +universe
  diff -r 9ecee4f634e3 hello.txt
  --- a/hello.txt
  +++ b/hello.txt
  @@ -1,1 +1,3 @@
   hello
  +world
  +universe


test popping revisions not in working dir ancestry

  $ hg qseries -v
  0 A empty
  $ hg up qparent
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg qpop
  popping empty
  patch queue now empty

  $ cd ..
  $ hg init deletion-order
  $ cd deletion-order

  $ touch a
  $ hg ci -Aqm0

  $ hg qnew rename-dir
  $ hg rm a
  $ hg qrefresh

  $ mkdir a b
  $ touch a/a b/b
  $ hg add -q a b
  $ hg qrefresh


test popping must remove files added in subdirectories first

  $ hg qpop
  popping rename-dir
  patch queue now empty
  $ cd ..


test case preservation through patch pushing especially on case
insensitive filesystem

  $ hg init casepreserve
  $ cd casepreserve

  $ hg qnew add-file1
  $ echo a > TeXtFiLe.TxT
  $ hg add TeXtFiLe.TxT
  $ hg qrefresh

  $ hg qnew add-file2
  $ echo b > AnOtHeRFiLe.TxT
  $ hg add AnOtHeRFiLe.TxT
  $ hg qrefresh

  $ hg qnew modify-file
  $ echo c >> AnOtHeRFiLe.TxT
  $ hg qrefresh

  $ hg qapplied
  add-file1
  add-file2
  modify-file
  $ hg qpop -a
  popping modify-file
  popping add-file2
  popping add-file1
  patch queue now empty

this qpush causes problems below, if case preservation on case
insensitive filesystem is not enough:
(1) unexpected "adding ..." messages are shown
(2) patching fails in modification of (1) files

  $ hg qpush -a
  applying add-file1
  applying add-file2
  applying modify-file
  now at: modify-file

Proper phase default with mq:

1. mq.secret=false

  $ rm .hg/store/phaseroots
  $ hg phase 'qparent::'
  0: draft
  1: draft
  2: draft
  $ echo '[mq]' >> $HGRCPATH
  $ echo 'secret=true' >> $HGRCPATH
  $ rm -f .hg/store/phaseroots
  $ hg phase 'qparent::'
  0: secret
  1: secret
  2: secret

Test that qfinish change phase when mq.secret=true

  $ hg qfinish qbase
  patch add-file1 finalized without changeset message
  $ hg phase 'all()'
  0: draft
  1: secret
  2: secret

Test that qfinish respect phases.new-commit setting

  $ echo '[phases]' >> $HGRCPATH
  $ echo 'new-commit=secret' >> $HGRCPATH
  $ hg qfinish qbase
  patch add-file2 finalized without changeset message
  $ hg phase 'all()'
  0: draft
  1: secret
  2: secret

(restore env for next test)

  $ sed -e 's/new-commit=secret//' $HGRCPATH > $TESTTMP/sedtmp
  $ cp $TESTTMP/sedtmp $HGRCPATH
  $ hg qimport -r 1 --name  add-file2

Test that qfinish preserve phase when mq.secret=false

  $ sed -e 's/secret=true/secret=false/' $HGRCPATH > $TESTTMP/sedtmp
  $ cp $TESTTMP/sedtmp $HGRCPATH
  $ hg qfinish qbase
  patch add-file2 finalized without changeset message
  $ hg phase 'all()'
  0: draft
  1: secret
  2: secret

Test that secret mq patch does not break hgweb

  $ cat > hgweb.cgi <<HGWEB
  > from mercurial import demandimport; demandimport.enable()
  > from mercurial.hgweb import hgweb
  > from mercurial.hgweb import wsgicgi
  > import cgitb
  > cgitb.enable()
  > app = hgweb('.', 'test')
  > wsgicgi.launch(app)
  > HGWEB
  $ . "$TESTDIR/cgienv"
#if msys
  $ PATH_INFO=//tags; export PATH_INFO
#else
  $ PATH_INFO=/tags; export PATH_INFO
#endif
  $ QUERY_STRING='style=raw'
  $ python hgweb.cgi | grep '^tip'
  tip	[0-9a-f]{40} (re)

  $ cd ..
