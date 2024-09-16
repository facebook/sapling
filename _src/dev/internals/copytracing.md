# Bisect-Based Copy Tracing

*Copy tracing* is a technique used to efficiently account for file copies and
renames when comparing histories. It is used for `diff` commands and merge-related
operations, such as `rebase`, `graft`, and `merge`. It simplifies resolving merge conflicts,
especially when large refactors, like directory renames, occur in frequently updated
repositories. Below is an example of a rebase involving file renames:

```sl-shell-example
$ sl
@  d78558192  1 second ago  alyssa
│  update b
│
o  2b089a0d8  15 seconds ago  alyssa
│  mv a -> b
│
│ o  b0d1b083d  36 seconds ago  alyssa
├─╯  update a
│
o  5b0d97d5a  46 seconds ago  alyssa
   add a
```

Without copy tracing, `sl` previously had to ask about renamed or copied files
during rebases:

```sl-shell-example
$ sl rebase -s b0d1b083d -d d78558192
rebasing b0d1b083d791 "update a"
other [source (being rebased)] changed a which local [dest (rebasing onto)] is missing
hint: if this is due to a renamed file, you can manually input the renamed path
use (c)hanged version, leave (d)eleted, or leave (u)nresolved, or input (r)enamed path?
```

With copy tracing, the merge *just works*, despite there being copied or renamed
files:

```sl-shell-example
$ sl rebase -s b0d1b083d -d d78558192
rebasing b0d1b083d791 "update a"
merging b and a to b
b0d1b083d791 -> 92444cbb366b "update a"
```

## Background

Historically, Sapling has used two copy-tracing solutions. However, as our
monorepos grow ([tens of millions of files, tens of millions of commits](https://engineering.fb.com/2022/11/15/open-source/sapling-source-control-scalable/)),
the previous solutions have become too slow for production use:
- **Full copy tracing** finds all the new files (M) that were added from
  merge base up to the top commit and for each file it checks if this file
  was copied from another file (N). For each pair of files, Sapling was walking
  through the file history (H) to check the copy-from the relationship.
  - The time complexity of this algorithm is `O(M * N * H)`, where N and H are huge.
    - Typically M (source) << N (destination)
  - Sapling records rename information directly in file headers, eliminating the
    need to compute file content similarity, which is different from Git's approach.
- **Heuristics copy tracing** assumes that moves or renames fall into one of two
  categories: (1) Within the same directory (same directory name but different
  file names); (2) Move from one directory to another (same file names but
  different directory names)
  - This approach reduces N to K (K is a configured constant value), resulting
    in a time complexity of `O(M * H)`, where H remains large and there is a large
    constant factor for reducing N to K.
  - Another issue is that if the renames do not match the heuristics, they
    cannot be found.

Before we explore bisect-based copy tracing, let's first examine how
[Git's rename detection](https://github.com/newren/presentations/blob/pdfs/merge-performance/merge-performance-slides.pdf)
works. Git's rename detection is similar to the heuristics copy tracing
mentioned earlier, but it includes additional heuristics and strategies
to enhance performance, such as "Remembering previous work", "Exact renames".
  - The time complexity is `O(M * S)`, where M is the same as above,
    S is the complexity of file content similarity computation.
  - It also shares the disadvantage of Sapling heuristics copy tracing
    when renames do not match heuristics. Otherwise, the time complexity
    will be `O(M * N * S)`.

Bisect-based copy tracing is built to achieve the following desired properties:
- **Scalability**: `O(M * log H)` time complexity, it bisects the file history
  rather than scanning commits sequentially.
- **Flexibility**: Not restricted by heuristics like 'Move from one directory to another'.
- **Abstracted**: Support both Sapling and Git backend repositories.
- **Efficiency**: Provides fast content similarity checks for cases where renames
  are not recorded in Sapling or when working with Git repositories.
- **User-Friendly**: Informative message when renames cannot be found, such as
  delete/modified conflicts.

## How?

### Scalability
The problem that copy tracing solves is: **given two commits, C1 and C2, and a path P1 in C1, we need to find the renamed path P2 in C2**.

This problem required a new algorithmic design to scale efficiently. The
basic idea is to break the problem into two steps:
- Bisect a commit `C3` that deletes `P1` in the `C1` to `C2` range.
- Examine `C3`, find what path `P1` was renamed to. If that path exists in `C2`,
  then we’re done. Otherwise recursively trace renames in the `C3` to `C2` range.

The efficient bisect is based on the [Segmented Changelog](https://github.com/facebook/sapling/blob/main/eden/scm/slides/201904-segmented-changelog/segmented-changelog.pdf) we developed for lazy commit graph downloading and improving DAG operations,
please check [this blog post](https://engineering.fb.com/2022/11/15/open-source/sapling-source-control-scalable/)
to learn more about Segmented Changelog.

### Flexibility
Since Sapling can efficiently trace rename commits by bisecting the history,
and then find the renames in a rename commit, we don't need heuristics to
reduce the large number N on destination side. This approach allows Sapling
to detect renames that would otherwise be missed by heuristics-based methods.

### Abstracted
We made the rename detection inside a commit abstracted. Whether it’s Sapling’s
tracked rename, or Git’s implicit content similar rename, or a combination
of them, they fit in the same abstraction and can be flexibly configured.

### Efficiency
Typical content similarity libraries often degrade to `O(N^2)` in the worst case,
where N is the line count (`O(N^2)` is the worst case for the Myers diff algorithm).
Our approach, `xdiff::edit_cost`, imposes a `max cost` limit, reducing the
complexity to `O(N)`.

### User-Friendly
When renames cannot be found, for example, file `a.txt` was renamed to `a.md`
and then deleted on the destination branch, the new copy tracing can identify
both the commit that renamed the file and also the commit that eventually
deleted it. This allows us to provide additional context to help resolve the
conflict:

```sl-shell-example
$ sl rebase -s 108b59d42 -d a1fcdc96b
...
other [source (being rebased)] changed a.txt which local [dest (rebasing onto)] is missing
hint: the missing file was probably deleted by commit 7f48dc97d540 with name 'a.md' in the branch rebasing onto
use (c)hanged version, leave (d)eleted, or leave (u)nresolved, or input (r)enamed path?
```
