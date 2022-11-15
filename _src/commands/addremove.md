---
sidebar_position: 2
---

## addremove | addrm
<!--
  @generated SignedSource<<031c73bb234acc01ca8218d57e286b85>>
  Run `./scripts/generate-command-markdown.py` to regenerate.
-->


**add all new files, delete all missing files**

Start tracking all new files and stop tracking all missing files
in the working copy. As with `sl add`, these changes take
effect at the next commit.

Unless file names are given, new files are ignored if they match any of
the patterns in `.gitignore`.

Use the `-s/--similarity` option to detect renamed files. This
option takes a percentage between 0 (disabled) and 100 (files must
be identical) as its parameter. With a parameter greater than 0,
this compares every removed file with every added file and records
those similar enough as renames. Detecting renamed files this way
can be expensive. After using this option, `sl status -C` can be
used to check which files were identified as moved or renamed. If
not specified, `-s/--similarity` defaults to 100 and only renames of
identical files are detected.

Examples:

- Files bar.c and foo.c are new,  while foobar.c has been removed (without using `sl remove`)  from the repository:

```
$ ls
bar.c foo.c
$ sl status
! foobar.c
? bar.c
? foo.c
$ sl addremove
adding bar.c
adding foo.c
removing foobar.c
$ sl status
A bar.c
A foo.c
R foobar.c
```

- A file foobar.c was moved to foo.c without using `sl rename`.  Afterwards, it was edited slightly:

```
$ ls
foo.c
$ sl status
! foobar.c
? foo.c
$ sl addremove --similarity 90
removing foobar.c
adding foo.c
recording removal of foobar.c as rename to foo.c (94% similar)
$ sl status -C
A foo.c
  foobar.c
R foobar.c
```

Returns 0 if all files are successfully added/removed.

## arguments
| shortname | fullname | default | description |
| - | - | - | - |
| `-s`| `--similarity`| | guess renamed files by similarity (0<=s<=100)|
| `-I`| `--include`| | include files matching the given patterns|
| `-X`| `--exclude`| | exclude files matching the given patterns|
| `-n`| `--dry-run`| | do not perform actions, just print output|
