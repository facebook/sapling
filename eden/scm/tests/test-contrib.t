  $ setconfig extensions.treemanifest=!
Set vars:

  $ CONTRIBDIR="$TESTDIR/../contrib"

Test simplemerge command:

  $ cp "$CONTRIBDIR/simplemerge" .
  $ echo base > base
  $ echo local > local
  $ cat base >> local
  $ cp local orig
  $ cat base > other
  $ echo other >> other

changing local directly

  $ hg debugpython -- simplemerge local base other && echo "merge succeeded"
  merge succeeded
  $ cat local
  local
  base
  other
  $ cp orig local

printing to stdout

  $ hg debugpython -- simplemerge -p local base other
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

  $ hg debugpython -- simplemerge -p conflict-local base conflict-other
  base
  <<<<<<< conflict-local
  not other
  =======
  other
  >>>>>>> conflict-other
  end
  [1]

1 label

  $ hg debugpython -- simplemerge -p -L foo conflict-local base conflict-other
  base
  <<<<<<< foo
  not other
  =======
  other
  >>>>>>> conflict-other
  end
  [1]

2 labels

  $ hg debugpython -- simplemerge -p -L foo -L bar conflict-local base conflict-other
  base
  <<<<<<< foo
  not other
  =======
  other
  >>>>>>> bar
  end
  [1]

3 labels

  $ hg debugpython -- simplemerge -p -L foo -L bar -L base conflict-local base conflict-other
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

  $ hg debugpython -- simplemerge -p -L foo -L bar -L baz -L buz conflict-local base conflict-other
  abort: can only specify three labels.
  [255]

binary file

  $ hg debugpython -- -c "f = file('binary-local', 'w'); f.write('\x00'); f.close()"
  $ cat orig >> binary-local
  $ hg debugpython -- simplemerge -p binary-local base other
  warning: binary-local looks like a binary file.
  [1]

binary file --text

  $ hg debugpython -- simplemerge -a -p binary-local base other 2>&1
  warning: binary-local looks like a binary file.
  \x00local (esc)
  base
  other

help

  $ hg debugpython -- simplemerge --help
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

  $ hg debugpython -- simplemerge
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

  $ hg debugpython -- simplemerge --foo -p local base other
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
