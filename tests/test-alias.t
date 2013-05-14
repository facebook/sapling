  $ HGFOO=BAR; export HGFOO
  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > graphlog=
  > 
  > [alias]
  > # should clobber ci but not commit (issue2993)
  > ci = version
  > myinit = init
  > optionalrepo = showconfig alias.myinit
  > cleanstatus = status -c
  > unknown = bargle
  > ambiguous = s
  > recursive = recursive
  > nodefinition =
  > no--cwd = status --cwd elsewhere
  > no-R = status -R elsewhere
  > no--repo = status --repo elsewhere
  > no--repository = status --repository elsewhere
  > no--config = status --config a.config=1
  > mylog = log
  > lognull = log -r null
  > shortlog = log --template '{rev} {node|short} | {date|isodate}\n'
  > positional = log --template '{\$2} {\$1} | {date|isodate}\n'
  > dln = lognull --debug
  > nousage = rollback
  > put = export -r 0 -o "\$FOO/%R.diff"
  > blank = !printf '\n'
  > self = !printf '\$0\n'
  > echoall = !printf '\$@\n'
  > echo1 = !printf '\$1\n'
  > echo2 = !printf '\$2\n'
  > echo13 = !printf '\$1 \$3\n'
  > count = !hg log -r "\$@" --template=. | wc -c | sed -e 's/ //g'
  > mcount = !hg log \$@ --template=. | wc -c | sed -e 's/ //g'
  > rt = root
  > tglog = glog --template "{rev}:{node|short}: '{desc}' {branches}\n"
  > idalias = id
  > idaliaslong = id
  > idaliasshell = !echo test
  > parentsshell1 = !echo one
  > parentsshell2 = !echo two
  > escaped1 = !printf 'test\$\$test\n'
  > escaped2 = !sh -c 'echo "HGFOO is \$\$HGFOO"'
  > escaped3 = !sh -c 'echo "\$1 is \$\$\$1"'
  > escaped4 = !printf '\$\$0 \$\$@\n'
  > 
  > [defaults]
  > mylog = -q
  > lognull = -q
  > log = -v
  > EOF


basic

  $ hg myinit alias


unknown

  $ hg unknown
  alias 'unknown' resolves to unknown command 'bargle'
  $ hg help unknown
  alias 'unknown' resolves to unknown command 'bargle'


ambiguous

  $ hg ambiguous
  alias 'ambiguous' resolves to ambiguous command 's'
  $ hg help ambiguous
  alias 'ambiguous' resolves to ambiguous command 's'


recursive

  $ hg recursive
  alias 'recursive' resolves to unknown command 'recursive'
  $ hg help recursive
  alias 'recursive' resolves to unknown command 'recursive'


no definition

  $ hg nodef
  no definition for alias 'nodefinition'
  $ hg help nodef
  no definition for alias 'nodefinition'


invalid options

  $ hg no--cwd
  error in definition for alias 'no--cwd': --cwd may only be given on the command line
  $ hg help no--cwd
  error in definition for alias 'no--cwd': --cwd may only be given on the command line
  $ hg no-R
  error in definition for alias 'no-R': -R may only be given on the command line
  $ hg help no-R
  error in definition for alias 'no-R': -R may only be given on the command line
  $ hg no--repo
  error in definition for alias 'no--repo': --repo may only be given on the command line
  $ hg help no--repo
  error in definition for alias 'no--repo': --repo may only be given on the command line
  $ hg no--repository
  error in definition for alias 'no--repository': --repository may only be given on the command line
  $ hg help no--repository
  error in definition for alias 'no--repository': --repository may only be given on the command line
  $ hg no--config
  error in definition for alias 'no--config': --config may only be given on the command line

optional repository

#if no-outer-repo
  $ hg optionalrepo
  init
#endif
  $ cd alias
  $ cat > .hg/hgrc <<EOF
  > [alias]
  > myinit = init -q
  > EOF
  $ hg optionalrepo
  init -q

no usage

  $ hg nousage
  no rollback information available

  $ echo foo > foo
  $ hg commit -Amfoo
  adding foo


with opts

  $ hg cleanst
  C foo


with opts and whitespace

  $ hg shortlog
  0 e63c23eaa88a | 1970-01-01 00:00 +0000

positional arguments

  $ hg positional
  abort: too few arguments for command alias
  [255]
  $ hg positional a
  abort: too few arguments for command alias
  [255]
  $ hg positional 'node|short' rev
  0 e63c23eaa88a | 1970-01-01 00:00 +0000

interaction with defaults

  $ hg mylog
  0:e63c23eaa88a
  $ hg lognull
  -1:000000000000


properly recursive

  $ hg dln
  changeset:   -1:0000000000000000000000000000000000000000
  parent:      -1:0000000000000000000000000000000000000000
  parent:      -1:0000000000000000000000000000000000000000
  manifest:    -1:0000000000000000000000000000000000000000
  user:        
  date:        Thu Jan 01 00:00:00 1970 +0000
  extra:       branch=default
  


path expanding

  $ FOO=`pwd` hg put
  $ cat 0.diff
  # HG changeset patch
  # User test
  # Date 0 0
  #      Thu Jan 01 00:00:00 1970 +0000
  # Node ID e63c23eaa88ae77967edcf4ea194d31167c478b0
  # Parent  0000000000000000000000000000000000000000
  foo
  
  diff -r 000000000000 -r e63c23eaa88a foo
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/foo	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +foo


simple shell aliases

  $ hg blank
  
  $ hg blank foo
  
  $ hg self
  self
  $ hg echoall
  
  $ hg echoall foo
  foo
  $ hg echoall 'test $2' foo
  test $2 foo
  $ hg echo1 foo bar baz
  foo
  $ hg echo2 foo bar baz
  bar
  $ hg echo13 foo bar baz test
  foo baz
  $ hg echo2 foo
  
  $ echo bar > bar
  $ hg commit -qA -m bar
  $ hg count .
  1
  $ hg count 'branch(default)'
  2
  $ hg mcount -r '"branch(default)"'
  2

  $ hg tglog
  @  1:042423737847: 'bar'
  |
  o  0:e63c23eaa88a: 'foo'
  


shadowing

  $ hg i
  hg: command 'i' is ambiguous:
      idalias idaliaslong idaliasshell identify import incoming init
  [255]
  $ hg id
  042423737847 tip
  $ hg ida
  hg: command 'ida' is ambiguous:
      idalias idaliaslong idaliasshell
  [255]
  $ hg idalias
  042423737847 tip
  $ hg idaliasl
  042423737847 tip
  $ hg idaliass
  test
  $ hg parentsshell
  hg: command 'parentsshell' is ambiguous:
      parentsshell1 parentsshell2
  [255]
  $ hg parentsshell1
  one
  $ hg parentsshell2
  two


shell aliases with global options

  $ hg init sub
  $ cd sub
  $ hg count 'branch(default)'
  abort: unknown revision 'default'!
  0
  $ hg -v count 'branch(default)'
  abort: unknown revision 'default'!
  0
  $ hg -R .. count 'branch(default)'
  abort: unknown revision 'default'!
  0
  $ hg --cwd .. count 'branch(default)'
  2
  $ hg echoall --cwd ..
  


repo specific shell aliases

  $ cat >> .hg/hgrc <<EOF
  > [alias]
  > subalias = !echo sub
  > EOF
  $ cat >> ../.hg/hgrc <<EOF
  > [alias]
  > mainalias = !echo main
  > EOF


shell alias defined in current repo

  $ hg subalias
  sub
  $ hg --cwd .. subalias > /dev/null
  hg: unknown command 'subalias'
  [255]
  $ hg -R .. subalias > /dev/null
  hg: unknown command 'subalias'
  [255]


shell alias defined in other repo

  $ hg mainalias > /dev/null
  hg: unknown command 'mainalias'
  [255]
  $ hg -R .. mainalias
  main
  $ hg --cwd .. mainalias
  main


shell aliases with escaped $ chars

  $ hg escaped1
  test$test
  $ hg escaped2
  HGFOO is BAR
  $ hg escaped3 HGFOO
  HGFOO is BAR
  $ hg escaped4 test
  $0 $@


invalid arguments

  $ hg rt foo
  hg rt: invalid arguments
  hg rt
  
  alias for: hg root
  
  use "hg help rt" to show the full help text
  [255]

invalid global arguments for normal commands, aliases, and shell aliases

  $ hg --invalid root
  hg: option --invalid not recognized
  Mercurial Distributed SCM
  
  basic commands:
  
   add           add the specified files on the next commit
   annotate      show changeset information by line for each file
   clone         make a copy of an existing repository
   commit        commit the specified files or all outstanding changes
   diff          diff repository (or selected files)
   export        dump the header and diffs for one or more changesets
   forget        forget the specified files on the next commit
   init          create a new repository in the given directory
   log           show revision history of entire repository or files
   merge         merge working directory with another revision
   pull          pull changes from the specified source
   push          push changes to the specified destination
   remove        remove the specified files on the next commit
   serve         start stand-alone webserver
   status        show changed files in the working directory
   summary       summarize working directory state
   update        update working directory (or switch revisions)
  
  use "hg help" for the full list of commands or "hg -v" for details
  [255]
  $ hg --invalid mylog
  hg: option --invalid not recognized
  Mercurial Distributed SCM
  
  basic commands:
  
   add           add the specified files on the next commit
   annotate      show changeset information by line for each file
   clone         make a copy of an existing repository
   commit        commit the specified files or all outstanding changes
   diff          diff repository (or selected files)
   export        dump the header and diffs for one or more changesets
   forget        forget the specified files on the next commit
   init          create a new repository in the given directory
   log           show revision history of entire repository or files
   merge         merge working directory with another revision
   pull          pull changes from the specified source
   push          push changes to the specified destination
   remove        remove the specified files on the next commit
   serve         start stand-alone webserver
   status        show changed files in the working directory
   summary       summarize working directory state
   update        update working directory (or switch revisions)
  
  use "hg help" for the full list of commands or "hg -v" for details
  [255]
  $ hg --invalid blank
  hg: option --invalid not recognized
  Mercurial Distributed SCM
  
  basic commands:
  
   add           add the specified files on the next commit
   annotate      show changeset information by line for each file
   clone         make a copy of an existing repository
   commit        commit the specified files or all outstanding changes
   diff          diff repository (or selected files)
   export        dump the header and diffs for one or more changesets
   forget        forget the specified files on the next commit
   init          create a new repository in the given directory
   log           show revision history of entire repository or files
   merge         merge working directory with another revision
   pull          pull changes from the specified source
   push          push changes to the specified destination
   remove        remove the specified files on the next commit
   serve         start stand-alone webserver
   status        show changed files in the working directory
   summary       summarize working directory state
   update        update working directory (or switch revisions)
  
  use "hg help" for the full list of commands or "hg -v" for details
  [255]

This should show id:

  $ hg --config alias.log='id' log
  000000000000 tip

This shouldn't:

  $ hg --config alias.log='id' history

  $ cd ../..
