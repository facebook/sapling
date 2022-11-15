---
sidebar_position: 18
---

## histedit
<!--
  @generated SignedSource<<fe9784db48ccaef4caf1e6c8024b898f>>
  Run `./scripts/generate-command-markdown.py` to regenerate.
-->


**interactively reorder, combine, or delete commits**

This command lets you edit a linear series of commits up to
and including the working copy, which should be clean.
You can:

- `pick` to (re)order a commit

- `drop` to omit a commit

- `mess` to reword a commit message

- `fold` to combine a commit with the preceding commit, using the later date

- `roll` like fold, but discarding this commit's description and date

- `edit` to edit a commit, preserving date

- `base` to checkout a commit and continue applying subsequent commits

There are multiple ways to select the root changeset:

- Specify ANCESTOR directly

- Otherwise, the value from the `histedit.defaultrev` config option  is used as a revset to select the base commit when ANCESTOR is not  specified. The first commit returned by the revset is used. By  default, this selects the editable history that is unique to the  ancestry of the working directory.

Examples:

- A number of changes have been made.  Commit `a113a4006` is no longer needed.

Start history editing from commit a:

```
sl histedit -r a113a4006
```

An editor opens, containing the list of commits,
with specific actions specified:

```
pick a113a4006 Zworgle the foobar
pick 822478b68 Bedazzle the zerlog
pick d275e7ed9 5 Morgify the cromulancy
```

Additional information about the possible actions
to take appears below the list of commits.

To remove commit a113a4006 from the history,
its action (at the beginning of the relevant line)
is changed to `drop`:

```
drop a113a4006 Zworgle the foobar
pick 822478b68 Bedazzle the zerlog
pick d275e7ed9 Morgify the cromulancy
```

- A number of changes have been made.  Commit fe2bff2ce and c9116c09e need to be swapped.

Start history editing from commit fe2bff2ce:

```
sl histedit -r fe2bff2ce
```

An editor opens, containing the list of commits,
with specific actions specified:

```
pick fe2bff2ce Blorb a morgwazzle
pick 99a93da65 Zworgle the foobar
pick c9116c09e Bedazzle the zerlog
```

To swap commits fe2bff2ce and c9116c09e, simply swap their lines:

```
pick 8ef592ce7cc4 4 Bedazzle the zerlog
pick 5339bf82f0ca 3 Zworgle the foobar
pick 252a1af424ad 2 Blorb a morgwazzle
```

Returns 0 on success, 1 if user intervention is required for
`edit` command or to resolve merge conflicts.

## arguments
| shortname | fullname | default | description |
| - | - | - | - |
| | `--commands`| | read history edits from the specified file|
| `-c`| `--continue`| `false`| continue an edit already in progress|
| | `--edit-plan`| `false`| edit remaining actions list|
| `-k`| `--keep`| `false`| don't strip old nodes after edit is complete|
| | `--abort`| `false`| abort an edit in progress|
| `-r`| `--rev`| | first revision to be edited|
| `-x`| `--retry`| `false`| retry exec command that failed and try to continue|
| | `--show-plan`| `false`| show remaining actions list|
