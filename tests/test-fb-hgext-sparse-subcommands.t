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
  
   list           List available sparse profiles
   explain        Show information on individual profiles
   include        include files in the sparse checkout
   exclude        exclude files in the sparse checkout
   delete         delete an include/exclude rule
   enableprofile  enables the specified profile
   disableprofile disables the specified profile
   reset          makes the repo full again
   importrules    Directly import sparse profile rules
   clear          Clear local sparse rules
   refresh        Refreshes the files on disk based on the sparse rules
   cwd            List all names in this directory
  
  (some details hidden, use --verbose to show complete help)

  $ hg help sparse --verbose | subcmds
  subcommands:
  
   list           List available sparse profiles - Show all available sparse
                  profiles, with the active profiles marked.
   explain        Show information on individual profiles
   include        include files in the sparse checkout
   exclude        exclude files in the sparse checkout
   delete         delete an include/exclude rule
   enableprofile  enables the specified profile
   disableprofile disables the specified profile
   reset          makes the repo full again
   importrules    Directly import sparse profile rules - Accepts a path to a
                  file containing rules in the .hgsparse format.  This allows
                  you to add *include*, *exclude* and *enable* rules in bulk.
                  Like the include, exclude and enable subcommands, the changes
                  are applied immediately.
   clear          Clear local sparse rules - Removes all local include and
                  exclude rules, while leaving any enabled profiles in place.
   refresh        Refreshes the files on disk based on the sparse rules - This
                  is only necessary if .hg/sparse was changed by hand.
   cwd            List all names in this directory - The list includes any names
                  that are excluded by the current sparse checkout; these are
                  annotated with a hyphen ('-') before the name.

  $ hg sparse --error-nonesuch | subcmds
  hg sparse: option --error-nonesuch not recognized
  subcommands:
  
   list           List available sparse profiles
   explain        Show information on individual profiles
   include        include files in the sparse checkout
   exclude        exclude files in the sparse checkout
   delete         delete an include/exclude rule
   enableprofile  enables the specified profile
   disableprofile disables the specified profile
   reset          makes the repo full again
   importrules    Directly import sparse profile rules
   clear          Clear local sparse rules
   refresh        Refreshes the files on disk based on the sparse rules
   cwd            List all names in this directory
  
  (use 'hg sparse -h' to show more help)

  $ hg sparse --verbose --error-nonesuch | subcmds
  hg sparse: option --error-nonesuch not recognized
  subcommands:
  
   list           List available sparse profiles
   explain        Show information on individual profiles
   include        include files in the sparse checkout
   exclude        exclude files in the sparse checkout
   delete         delete an include/exclude rule
   enableprofile  enables the specified profile
   disableprofile disables the specified profile
   reset          makes the repo full again
   importrules    Directly import sparse profile rules
   clear          Clear local sparse rules
   refresh        Refreshes the files on disk based on the sparse rules
   cwd            List all names in this directory
  
  (use 'hg sparse -h' to show more help)
