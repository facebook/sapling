---
sidebar_position: 7
---

## cat
<!--
  @generated SignedSource<<2784a15077e1fbbbb458e4724f1a4011>>
  Run `./scripts/generate-command-markdown.py` to regenerate.
-->


**output file content at a particular revision**

Output the specified files&#x27; content at the specified revision. If
no revision is given, the parent of the working directory is used.

Use `--output` to write files or directories to disk using the following
formatting rules:

`%%`
literal "%" character

`%s`
basename of file being printed

`%d`
dirname of file being printed, or '.' if in repository root

`%p`
root-relative path name of file being printed

`%H`
commit hash (40 hexadecimal digits)

`%h`
short commit hash (12 hexadecimal digits)

`%b`
basename of the repository

Examples:

- Recursively export directory foo/bar to disk:

```
sl cat -r fbc6b8c381 --output "/tmp/export/%p" path:foo/bar
```

- Output all Rust files' content under foo/bar to stdout:

```
sl cat -r fbc6b8c381 "glob:foo/bar/**/*.rs"
```

- Output the content of something/important.txt at bookmark main to /tmp/file:

```
sl cat -r main --output /tmp/file something/important.txt
```

To operate without a local repo, specify `-R/--repository` as a Sapling
Remote API capable URL. The local on-disk cache will still be used to avoid
remote fetches.

See `sl help patterns` for more information on specifying file patterns.

Returns 0 if there were no errors and at least one file was output.

## arguments
| shortname | fullname | default | description |
| - | - | - | - |
| `-o`| `--output`| | print output to file with formatted name|
| `-r`| `--rev`| | print the given revision|
| `-I`| `--include`| | include files matching the given patterns|
| `-X`| `--exclude`| | exclude files matching the given patterns|
