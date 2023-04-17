# Submodule

Sapling has basic support for Git submodules.

Sapling does not have a `submodule` command. Commands that change the working
copy like `goto` or `clone` will recursively change submodules. Other commands
like `commit`, `pull`, `status`, `diff` will treat a submodule as a special
file that only contains a commit hash. Those commands ignore files inside
submodules.

## Concepts

### Git submodule

A Git submodule has three basic properties: URL (where to fetch the submodule),
path (where to write to), and commit hash (which commit to use).

The URL and path are specified in the check-in file `.gitmodules`. The commit
hash is stored specially at the given path.

Depending on operations, a submodule might behave like a file or a repository.

### Submodule as a single file

When you run `diff`, `cat`, `status` or commands that directly or indirectly
ask for the content of a submodule, the submodule behaves like a single file
with the content `Subproject commit HASH`, it will not behave like a directory.

For example, `status` and `diff` only shows the commit hash change of
submodules. They do not show individual file changes inside the submodules.
`sl cat` treats file paths inside submodules as non existent.

When you run `commit`, a submodule is also treated as a single file with just
its commit hash. `commit` will not recursively make commits in submodules.
Same for `amend`.

### Submodule as a repository

When you run `goto`, `revert` or commands that ask Sapling to change the
working copy to match the content of a submodule, Sapling will pull the
submodule on demand, create the submodule repository on demand, and ask the
submodule repository to checkout the specified commit.

When you use `cd` to enter a submodule, the submodule works like a standalone
repository.

## Common operations

### Clone a repository with submodules

Sapling clones submodules recursively [^1]; there is no need to use flags like
`--recurse`, or use additional commands to initialize the submodules.

### Use a different commit in a submodule

Imagine you have a submodule at `third_party/fmt`. The submodule is currently
at commit `a337011`, and you want to use commit `1f575fd` instead. You can make
such change by running `sl goto` in the submodule:

```
$ cd third_party/fmt
$ sl goto 1f575fd
```

Now the parent repo will notice the change. `sl status` will show
`third_party/fmt` as "modified":

```
$ cd ../..
$ sl status
M third_party/fmt
```

You can run `sl diff` to double check the commit hash change is from
`a337011` to `1f575fd`:

```
$ sl diff
diff --git a/third_party/fmt b/third_party/fmt
--- a/third_party/fmt
+++ b/third_party/fmt
@@ -1,1 +1,1 @@
-Subproject commit a33701196adfad74917046096bf5a2aa0ab0bb50
+Subproject commit 1f575fd5c90278bcf723f72737f0f63c1951bea3
```

If you need to abandon changes in a submodule, use `revert`:

```
$ sl revert third_party/fmt
```

Finally, remember to commit the submodule change:

```
$ sl commit -m "Update third_party/fmt to 1f575fd"
```

Note `commit` only makes a single commit in the parent repo. It does not
recursively make commits in submodules. This is because the parent repo only
tracks the commit hashes of submodules and does not directly care about
changed files in submodules.

### Show changed files in a submodule

You can use `sl status` within a submodule to list changed files in that
submodule:

```
$ cd third_party/fmt
$ sl status
```

Running `sl status` from the parent repo will not list changed files in
submodule. Although changed files are not shown, changed commits are
always shown. You might want to always `sl commit` changes in submodules
so submodule changes can be detected from the parent repo when using
`sl status` or `sl diff`.

If you do need to list changed files in all submodules, you might want to
use a shell script like:

```bash
for i in `grep 'path =' .gitmodules | sed 's/.*=//'`; do sl status --pager=off --cwd $i; done
```

In the future we might add a convenient way to run `status` recursively in
submodules.


### Pull submodule changes

When you run `sl goto` from the parent repo, Sapling will pull required
submodule repos on demand in order to complete the `goto` operation.

Right now, Sapling might only pull the commit needed and will not pull branches
like `main` or `master`. If you want to pull branches explicitly, you can pull
it within the submodule:

```
$ cd path/to/submodule
$ sl pull -B main
```

If you run `sl pull` from the parent repo, Sapling does not pull submodule
repos recursively.

### Push submodule changes

You can push submodule changes to the remote server by running `sl push` within
the submodule:

```
$ cd path/to/submodule
$ sl push --to main
```

If you run `sl push` from the parent repo, Sapling does not push submodule
repos recursively.

### Add, remove, or rename a submodule

Right now, these are not supported. In the future we might make `sl clone`
detect the submodule use-case, and write the repo data to the right location,
and update `sl add`, `sl mv`, `sl rm` to update `.gitmodules` automatically.

[^1]: Submodules are not cloned like regular repos where there is usually a
    `remote/main` branch after clone. This is because Sapling attempts to pull
    by the commit hash to complete the working copy update. To obtain
    `remote/main` in a submodule, you can run `sl pull -B main`.
