Set vars:

  $ CONTRIBDIR=$TESTDIR/../contrib

Prepare repo-a:

  $ hg init repo-a
  $ cd repo-a

  $ echo this is file a > a
  $ hg add a
  $ hg commit -m first

  $ echo adding to file a >> a
  $ hg commit -m second

  $ echo adding more to file a >> a
  $ hg commit -m third

  $ hg verify
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  1 files, 3 changesets, 3 total revisions

Dumping revlog of file a to stdout:

  $ python $CONTRIBDIR/dumprevlog .hg/store/data/a.i
  file: .hg/store/data/a.i
  node: 183d2312b35066fb6b3b449b84efc370d50993d0
  linkrev: 0
  parents: 0000000000000000000000000000000000000000 0000000000000000000000000000000000000000
  length: 15
  -start-
  this is file a
  
  -end-
  node: b1047953b6e6b633c0d8197eaa5116fbdfd3095b
  linkrev: 1
  parents: 183d2312b35066fb6b3b449b84efc370d50993d0 0000000000000000000000000000000000000000
  length: 32
  -start-
  this is file a
  adding to file a
  
  -end-
  node: 8c4fd1f7129b8cdec6c7f58bf48fb5237a4030c1
  linkrev: 2
  parents: b1047953b6e6b633c0d8197eaa5116fbdfd3095b 0000000000000000000000000000000000000000
  length: 54
  -start-
  this is file a
  adding to file a
  adding more to file a
  
  -end-

Dump all revlogs to file repo.dump:

  $ find .hg/store -name "*.i" | sort | xargs python $CONTRIBDIR/dumprevlog > ../repo.dump
  $ cd ..

Undumping into repo-b:

  $ hg init repo-b
  $ cd repo-b
  $ python $CONTRIBDIR/undumprevlog < ../repo.dump
  .hg/store/00changelog.i
  .hg/store/00manifest.i
  .hg/store/data/a.i
  $ cd ..

Rebuild fncache with clone --pull:

  $ hg clone --pull -U repo-b repo-c
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 3 changesets with 3 changes to 1 files

Verify:

  $ hg -R repo-c verify
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  1 files, 3 changesets, 3 total revisions

Compare repos:

  $ hg -R repo-c incoming repo-a
  comparing with repo-a
  searching for changes
  no changes found
  [1]

  $ hg -R repo-a incoming repo-c
  comparing with repo-c
  searching for changes
  no changes found
  [1]


Test shrink-revlog:
  $ cd repo-a
  $ hg --config extensions.shrink=$CONTRIBDIR/shrink-revlog.py shrink
  shrinking $TESTTMP/repo-a/.hg/store/00manifest.i
  reading revs
  sorting revs
  writing revs
  old file size:          324 bytes (   0.0 MiB)
  new file size:          324 bytes (   0.0 MiB)
  shrinkage: 0.0% (1.0x)
  note: old revlog saved in:
    $TESTTMP/repo-a/.hg/store/00manifest.i.old
    $TESTTMP/repo-a/.hg/store/00manifest.d.old
  (You can delete those files when you are satisfied that your
  repository is still sane.  Running 'hg verify' is strongly recommended.)
  $ hg verify
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  1 files, 3 changesets, 3 total revisions
  $ cd ..

Test simplemerge command:

  $ cp "$CONTRIBDIR/simplemerge" .
  $ echo base > base
  $ echo local > local
  $ cat base >> local
  $ cp local orig
  $ cat base > other
  $ echo other >> other

changing local directly

  $ python simplemerge local base other && echo "merge succeeded"
  merge succeeded
  $ cat local
  local
  base
  other
  $ cp orig local

printing to stdout

  $ python simplemerge -p local base other
  local
  base
  other

local:

  $ cat local
  local
  base

conflicts

  $ cp base conflict-local
  $ cp other conflict-other
  $ echo not other >> conflict-local
  $ echo end >> conflict-local
  $ echo end >> conflict-other
  $ python simplemerge -p conflict-local base conflict-other
  base
  <<<<<<< conflict-local
  not other
  =======
  other
  >>>>>>> conflict-other
  end
  warning: conflicts during merge.
  [1]

--no-minimal

  $ python simplemerge -p --no-minimal conflict-local base conflict-other
  base
  <<<<<<< conflict-local
  not other
  end
  =======
  other
  end
  >>>>>>> conflict-other
  warning: conflicts during merge.
  [1]

1 label

  $ python simplemerge -p -L foo conflict-local base conflict-other
  base
  <<<<<<< foo
  not other
  =======
  other
  >>>>>>> conflict-other
  end
  warning: conflicts during merge.
  [1]

2 labels

  $ python simplemerge -p -L foo -L bar conflict-local base conflict-other
  base
  <<<<<<< foo
  not other
  =======
  other
  >>>>>>> bar
  end
  warning: conflicts during merge.
  [1]

too many labels

  $ python simplemerge -p -L foo -L bar -L baz conflict-local base conflict-other
  abort: can only specify two labels.
  [255]

binary file

  $ python -c "f = file('binary-local', 'w'); f.write('\x00'); f.close()"
  $ cat orig >> binary-local
  $ python simplemerge -p binary-local base other
  warning: binary-local looks like a binary file.
  [1]

binary file --text

  $ python simplemerge -a -p binary-local base other 2>&1
  warning: binary-local looks like a binary file.
  \x00local (esc)
  base
  other

help

  $ python simplemerge --help
  simplemerge [OPTS] LOCAL BASE OTHER
  
      Simple three-way file merge utility with a minimal feature set.
  
      Apply to LOCAL the changes necessary to go from BASE to OTHER.
  
      By default, LOCAL is overwritten with the results of this operation.
  
  options:
   -L --label       labels to use on conflict markers
   -a --text        treat all files as text
   -p --print       print results instead of overwriting LOCAL
      --no-minimal  do not try to minimize conflict regions
   -h --help        display help and exit
   -q --quiet       suppress output

wrong number of arguments

  $ python simplemerge
  simplemerge: wrong number of arguments
  simplemerge [OPTS] LOCAL BASE OTHER
  
      Simple three-way file merge utility with a minimal feature set.
  
      Apply to LOCAL the changes necessary to go from BASE to OTHER.
  
      By default, LOCAL is overwritten with the results of this operation.
  
  options:
   -L --label       labels to use on conflict markers
   -a --text        treat all files as text
   -p --print       print results instead of overwriting LOCAL
      --no-minimal  do not try to minimize conflict regions
   -h --help        display help and exit
   -q --quiet       suppress output
  [1]

bad option

  $ python simplemerge --foo -p local base other
  simplemerge: option --foo not recognized
  simplemerge [OPTS] LOCAL BASE OTHER
  
      Simple three-way file merge utility with a minimal feature set.
  
      Apply to LOCAL the changes necessary to go from BASE to OTHER.
  
      By default, LOCAL is overwritten with the results of this operation.
  
  options:
   -L --label       labels to use on conflict markers
   -a --text        treat all files as text
   -p --print       print results instead of overwriting LOCAL
      --no-minimal  do not try to minimize conflict regions
   -h --help        display help and exit
   -q --quiet       suppress output
  [1]
