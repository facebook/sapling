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
   files          List all files included in a profiles
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
  
  (Use hg help sparse [subcommand] to show complete subcommand help)
  
  (some details hidden, use --verbose to show complete help)

  $ hg help sparse --verbose | subcmds
  subcommands:
  
   list [OPTION]...                  List available sparse profiles - Show all
                                     available sparse profiles, with the active
                                     profiles marked.  You can filter profiles
                                     with `--with-field [FIELD]` and `--without-
                                     field [FIELD]`; you can specify these
                                     options more than once to set multiple
                                     criteria, which all must match for a
                                     profile to be listed.  By default,
                                     `--without-field hidden` is implied unless
                                     you use the --verbose switch to include
                                     hidden profiles.
   explain [OPTION]... [PROFILE]...  Show information on individual profiles -
                                     If --verbose is given, calculates the file
                                     size impact of a profile (slow).
   files [OPTION]...                 List all files included in a profiles - If
                                     files are given to match, this command only
                                     prints the names of the files in a profile
                                     that match those patterns.
   include [RULE]...                 include files in the sparse checkout - The
                                     effects of adding or deleting an include or
                                     exclude rule are applied immediately. If
                                     applying the new rule would cause a file
                                     with pending changes to be added or
                                     removed, the command will fail. Pass
                                     --force to force a rule change even with
                                     pending changes (the changes on disk will
                                     be preserved).
   exclude [RULE]...                 exclude files in the sparse checkout - The
                                     effects of adding or deleting an include or
                                     exclude rule are applied immediately. If
                                     applying the new rule would cause a file
                                     with pending changes to be added or
                                     removed, the command will fail. Pass
                                     --force to force a rule change even with
                                     pending changes (the changes on disk will
                                     be preserved).
   delete [RULE]...                  delete an include/exclude rule - The
                                     effects of adding or deleting an include or
                                     exclude rule are applied immediately. If
                                     applying the new rule would cause a file
                                     with pending changes to be added or
                                     removed, the command will fail. Pass
                                     --force to force a rule change even with
                                     pending changes (the changes on disk will
                                     be preserved).
   enableprofile                     enables the specified profile
   disableprofile                    disables the specified profile
   reset                             makes the repo full again
   importrules [OPTION]... [FILE]... Directly import sparse profile rules -
                                     Accepts a path to a file containing rules
                                     in the .hgsparse format.  This allows you
                                     to add *include*, *exclude* and *enable*
                                     rules in bulk. Like the include, exclude
                                     and enable subcommands, the changes are
                                     applied immediately.
   clear [OPTION]...                 Clear local sparse rules - Removes all
                                     local include and exclude rules, while
                                     leaving any enabled profiles in place.
   refresh [OPTION]...               Refreshes the files on disk based on the
                                     sparse rules - This is only necessary if
                                     .hg/sparse was changed by hand.
   cwd                               List all names in this directory - The list
                                     includes any names that are excluded by the
                                     current sparse checkout; these are
                                     annotated with a hyphen ('-') before the
                                     name.
  
  (Use hg help sparse [subcommand] to show complete subcommand help)

  $ hg sparse --error-nonesuch | subcmds
  hg sparse: option --error-nonesuch not recognized
  subcommands:
  
   list           List available sparse profiles
   explain        Show information on individual profiles
   files          List all files included in a profiles
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
  
  (Use hg help sparse [subcommand] to show complete subcommand help)
  
  (use 'hg sparse -h' to show more help)

  $ hg sparse --verbose --error-nonesuch | subcmds
  hg sparse: option --error-nonesuch not recognized
  subcommands:
  
   list           List available sparse profiles
   explain        Show information on individual profiles
   files          List all files included in a profiles
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
  
  (Use hg help sparse [subcommand] to show complete subcommand help)
  
  (use 'hg sparse -h' to show more help)

  $ hg help sparse list
  hg sparse list [OPTION]...
  
  List available sparse profiles
  
      Show all available sparse profiles, with the active profiles marked.
  
      You can filter profiles with '--with-field [FIELD]' and '--without-field
      [FIELD]'; you can specify these options more than once to set multiple
      criteria, which all must match for a profile to be listed.
  
      By default, '--without-field hidden' is implied unless you use the
      --verbose switch to include hidden profiles.
  
  options ([+] can be repeated):
  
   -r --rev REV                 explain the profile(s) against the specified
                                revision
      --with-field FIELD [+]    Only show profiles that have defined the named
                                metadata field
      --without-field FIELD [+] Only show profiles that do have not defined the
                                named metadata field
   -T --template TEMPLATE       display with template
  
  subcommands:
  
   list           List available sparse profiles
   explain        Show information on individual profiles
   files          List all files included in a profiles
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
  
  (Use hg help sparse [subcommand] to show complete subcommand help)
  
  (some details hidden, use --verbose to show complete help)

  $ hg help sparse explain
  hg sparse explain [OPTION]... [PROFILE]...
  
  Show information on individual profiles
  
      If --verbose is given, calculates the file size impact of a profile
      (slow).
  
  options:
  
   -r --rev REV           explain the profile(s) against the specified revision
   -T --template TEMPLATE display with template
  
  subcommands:
  
   list           List available sparse profiles
   explain        Show information on individual profiles
   files          List all files included in a profiles
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
  
  (Use hg help sparse [subcommand] to show complete subcommand help)
  
  (some details hidden, use --verbose to show complete help)
  $ hg sparse explain --nonesuch
  hg sparse explain: option --nonesuch not recognized
  hg sparse explain [OPTION]... [PROFILE]...
  
  Show information on individual profiles
  
  options:
  
   -r --rev REV           explain the profile(s) against the specified revision
   -T --template TEMPLATE display with template
  
  subcommands:
  
   list           List available sparse profiles
   explain        Show information on individual profiles
   files          List all files included in a profiles
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
  
  (Use hg help sparse [subcommand] to show complete subcommand help)
  
  (use 'hg sparse explain -h' to show more help)
  [255]

