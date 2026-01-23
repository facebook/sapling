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
  $ SHORT_B=38c22ebcb150
  $ hg cat -R test:server -r $SHORT_B foo
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

Test cat --output with format specifiers:

  $ mkdir output
  $ hg cat -R test:server -r $B --output 'output/%s' foo dir/file
  $ cat output/foo
  foo content
  $ cat output/file
  dir content

Test --output with %p (full path):

  $ rm -rf output
  $ hg cat -R test:server -r $B --output 'output/%p' dir/file
  $ cat output/dir/file
  dir content

Test --output with %d (dirname):

  $ rm -rf output
  $ hg cat -R test:server -r $B --output 'output/%d_file' dir/file
  $ cat output/dir_file
  dir content

Test --output with %H (full hash):

  $ rm -rf output
  $ hg cat -R test:server -r $B --output 'output/%H' foo
  $ ls output
  38c22ebcb15088febc0bceaf2da8f0f6dd7bbc52

Test --output with %h (short hash):

  $ rm -rf output
  $ hg cat -R test:server -r $SHORT_B --output 'output/%h_%s' foo
  $ cat output/${SHORT_B}_foo
  foo content

Test --output with %% (literal percent):

  $ rm -rf output
  $ hg cat -R test:server -r $B --output 'output/%%test' foo
  $ cat 'output/%test'
  foo content
