test sparse subcommands (help, verbose)

  $ cat > $TESTTMP/subcommands.py <<EOF
  > from __future__ import absolute_import, print_function
  > import sys
  > seen = False
  > for line in sys.stdin:
  >     if 'subcommands:' in line:
  >         seen = True
  >     if seen:
  >         print(line, end='')
  > EOF
  $ subcmds () {
  >   $PYTHON $TESTTMP/subcommands.py
  > }

  $ hg init myrepo
  $ cd myrepo
  $ cat > .hg/hgrc <<EOF
  > [extensions]
  > sparse=$TESTDIR/../hgext/fbsparse.py
  > EOF

  $ hg help sparse | subcmds
  subcommands:
  
   list List available sparse profiles
  
  (some details hidden, use --verbose to show complete help)

  $ hg help sparse --verbose | subcmds
  subcommands:
  
   list List available sparse profiles - Show all available sparse profiles,
        with the active profiles marked.

  $ hg sparse --error-nonesuch | subcmds
  hg sparse: option --error-nonesuch not recognized
  subcommands:
  
   list List available sparse profiles
  
  (use 'hg sparse -h' to show more help)

  $ hg sparse --verbose --error-nonesuch | subcmds
  hg sparse: option --error-nonesuch not recognized
  subcommands:
  
   list List available sparse profiles
  
  (use 'hg sparse -h' to show more help)
