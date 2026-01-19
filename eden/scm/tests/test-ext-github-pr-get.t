#require no-eden

#inprocess-hg-incompatible

  $ eagerepo
  $ enable github

Test help output for `sl pr get`

  $ hg help pr get
  hg pr get PULL_REQUEST
  
  import an entire PR stack into your working copy
  
      The PULL_REQUEST can be specified as either a URL:
      'https://github.com/facebook/sapling/pull/321' or just the PR number
      within the GitHub repository identified by 'sl config paths.default'.
  
      Unlike 'sl pr pull' which imports only a single PR, this command discovers
      and imports the entire stack of PRs based on the stack information in the
      PR body.
  
      Use --downstack to fetch only PRs from the target towards trunk, skipping
      any upstack (descendant) PRs.
  
  Options:
  
   -g --goto      goto the target pull request after importing the stack
      --downstack only fetch PRs from target towards trunk (skip upstack)
  
  (some details hidden, use --verbose to show complete help)

Test error without argument

  $ hg init repo
  $ cd repo
  $ echo a > a1
  $ hg ci -Am addfile
  adding a1

  $ hg pr get
  abort: PR URL or number must be specified. See 'hg pr get -h'.
  [255]

Test error for non-github repo

  $ hg pr get 123
  abort: This does not appear to be a GitHub repo
  [255]

Test pr get appears in pr subcommand list

  $ hg help pr | grep "get"
      get         import an entire PR stack into your working copy

  $ cd ..
