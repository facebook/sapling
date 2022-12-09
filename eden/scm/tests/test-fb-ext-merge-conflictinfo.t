#chg-compatible

  $ configure modernclient
  $ configure mutation-norecord
  $ enable conflictinfo rebase

1) Make the repo
  $ newclientrepo basic

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
  $ hg log -G -T '({node}) {desc}\nbookmark: {bookmarks}\nfiles: {files}\n\n'
  @  (13124abb51b9fbac518b2b8722df68e012ecfc58) second version, b
  │  bookmark: b
  │  files: F1 F2
  │
  │ o  (6dd692b7db4a573115a661237cb90b506bccc45d) first version, a
  ├─╯  bookmark: a
  │    files: F1 F2
  │
  o  (fd428402857cd43d472566f429df85e40be9cb2a) initial commit
     bookmark:
     files: F1 F2
  
  $ hg merge a
  merging F1
  merging F2
  warning: 1 conflicts while merging F1! (edit, then use 'hg resolve --mark')
  warning: 1 conflicts while merging F2! (edit, then use 'hg resolve --mark')
  0 files updated, 0 files merged, 0 files removed, 2 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg goto -C .' to abandon
  [1]

5) Get the paths:
  $ hg resolve --tool internal:dumpjson --all
  [
   {
    "command": "merge",
    "command_details": {"cmd": "merge", "to_abort": "goto --clean", "to_continue": "merge --continue"},
    "conflicts": [{"base": {"contents": "Unconflicted base, F1\n", "exists": true, "isexec": false, "issymlink": false}, "local": {"contents": "Second conflicted version, F1\n", "exists": true, "isexec": false, "issymlink": false}, "other": {"contents": "First conflicted version, F1\n", "exists": true, "isexec": false, "issymlink": false}, "output": {"contents": "<<<<<<< working copy: 13124abb51b9 b - test: second version, b\nSecond conflicted version, F1\n=======\nFirst conflicted version, F1\n>>>>>>> merge rev:    6dd692b7db4a a - test: first version, a\n", "exists": true, "isexec": false, "issymlink": false, "path": "$TESTTMP/basic/F1"}, "path": "F1"}, {"base": {"contents": "Unconflicted base, F2\n", "exists": true, "isexec": false, "issymlink": false}, "local": {"contents": "Second conflicted version, F2\n", "exists": true, "isexec": false, "issymlink": false}, "other": {"contents": "First conflicted version, F2\n", "exists": true, "isexec": false, "issymlink": false}, "output": {"contents": "<<<<<<< working copy: 13124abb51b9 b - test: second version, b\nSecond conflicted version, F2\n=======\nFirst conflicted version, F2\n>>>>>>> merge rev:    6dd692b7db4a a - test: first version, a\n", "exists": true, "isexec": false, "issymlink": false, "path": "$TESTTMP/basic/F2"}, "path": "F2"}],
    "pathconflicts": []
   }
  ]

6) Only requested paths get dumped
  $ hg resolve --tool internal:dumpjson F2
  [
   {
    "command": "merge",
    "command_details": {"cmd": "merge", "to_abort": "goto --clean", "to_continue": "merge --continue"},
    "conflicts": [{"base": {"contents": "Unconflicted base, F2\n", "exists": true, "isexec": false, "issymlink": false}, "local": {"contents": "Second conflicted version, F2\n", "exists": true, "isexec": false, "issymlink": false}, "other": {"contents": "First conflicted version, F2\n", "exists": true, "isexec": false, "issymlink": false}, "output": {"contents": "<<<<<<< working copy: 13124abb51b9 b - test: second version, b\nSecond conflicted version, F2\n=======\nFirst conflicted version, F2\n>>>>>>> merge rev:    6dd692b7db4a a - test: first version, a\n", "exists": true, "isexec": false, "issymlink": false, "path": "$TESTTMP/basic/F2"}, "path": "F2"}],
    "pathconflicts": []
   }
  ]

7) Ensure the paths point to the right contents:
  $ getcontents() { # Usage: getcontents <path> <version>
  >  local script="import sys, json; ui.writebytes(('%s\n' % json.load(sys.stdin)[0][\"conflicts\"][$1][\"$2\"][\"contents\"]).encode('utf-8'))"
  >  hg resolve --tool internal:dumpjson --all | hg debugsh -c "$script"
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

  $ reset() {
  >   cd $TESTTMP
  >   rm -rf foo
  >   newclientrepo foo
  >   echo "base" > file
  >   hg commit -Aqm "base"
  > }
  $ logg() {
  >   hg log -G -T '({node}) {desc}\naffected: {files}\ndeleted: {file_dels}\n\n'
  > }

Test case 0: A merge of just contents conflicts (not usually a corner case),
but the user had local changes and ran `merge -f`.

tldr: Since we can premerge, the working copy is backed up to an origfile.
  $ reset
  $ echo "change" > file
  $ hg commit -Aqm "dest"
  $ hg up -q 'desc(base)'
  $ echo "other change" > file
  $ hg commit -Aqm "source"
  $ hg up -q 'desc(dest)'
  $ logg
  o  (9b65ba2922f0e466c10e5344d8691afa631e353b) source
  │  affected: file
  │  deleted:
  │
  │ @  (fd7d10c36158e4f6e713ca1c40ddebce2b55a868) dest
  ├─╯  affected: file
  │    deleted:
  │
  o  (01813a66ce08dcc7d684f337c68bd61a4982de10) base
     affected: file
     deleted:
  
  $ echo "some local changes" > file
  $ hg merge 'desc(source)' -f
  merging file
  warning: 1 conflicts while merging file! (edit, then use 'hg resolve --mark')
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg goto -C .' to abandon
  [1]

  $ hg resolve --tool=internal:dumpjson --all
  [
   {
    "command": "merge",
    "command_details": {"cmd": "merge", "to_abort": "goto --clean", "to_continue": "merge --continue"},
    "conflicts": [{"base": {"contents": "base\n", "exists": true, "isexec": false, "issymlink": false}, "local": {"contents": "some local changes\n", "exists": true, "isexec": false, "issymlink": false}, "other": {"contents": "other change\n", "exists": true, "isexec": false, "issymlink": false}, "output": {"contents": "<<<<<<< working copy: fd7d10c36158 - test: dest\nsome local changes\n=======\nother change\n>>>>>>> merge rev:    9b65ba2922f0 - test: source\n", "exists": true, "isexec": false, "issymlink": false, "path": "$TESTTMP/foo/file"}, "path": "file"}],
    "pathconflicts": []
   }
  ]

Test case 0b: Like #0 but with a corner case: source deleted, local changed
*and* had local changes using merge -f.

tldr: Since we couldn't premerge, the working copy is left alone.
  $ reset
  $ echo "change" > file
  $ hg commit -Aqm "dest"
  $ hg up -q 'desc(base)'
  $ hg rm file
  $ hg commit -Aqm "source"
  $ hg up -q 'desc(dest)'
  $ logg
  o  (25c2ef28f4c763dd5068d3aa96cafa1342fe5280) source
  │  affected: file
  │  deleted: file
  │
  │ @  (fd7d10c36158e4f6e713ca1c40ddebce2b55a868) dest
  ├─╯  affected: file
  │    deleted:
  │
  o  (01813a66ce08dcc7d684f337c68bd61a4982de10) base
     affected: file
     deleted:
  
  $ echo "some local changes" > file
  $ chmod +x file
  $ hg merge 'desc(source)' -f
  local [working copy] changed file which other [merge rev] deleted
  use (c)hanged version, (d)elete, or leave (u)nresolved? u
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg goto -C .' to abandon
  [1]

  $ hg resolve --tool=internal:dumpjson --all
  [
   {
    "command": "merge",
    "command_details": {"cmd": "merge", "to_abort": "goto --clean", "to_continue": "merge --continue"},
    "conflicts": [{"base": {"contents": "base\n", "exists": true, "isexec": false, "issymlink": false}, "local": {"contents": "some local changes\n", "exists": true, "isexec": true, "issymlink": false}, "other": {"contents": null, "exists": false, "isexec": null, "issymlink": null}, "output": {"contents": "some local changes\n", "exists": true, "isexec": true, "issymlink": false, "path": "$TESTTMP/foo/file"}, "path": "file"}],
    "pathconflicts": []
   }
  ]

Test case 1: Source deleted, dest changed
  $ reset
  $ echo "change" > file
  $ hg commit -Aqm "dest"
  $ hg up -q 'desc(base)'
  $ hg rm file
  $ hg commit -Aqm "source"
  $ hg up -q 'desc(dest)'
  $ logg
  o  (25c2ef28f4c763dd5068d3aa96cafa1342fe5280) source
  │  affected: file
  │  deleted: file
  │
  │ @  (fd7d10c36158e4f6e713ca1c40ddebce2b55a868) dest
  ├─╯  affected: file
  │    deleted:
  │
  o  (01813a66ce08dcc7d684f337c68bd61a4982de10) base
     affected: file
     deleted:
  

  $ hg rebase -d 'desc(dest)' -s 'desc(source)'
  rebasing 25c2ef28f4c7 "source"
  local [dest] changed file which other [source] deleted
  use (c)hanged version, (d)elete, or leave (u)nresolved? u
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]
  $ hg resolve --tool=internal:dumpjson --all
  [
   {
    "command": "rebase",
    "command_details": {"cmd": "rebase", "to_abort": "rebase --abort", "to_continue": "rebase --continue"},
    "conflicts": [{"base": {"contents": "base\n", "exists": true, "isexec": false, "issymlink": false}, "local": {"contents": "change\n", "exists": true, "isexec": false, "issymlink": false}, "other": {"contents": null, "exists": false, "isexec": null, "issymlink": null}, "output": {"contents": "change\n", "exists": true, "isexec": false, "issymlink": false, "path": "$TESTTMP/foo/file"}, "path": "file"}],
    "pathconflicts": []
   }
  ]
Test case 1b: Like #1 but with a merge, with local changes
  $ reset
  $ echo "change" > file
  $ hg commit -Aqm "dest"
  $ hg up -q 'desc(base)'
  $ hg rm file
  $ hg commit -Aqm "source"
  $ hg up -q 'desc(dest)'
  $ logg
  o  (25c2ef28f4c763dd5068d3aa96cafa1342fe5280) source
  │  affected: file
  │  deleted: file
  │
  │ @  (fd7d10c36158e4f6e713ca1c40ddebce2b55a868) dest
  ├─╯  affected: file
  │    deleted:
  │
  o  (01813a66ce08dcc7d684f337c68bd61a4982de10) base
     affected: file
     deleted:
  
  $ echo "some local changes" > file
  $ hg merge 'desc(source)' -f
  local [working copy] changed file which other [merge rev] deleted
  use (c)hanged version, (d)elete, or leave (u)nresolved? u
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg goto -C .' to abandon
  [1]

  $ hg resolve --tool=internal:dumpjson --all
  [
   {
    "command": "merge",
    "command_details": {"cmd": "merge", "to_abort": "goto --clean", "to_continue": "merge --continue"},
    "conflicts": [{"base": {"contents": "base\n", "exists": true, "isexec": false, "issymlink": false}, "local": {"contents": "some local changes\n", "exists": true, "isexec": false, "issymlink": false}, "other": {"contents": null, "exists": false, "isexec": null, "issymlink": null}, "output": {"contents": "some local changes\n", "exists": true, "isexec": false, "issymlink": false, "path": "$TESTTMP/foo/file"}, "path": "file"}],
    "pathconflicts": []
   }
  ]
Test case 2: Source changed, dest deleted
  $ reset
  $ echo "change" > file
  $ hg commit -Aqm "source"
  $ hg up -q 'desc(base)'
  $ hg rm file
  $ hg commit -Aqm "dest"
  $ hg up --q 'desc(dest)'
  $ logg
  @  (66a38a15024ce5297f27bab5b7f17870de6d0d96) dest
  │  affected: file
  │  deleted: file
  │
  │ o  (ec87889f5f908dd874cf31122628f081037e4bf5) source
  ├─╯  affected: file
  │    deleted:
  │
  o  (01813a66ce08dcc7d684f337c68bd61a4982de10) base
     affected: file
     deleted:
  

  $ hg rebase -d 'desc(dest)' -s 'desc(source)'
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
    "conflicts": [{"base": {"contents": "base\n", "exists": true, "isexec": false, "issymlink": false}, "local": {"contents": null, "exists": false, "isexec": null, "issymlink": null}, "other": {"contents": "change\n", "exists": true, "isexec": false, "issymlink": false}, "output": {"contents": null, "exists": false, "isexec": null, "issymlink": null, "path": "$TESTTMP/foo/file"}, "path": "file"}],
    "pathconflicts": []
   }
  ]
Test case 3: Source changed, dest moved
  $ reset
  $ echo "change" > file
  $ hg commit -Aqm "source"
  $ hg up -q 'desc(base)'
  $ hg mv file file_newloc
  $ hg commit -Aqm "dest"
  $ hg up -q 'desc(dest)'
  $ logg
  @  (d168768b462ba7bdf7d27a2c2e317362498a0a65) dest
  │  affected: file file_newloc
  │  deleted: file
  │
  │ o  (ec87889f5f908dd874cf31122628f081037e4bf5) source
  ├─╯  affected: file
  │    deleted:
  │
  o  (01813a66ce08dcc7d684f337c68bd61a4982de10) base
     affected: file
     deleted:
  

  $ hg rebase -d 'desc(dest)' -s 'desc(source)'
  rebasing ec87889f5f90 "source"
  merging file_newloc and file to file_newloc
  $ hg up -q 'desc(source)' # source
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
  $ reset
  $ echo "change" > file
  $ hg commit -Aqm "source"
  $ hg up -q 'desc(base)'
  $ hg mv file file_newloc
  $ hg commit -Aqm "dest"
  $ hg up -q 'desc(dest)'
  $ logg
  @  (d168768b462ba7bdf7d27a2c2e317362498a0a65) dest
  │  affected: file file_newloc
  │  deleted: file
  │
  │ o  (ec87889f5f908dd874cf31122628f081037e4bf5) source
  ├─╯  affected: file
  │    deleted:
  │
  o  (01813a66ce08dcc7d684f337c68bd61a4982de10) base
     affected: file
     deleted:
  

  $ hg rebase -d 'desc(dest)' -s 'desc(source)' --config experimental.copytrace=off
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
    "conflicts": [{"base": {"contents": "base\n", "exists": true, "isexec": false, "issymlink": false}, "local": {"contents": null, "exists": false, "isexec": null, "issymlink": null}, "other": {"contents": "change\n", "exists": true, "isexec": false, "issymlink": false}, "output": {"contents": null, "exists": false, "isexec": null, "issymlink": null, "path": "$TESTTMP/foo/file"}, "path": "file"}],
    "pathconflicts": []
   }
  ]
Test case 5: Source moved, dest changed
  $ reset
  $ hg mv file file_newloc
  $ hg commit -Aqm "source"
  $ hg up -q 'desc(base)'
  $ echo "change" > file
  $ hg commit -Aqm "dest"
  $ hg up -q 'desc(dest)'
  $ logg
  @  (fd7d10c36158e4f6e713ca1c40ddebce2b55a868) dest
  │  affected: file
  │  deleted:
  │
  │ o  (e6e7483a895027a7b6f8146011cce3b46ef5d8d6) source
  ├─╯  affected: file file_newloc
  │    deleted: file
  │
  o  (01813a66ce08dcc7d684f337c68bd61a4982de10) base
     affected: file
     deleted:
  

  $ hg rebase -d 'desc(dest)' -s 'desc(source)'
  rebasing e6e7483a8950 "source"
  merging file and file_newloc to file_newloc
  $ hg up 'desc(source)'
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
  $ reset
  $ hg mv file file_newloc
  $ hg commit -Aqm "source"
  $ hg up -q 'desc(base)'
  $ echo "change" > file
  $ hg commit -Aqm "dest"
  $ hg up -q 'desc(dest)'
  $ logg
  @  (fd7d10c36158e4f6e713ca1c40ddebce2b55a868) dest
  │  affected: file
  │  deleted:
  │
  │ o  (e6e7483a895027a7b6f8146011cce3b46ef5d8d6) source
  ├─╯  affected: file file_newloc
  │    deleted: file
  │
  o  (01813a66ce08dcc7d684f337c68bd61a4982de10) base
     affected: file
     deleted:
  

  $ hg rebase -d 'desc(dest)' -s 'desc(source)' --config experimental.copytrace=off
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
    "conflicts": [{"base": {"contents": "base\n", "exists": true, "isexec": false, "issymlink": false}, "local": {"contents": "change\n", "exists": true, "isexec": false, "issymlink": false}, "other": {"contents": null, "exists": false, "isexec": null, "issymlink": null}, "output": {"contents": "change\n", "exists": true, "isexec": false, "issymlink": false, "path": "$TESTTMP/foo/file"}, "path": "file"}],
    "pathconflicts": []
   }
  ]
Test case 7: Source is a directory, dest is a file (base is still a file)
  $ reset
  $ hg rm file
  $ mkdir file # "file" is a stretch
  $ echo "this will cause problems" >> file/subfile
  $ hg commit -Aqm "source"
  $ hg up -q 'desc(base)'
  $ echo "change" > file
  $ hg commit -Aqm "dest"
  $ hg up -q 'desc(dest)'
  $ logg
  @  (fd7d10c36158e4f6e713ca1c40ddebce2b55a868) dest
  │  affected: file
  │  deleted:
  │
  │ o  (8679c40703d2db639fc4f0a9409ed58f0e6f0809) source
  ├─╯  affected: file file/subfile
  │    deleted: file
  │
  o  (01813a66ce08dcc7d684f337c68bd61a4982de10) base
     affected: file
     deleted:
  

  $ hg rebase -d 'desc(dest)' -s 'desc(source)'
  rebasing * "source" (glob)
  abort:*: $TESTTMP/foo/file (glob)
  (current process runs with uid 42) (?)
  ($TESTTMP/foo/file: mode 0o52, uid 42, gid 42) (?)
  ($TESTTMP/foo: mode 0o52, uid 42, gid 42) (?)
  [255]
  $ hg resolve --tool=internal:dumpjson --all
  [abort:*: $TESTTMP/foo/file (glob)
  (current process runs with uid 42) (?)
  ($TESTTMP/foo/file: mode 0o52, uid 42, gid 42) (?)
  ($TESTTMP/foo: mode 0o52, uid 42, gid 42) (?)
  [255]
Test case 8: Source is a file, dest is a directory (base is still a file)
  $ reset
  $ echo "change" > file
  $ hg commit -Aqm "source"
  $ hg up -q 'desc(base)'
  $ hg rm file
  $ mkdir file # "file"
  $ echo "this will cause problems" >> file/subfile
  $ hg commit -Aqm "dest"
  $ hg up -q 'desc(dest)'
  $ logg
  @  (1803169f37a9243ff3ba460d0cc4b95347fa0d82) dest
  │  affected: file file/subfile
  │  deleted: file
  │
  │ o  (ec87889f5f908dd874cf31122628f081037e4bf5) source
  ├─╯  affected: file
  │    deleted:
  │
  o  (01813a66ce08dcc7d684f337c68bd61a4982de10) base
     affected: file
     deleted:
  

  $ hg rebase -d 'desc(dest)' -s 'desc(source)'
  rebasing ec87889f5f90 "source"
  abort:*: $TESTTMP/foo/file (glob)
  (current process runs with uid 42) (?)
  ($TESTTMP/foo/file: mode 0o52, uid 42, gid 42) (?)
  ($TESTTMP/foo: mode 0o52, uid 42, gid 42) (?)
  [255]
  $ hg resolve --tool=internal:dumpjson --all
  [
   {
    "command": "rebase",
    "command_details": {"cmd": "rebase", "to_abort": "rebase --abort", "to_continue": "rebase --continue"},
    "conflicts": [{"base": {"contents": "base\n", "exists": true, "isexec": false, "issymlink": false}, "local": {"contents": null, "exists": false, "isexec": null, "issymlink": null}, "other": {"contents": "change\n", "exists": true, "isexec": false, "issymlink": false}, "output": {"contents": null, "exists": false, "isexec": null, "issymlink": null, "path": "$TESTTMP/foo/file"}, "path": "file"}],
    "pathconflicts": []
   }
  ]
Test case 9: Source is a binary file, dest is a file (base is still a file)
  $ reset
  $ printf '\0' > file
  $ hg commit -Aqm "source"
  $ hg up -q 'desc(base)'
  $ echo "change" > file
  $ hg commit -Aqm "dest"
  $ hg up -q 'desc(dest)'
  $ logg
  @  (fd7d10c36158e4f6e713ca1c40ddebce2b55a868) dest
  │  affected: file
  │  deleted:
  │
  │ o  (b6e55a03a5dc98e4ce5ef82f8f967f2188b32608) source
  ├─╯  affected: file
  │    deleted:
  │
  o  (01813a66ce08dcc7d684f337c68bd61a4982de10) base
     affected: file
     deleted:
  

  $ hg rebase -d 'desc(dest)' -s 'desc(source)'
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
    "conflicts": [{"base": {"contents": "base\n", "exists": true, "isexec": false, "issymlink": false}, "local": {"contents": "change\n", "exists": true, "isexec": false, "issymlink": false}, "other": {"contents": "\u0000", "exists": true, "isexec": false, "issymlink": false}, "output": {"contents": "change\n", "exists": true, "isexec": false, "issymlink": false, "path": "$TESTTMP/foo/file"}, "path": "file"}],
    "pathconflicts": []
   }
  ]
Test case 10: Source is a file, dest is a binary file (base is still a file)
  $ reset
  $ echo "change" > file
  $ hg commit -Aqm "source"
  $ hg up -q 'desc(base)'
  $ printf '\0' > file
  $ hg commit -Aqm "dest"
  $ hg up -q 'desc(dest)'
  $ logg
  @  (48fb032ebb6733fb9b62d0a11c1b4a538c4840a1) dest
  │  affected: file
  │  deleted:
  │
  │ o  (ec87889f5f908dd874cf31122628f081037e4bf5) source
  ├─╯  affected: file
  │    deleted:
  │
  o  (01813a66ce08dcc7d684f337c68bd61a4982de10) base
     affected: file
     deleted:
  

  $ hg rebase -d 'desc(dest)' -s 'desc(source)'
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
    "conflicts": [{"base": {"contents": "base\n", "exists": true, "isexec": false, "issymlink": false}, "local": {"contents": "\u0000", "exists": true, "isexec": false, "issymlink": false}, "other": {"contents": "change\n", "exists": true, "isexec": false, "issymlink": false}, "output": {"contents": "\u0000", "exists": true, "isexec": false, "issymlink": false, "path": "$TESTTMP/foo/file"}, "path": "file"}],
    "pathconflicts": []
   }
  ]
Test case 11: Source is a symlink, dest is a file (base is still a file)
  $ reset
  $ rm file
  $ ln -s somepath file
  $ hg commit -Aqm "source"
  $ hg up -q 'desc(base)'
  $ echo "change" > file
  $ hg commit -Aqm "dest"
  $ hg up -q 'desc(dest)'
  $ logg
  @  (fd7d10c36158e4f6e713ca1c40ddebce2b55a868) dest
  │  affected: file
  │  deleted:
  │
  │ o  (06aece48b59fc832b921a114492f962a5b358b22) source
  ├─╯  affected: file
  │    deleted:
  │
  o  (01813a66ce08dcc7d684f337c68bd61a4982de10) base
     affected: file
     deleted:
  

  $ hg rebase -d 'desc(dest)' -s 'desc(source)'
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
    "conflicts": [{"base": {"contents": "base\n", "exists": true, "isexec": false, "issymlink": false}, "local": {"contents": "change\n", "exists": true, "isexec": false, "issymlink": false}, "other": {"contents": "somepath", "exists": true, "isexec": false, "issymlink": true}, "output": {"contents": "change\n", "exists": true, "isexec": false, "issymlink": false, "path": "$TESTTMP/foo/file"}, "path": "file"}],
    "pathconflicts": []
   }
  ]
Test case 12: Source is a file, dest is a symlink (base is still a file)
  $ reset
  $ echo "change" > file
  $ hg commit -Aqm "source"
  $ hg up -q 'desc(base)'
  $ rm file
  $ ln -s somepath file
  $ hg commit -Aqm "dest"
  $ hg up -q 'desc(dest)'
  $ logg
  @  (c4bbf66fc0d73a7b05e64344fa86a678e19c35a2) dest
  │  affected: file
  │  deleted:
  │
  │ o  (ec87889f5f908dd874cf31122628f081037e4bf5) source
  ├─╯  affected: file
  │    deleted:
  │
  o  (01813a66ce08dcc7d684f337c68bd61a4982de10) base
     affected: file
     deleted:
  

  $ hg rebase -d 'desc(dest)' -s 'desc(source)'
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
    "conflicts": [{"base": {"contents": "base\n", "exists": true, "isexec": false, "issymlink": false}, "local": {"contents": "somepath", "exists": true, "isexec": false, "issymlink": true}, "other": {"contents": "change\n", "exists": true, "isexec": false, "issymlink": false}, "output": {"contents": "somepath", "exists": true, "isexec": false, "issymlink": true, "path": "$TESTTMP/foo/file"}, "path": "file"}],
    "pathconflicts": []
   }
  ]
  $ cd ..

command_details works correctly with commands that drop both a statefile and a
mergestate (like shelve):
  $ newclientrepo command_details
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
  shelved as c
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg up -q b
  $ hg unshelve
  unshelving change 'c'
  rebasing shelved changes
  rebasing b0582bede31d "shelve changes to: c"
  merging b
  warning: 1 conflicts while merging b! (edit, then use 'hg resolve --mark')
  unresolved conflicts (see 'hg resolve', then 'hg unshelve --continue')
  [1]
  $ hg resolve --tool=internal:dumpjson --all
  [
   {
    "command": "unshelve",
    "command_details": {"cmd": "unshelve", "to_abort": "unshelve --abort", "to_continue": "unshelve --continue"},
    "conflicts": [{"base": {"contents": "", "exists": true, "isexec": false, "issymlink": false}, "local": {"contents": "b", "exists": true, "isexec": false, "issymlink": false}, "other": {"contents": "state\n", "exists": true, "isexec": false, "issymlink": false}, "output": {"contents": "<<<<<<< dest:   488e1b7e7341 b - test: b\nb=======\nstate\n>>>>>>> source: b0582bede31d - test: shelve changes to: c\n", "exists": true, "isexec": false, "issymlink": false, "path": "$TESTTMP/command_details/b"}, "path": "b"}],
    "pathconflicts": []
   }
  ]
