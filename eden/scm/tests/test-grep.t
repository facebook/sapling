  $ setconfig drawdag.defaultfiles=false

  $ setconfig grep.use-rust=true

  $ newclientrepo
  $ drawdag <<EOS
  > A  # A/apple = apple\n
  >    # A/banana = banana\n
  >    # A/fruits = apple\nbanana\norange\n
  > EOS
  $ hg go -q $A

  $ hg grep apple | sort
  apple:apple
  fruits:apple

  $ hg grep apple path:fruits
  fruits:apple

  $ hg grep doesntexist
  [1]

  $ hg grep 're:(oops'
  abort: invalid grep pattern 're:(oops': Error { kind: Regex("regex parse error:\n    (?:re:(oops)\n    ^\nerror: unclosed group") }
  [255]

Test -i (ignore case):
  $ hg grep APPLE
  [1]
  $ hg grep -i APPLE | sort
  apple:apple
  fruits:apple

Test -n (line numbers):
  $ hg grep -n banana | sort
  banana:1:banana
  fruits:2:banana

Test -l (files with matches):
  $ hg grep -l apple | sort
  apple
  fruits

Test -w (word regexp):
  $ hg grep app | sort
  apple:apple
  fruits:apple
  $ hg grep -w app
  [1]

Test -V (invert match):
  $ hg grep -V apple path:fruits
  fruits:banana
  fruits:orange

Test -F (fixed strings) - create a file with regex metacharacters:
  $ echo 'a.ple' > dotfile
  $ hg commit -Aqm 'add dotfile'
  $ hg grep -F 'a.ple'
  dotfile:a.ple

Test -A (after context):
  $ hg grep -A 1 apple path:fruits
  fruits:apple
  fruits-banana

Test -B (before context):
  $ hg grep -B 1 banana path:fruits
  fruits-apple
  fruits:banana

Test -C (context - before and after):
  $ hg grep -C 1 banana path:fruits
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
  $ hg commit -Aqm 'add multiline'
  $ hg grep -C 1 match path:multiline
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
  $ hg grep --color=always apple path:apple
  \x1b[0m\x1b[35mapple\x1b[0m:\x1b[0m\x1b[1m\x1b[31mapple\x1b[0m (esc)

Test color output with line numbers:
  $ hg grep --color=always -n banana path:banana
  \x1b[0m\x1b[35mbanana\x1b[0m:\x1b[0m\x1b[32m1\x1b[0m:\x1b[0m\x1b[1m\x1b[31mbanana\x1b[0m (esc)

Test color disabled explicitly:
  $ hg grep --color=off apple path:apple
  apple:apple

#endif

Test JSON output (-T json):
  $ hg grep -T json apple path:apple
  [
    {"path":"apple","text":"apple"}
  ]

Test JSON output with line numbers:
  $ hg grep -T json -n banana path:fruits
  [
    {"path":"fruits","line_number":2,"text":"banana"}
  ]

Test JSON output with multiple matches:
  $ hg grep -T json apple path:apple path:fruits | pp --sort
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
  $ hg grep -T jsonl apple path:apple
  {"path":"apple","text":"apple"}

Test JSON Lines output with line numbers:
  $ hg grep -T jsonl -n banana path:fruits
  {"path":"fruits","line_number":2,"text":"banana"}

Test JSON Lines output with multiple matches:
  $ hg grep -T jsonl apple path:apple path:fruits | sort
  {"path":"apple","text":"apple"}
  {"path":"fruits","text":"apple"}

Test JSON output with -l (files with matches):
  $ hg grep -T json -l apple path:apple path:fruits | pp --sort
  [
    {
      "path": "apple"
    },
    {
      "path": "fruits"
    }
  ]

Test JSON Lines output with -l:
  $ hg grep -T jsonl -l apple path:apple path:fruits | sort
  {"path":"apple"}
  {"path":"fruits"}

Test JSON output with -V (invert match):
  $ hg grep -T json -V apple path:fruits
  [
    {"path":"fruits","text":"banana"},
    {"path":"fruits","text":"orange"}
  ]

Test unsupported flags with -T json:
  $ hg grep -T json -A 1 apple
  abort: -A/--after-context is not supported with -T json
  [255]
  $ hg grep -T json -B 1 apple
  abort: -B/--before-context is not supported with -T json
  [255]
  $ hg grep -T json -C 1 apple
  abort: -C/--context is not supported with -T json
  [255]

Test "." is the default file pattern (search cwd):
  $ mkdir subdir
  $ echo 'sub banana' > subdir/subfile
  $ hg commit -Aqm 'add subdir'
  $ hg grep banana | sort
  banana:banana
  fruits:banana
  subdir/subfile:sub banana
  $ cd subdir
  $ hg grep sub
  subfile:sub banana
  $ cd ..

Test grep in uncommitted changes:
  $ echo 'findme' > uncommitted_file
  $ hg add uncommitted_file
  $ hg grep findme
  uncommitted_file:findme

Test grep does not search untracked files:
  $ echo 'untracked_content' > untracked_file
  $ hg grep untracked_content
  [1]

Test grep does not search ignored files:
  $ echo 'ignored_content' > ignored_file
  $ echo 'ignored_file' > .gitignore
  $ hg add .gitignore
  $ hg grep ignored_content
  [1]

Test grep does not search removed files:
  $ hg commit -m 'add files'
  $ echo 'removed_content' > removed_file
  $ hg commit -Aqm 'add removed_file'
  $ hg rm removed_file
  $ hg grep removed_content
  [1]

Test grep does search deleted files (tracked but missing from disk):
  $ echo 'deleted_content' > deleted_file
  $ hg commit -Aqm 'add deleted_file'
  $ rm deleted_file
  $ hg grep deleted_content
  deleted_file:deleted_content

Test --rev searches specific revision (not working copy):
  $ echo 'new_content' > new_file
  $ hg commit -Aqm 'add new_file'
  $ echo 'uncommitted' >> new_file
  $ hg grep uncommitted
  new_file:uncommitted
  $ hg grep -r . uncommitted
  [1]
  $ hg grep -r . new_content
  new_file:new_content

Test --rev can search older revisions:
  $ hg grep -r $A apple | sort
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
  $ hg book -r $B main

Test repoless grep requires file pattern:
  $ hg grep -R test:server -r $B content
  abort: FILE pattern(s) required in repoless mode
  [255]

Test repoless grep requires --rev:
  $ hg grep -R test:server content pattern
  abort: --rev is required for repoless grep
  [255]

  $ hg grep -R test:server -r $B content path: | sort
  bar:bar content
  dir/file:dir content
  foo:foo content
  other:other content

  $ hg grep -R test:server -r $B content dir | sort
  dir/file:dir content

  $ hg grep -R test:server -r $A content path: | sort
  bar:bar content
  foo:foo content

  $ hg grep -R test:server -r main 'dir content' path:
  dir/file:dir content


Can grep unpulled revisions from on-disk repo:
  $ newclientrepo unpulled-client server
  $ hg grep -r $A foo
  foo:foo content
