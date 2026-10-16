
#require no-eden


  $ eagerepo
  $ newext testcommands <<EOF
  > from sapling import registrar
  > cmdtable = {}
  > command = registrar.command(cmdtable)
  > @command('test', [], 'sl test SUBCOMMAND', subonly=True)
  > def test(ui, repo):
  >     """test command"""
  >     ui.status("test command called (should not happen)\n")
  > subcmd = test.subcommand(categories=[("First Category", ["one"])])
  > @subcmd('one', [])
  > def testone(ui, repo):
  >     """first test subcommand"""
  >     ui.status("test subcommand one called\n")
  > @subcmd('two', [])
  > def testone(ui, repo):
  >     """second test subcommand"""
  >     ui.status("test subcommand two called\n")
  > @command('othertest', [], 'sl othertest [SUBCOMMAND]')
  > def othertest(ui, repo, parameter):
  >     """other test command"""
  >     ui.status("other test command called with '%s'\n" % parameter)
  > othersubcmd = othertest.subcommand()
  > @othersubcmd('alpha|alfa', [])
  > def othertestalpha(ui, repo, parameter):
  >     """other test subcommand alpha"""
  >     ui.status("other test command alpha called with '%s'\n" % parameter)
  > nestedsubcmd = othertestalpha.subcommand()
  > @nestedsubcmd('beta', [])
  > def othertestalphabeta(ui, repo):
  >     """other test subcommand alpha subcommand beta"""
  >     ui.status("other test command alpha/beta called\n")
  > EOF

  $ readconfig <<EOF
  > [alias]
  > xt = test
  > xt1 = test one
  > xt0 = test nonexistent
  > yt = othertest
  > yta = othertest alpha
  > ytf = othertest foo
  > EOF

  $ sl init

  $ sl test
  sl test: subcommand required
  [255]


  $ sl test one
  test subcommand one called
  $ sl test two
  test subcommand two called
  $ sl test nonexistent
  sl test: unknown subcommand 'nonexistent'
  [255]


  $ sl tes o
  unknown command 'tes'
  (use 'sl help' to get help)
  [255]

  $ sl xt
  sl test: subcommand required
  [255]


  $ sl xt one
  test subcommand one called
  $ sl xt too
  sl test: unknown subcommand 'too'
  [255]
  $ sl xt1
  test subcommand one called

  $ sl xt0
  sl test: unknown subcommand 'nonexistent'
  [255]

  $ sl othertest
  sl othertest: invalid arguments
  (use 'sl othertest -h' to get help)
  [255]
  $ sl othertest foo
  other test command called with 'foo'
  $ sl othertest alpha
  sl othertest alpha: invalid arguments
  (use 'sl othertest alpha -h' to get help)
  [255]
  $ sl othertest alfa foo
  other test command alpha called with 'foo'
  $ sl othertest alpha beta
  other test command alpha/beta called
  $ sl yt
  sl othertest: invalid arguments
  (use 'sl othertest -h' to get help)
  [255]
  $ sl yta foo
  other test command alpha called with 'foo'
  $ sl ytf
  other test command called with 'foo'

  $ sl help test
  sl test SUBCOMMAND
  
  test command
  
  First Category:
  
   one           first test subcommand
  
  Other Subcommands:
  
   two           second test subcommand
  
  (use 'sl help test SUBCOMMAND' to show complete subcommand help)
  
  (some details hidden, use --verbose to show complete help)


  $ sl help test --quiet
  sl test SUBCOMMAND
  
  test command
  
  First Category:
  
   one           first test subcommand
  
  Other Subcommands:
  
   two           second test subcommand


  $ sl help test one
  sl test one
  
  first test subcommand
  
  (some details hidden, use --verbose to show complete help)


  $ sl help test one --quiet
  sl test one
  
  first test subcommand

  $ sl help test two --verbose
  sl test two
  
  second test subcommand
  
  Global options ([+] can be repeated):
  
   -R --repository REPO       repository root directory or name of overlay
                              bundle file
      --cwd DIR               change working directory
   -y --noninteractive        do not prompt, use the default answer for all
                              prompts
   -q --quiet                 suppress output
   -v --verbose               enable additional output
      --color TYPE            when to colorize (boolean, always, auto, never, or
                              debug)
      --config CONFIG [+]     set/override config option (use
                              'section.name=value')
      --configfile FILE [+]   enables the given config file
      --debug                 enable debugging output
      --debugger              start debugger
      --encoding ENCODE       set the charset encoding (default: utf-8)
      --encodingmode MODE     set the charset encoding mode (default: strict)
      --insecure              do not verify server certificate
      --outputencoding ENCODE set the output encoding (default: utf-8)
      --traceback             always print a traceback on exception
      --trace                 enable more detailed tracing
      --time                  time how long the command takes
      --profile               print command execution profile
      --version               output version information and exit
   -h --help                  display help and exit
      --hidden                consider hidden changesets
      --pager TYPE            when to paginate (boolean, always, auto, or never)
                              (default: auto)
      --reason VALUE [+]      why this runs, usually set by automation
                              (ADVANCED)



  $ sl help test nonexistent
  abort: 'test' has no such subcommand: nonexistent
  (run 'sl help test' to see available subcommands)
  [255]
  $ sl othertest --help --verbose
  sl othertest [SUBCOMMAND]
  
  other test command
  
  Global options ([+] can be repeated):
  
   -R --repository REPO       repository root directory or name of overlay
                              bundle file
      --cwd DIR               change working directory
   -y --noninteractive        do not prompt, use the default answer for all
                              prompts
   -q --quiet                 suppress output
   -v --verbose               enable additional output
      --color TYPE            when to colorize (boolean, always, auto, never, or
                              debug)
      --config CONFIG [+]     set/override config option (use
                              'section.name=value')
      --configfile FILE [+]   enables the given config file
      --debug                 enable debugging output
      --debugger              start debugger
      --encoding ENCODE       set the charset encoding (default: utf-8)
      --encodingmode MODE     set the charset encoding mode (default: strict)
      --insecure              do not verify server certificate
      --outputencoding ENCODE set the output encoding (default: utf-8)
      --traceback             always print a traceback on exception
      --trace                 enable more detailed tracing
      --time                  time how long the command takes
      --profile               print command execution profile
      --version               output version information and exit
   -h --help                  display help and exit
      --hidden                consider hidden changesets
      --pager TYPE            when to paginate (boolean, always, auto, or never)
                              (default: auto)
      --reason VALUE [+]      why this runs, usually set by automation
                              (ADVANCED)
  
  Subcommands:
  
   alpha, alfa   other test subcommand alpha
  
  (use 'sl help othertest SUBCOMMAND' to show complete subcommand help)


  $ sl help xt
  alias for: test
  
  sl test SUBCOMMAND
  
  test command
  
  First Category:
  
   one           first test subcommand
  
  Other Subcommands:
  
   two           second test subcommand
  
  (use 'sl help test SUBCOMMAND' to show complete subcommand help)
  
  (some details hidden, use --verbose to show complete help)


  $ sl help xt one
  alias for: test one
  
  sl test one
  
  first test subcommand
  
  (some details hidden, use --verbose to show complete help)


  $ sl help xt1
  alias for: test one
  
  sl test one
  
  first test subcommand
  
  (some details hidden, use --verbose to show complete help)


  $ sl othertest alpha beta --help
  sl othertest alpha beta
  
  other test subcommand alpha subcommand beta
  
  (some details hidden, use --verbose to show complete help)


