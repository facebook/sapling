#require no-eden

  $ setconfig drawdag.defaultfiles=false

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

  $ SHORT_B=$(echo $B | python -c "import sys; print(sys.stdin.read()[:12], end='')")
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
  e852cc83929aae9b8d3b025c327dbbc858924676

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

Test --output with absolute path:

  $ rm -rf output
  $ hg cat -R test:server -r $B --output "$TESTTMP/output/%s" foo
  $ cat "$TESTTMP/output/foo"
  foo content

Test --output with %% in directory name:

  $ rm -rf output
  $ hg cat -R test:server -r $B --output 'output/%%dir%%/%s' foo
  $ cat 'output/%dir%/foo'
  foo content

Test --output with deeper path (multiple components):

  $ rm -rf output
  $ hg cat -R test:server -r $B --output 'output/a/b/c/%s' foo
  $ cat output/a/b/c/foo
  foo content

  $ rm -rf output
  $ hg cat -R test:server -r $B --output 'output/%h/files/%p' dir/file
  $ cat output/${SHORT_B}/files/dir/file
  dir content

Test we don't blow away existing directories:

  $ rm -rf output
  $ mkdir -p output/precious
  $ echo "precious content" > output/precious/file
  $ hg cat -R test:server -r $B --output output/precious dir/file
  abort: can't clear conflicts after handling error "failed to open file `*precious`: *": cannot write to "*precious": conflicting directory exists at "*precious" (glob)
  [255]
  $ cat output/precious/file
  precious content

#if no-windows

Test cat --output preserves executable and symlink metadata:

  $ newserver server2
  $ drawdag <<'EOS'
  > A  # A/script = #!/bin/sh\necho hello\n (executable)
  >    # A/link = script (symlink)
  >    # A/normal = normal content\n
  > EOS

  $ rm -rf output
  $ hg cat -R test:server2 -r $A --output 'output/%p' script link normal
  $ ls -l output
  l* link -> script (glob)
  -rw-r--r-- normal
  -rwxr-xr-x script

#endif

Test cat handles identical files with different paths:

  $ newserver server-identical
  $ drawdag <<'EOS'
  > A  # A/one = content\n
  >    # A/two = content\n
  > EOS

  $ rm -rf output
  $ hg cat -R test:server-identical -r $A --output 'output/%p'
  $ ls output | sort
  one
  two
  $ cat output/one
  content
  $ cat output/two
  content

Test code tenting (sparse profile filtering):

  $ newserver server3
  $ drawdag <<'EOS'
  > A  # A/allowed/file1 = allowed file 1\n
  >    # A/allowed/file2 = allowed file 2\n
  >    # A/blocked/secret = secret content\n
  >    # A/other = other content\n
  >    # A/sparse/profile = [metadata]\ntitle: frontend sparse profile\n[include]\nallowed\n
  > EOS

Cat without sparse profile shows all files:

  $ hg cat -R test:server3 -r $A allowed/file1 blocked/secret | sort
  allowed file 1
  secret content

Cat with sparse profile filters to allowed paths:

  $ hg cat -R test:server3 -r $A --config clone.eden-sparse-filter.foo=sparse/profile allowed/file1
  allowed file 1

  $ hg cat -R test:server3 -r $A --config clone.eden-sparse-filter.foo=sparse/profile blocked/secret
  [1]

  $ rm -rf output
  $ hg cat -R test:server3 -r $A --config clone.eden-sparse-filter.foo=sparse/profile --output 'output/%p' 'glob:**'
  $ find output -type f | sort
  output/allowed/file1
  output/allowed/file2

  $ rm -rf output
Empty profile should allow everything:
  $ hg cat -R test:server3 -r $A --config clone.eden-sparse-filter.foo= --output 'output/%p' path:
  $ find output -type f | sort
  output/allowed/file1
  output/allowed/file2
  output/blocked/secret
  output/other
  output/sparse/profile


#if no-windows

Test pager is used for stdout output:

  $ cat >> $TESTTMP/fakepager.py <<EOF
  > import sys
  > # Write to a file to work around pager output going to "real" stdout.
  > with open("paged", "w") as f:
  >   for line in sys.stdin.buffer:
  >       f.write('paged! %r\n' % line.decode())
  > EOF

  $ hg cat -R test:server -r $B foo --pager=true --config 'pager.pager=hg debugpython $TESTTMP/fakepager.py'
  $ cat paged
  paged! 'foo content\n'

#endif
