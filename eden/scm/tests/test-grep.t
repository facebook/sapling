  $ setconfig drawdag.defaultfiles=false

  $ setconfig grep.use-rust=true

  $ newclientrepo
  $ drawdag <<EOS
  > A  # A/apple = apple\n
  >    # A/banana = banana\n
  >    # A/fruits = apple\nbanana\norange\n
  > EOS
  $ sl go -q $A

  $ sl grep apple | sort
  apple:apple
  fruits:apple

  $ sl grep apple path:fruits
  fruits:apple

  $ sl grep doesntexist
  [1]

  $ sl grep 're:(oops'
  abort: invalid grep pattern 're:(oops': Error { kind: Regex("regex parse error:\n    (?:re:(oops)\n    ^\nerror: unclosed group") }
  [255]

Test -i (ignore case):
  $ sl grep APPLE
  [1]
  $ sl grep -i APPLE | sort
  apple:apple
  fruits:apple

Test -n (line numbers):
  $ sl grep -n banana | sort
  banana:1:banana
  fruits:2:banana

Test -l (files with matches):
  $ sl grep -l apple | sort
  apple
  fruits

Test -w (word regexp):
  $ sl grep app | sort
  apple:apple
  fruits:apple
  $ sl grep -w app
  [1]

Test -V (invert match):
  $ sl grep -V apple path:fruits
  fruits:banana
  fruits:orange

Test -F (fixed strings) - create a file with regex metacharacters:
  $ echo 'a.ple' > dotfile
  $ sl commit -Aqm 'add dotfile'
  $ sl grep -F 'a.ple'
  dotfile:a.ple

Test -A (after context):
  $ sl grep -A 1 apple path:fruits
  fruits:apple
  fruits-banana

Test -B (before context):
  $ sl grep -B 1 banana path:fruits
  fruits-apple
  fruits:banana

Test -C (context - before and after):
  $ sl grep -C 1 banana path:fruits
  fruits-apple
  fruits:banana
  fruits-orange

Test context break between separate match groups:
  $ cat > multiline << 'EOF'
  > line1
  > match1
  > line2
  > line3
  > line4
  > match2
  > line5
  > EOF
  $ sl commit -Aqm 'add multiline'
  $ sl grep -C 1 match path:multiline
  multiline-line1
  multiline:match1
  multiline-line2
  --
  multiline-line4
  multiline:match2
  multiline-line5

Color seems to work on Windows, but not in the tests.
#if no-windows

Test color output (--color=always forces color even without tty):
  $ sl grep --color=always apple path:apple
  [35mapple[39m:[0m[1m[31mapple[0m

Test color output with line numbers:
  $ sl grep --color=always -n banana path:banana
  [35mbanana[39m:[32m1[39m:[0m[1m[31mbanana[0m

Test color disabled explicitly:
  $ sl grep --color=off apple path:apple
  apple:apple

#endif

Test JSON output (-T json):
  $ sl grep -T json apple path:apple
  [
    {"path":"apple","text":"apple"}
  ]

Test JSON output with line numbers:
  $ sl grep -T json -n banana path:fruits
  [
    {"path":"fruits","line_number":2,"text":"banana"}
  ]

Test JSON output with multiple matches:
  $ sl grep -T json apple path:apple path:fruits | pp --sort
  [
    {
      "path": "apple",
      "text": "apple"
    },
    {
      "path": "fruits",
      "text": "apple"
    }
  ]

Test JSON Lines output (-T jsonl):
  $ sl grep -T jsonl apple path:apple
  {"path":"apple","text":"apple"}

Test JSON Lines output with line numbers:
  $ sl grep -T jsonl -n banana path:fruits
  {"path":"fruits","line_number":2,"text":"banana"}

Test JSON Lines output with multiple matches:
  $ sl grep -T jsonl apple path:apple path:fruits | sort
  {"path":"apple","text":"apple"}
  {"path":"fruits","text":"apple"}

Test JSON output with -l (files with matches):
  $ sl grep -T json -l apple path:apple path:fruits | pp --sort
  [
    {
      "path": "apple"
    },
    {
      "path": "fruits"
    }
  ]

Test JSON Lines output with -l:
  $ sl grep -T jsonl -l apple path:apple path:fruits | sort
  {"path":"apple"}
  {"path":"fruits"}

Test JSON output with -V (invert match):
  $ sl grep -T json -V apple path:fruits
  [
    {"path":"fruits","text":"banana"},
    {"path":"fruits","text":"orange"}
  ]

Test unsupported flags with -T json:
  $ sl grep -T json -A 1 apple
  abort: -A/--after-context is not supported with -T json
  [255]
  $ sl grep -T json -B 1 apple
  abort: -B/--before-context is not supported with -T json
  [255]
  $ sl grep -T json -C 1 apple
  abort: -C/--context is not supported with -T json
  [255]

Test "." is the default file pattern (search cwd):
  $ mkdir subdir
  $ echo 'sub banana' > subdir/subfile
  $ sl commit -Aqm 'add subdir'
  $ sl grep banana | sort
  banana:banana
  fruits:banana
  subdir/subfile:sub banana
  $ cd subdir
  $ sl grep sub
  subfile:sub banana
  $ cd ..

Test grep in uncommitted changes:
  $ echo 'findme' > uncommitted_file
  $ sl add uncommitted_file
  $ sl grep findme
  uncommitted_file:findme

Test grep does not search untracked files:
  $ echo 'untracked_content' > untracked_file
  $ sl grep untracked_content
  [1]

Test grep does not search ignored files:
  $ echo 'ignored_content' > ignored_file
  $ echo 'ignored_file' > .gitignore
  $ sl add .gitignore
  $ sl grep ignored_content
  [1]

Test grep does not search removed files:
  $ sl commit -m 'add files'
  $ echo 'removed_content' > removed_file
  $ sl commit -Aqm 'add removed_file'
  $ sl rm removed_file
  $ sl grep removed_content
  [1]

Test grep does search deleted files (tracked but missing from disk):
  $ echo 'deleted_content' > deleted_file
  $ sl commit -Aqm 'add deleted_file'
  $ rm deleted_file
  $ sl grep deleted_content
  deleted_file:deleted_content

Test --rev searches specific revision (not working copy):
  $ echo 'new_content' > new_file
  $ sl commit -Aqm 'add new_file'
  $ echo 'uncommitted' >> new_file
  $ sl grep uncommitted
  new_file:uncommitted
  $ sl grep -r . uncommitted
  [1]
  $ sl grep -r . new_content
  new_file:new_content

Test --rev can search older revisions:
  $ sl grep -r $A apple | sort
  apple:apple
  fruits:apple

Test repoless grep with test:server -R url:
  $ eagerepo
  $ newserver server
  $ drawdag <<'EOS'
  > B  # B/dir/file = dir content\n
  > |  # B/other = other content\n
  > A  # A/foo = foo content\n
  >    # A/bar = bar content\n
  > EOS
  $ sl book -r $B main

Test repoless grep requires file pattern:
  $ sl grep -R test:server -r $B content
  abort: FILE pattern(s) required in repoless mode
  [255]

Test repoless grep requires --rev:
  $ sl grep -R test:server content pattern
  abort: --rev is required for repoless grep
  [255]

  $ sl grep -R test:server -r $B content path: | sort
  bar:bar content
  dir/file:dir content
  foo:foo content
  other:other content

  $ sl grep -R test:server -r $B content dir | sort
  dir/file:dir content

  $ sl grep -R test:server -r $A content path: | sort
  bar:bar content
  foo:foo content

  $ sl grep -R test:server -r main 'dir content' path:
  dir/file:dir content


Can grep unpulled revisions from on-disk repo:
  $ newclientrepo unpulled-client server
  $ sl grep -r $A foo
  foo:foo content


Test --unknown searches untracked files:
  $ newclientrepo unknown-test
  $ echo 'tracked content' > tracked
  $ sl commit -Aqm 'initial'

Without --unknown, untracked files are not searched:
  $ echo 'findme in untracked' > untracked_file
  $ sl grep findme
  [1]

With --unknown, untracked files are searched:
  $ sl grep --unknown findme
  untracked_file:findme in untracked

--unknown also searches tracked files:
  $ sl grep --unknown -w content path:tracked
  tracked:tracked content

--unknown with -l (files with matches):
  $ echo 'findme too' > another_untracked
  $ sl grep --unknown -l findme | sort
  another_untracked
  untracked_file

--unknown with --rev is an error:
  $ sl grep --unknown -r . findme
  abort: --unknown is only supported for the working directory (wdir)
  [255]

--unknown with -X excludes untracked files:
  $ sl grep --unknown findme -X untracked_file
  another_untracked:findme too

Test grep skips binary files:
  $ newclientrepo binary-test
  $ printf 'text match\n' > text_file
  $ printf 'binary match\0 here\n' > binary_file
  $ sl commit -Aqm 'add files'
  $ sl grep match
  text_file:text match

#if symlink
Test grep skips symlink blobs:
  $ newclientrepo symlink-test
  $ echo 'target content' > target_file
  $ ln -s target_file sym_link
  $ sl add target_file sym_link
  $ sl commit -qm 'add symlink'
  $ sl grep target_file
  [1]
  $ sl grep -l target_file
  [1]
  $ sl grep -T json target_file
  []
  $ sl grep 'target content'
  target_file:target content
#endif

Test grep with --include filters by file pattern:
  $ newclientrepo include-test
  $ mkdir -p src lib
  $ echo 'hello world' > src/main.py
  $ echo 'hello test' > src/main.rs
  $ echo 'hello lib' > lib/util.py
  $ sl commit -Aqm 'add files'
  $ sl grep -I '**.py' hello | sort
  lib/util.py:hello lib
  src/main.py:hello world

No spurious match-full-traversal hint when a path narrows the search:
  $ sl grep -I '**.py' hello src --config hint.ack-match-full-traversal=false
  src/main.py:hello world

Test BRE-style \| alternation without -E.
Both \| and | work as alternation (superset of BRE and ERE):
  $ newclientrepo bre-test
  $ echo 'apple' > a
  $ echo 'banana' > b
  $ echo 'cherry' > c
  $ sl commit -Aqm 'add files'

  $ sl grep 'apple\|banana' | sort
  a:apple
  b:banana
  $ sl grep 'apple|banana' | sort
  a:apple
  b:banana

With -E, no BRE conversion — \| is literal, | is alternation:
  $ sl grep -E 'apple\|banana'
  [1]
  $ sl grep -E 'apple|banana' | sort
  a:apple
  b:banana

\( is NOT converted (users use it for literal paren, e.g. function calls):
  $ echo 'f(x) = 1' > parens
  $ sl commit -Aqm 'add parens'
  $ sl grep 'f\(x\)' path:parens
  parens:f(x) = 1

Escaped backslash is not converted:
  $ echo 'back\slash' > d
  $ sl commit -Aqm 'add d'
  $ sl grep 'back\\' path:d
  d:back\slash
