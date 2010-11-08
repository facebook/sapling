
  $ cp "$TESTDIR"/../contrib/simplemerge .
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
  abort: binary-local looks like a binary file.
  [255]

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
