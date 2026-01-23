#require no-eden

Test repoless "cat" command using eagerepo.

  $ eagerepo

Create an eagerepo server with some files:

  $ newserver server
  $ drawdag <<'EOS'
  > B  # B/dir/file = dir content\n
  > |  # B/other = other content\n
  > A  # A/foo = foo content\n
  >    # A/bar = bar content\n
  > EOS
  $ hg book -r $B main

Test repoless cat with full commit hash:

  $ hg cat -R test:server -r $B foo
  foo content

  $ hg cat -R test:server -r $B dir/file
  dir content

Test cat with multiple files:

  $ hg cat -R test:server -r $B foo bar | sort
  bar content
  foo content

  $ hg cat -R test:server -r $B dir/file other | sort
  dir content
  other content

Test cat from earlier commit:

  $ hg cat -R test:server -r $A foo
  foo content

  $ hg cat -R test:server -r $A bar
  bar content

Test cat with short commit hash prefix:

  $ echo $B
  38c22ebcb15088febc0bceaf2da8f0f6dd7bbc52
  $ hg cat -R test:server -r 38c22ebcb1 foo
  foo content

Test cat with bookmark:

  $ LOG=edenapi=debug hg cat -R test:server -r main foo
  foo content

Test cat with file that doesn't exist in commit:

  $ hg cat -R test:server -r $A dir/file
  [1]

Test cat with nonexistent commit:

  $ hg cat -R test:server -r deadbeef1234567890abcdef1234567890abcdef foo
  abort: unknown revision 'deadbeef1234567890abcdef1234567890abcdef'
  [255]

Test cat with nonexistent bookmark:

  $ hg cat -R test:server -r nonexistent foo
  abort: unknown revision 'nonexistent'
  [255]
