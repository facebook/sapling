Set vars:

  $ CONTRIBDIR="$TESTDIR/../contrib"

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

  $ python "$CONTRIBDIR/dumprevlog" .hg/store/data/a.i
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

  $ find .hg/store -name "*.i" | sort | xargs python "$CONTRIBDIR/dumprevlog" > ../repo.dump
  $ cd ..

Undumping into repo-b:

  $ hg init repo-b
  $ cd repo-b
  $ python "$CONTRIBDIR/undumprevlog" < ../repo.dump
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
  [1]

3 labels

  $ python simplemerge -p -L foo -L bar -L base conflict-local base conflict-other
  base
  <<<<<<< foo
  not other
  end
  ||||||| base
  =======
  other
  end
  >>>>>>> bar
  [1]

too many labels

  $ python simplemerge -p -L foo -L bar -L baz -L buz conflict-local base conflict-other
  abort: can only specify three labels.
  [255]

binary file

  $ $PYTHON -c "f = file('binary-local', 'w'); f.write('\x00'); f.close()"
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
      --no-minimal  no effect (DEPRECATED)
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
      --no-minimal  no effect (DEPRECATED)
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
      --no-minimal  no effect (DEPRECATED)
   -h --help        display help and exit
   -q --quiet       suppress output
  [1]
