  $ extpath=`dirname $TESTDIR`
  $ cp $extpath/show.py $TESTTMP # use $TESTTMP substitution in message
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > show=$TESTTMP/show.py
  > EOF

We assume that log basically works (it has its own tests). This just covers uses
of show that might break even if log works.

Show on empty repository: checking consistency

  $ hg init empty
  $ cd empty
  $ hg show
  changeset:   -1:000000000000
  tag:         tip
  user:        
  date:        Thu Jan 01 00:00:00 1970 +0000
  
  

  $ hg show 1
  abort: unknown revision '1'!
  [255]
  $ hg show 'branch(name)'
  abort: unknown revision 'name'!
  [255]
  $ hg show null -q
  changeset:   -1:000000000000
  tag:         tip
  user:        
  date:        Thu Jan 01 00:00:00 1970 +0000
  
  

Check --stat

  $ hg init stat
  $ cd stat
  $ echo show > x
  $ hg commit -qAm x
  $ hg show --stat
  changeset:   0:852a8d467a01
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       x
  description:
  x
  
  
   x |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)
  
  diff -r 000000000000 -r 852a8d467a01 x
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/x	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +show
  




  $ echo more >> x
  $ hg commit -qAm longer
  $ hg show --stat
  changeset:   1:b73358b94785
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       x
  description:
  longer
  
  
   x |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)
  
  diff -r 852a8d467a01 -r b73358b94785 x
  --- a/x	Thu Jan 01 00:00:00 1970 +0000
  +++ b/x	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +1,2 @@
   show
  +more
  




  $ echo remove > x
  $ hg commit -qAm remove
  $ hg show --stat
  changeset:   2:3d74ea61c11c
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       x
  description:
  remove
  
  
   x |  3 +--
   1 files changed, 1 insertions(+), 2 deletions(-)
  
  diff -r b73358b94785 -r 3d74ea61c11c x
  --- a/x	Thu Jan 01 00:00:00 1970 +0000
  +++ b/x	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,2 +1,1 @@
  -show
  -more
  +remove
  



  $ hg show --stat 0
  changeset:   0:852a8d467a01
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       x
  description:
  x
  
  
   x |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)
  
  diff -r 000000000000 -r 852a8d467a01 x
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/x	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +show
  



Check --git and -g

  $ hg init git
  $ cd git
  $ echo git > file
  $ hg commit -qAm file
  $ hg show --git
  changeset:   0:2a575d662478
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       file
  description:
  file
  
  
  diff --git a/file b/file
  new file mode 100644
  --- /dev/null
  +++ b/file
  @@ -0,0 +1,1 @@
  +git
  


  $ echo more >> file
  $ hg commit -qAm file
  $ hg show -g
  changeset:   1:a23f7b259024
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       file
  description:
  file
  
  
  diff --git a/file b/file
  --- a/file
  +++ b/file
  @@ -1,1 +1,2 @@
   git
  +more
  


  $ hg show -g 0
  changeset:   0:2a575d662478
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       file
  description:
  file
  
  
  diff --git a/file b/file
  new file mode 100644
  --- /dev/null
  +++ b/file
  @@ -0,0 +1,1 @@
  +git
  


Check hg show '' fails to parse the revision

  $ hg show ''
  hg: parse error: empty query
  [255]

Confirm that --help works (it didn't when we used an alias)

  $ hg show --help
  hg show [OPTION]... [REV]
  
  Shows the given revision in detail, or '.' if no revision is given.
  
      This behaves similarly to 'hg log -vp -r [OPTION].. REV', or if called
      without a REV, 'hg log -vp -r [OPTION].. .' Use :hg'log' for more powerful
      operations than supported by hg show
  
      See 'hg help templates' for more about pre-packaged styles and specifying
      custom templates.
  
  (use "hg help -e show" to show help for the show extension)
  
  options ([+] can be repeated):
  
   -g --git                 use git extended diff format
      --stat                output diffstat-style summary of changes
   -T --template TEMPLATE   display with template
   -I --include PATTERN [+] include names matching the given patterns
   -X --exclude PATTERN [+] exclude names matching the given patterns
  
  (some details hidden, use --verbose to show complete help)
  $ hg show --help --verbose
  hg show [OPTION]... [REV]
  
  Shows the given revision in detail, or '.' if no revision is given.
  
      This behaves similarly to 'hg log -vp -r [OPTION].. REV', or if called
      without a REV, 'hg log -vp -r [OPTION].. .' Use :hg'log' for more powerful
      operations than supported by hg show
  
      See 'hg help templates' for more about pre-packaged styles and specifying
      custom templates.
  
  (use "hg help -e show" to show help for the show extension)
  
  options ([+] can be repeated):
  
   -g --git                 use git extended diff format
      --stat                output diffstat-style summary of changes
      --style STYLE         display using template map file (DEPRECATED)
   -T --template TEMPLATE   display with template
   -I --include PATTERN [+] include names matching the given patterns
   -X --exclude PATTERN [+] exclude names matching the given patterns
  
  global options ([+] can be repeated):
  
   -R --repository REPO   repository root directory or name of overlay bundle
                          file
      --cwd DIR           change working directory
   -y --noninteractive    do not prompt, automatically pick the first choice for
                          all prompts
   -q --quiet             suppress output
   -v --verbose           enable additional output
      --config CONFIG [+] set/override config option (use 'section.name=value')
      --debug             enable debugging output
      --debugger          start debugger
      --encoding ENCODE   set the charset encoding (default: ascii)
      --encodingmode MODE set the charset encoding mode (default: strict)
      --traceback         always print a traceback on exception
      --time              time how long the command takes
      --profile           print command execution profile
      --version           output version information and exit
   -h --help              display help and exit
      --hidden            consider hidden changesets
