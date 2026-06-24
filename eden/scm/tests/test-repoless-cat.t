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
  $ sl book -r $B main

Test repoless cat with full commit hash:

  $ sl cat -R test:server -r $B foo
  foo content

  $ sl cat -R test:server -r $B dir/file
  dir content

Test cat with multiple files:

  $ sl cat -R test:server -r $B foo bar | sort
  bar content
  foo content

  $ sl cat -R test:server -r $B dir/file other | sort
  dir content
  other content

Test cat from earlier commit:

  $ sl cat -R test:server -r $A foo
  foo content

  $ sl cat -R test:server -r $A bar
  bar content

Test cat with short commit hash prefix:

  $ SHORT_B=$(echo $B | python -c "import sys; print(sys.stdin.read()[:12], end='')")
  $ sl cat -R test:server -r $SHORT_B foo
  foo content

Test cat with bookmark:

  $ LOG=edenapi=debug sl cat -R test:server -r main foo
  foo content

Test cat with file that doesn't exist in commit:

  $ sl cat -R test:server -r $A dir/file
  [1]

Test cat with nonexistent commit:

  $ sl cat -R test:server -r deadbeef1234567890abcdef1234567890abcdef foo
  abort: unknown revision 'deadbeef1234567890abcdef1234567890abcdef'
  [255]

Test cat with nonexistent bookmark:

  $ sl cat -R test:server -r nonexistent foo
  abort: unknown revision 'nonexistent'
  [255]

Test cat --output with format specifiers:

  $ mkdir output
  $ sl cat -R test:server -r $B --output 'output/%s' foo dir/file
  $ cat output/foo
  foo content
  $ cat output/file
  dir content

Test --output with %p (full path):

  $ rm -rf output
  $ sl cat -R test:server -r $B --output 'output/%p' dir/file
  $ cat output/dir/file
  dir content

Test --output with %d (dirname):

  $ rm -rf output
  $ sl cat -R test:server -r $B --output 'output/%d_file' dir/file
  $ cat output/dir_file
  dir content

Test --output with %H (full hash):

  $ rm -rf output
  $ sl cat -R test:server -r $B --output 'output/%H' foo
  $ ls output
  e852cc83929aae9b8d3b025c327dbbc858924676

Test --output with %h (short hash):

  $ rm -rf output
  $ sl cat -R test:server -r $SHORT_B --output 'output/%h_%s' foo
  $ cat output/${SHORT_B}_foo
  foo content

Test --output with %% (literal percent):

  $ rm -rf output
  $ sl cat -R test:server -r $B --output 'output/%%test' foo
  $ cat 'output/%test'
  foo content

Test --output with absolute path:

  $ rm -rf output
  $ sl cat -R test:server -r $B --output "$TESTTMP/output/%s" foo
  $ cat "$TESTTMP/output/foo"
  foo content

Test --tar writes archive to stdout by default and uses --output as entry template:

  $ sl cat -R test:server -r $B --tar foo dir/file | tar tf - | sort
  dir/file
  foo
  $ rm -rf output
  $ sl cat -R test:server -r $B --tar --output 'archive/%p' foo dir/file > output.tar
  $ tar tf output.tar | sort
  archive/dir/file
  archive/foo
  $ mkdir output
  $ tar xf output.tar -C output
  $ cat output/archive/foo
  foo content
  $ cat output/archive/dir/file
  dir content
  $ sl cat -R test:server -r $B --tar --output - foo dir/file | tar tf - | sort
  dir/file
  foo

Test --binary-file-size-threshold replaces large binary files:

  $ newserver server-binary
  $ drawdag <<'EOS'
  > A
  > python:
  > commit("A", files={
  >     "large_binary": b85(b"abc\0def\n"),
  >     "binary_link": b85(b"target\0name (symlink)"),
  >     "large_text": "abcdef\n",
  > })
  > EOS

  $ sl cat -R test:server-binary -r $A --binary-file-size-threshold 4 large_binary
  This is a placeholder for a large binary file
  
  Original file path: large_binary
  Original file id: * (glob)
  Original file size: 8

  $ sl cat -R test:server-binary -r $A --binary-file-size-threshold 4 binary_link
  This is a placeholder for a large binary file
  
  Original file path: binary_link
  Original file id: * (glob)
  Original file size: 11

  $ sl cat -R test:server-binary -r $A --binary-file-size-threshold 4 large_text
  abcdef

Test --binary-file-size-threshold applies to tar output:

  $ rm -rf output filtered.tar
  $ sl cat -R test:server-binary -r $A --binary-file-size-threshold 4 --tar --output 'export/%p' large_binary binary_link large_text > filtered.tar
  $ mkdir output
  $ tar xf filtered.tar -C output
  $ cat output/export/large_binary
  This is a placeholder for a large binary file
  
  Original file path: large_binary
  Original file id: * (glob)
  Original file size: 8
  $ cat output/export/binary_link
  This is a placeholder for a large binary file
  
  Original file path: binary_link
  Original file id: * (glob)
  Original file size: 11
  $ cat output/export/large_text
  abcdef

Test --output with %% in directory name:

  $ rm -rf output
  $ sl cat -R test:server -r $B --output 'output/%%dir%%/%s' foo
  $ cat 'output/%dir%/foo'
  foo content

Test --output with deeper path (multiple components):

  $ rm -rf output
  $ sl cat -R test:server -r $B --output 'output/a/b/c/%s' foo
  $ cat output/a/b/c/foo
  foo content

  $ rm -rf output
  $ sl cat -R test:server -r $B --output 'output/%h/files/%p' dir/file
  $ cat output/${SHORT_B}/files/dir/file
  dir content

Test we don't blow away existing directories:

  $ rm -rf output
  $ mkdir -p output/precious
  $ echo "precious content" > output/precious/file
  $ sl cat -R test:server -r $B --output output/precious dir/file
  abort: can't clear conflicts after handling error "failed to write to file `precious`*": cannot write to "*precious": conflicting directory exists at "*precious" (glob)
  [255]
  $ cat output/precious/file
  precious content

#if unix-permissions symlink

Test cat --output preserves executable and symlink metadata:

  $ newserver server2
  $ drawdag <<'EOS'
  > A  # A/script = #!/bin/sh\necho hello\n (executable)
  >    # A/link = script (symlink)
  >    # A/normal = normal content\n
  > EOS

  $ rm -rf output
  $ sl cat -R test:server2 -r $A --output 'output/%p' script link normal
  $ ls -l output
  l* link -> script (glob)
  -rw-r--r-- normal
  -rwxr-xr-x script

Test cat --tar preserves executable and symlink metadata:

  $ rm -rf output output.tar
  $ sl cat -R test:server2 -r $A --tar script link normal > output.tar
  $ tar tf output.tar | sort
  link
  normal
  script
  $ mkdir output
  $ tar xf output.tar -C output
  $ f output --recurse -MtmsB4
  output: directory with 3 files, mode=755
  output/link -> script: link, size=6
  output/normal: file, size=15, mode=644, md5=d811
  output/script: file, size=21, mode=755, md5=d604

Test cat --tar reports invalid UTF-8 symlink targets with path and file id:

  $ newserver server-invalid-link
  $ drawdag <<'EOS'
  > A
  > python:
  > commit("A", files={"badlink": b85(b"\xfftarget (symlink)")})
  > EOS
  $ sl cat -R test:server-invalid-link -r $A --tar badlink > output.tar
  abort: invalid UTF-8 symlink target for badlink (file id *): [255, 116, 97, 114, 103, 101, 116]: invalid utf-8 sequence of 1 bytes from index 0 (glob)
  [255]

#endif

Test cat handles identical files with different paths:

  $ newserver server-identical
  $ drawdag <<'EOS'
  > A  # A/one = content\n
  >    # A/two = content\n
  > EOS

  $ rm -rf output
  $ sl cat -R test:server-identical -r $A --output 'output/%p'
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

  $ sl cat -R test:server3 -r $A allowed/file1 blocked/secret | sort
  allowed file 1
  secret content

Cat with sparse profile filters to allowed paths:

  $ sl cat -R test:server3 -r $A --config clone.eden-sparse-filter.foo=sparse/profile allowed/file1
  allowed file 1

  $ sl cat -R test:server3 -r $A --config clone.eden-sparse-filter.foo=sparse/profile blocked/secret
  [1]

  $ rm -rf output
  $ sl cat -R test:server3 -r $A --config clone.eden-sparse-filter.foo=sparse/profile --output 'output/%p' 'glob:**'
  $ find output -type f | sort
  output/allowed/file1
  output/allowed/file2

  $ rm -rf output
Empty profile should allow everything:
  $ sl cat -R test:server3 -r $A --config clone.eden-sparse-filter.foo= --output 'output/%p' path:
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

  $ sl cat -R test:server -r $B foo --pager=true --config 'pager.pager=sl debugpython $TESTTMP/fakepager.py'
  $ cat paged
  paged! 'foo content\n'

#endif
