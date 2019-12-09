#chg-compatible

TODO: Make this test compatibile with obsstore enabled.
  $ setconfig experimental.evolution=
1) Make the repo
  $ mkdir basic
  $ cd basic
  $ hg init
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > conflictinfo=
  > EOF

2) Can't run dumpjson outside a conflict
  $ hg resolve --tool internal:dumpjson
  abort: no files or directories specified
  (use --all to re-merge all unresolved files)
  [255]

3) Make a simple conflict
  $ echo "Unconflicted base, F1" > F1
  $ echo "Unconflicted base, F2" > F2
  $ hg commit -Aqm "initial commit"
  $ echo "First conflicted version, F1" > F1
  $ echo "First conflicted version, F2" > F2
  $ hg commit -m "first version, a"
  $ hg bookmark a
  $ hg checkout .~1
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (leaving bookmark a)
  $ echo "Second conflicted version, F1" > F1
  $ echo "Second conflicted version, F2" > F2
  $ hg commit -m "second version, b"
  $ hg bookmark b
  $ hg log -G -T '({rev}) {desc}\nbookmark: {bookmarks}\nfiles: {files}\n\n'
  @  (2) second version, b
  |  bookmark: b
  |  files: F1 F2
  |
  | o  (1) first version, a
  |/   bookmark: a
  |    files: F1 F2
  |
  o  (0) initial commit
     bookmark:
     files: F1 F2
  
  $ hg merge a
  merging F1
  merging F2
  warning: 1 conflicts while merging F1! (edit, then use 'hg resolve --mark')
  warning: 1 conflicts while merging F2! (edit, then use 'hg resolve --mark')
  0 files updated, 0 files merged, 0 files removed, 2 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  [1]

5) Get the paths:
  $ hg resolve --tool internal:dumpjson --all
  [
   {
    "command": "merge",
    "command_details": {"cmd": "merge", "to_abort": "update --clean", "to_continue": "merge --continue"},
    "conflicts": [{"base": {"contents": "Unconflicted base, F1\n", "exists": true, "isexec": false, "issymlink": false}, "local": {"contents": "Second conflicted version, F1\n", "exists": true, "isexec": false, "issymlink": false}, "other": {"contents": "First conflicted version, F1\n", "exists": true, "isexec": false, "issymlink": false}, "output": {"contents": "<<<<<<< working copy: 13124abb51b9 b - test: second version, b\nSecond conflicted version, F1\n=======\nFirst conflicted version, F1\n>>>>>>> merge rev:    6dd692b7db4a a - test: first version, a\n", "exists": true, "isexec": false, "issymlink": false, "path": "$TESTTMP/basic/F1"}, "path": "F1"}, {"base": {"contents": "Unconflicted base, F2\n", "exists": true, "isexec": false, "issymlink": false}, "local": {"contents": "Second conflicted version, F2\n", "exists": true, "isexec": false, "issymlink": false}, "other": {"contents": "First conflicted version, F2\n", "exists": true, "isexec": false, "issymlink": false}, "output": {"contents": "<<<<<<< working copy: 13124abb51b9 b - test: second version, b\nSecond conflicted version, F2\n=======\nFirst conflicted version, F2\n>>>>>>> merge rev:    6dd692b7db4a a - test: first version, a\n", "exists": true, "isexec": false, "issymlink": false, "path": "$TESTTMP/basic/F2"}, "path": "F2"}],
    "pathconflicts": []
   }
  ]

6) Only requested paths get dumped
  $ hg resolve --tool internal:dumpjson F2
  [
   {
    "command": "merge",
    "command_details": {"cmd": "merge", "to_abort": "update --clean", "to_continue": "merge --continue"},
    "conflicts": [{"base": {"contents": "Unconflicted base, F2\n", "exists": true, "isexec": false, "issymlink": false}, "local": {"contents": "Second conflicted version, F2\n", "exists": true, "isexec": false, "issymlink": false}, "other": {"contents": "First conflicted version, F2\n", "exists": true, "isexec": false, "issymlink": false}, "output": {"contents": "<<<<<<< working copy: 13124abb51b9 b - test: second version, b\nSecond conflicted version, F2\n=======\nFirst conflicted version, F2\n>>>>>>> merge rev:    6dd692b7db4a a - test: first version, a\n", "exists": true, "isexec": false, "issymlink": false, "path": "$TESTTMP/basic/F2"}, "path": "F2"}],
    "pathconflicts": []
   }
  ]

7) Ensure the paths point to the right contents:
  $ getcontents() { # Usage: getcontents <path> <version>
  >  local script="import sys, json; print json.load(sys.stdin)[0][\"conflicts\"][$1][\"$2\"][\"contents\"]"
  >  local result=`hg resolve --tool internal:dumpjson --all | python -c "$script"`
  >  echo "$result"
  > }
  $ echo `getcontents 0 "base"`
  Unconflicted base, F1
  $ echo `getcontents 0 "other"`
  First conflicted version, F1
  $ echo `getcontents 0 "local"`
  Second conflicted version, F1
  $ echo `getcontents 1 "base"`
  Unconflicted base, F2
  $ echo `getcontents 1 "other"`
  First conflicted version, F2
  $ echo `getcontents 1 "local"`
  Second conflicted version, F2

Tests merge conflict corner cases (file-to-directory, binary-to-symlink, etc.)
"other" == source
"local" == dest

Setup
  $ cd ..
  $ rm -rf basic
  $ mkdir cornercases
  $ cd cornercases
  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > rebase=
  > conflictinfo=
  > EOF

  $ reset() {
  >   rm -rf foo
  >   mkdir foo
  >   cd foo
  >   hg init
  >   echo "base" > file
  >   hg commit -Aqm "base"
  > }
  $ logg() {
  >   hg log -G -T '({rev}) {desc}\naffected: {files}\ndeleted: {file_dels}\n\n'
  > }

Test case 0: A merge of just contents conflicts (not usually a corner case),
but the user had local changes and ran `merge -f`.

tldr: Since we can premerge, the working copy is backed up to an origfile.
  $ reset
  $ echo "change" > file
  $ hg commit -Aqm "dest"
  $ hg up -q 0
  $ echo "other change" > file
  $ hg commit -Aqm "source"
  $ hg up -q 1
  $ logg
  o  (2) source
  |  affected: file
  |  deleted:
  |
  | @  (1) dest
  |/   affected: file
  |    deleted:
  |
  o  (0) base
     affected: file
     deleted:
  
  $ echo "some local changes" > file
  $ hg merge 2 -f
  merging file
  warning: 1 conflicts while merging file! (edit, then use 'hg resolve --mark')
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  [1]

  $ hg resolve --tool=internal:dumpjson --all
  [
   {
    "command": "merge",
    "command_details": {"cmd": "merge", "to_abort": "update --clean", "to_continue": "merge --continue"},
    "conflicts": [{"base": {"contents": "base\n", "exists": true, "isexec": false, "issymlink": false}, "local": {"contents": "some local changes\n", "exists": true, "isexec": false, "issymlink": false}, "other": {"contents": "other change\n", "exists": true, "isexec": false, "issymlink": false}, "output": {"contents": "<<<<<<< working copy: fd7d10c36158 - test: dest\nsome local changes\n=======\nother change\n>>>>>>> merge rev:    9b65ba2922f0 - test: source\n", "exists": true, "isexec": false, "issymlink": false, "path": "$TESTTMP/cornercases/foo/file"}, "path": "file"}],
    "pathconflicts": []
   }
  ]

Test case 0b: Like #0 but with a corner case: source deleted, local changed
*and* had local changes using merge -f.

tldr: Since we couldn't premerge, the working copy is left alone.
  $ reset
  $ echo "change" > file
  $ hg commit -Aqm "dest"
  $ hg up -q 0
  $ hg rm file
  $ hg commit -Aqm "source"
  $ hg up -q 1
  $ logg
  o  (2) source
  |  affected: file
  |  deleted: file
  |
  | @  (1) dest
  |/   affected: file
  |    deleted:
  |
  o  (0) base
     affected: file
     deleted:
  
  $ echo "some local changes" > file
  $ chmod +x file
  $ hg merge 2 -f
  local [working copy] changed file which other [merge rev] deleted
  use (c)hanged version, (d)elete, or leave (u)nresolved? u
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  [1]

  $ hg resolve --tool=internal:dumpjson --all
  [
   {
    "command": "merge",
    "command_details": {"cmd": "merge", "to_abort": "update --clean", "to_continue": "merge --continue"},
    "conflicts": [{"base": {"contents": "base\n", "exists": true, "isexec": false, "issymlink": false}, "local": {"contents": "some local changes\n", "exists": true, "isexec": true, "issymlink": false}, "other": {"contents": null, "exists": false, "isexec": null, "issymlink": null}, "output": {"contents": "some local changes\n", "exists": true, "isexec": true, "issymlink": false, "path": "$TESTTMP/cornercases/foo/foo/file"}, "path": "file"}],
    "pathconflicts": []
   }
  ]

Test case 1: Source deleted, dest changed
  $ reset
  $ echo "change" > file
  $ hg commit -Aqm "dest"
  $ hg up -q 0
  $ hg rm file
  $ hg commit -Aqm "source"
  $ hg up -q 1
  $ logg
  o  (2) source
  |  affected: file
  |  deleted: file
  |
  | @  (1) dest
  |/   affected: file
  |    deleted:
  |
  o  (0) base
     affected: file
     deleted:
  

  $ hg rebase -d 1 -s 2
  rebasing 25c2ef28f4c7 "source" (tip)
  local [dest] changed file which other [source] deleted
  use (c)hanged version, (d)elete, or leave (u)nresolved? u
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]
  $ hg resolve --tool=internal:dumpjson --all
  [
   {
    "command": "rebase",
    "command_details": {"cmd": "rebase", "to_abort": "rebase --abort", "to_continue": "rebase --continue"},
    "conflicts": [{"base": {"contents": "base\n", "exists": true, "isexec": false, "issymlink": false}, "local": {"contents": "change\n", "exists": true, "isexec": false, "issymlink": false}, "other": {"contents": null, "exists": false, "isexec": null, "issymlink": null}, "output": {"contents": "change\n", "exists": true, "isexec": false, "issymlink": false, "path": "$TESTTMP/cornercases/foo/foo/foo/file"}, "path": "file"}],
    "pathconflicts": []
   }
  ]
Test case 1b: Like #1 but with a merge, with local changes
  $ reset
  $ echo "change" > file
  $ hg commit -Aqm "dest"
  $ hg up -q 0
  $ hg rm file
  $ hg commit -Aqm "source"
  $ hg up -q 1
  $ logg
  o  (2) source
  |  affected: file
  |  deleted: file
  |
  | @  (1) dest
  |/   affected: file
  |    deleted:
  |
  o  (0) base
     affected: file
     deleted:
  
  $ echo "some local changes" > file
  $ hg merge 2 -f
  local [working copy] changed file which other [merge rev] deleted
  use (c)hanged version, (d)elete, or leave (u)nresolved? u
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  [1]

  $ hg resolve --tool=internal:dumpjson --all
  [
   {
    "command": "merge",
    "command_details": {"cmd": "merge", "to_abort": "update --clean", "to_continue": "merge --continue"},
    "conflicts": [{"base": {"contents": "base\n", "exists": true, "isexec": false, "issymlink": false}, "local": {"contents": "some local changes\n", "exists": true, "isexec": false, "issymlink": false}, "other": {"contents": null, "exists": false, "isexec": null, "issymlink": null}, "output": {"contents": "some local changes\n", "exists": true, "isexec": false, "issymlink": false, "path": "$TESTTMP/cornercases/foo/foo/foo/foo/file"}, "path": "file"}],
    "pathconflicts": []
   }
  ]
Test case 2: Source changed, dest deleted
  $ cd ..
  $ reset
  $ echo "change" > file
  $ hg commit -Aqm "source"
  $ hg up -q 0
  $ hg rm file
  $ hg commit -Aqm "dest"
  $ hg up --q 2
  $ logg
  @  (2) dest
  |  affected: file
  |  deleted: file
  |
  | o  (1) source
  |/   affected: file
  |    deleted:
  |
  o  (0) base
     affected: file
     deleted:
  

  $ hg rebase -d 2 -s 1
  rebasing ec87889f5f90 "source"
  other [source] changed file which local [dest] deleted
  use (c)hanged version, leave (d)eleted, leave (u)nresolved, or input (r)enamed path? u
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]
  $ hg resolve --tool=internal:dumpjson --all
  [
   {
    "command": "rebase",
    "command_details": {"cmd": "rebase", "to_abort": "rebase --abort", "to_continue": "rebase --continue"},
    "conflicts": [{"base": {"contents": "base\n", "exists": true, "isexec": false, "issymlink": false}, "local": {"contents": null, "exists": false, "isexec": null, "issymlink": null}, "other": {"contents": "change\n", "exists": true, "isexec": false, "issymlink": false}, "output": {"contents": null, "exists": false, "isexec": null, "issymlink": null, "path": "$TESTTMP/cornercases/foo/foo/foo/foo/file"}, "path": "file"}],
    "pathconflicts": []
   }
  ]
Test case 3: Source changed, dest moved
  $ cd ..
  $ reset
  $ echo "change" > file
  $ hg commit -Aqm "source"
  $ hg up -q 0
  $ hg mv file file_newloc
  $ hg commit -Aqm "dest"
  $ hg up -q 2
  $ logg
  @  (2) dest
  |  affected: file file_newloc
  |  deleted: file
  |
  | o  (1) source
  |/   affected: file
  |    deleted:
  |
  o  (0) base
     affected: file
     deleted:
  

  $ hg rebase -d 2 -s 1
  rebasing ec87889f5f90 "source"
  merging file_newloc and file to file_newloc
  saved backup bundle to $TESTTMP/cornercases/foo/foo/foo/foo/.hg/strip-backup/ec87889f5f90-e39a76b8-rebase.hg (glob)
  $ hg up -q 2 # source
  $ cat file_newloc # Should follow:
  change
  $ hg resolve --tool=internal:dumpjson --all
  [
   {
    "command": null,
    "conflicts": [],
    "pathconflicts": []
   }
  ]
Test case 4: Source changed, dest moved (w/o copytracing)
  $ cd ..
  $ reset
  $ echo "change" > file
  $ hg commit -Aqm "source"
  $ hg up -q 0
  $ hg mv file file_newloc
  $ hg commit -Aqm "dest"
  $ hg up -q 2
  $ logg
  @  (2) dest
  |  affected: file file_newloc
  |  deleted: file
  |
  | o  (1) source
  |/   affected: file
  |    deleted:
  |
  o  (0) base
     affected: file
     deleted:
  

  $ hg rebase -d 2 -s 1 --config experimental.copytrace=off
  rebasing ec87889f5f90 "source"
  other [source] changed file which local [dest] deleted
  use (c)hanged version, leave (d)eleted, leave (u)nresolved, or input (r)enamed path? u
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]
  $ hg resolve --tool=internal:dumpjson --all
  [
   {
    "command": "rebase",
    "command_details": {"cmd": "rebase", "to_abort": "rebase --abort", "to_continue": "rebase --continue"},
    "conflicts": [{"base": {"contents": "base\n", "exists": true, "isexec": false, "issymlink": false}, "local": {"contents": null, "exists": false, "isexec": null, "issymlink": null}, "other": {"contents": "change\n", "exists": true, "isexec": false, "issymlink": false}, "output": {"contents": null, "exists": false, "isexec": null, "issymlink": null, "path": "$TESTTMP/cornercases/foo/foo/foo/foo/file"}, "path": "file"}],
    "pathconflicts": []
   }
  ]
Test case 5: Source moved, dest changed
  $ cd ..
  $ reset
  $ hg mv file file_newloc
  $ hg commit -Aqm "source"
  $ hg up -q 0
  $ echo "change" > file
  $ hg commit -Aqm "dest"
  $ hg up -q 2
  $ logg
  @  (2) dest
  |  affected: file
  |  deleted:
  |
  | o  (1) source
  |/   affected: file file_newloc
  |    deleted: file
  |
  o  (0) base
     affected: file
     deleted:
  

  $ hg rebase -d 2 -s 1
  rebasing e6e7483a8950 "source"
  merging file and file_newloc to file_newloc
  saved backup bundle to $TESTTMP/cornercases/foo/foo/foo/foo/.hg/strip-backup/e6e7483a8950-8e128ac2-rebase.hg (glob)
  $ hg up 2
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ cat file_newloc
  change
  $ hg resolve --tool=internal:dumpjson --all
  [
   {
    "command": null,
    "conflicts": [],
    "pathconflicts": []
   }
  ]
Test case 6: Source moved, dest changed (w/o copytracing)
  $ cd ..
  $ reset
  $ hg mv file file_newloc
  $ hg commit -Aqm "source"
  $ hg up -q 0
  $ echo "change" > file
  $ hg commit -Aqm "dest"
  $ hg up -q 2
  $ logg
  @  (2) dest
  |  affected: file
  |  deleted:
  |
  | o  (1) source
  |/   affected: file file_newloc
  |    deleted: file
  |
  o  (0) base
     affected: file
     deleted:
  

  $ hg rebase -d 2 -s 1 --config experimental.copytrace=off
  rebasing e6e7483a8950 "source"
  local [dest] changed file which other [source] deleted
  use (c)hanged version, (d)elete, or leave (u)nresolved? u
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]
  $ hg resolve --tool=internal:dumpjson --all
  [
   {
    "command": "rebase",
    "command_details": {"cmd": "rebase", "to_abort": "rebase --abort", "to_continue": "rebase --continue"},
    "conflicts": [{"base": {"contents": "base\n", "exists": true, "isexec": false, "issymlink": false}, "local": {"contents": "change\n", "exists": true, "isexec": false, "issymlink": false}, "other": {"contents": null, "exists": false, "isexec": null, "issymlink": null}, "output": {"contents": "change\n", "exists": true, "isexec": false, "issymlink": false, "path": "$TESTTMP/cornercases/foo/foo/foo/foo/file"}, "path": "file"}],
    "pathconflicts": []
   }
  ]
Test case 7: Source is a directory, dest is a file (base is still a file)
  $ cd ..
  $ reset
  $ hg rm file
  $ mkdir file # "file" is a stretch
  $ echo "this will cause problems" >> file/subfile
  $ hg commit -Aqm "source"
  $ hg up -q 0
  $ echo "change" > file
  $ hg commit -Aqm "dest"
  $ hg up -q 2
  $ logg
  @  (2) dest
  |  affected: file
  |  deleted:
  |
  | o  (1) source
  |/   affected: file file/subfile
  |    deleted: file
  |
  o  (0) base
     affected: file
     deleted:
  

  $ hg rebase -d 2 -s 1
  rebasing ed93aeac6b3c "source"
  abort:*: '$TESTTMP/cornercases/foo/foo/foo/foo/file' (glob)
  [255]
  $ hg resolve --tool=internal:dumpjson --all
  [abort:*: $TESTTMP/cornercases/foo/foo/foo/foo/file (glob)
  [255]
Test case 8: Source is a file, dest is a directory (base is still a file)
  $ cd ..
  $ reset
  $ echo "change" > file
  $ hg commit -Aqm "source"
  $ hg up -q 0
  $ hg rm file
  $ mkdir file # "file"
  $ echo "this will cause problems" >> file/subfile
  $ hg commit -Aqm "dest"
  $ hg up -q 2
  $ logg
  @  (2) dest
  |  affected: file file/subfile
  |  deleted: file
  |
  | o  (1) source
  |/   affected: file
  |    deleted:
  |
  o  (0) base
     affected: file
     deleted:
  

  $ hg rebase -d 2 -s 1
  rebasing ec87889f5f90 "source"
  abort:*: '$TESTTMP/cornercases/foo/foo/foo/foo/file' (glob)
  [255]
  $ hg resolve --tool=internal:dumpjson --all
  [
   {
    "command": "rebase",
    "command_details": {"cmd": "rebase", "to_abort": "rebase --abort", "to_continue": "rebase --continue"},
    "conflicts": [{"base": {"contents": "base\n", "exists": true, "isexec": false, "issymlink": false}, "local": {"contents": null, "exists": false, "isexec": null, "issymlink": null}, "other": {"contents": "change\n", "exists": true, "isexec": false, "issymlink": false}, "output": {"contents": null, "exists": false, "isexec": null, "issymlink": null, "path": "$TESTTMP/cornercases/foo/foo/foo/foo/file"}, "path": "file"}],
    "pathconflicts": []
   }
  ]
Test case 9: Source is a binary file, dest is a file (base is still a file)
  $ cd ..
  $ reset
  $ python -c 'f = open("file", "w"); f.write("\x00")'
  $ hg commit -Aqm "source"
  $ hg up -q 0
  $ echo "change" > file
  $ hg commit -Aqm "dest"
  $ hg up -q 2
  $ logg
  @  (2) dest
  |  affected: file
  |  deleted:
  |
  | o  (1) source
  |/   affected: file
  |    deleted:
  |
  o  (0) base
     affected: file
     deleted:
  

  $ hg rebase -d 2 -s 1
  rebasing b6e55a03a5dc "source"
  merging file
  warning: ([^\s]+) looks like a binary file. (re)
  warning: 1 conflicts while merging file! (edit, then use 'hg resolve --mark')
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]
  $ cat -v file # The local version should be left in the working copy
  change
  $ hg resolve --tool=internal:dumpjson --all
  [
   {
    "command": "rebase",
    "command_details": {"cmd": "rebase", "to_abort": "rebase --abort", "to_continue": "rebase --continue"},
    "conflicts": [{"base": {"contents": "base\n", "exists": true, "isexec": false, "issymlink": false}, "local": {"contents": "change\n", "exists": true, "isexec": false, "issymlink": false}, "other": {"contents": "\u0000", "exists": true, "isexec": false, "issymlink": false}, "output": {"contents": "change\n", "exists": true, "isexec": false, "issymlink": false, "path": "$TESTTMP/cornercases/foo/foo/foo/foo/file"}, "path": "file"}],
    "pathconflicts": []
   }
  ]
Test case 10: Source is a file, dest is a binary file (base is still a file)
  $ cd ..
  $ reset
  $ echo "change" > file
  $ hg commit -Aqm "source"
  $ hg up -q 0
  $ python -c 'f = open("file", "w"); f.write("\x00")'
  $ hg commit -Aqm "dest"
  $ hg up -q 2
  $ logg
  @  (2) dest
  |  affected: file
  |  deleted:
  |
  | o  (1) source
  |/   affected: file
  |    deleted:
  |
  o  (0) base
     affected: file
     deleted:
  

  $ hg rebase -d 2 -s 1
  rebasing ec87889f5f90 "source"
  merging file
  warning: ([^\s]+) looks like a binary file. (re)
  warning: 1 conflicts while merging file! (edit, then use 'hg resolve --mark')
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]
  $ cat -v file
  ^@ (no-eol)
  $ hg resolve --tool=internal:dumpjson --all
  [
   {
    "command": "rebase",
    "command_details": {"cmd": "rebase", "to_abort": "rebase --abort", "to_continue": "rebase --continue"},
    "conflicts": [{"base": {"contents": "base\n", "exists": true, "isexec": false, "issymlink": false}, "local": {"contents": "\u0000", "exists": true, "isexec": false, "issymlink": false}, "other": {"contents": "change\n", "exists": true, "isexec": false, "issymlink": false}, "output": {"contents": "\u0000", "exists": true, "isexec": false, "issymlink": false, "path": "$TESTTMP/cornercases/foo/foo/foo/foo/file"}, "path": "file"}],
    "pathconflicts": []
   }
  ]
Test case 11: Source is a symlink, dest is a file (base is still a file)
  $ cd ..
  $ reset
  $ rm file
  $ ln -s somepath file
  $ hg commit -Aqm "source"
  $ hg up -q 0
  $ echo "change" > file
  $ hg commit -Aqm "dest"
  $ hg up -q 2
  $ logg
  @  (2) dest
  |  affected: file
  |  deleted:
  |
  | o  (1) source
  |/   affected: file
  |    deleted:
  |
  o  (0) base
     affected: file
     deleted:
  

  $ hg rebase -d 2 -s 1
  rebasing 06aece48b59f "source"
  merging file
  warning: internal :merge cannot merge symlinks for file
  warning: 1 conflicts while merging file! (edit, then use 'hg resolve --mark')
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]
  $ cat -v file
  change
  $ hg resolve --tool=internal:dumpjson --all
  [
   {
    "command": "rebase",
    "command_details": {"cmd": "rebase", "to_abort": "rebase --abort", "to_continue": "rebase --continue"},
    "conflicts": [{"base": {"contents": "base\n", "exists": true, "isexec": false, "issymlink": false}, "local": {"contents": "change\n", "exists": true, "isexec": false, "issymlink": false}, "other": {"contents": "somepath", "exists": true, "isexec": false, "issymlink": true}, "output": {"contents": "change\n", "exists": true, "isexec": false, "issymlink": false, "path": "$TESTTMP/cornercases/foo/foo/foo/foo/file"}, "path": "file"}],
    "pathconflicts": []
   }
  ]
Test case 12: Source is a file, dest is a symlink (base is still a file)
  $ cd ..
  $ reset
  $ echo "change" > file
  $ hg commit -Aqm "source"
  $ hg up -q 0
  $ rm file
  $ ln -s somepath file
  $ hg commit -Aqm "dest"
  $ hg up -q 2
  $ logg
  @  (2) dest
  |  affected: file
  |  deleted:
  |
  | o  (1) source
  |/   affected: file
  |    deleted:
  |
  o  (0) base
     affected: file
     deleted:
  

  $ hg rebase -d 2 -s 1
  rebasing ec87889f5f90 "source"
  merging file
  warning: internal :merge cannot merge symlinks for file
  warning: 1 conflicts while merging file! (edit, then use 'hg resolve --mark')
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]
  $ test -f file && echo "Exists" || echo "Does not exist"
  Does not exist
  $ ls file
  file
  $ hg resolve --tool=internal:dumpjson --all
  [
   {
    "command": "rebase",
    "command_details": {"cmd": "rebase", "to_abort": "rebase --abort", "to_continue": "rebase --continue"},
    "conflicts": [{"base": {"contents": "base\n", "exists": true, "isexec": false, "issymlink": false}, "local": {"contents": "somepath", "exists": true, "isexec": false, "issymlink": true}, "other": {"contents": "change\n", "exists": true, "isexec": false, "issymlink": false}, "output": {"contents": "somepath", "exists": true, "isexec": false, "issymlink": true, "path": "$TESTTMP/cornercases/foo/foo/foo/foo/file"}, "path": "file"}],
    "pathconflicts": []
   }
  ]
  $ cd ..

command_details works correctly with commands that drop both a statefile and a
mergestate (like shelve):
  $ hg init command_details
  $ cd command_details
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > shelve=
  > [experimental]
  > evolution = createmarkers
  > EOF
  $ hg debugdrawdag <<'EOS'
  > b c
  > |/
  > a
  > EOS
  $ hg up -q c
  $ echo 'state' > b
  $ hg add -q
  $ hg shelve
  shelved as default
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg up -q b
  $ hg unshelve
  unshelving change 'default'
  rebasing shelved changes
  rebasing b0582bede31d "shelve changes to: c" (tip)
  merging b
  warning: 1 conflicts while merging b! (edit, then use 'hg resolve --mark')
  unresolved conflicts (see 'hg resolve', then 'hg unshelve --continue')
  [1]
  $ hg resolve --tool=internal:dumpjson --all
  [
   {
    "command": "unshelve",
    "command_details": {"cmd": "unshelve", "to_abort": "unshelve --abort", "to_continue": "unshelve --continue"},
    "conflicts": [{"base": {"contents": "", "exists": true, "isexec": false, "issymlink": false}, "local": {"contents": "b", "exists": true, "isexec": false, "issymlink": false}, "other": {"contents": "state\n", "exists": true, "isexec": false, "issymlink": false}, "output": {"contents": "<<<<<<< dest:   488e1b7e7341 b - test: b\nb=======\nstate\n>>>>>>> source: b0582bede31d - test: shelve changes to: c\n", "exists": true, "isexec": false, "issymlink": false, "path": "$TESTTMP/cornercases/foo/foo/foo/command_details/b"}, "path": "b"}],
    "pathconflicts": []
   }
  ]
