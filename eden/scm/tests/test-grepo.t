#require git no-windows no-eden symlink

  $ . $TESTDIR/git.sh
  $ enable smartlog
  $ setconfig grepo.manifestpath=.repo/manifests/static/static.xml

Set up small project repos to simulate repos managed by the repo tool.
project-a is the outer project at vendor/a; project-c is a nested project at
vendor/a/sub/c. In real-world .repo workspaces, vendor/a/.git does not
manage vendor/a/sub/c because a .gitignore at vendor/a/sub/.gitignore
excludes its subdirectories.

Each project gets a second commit so the manifest history below can bump it.

  $ git init -q -b main project-a
  $ cd project-a
  $ echo "project-a content" > README
  $ git add README && git commit -qm 'init a'
  $ A_REV=$(git rev-parse HEAD)
  $ echo "project-a content v2" > README
  $ git add README && git commit -qm 'update a'
  $ A2_REV=$(git rev-parse HEAD)
  $ cd ..

  $ git init -q -b main project-b
  $ cd project-b
  $ echo "project-b content" > README
  $ git add README && git commit -qm 'init b'
  $ B_REV=$(git rev-parse HEAD)
  $ echo "project-b content v2" > README
  $ git add README && git commit -qm 'update b'
  $ B2_REV=$(git rev-parse HEAD)
  $ cd ..

  $ git init -q -b main project-c
  $ cd project-c
  $ echo "project-c content" > README
  $ git add README && git commit -qm 'init c'
  $ C_REV=$(git rev-parse HEAD)
  $ echo "project-c content v2" > README
  $ git add README && git commit -qm 'update c'
  $ C2_REV=$(git rev-parse HEAD)
  $ cd ..

  $ mkdir repodir && cd repodir

Set up .repo/manifests as its own git repo with .git symlinked to manifests.git:

  $ git init -q -b main .repo/manifests
  $ mv .repo/manifests/.git .repo/manifests.git
  $ ln -s ../manifests.git .repo/manifests/.git
  $ mkdir -p .repo/manifests/static
  $ cat > .repo/manifests/static/static.xml << EOF
  > <?xml version="1.0" encoding="UTF-8"?>
  > <manifest>
  >   <remote name="origin" fetch="file://$TESTTMP"/>
  >   <default revision="main" remote="origin"/>
  >   <project name="project-a" path="vendor/a" revision="$A_REV"/>
  >   <project name="project-b" path="frameworks/b" revision="$B_REV"/>
  >   <project name="project-c" path="vendor/a/sub/c" revision="$C_REV"/>
  > </manifest>
  > EOF
  $ cd .repo/manifests && git add static/static.xml && git commit -qm 'add manifest' && cd ../..

Three more manifest commits, each bumping a different project. The last one
bumps only the nested project vendor/a/sub/c. The sapling commit graph in a
grepo workspace is the .repo/manifests git history, so commits are made there:

  $ writemanifest() {
  >   cat > .repo/manifests/static/static.xml << EOF
  > <?xml version="1.0" encoding="UTF-8"?>
  > <manifest>
  >   <remote name="origin" fetch="file://$TESTTMP"/>
  >   <default revision="main" remote="origin"/>
  >   <project name="project-a" path="vendor/a" revision="$1"/>
  >   <project name="project-b" path="frameworks/b" revision="$2"/>
  >   <project name="project-c" path="vendor/a/sub/c" revision="$3"/>
  > </manifest>
  > EOF
  >   cd .repo/manifests && git add static/static.xml && git commit -qm "$4" && cd ../..
  > }

  $ writemanifest $A2_REV $B_REV $C_REV 'bump vendor/a'
  $ REV_AFTER_BUMP_A=$(cd .repo/manifests && git rev-parse HEAD)
  $ writemanifest $A2_REV $B2_REV $C_REV 'bump frameworks/b'
  $ REV_AFTER_BUMP_B=$(cd .repo/manifests && git rev-parse HEAD)
  $ writemanifest $A2_REV $B2_REV $C2_REV 'bump vendor/a/sub/c'
  $ REV_AFTER_BUMP_C=$(cd .repo/manifests && git rev-parse HEAD)

Set up projects with .git symlinks back to .repo/projects/:

  $ mkdir -p .repo/projects/vendor .repo/projects/frameworks
  $ git clone -q file://$TESTTMP/project-a vendor/a
  $ mv vendor/a/.git .repo/projects/vendor/a.git
  $ ln -s ../../.repo/projects/vendor/a.git vendor/a/.git

  $ git clone -q file://$TESTTMP/project-b frameworks/b
  $ mv frameworks/b/.git .repo/projects/frameworks/b.git
  $ ln -s ../../.repo/projects/frameworks/b.git frameworks/b/.git

vendor/a/.git does not manage vendor/a/sub/c because of the .gitignore at
vendor/a/sub/.gitignore. project-c is cloned in underneath as its own 
independent git repo:

  $ mkdir -p vendor/a/sub
  $ cat > vendor/a/sub/.gitignore << 'EOF'
  > # ignore all subdirs
  > */
  > EOF
  $ git clone -q file://$TESTTMP/project-c vendor/a/sub/c
  $ mv vendor/a/sub/c/.git .repo/projects/vendor/a/sub/c.git
  $ ln -s ../../../../.repo/projects/vendor/a/sub/c.git vendor/a/sub/c/.git

Add a top-level file:

  $ echo "top-level file" > BUILD

Add "enable_sl" file which is used as a config flag for identity:

  $ touch .repo/enable_sl

Sapling recognizes .repo identity
  $ sl root
  $TESTTMP/repodir

  $ sl smartlog -T '{desc}'
  @  bump vendor/a/sub/c
  │
  o  bump frameworks/b
  │
  o  bump vendor/a
  │
  o  add manifest

clean status
  $ sl status

  $ sl log -r . -T "desc:\n  {desc}\nfiles:\n{files % '  {file}\n'}"
  desc:
    bump vendor/a/sub/c
  files:
    vendor/a/sub/c

modified outer project is reported by status
  $ cd vendor/a
  $ echo "project vendor/a" > README
  $ git add README && git commit -qm 'add README'
  $ A_LOCAL_REV=$(git rev-parse HEAD)
  $ cd ../..
  $ sl status
  M vendor/a

Diff shows subproject commit change for the outer project:

  $ sl diff
  diff -r * vendor/a (glob)
  --- a/vendor/a	* (glob)
  +++ b/vendor/a	* (glob)
  @@ -1,1 +1,1 @@
  -Subproject commit 7d040f902e73e68e8ead5bd185e0efcb1adbeb55
  +Subproject commit 1f165e588b86d366379b684dedb0892249bebb89

Modified nested (overlapping) project is reported by status:

  $ cd vendor/a/sub/c
  $ echo "project vendor/a/sub/c" > README
  $ git add README && git commit -qm 'modify c'
  $ C_LOCAL_REV=$(git rev-parse HEAD)
  $ cd ../../../..
  $ sl status
  M vendor/a
  M vendor/a/sub/c

Exact-path diff also works for the nested overlapping project:

  $ sl diff vendor/a/sub/c
  diff -r * vendor/a/sub/c (glob)
  --- a/vendor/a/sub/c	* (glob)
  +++ b/vendor/a/sub/c	* (glob)
  @@ -1,1 +1,1 @@
  -Subproject commit 0b678834be64557c4e8710c49ef2fc96886a15a0
  +Subproject commit c55758fb8d7a24213ba3288ab808a839b6049513

Modified non-overlapping project is reported by status:

  $ cd frameworks/b
  $ echo "project frameworks/b" > README
  $ git add README && git commit -qm 'modify b'
  $ B_LOCAL_REV=$(git rev-parse HEAD)
  $ cd ../..
  $ sl status
  M frameworks/b
  M vendor/a
  M vendor/a/sub/c

Exact-path diff also works for the non-overlapping project:

  $ sl diff frameworks/b
  diff -r * frameworks/b (glob)
  --- a/frameworks/b	* (glob)
  +++ b/frameworks/b	* (glob)
  @@ -1,1 +1,1 @@
  -Subproject commit 434524b4d4743bcdf1e15d26adca081cfe8fd7d5
  +Subproject commit ab01d5a104b4500852675c2b96bd84773899b371

sl debuggitmodules lists grepo projects as Submodule entries.
Mainly used for ISL integration.

  $ sl debuggitmodules
  [submodule "project-b"]
  	url=
  	path=frameworks/b
  	ref=* (glob)
  	active=true
  [submodule "project-a"]
  	url=
  	path=vendor/a
  	ref=* (glob)
  	active=true
  [submodule "project-c"]
  	url=
  	path=vendor/a/sub/c
  	ref=* (glob)
  	active=true

(bad: blame doesn't work)
  $ sl blame vendor/a
  abort: vendor/a@000000000000: not found in manifest!
  [255]

Helpers for the `goto` tests below. Each test does 4 steps:
1. `reset_workspace` makes the workspace clean.
2. The test makes one kind of change.
3. The test runs `sl goto` with one flag.
4. `workspace_state` prints everything `goto` can change.

`map_rev_names` replaces each known hash with its variable name, like `A_REV`
or `REV_AFTER_BUMP_C`. All these variables must be set before you call it. An
empty one breaks the `sed` command.

  $ map_rev_names() {
  >   sed -e "s/$A_REV/A_REV/g" -e "s/$A2_REV/A2_REV/g" -e "s/$A_LOCAL_REV/A_LOCAL_REV/g" \
  >       -e "s/$B_REV/B_REV/g" -e "s/$B2_REV/B2_REV/g" -e "s/$B_LOCAL_REV/B_LOCAL_REV/g" \
  >       -e "s/$C_REV/C_REV/g" -e "s/$C2_REV/C2_REV/g" -e "s/$C_LOCAL_REV/C_LOCAL_REV/g" \
  >       -e "s/$REV_AFTER_BUMP_A/REV_AFTER_BUMP_A/g" \
  >       -e "s/$REV_AFTER_BUMP_B/REV_AFTER_BUMP_B/g" \
  >       -e "s/$REV_AFTER_BUMP_C/REV_AFTER_BUMP_C/g"
  > }

`project_heads` prints the HEAD of each project. Under each HEAD it prints the
project's modified files. It does not print untracked files.

  $ project_heads() {
  >   for p in vendor/a frameworks/b vendor/a/sub/c; do
  >     echo "$p: $(git -C $p rev-parse HEAD | map_rev_names)"
  >     git -C $p status --porcelain --untracked-files=no
  >   done
  > }

`read_manifest_revs` reads a manifest from stdin. It prints one `path=REV` pair
per project, all on one line.

  $ read_manifest_revs() {
  >   echo $(grep '<project' | sed 's/.*path="([^"]*)" revision="([^"]*)".*/\1=\2/' | map_rev_names)
  > }

`workspace_state` prints:
- `sl_workingcopy_parent`: the commit message of `.`.
- `manifests HEAD`: the branch that HEAD of the manifests repo points to, or
`detached`. Then the commit.
- `static.xml index`: the revisions in the Git index of the manifests repo.
- `static.xml disk`: the revisions in the file on disk. The two differ when
`goto` moves the index but does not rewrite the file.
- `manifests:`: `git status` of the manifests repo.
- The project HEADs, from `project_heads`.
- `sl status:`: what Sapling says is modified.

  $ workspace_state() {
  >   echo "sl_workingcopy_parent: $(sl log -r . -T '{desc}')"
  >   echo "manifests HEAD: $(git -C .repo/manifests symbolic-ref -q HEAD || echo detached) $(git -C .repo/manifests rev-parse HEAD | map_rev_names)"
  >   echo "static.xml index: $(git -C .repo/manifests show :static/static.xml | read_manifest_revs)"
  >   echo "static.xml disk: $(read_manifest_revs < .repo/manifests/static/static.xml)"
  >   git -C .repo/manifests status --porcelain | sed 's/^/manifests: /'
  >   project_heads
  >   sl status | sed 's/^/sl status: /'
  > }

`reset_workspace [REV]` gives a clean checkout of manifest commit REV. REV
defaults to `$REV_AFTER_BUMP_C`. It uses Git, not `sl goto`, so it works even
after a `goto` that failed halfway. It:
- points `refs/heads/main` back at REV. This brings back commits that an
earlier `goto` orphaned.
- attaches HEAD of the manifests repo to `main`.
- resets the manifests index and files.
- checks out each project at its revision in `static.xml` of REV.
It does not remove untracked files.

  $ reset_workspace() {
  >   local rev=${1:-$REV_AFTER_BUMP_C}
  >   git -C .repo/manifests update-ref refs/heads/main $rev
  >   git -C .repo/manifests symbolic-ref HEAD refs/heads/main
  >   git -C .repo/manifests reset -q --hard
  >   for p in vendor/a frameworks/b vendor/a/sub/c; do
  >     local r=$(grep "path=\"$p\"" .repo/manifests/static/static.xml | sed 's/.*revision="([^"]*)".*/\1/')
  >     git -C $p checkout -q -f --detach $r
  >   done
  > }

The status tests above left every project at a local commit. `reset_workspace`
puts each project back at its revision in `static.xml` of `$REV_AFTER_BUMP_C`:

  $ workspace_state
  sl_workingcopy_parent: bump vendor/a/sub/c
  manifests HEAD: refs/heads/main REV_AFTER_BUMP_C
  static.xml index: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C2_REV
  static.xml disk: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C2_REV
  vendor/a: A_LOCAL_REV
  frameworks/b: B_LOCAL_REV
  vendor/a/sub/c: C_LOCAL_REV
  sl status: M frameworks/b
  sl status: M vendor/a
  sl status: M vendor/a/sub/c
  $ reset_workspace
  $ workspace_state
  sl_workingcopy_parent: bump vendor/a/sub/c
  manifests HEAD: refs/heads/main REV_AFTER_BUMP_C
  static.xml index: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C2_REV
  static.xml disk: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C2_REV
  vendor/a: A2_REV
  frameworks/b: B2_REV
  vendor/a/sub/c: C2_REV

Below we make three changes:
- edit the revision of `vendor/a/sub/c` in `static.xml`.
- check out `vendor/a/sub/c` at that revision.
- change a file in `frameworks/b` without committing it.
`workspace_state` shows all three. `reset_workspace` undoes all three. Note that
Sapling says `vendor/a/sub/c` is modified, even though it matches `static.xml`
on disk. This is because `sl status` compares with the committed manifest, not
with the file on disk.

  $ sed -i "s/$C2_REV/$C_REV/" .repo/manifests/static/static.xml
  $ git -C vendor/a/sub/c checkout -q --detach $C_REV
  $ echo "uncommitted change" > frameworks/b/README
  $ workspace_state
  sl_workingcopy_parent: bump vendor/a/sub/c
  manifests HEAD: refs/heads/main REV_AFTER_BUMP_C
  static.xml index: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C2_REV
  static.xml disk: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C_REV
  manifests:  M static/static.xml
  vendor/a: A2_REV
  frameworks/b: B2_REV
   M README
  vendor/a/sub/c: C_REV
  sl status: M vendor/a/sub/c
  $ reset_workspace
  $ workspace_state
  sl_workingcopy_parent: bump vendor/a/sub/c
  manifests HEAD: refs/heads/main REV_AFTER_BUMP_C
  static.xml index: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C2_REV
  static.xml disk: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C2_REV
  vendor/a: A2_REV
  frameworks/b: B2_REV
  vendor/a/sub/c: C2_REV

`reset_workspace` also undoes a `goto`. It moves `.` back. It also brings back
the manifest commits that `goto` orphaned:

  $ sl goto -q $REV_AFTER_BUMP_A
  $ sl smartlog -T '{desc}' --all
  @  bump vendor/a
  │
  o  add manifest
  $ reset_workspace
  $ workspace_state
  sl_workingcopy_parent: bump vendor/a/sub/c
  manifests HEAD: refs/heads/main REV_AFTER_BUMP_C
  static.xml index: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C2_REV
  static.xml disk: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C2_REV
  vendor/a: A2_REV
  frameworks/b: B2_REV
  vendor/a/sub/c: C2_REV
  $ sl smartlog -T '{desc}' --all
  @  bump vendor/a/sub/c
  │
  o  bump frameworks/b
  │
  o  bump vendor/a
  │
  o  add manifest

`goto` tests: clean workspace

Every test below starts clean at `$REV_AFTER_BUMP_C`:
- The manifests repo has no changes.
- Each project is at its revision in `static.xml`, with no changes.
- `sl status` is empty.
Then it runs `sl goto $REV_AFTER_BUMP_B`. The goto updates the revision of
only `vendor/a/sub/c`, both in `static.xml` and the checked out submodule
(`C2_REV` -> `C_REV`).

Nothing is dirty, so every flag should give the same result:
- `.` is the target.
- Every project is checked out at its revision in the target's `static.xml`.
- The manifests index and `static.xml` on disk both match the target. So
`git status` in the manifests repo is empty.
- `sl status` is empty.
- No manifest commit gets orphaned.
Lines marked `(bad: ...)` show where today's behavior is different.

Each test ends with the args that `sl goto` passes to `hg.updatetotally`:
- `clean=True` or `updatecheck="noconflict"` uses the Rust checkout.
- `updatecheck="none"` uses the Python checkout.
- `updatecheck="abort"` first aborts if Sapling sees uncommitted changes. Then
it works like `updatecheck="none"`.

Default flags. Expected: the result above.
(bad: `static.xml` on disk still says `C2_REV`. The manifests index did not
move either. So the manifests repo shows a staged change.)
(bad: `main` moved back. The `bump vendor/a/sub/c` commit is orphaned.)
(bad: it says "0 files updated", but it checked out a project.)
clean=False, updatecheck="noconflict"

  $ sl goto $REV_AFTER_BUMP_B
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ workspace_state
  sl_workingcopy_parent: bump frameworks/b
  manifests HEAD: refs/heads/main REV_AFTER_BUMP_B
  static.xml index: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C2_REV
  static.xml disk: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C2_REV
  manifests: M  static/static.xml
  vendor/a: A2_REV
  frameworks/b: B2_REV
  vendor/a/sub/c: C_REV
  $ sl smartlog -T '{desc}' --all
  @  bump frameworks/b
  │
  o  bump vendor/a
  │
  o  add manifest

`--check`. Expected: the result above. The manifests index moves to the target.
The default flags do not do this.
(bad: `static.xml` on disk still says `C2_REV`. So the manifests repo shows an
unstaged change.)
(bad: `main` moved back and "0 files updated", same as the default flags.)
clean=False, updatecheck="abort"

  $ reset_workspace
  $ sl goto --check $REV_AFTER_BUMP_B
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ workspace_state
  sl_workingcopy_parent: bump frameworks/b
  manifests HEAD: refs/heads/main REV_AFTER_BUMP_B
  static.xml index: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C_REV
  static.xml disk: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C2_REV
  manifests:  M static/static.xml
  vendor/a: A2_REV
  frameworks/b: B2_REV
  vendor/a/sub/c: C_REV

`--merge`. Expected: the result above. Nothing is dirty, so it works like
`--check`.
(bad: same as `--check`)
clean=False, updatecheck="none"

  $ reset_workspace
  $ sl goto --merge $REV_AFTER_BUMP_B
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ workspace_state
  sl_workingcopy_parent: bump frameworks/b
  manifests HEAD: refs/heads/main REV_AFTER_BUMP_B
  static.xml index: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C_REV
  static.xml disk: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C2_REV
  manifests:  M static/static.xml
  vendor/a: A2_REV
  frameworks/b: B2_REV
  vendor/a/sub/c: C_REV

`--clean`. Expected: the result above. Nothing is dirty, so it works like the
default flags.
(bad: same as the default flags)
clean=True

  $ reset_workspace
  $ sl goto --clean $REV_AFTER_BUMP_B
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ workspace_state
  sl_workingcopy_parent: bump frameworks/b
  manifests HEAD: refs/heads/main REV_AFTER_BUMP_B
  static.xml index: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C2_REV
  static.xml disk: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C2_REV
  manifests: M  static/static.xml
  vendor/a: A2_REV
  frameworks/b: B2_REV
  vendor/a/sub/c: C_REV

Default flags, going to `$REV_AFTER_BUMP_A`. The goto updates the revision of
two projects, `frameworks/b` and `vendor/a/sub/c`, both in `static.xml` and the
checked out submodules. Expected: the result above. Both submodules are checked
out at the new revisions.
(bad: `static.xml` on disk and the manifests index stay at the source, same as
above.)
clean=False, updatecheck="noconflict"

  $ reset_workspace
  $ sl goto $REV_AFTER_BUMP_A
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ workspace_state
  sl_workingcopy_parent: bump vendor/a
  manifests HEAD: refs/heads/main REV_AFTER_BUMP_A
  static.xml index: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C2_REV
  static.xml disk: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C2_REV
  manifests: M  static/static.xml
  vendor/a: A2_REV
  frameworks/b: B_REV
  vendor/a/sub/c: C_REV

Default flags, going forward to the commit that the last `goto` orphaned.
`goto` still finds it by hash, and it ends in the right state. But that is only
because `static.xml` on disk and the manifests index never left
`$REV_AFTER_BUMP_C`.
clean=False, updatecheck="noconflict"

  $ sl goto $REV_AFTER_BUMP_C
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ workspace_state
  sl_workingcopy_parent: bump vendor/a/sub/c
  manifests HEAD: refs/heads/main REV_AFTER_BUMP_C
  static.xml index: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C2_REV
  static.xml disk: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C2_REV
  vendor/a: A2_REV
  frameworks/b: B2_REV
  vendor/a/sub/c: C2_REV

`goto` tests: dirty project

Every test starts clean at `$REV_AFTER_BUMP_C` and makes one project dirty.
Then it runs `sl goto $REV_AFTER_BUMP_B`. The dirty project is one of:
- `vendor/a/sub/c`. The goto updates the revision of this project, both in
`static.xml` and the checked out submodule.
- `frameworks/b`. The goto keeps the revision of this project, both in
`static.xml` and the checked out submodule.
The project is dirty in one of two ways:
- A file has an uncommitted change. Sapling can't see this, so `sl status` is
empty.
- The project has a local commit (`*_LOCAL_REV`). Sapling shows the project as
modified.

Expected:
- Default flags and `--merge`: if the goto keeps the project's revision, keep
the change. If the goto updates it and the change is not committed, treat the
project as a conflicting project (see below). If the goto updates it and the
change is a local commit, refuse before changing anything.
- `--check`: for a local commit, refuse before changing anything. Sapling sees
the commit. For an uncommitted change, work like the default flags. Sapling
can't see the change.
- `--clean`: throw away the change and check out the target.

A conflicting project is a project where Git refuses the checkout because of a
change in the project. The checkout is not atomic. goto first moves `.` and the
manifests HEAD to the target. Then Git checks out each project. A conflicting
project stays at its source revision and keeps its change. The other projects
move to the target. At the end, goto reports all conflicting projects in one
error.

The problems from the clean tests (`static.xml` not rewritten, `main` moved
back) show up here too. We do not mark them again.

Uncommitted change in `vendor/a/sub/c`. The goto updates the revision of
this project.

Default flags. Expected: `vendor/a/sub/c` is a conflicting project. `.` and
`main` move to `$REV_AFTER_BUMP_B`. `vendor/a/sub/c` stays at `C2_REV` and keeps
the change. goto reports that `vendor/a/sub/c` failed.
(bad: the error is the raw Git error.)
clean=False, updatecheck="noconflict"

  $ reset_workspace
  $ echo "uncommitted change" > vendor/a/sub/c/README
  $ sl goto $REV_AFTER_BUMP_B > $TESTTMP/goto.out 2>&1
  [255]
  $ map_rev_names < $TESTTMP/goto.out
  abort: Command exited with code 1
    git --git-dir=$TESTTMP/repodir/vendor/a/sub/c/.git checkout -d --recurse-submodules C_REV
      error: Your local changes to the following files would be overwritten by checkout:
      	README
      Please commit your changes or stash them before you switch branches.
      Aborting
  $ workspace_state
  sl_workingcopy_parent: bump frameworks/b
  manifests HEAD: refs/heads/main REV_AFTER_BUMP_B
  static.xml index: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C2_REV
  static.xml disk: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C2_REV
  manifests: M  static/static.xml
  vendor/a: A2_REV
  frameworks/b: B2_REV
  vendor/a/sub/c: C2_REV
   M README
  sl status: M vendor/a/sub/c

`--check`. Expected: `sl status` is empty, so the check passes. Then the result
is the same as for the default flags.
(bad: same as the default flags.)
clean=False, updatecheck="abort"

  $ reset_workspace
  $ echo "uncommitted change" > vendor/a/sub/c/README
  $ sl goto --check $REV_AFTER_BUMP_B > $TESTTMP/goto.out 2>&1
  [255]
  $ map_rev_names < $TESTTMP/goto.out
  abort: Command exited with code 1
    git --git-dir=$TESTTMP/repodir/vendor/a/sub/c/.git checkout -d --recurse-submodules C_REV
      error: Your local changes to the following files would be overwritten by checkout:
      	README
      Please commit your changes or stash them before you switch branches.
      Aborting
  $ workspace_state
  sl_workingcopy_parent: bump frameworks/b
  manifests HEAD: refs/heads/main REV_AFTER_BUMP_B
  static.xml index: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C_REV
  static.xml disk: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C2_REV
  manifests:  M static/static.xml
  vendor/a: A2_REV
  frameworks/b: B2_REV
  vendor/a/sub/c: C2_REV
   M README
  sl status: M vendor/a/sub/c

`--merge`. Expected: the same result as the default flags.
(bad: same as the default flags.)
clean=False, updatecheck="none"

  $ reset_workspace
  $ echo "uncommitted change" > vendor/a/sub/c/README
  $ sl goto --merge $REV_AFTER_BUMP_B > $TESTTMP/goto.out 2>&1
  [255]
  $ map_rev_names < $TESTTMP/goto.out
  abort: Command exited with code 1
    git --git-dir=$TESTTMP/repodir/vendor/a/sub/c/.git checkout -d --recurse-submodules C_REV
      error: Your local changes to the following files would be overwritten by checkout:
      	README
      Please commit your changes or stash them before you switch branches.
      Aborting
  $ workspace_state
  sl_workingcopy_parent: bump frameworks/b
  manifests HEAD: refs/heads/main REV_AFTER_BUMP_B
  static.xml index: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C_REV
  static.xml disk: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C2_REV
  manifests:  M static/static.xml
  vendor/a: A2_REV
  frameworks/b: B2_REV
  vendor/a/sub/c: C2_REV
   M README
  sl status: M vendor/a/sub/c

`--clean`. Expected: throw away the change and check out `C_REV`.
clean=True

  $ reset_workspace
  $ echo "uncommitted change" > vendor/a/sub/c/README
  $ sl goto --clean $REV_AFTER_BUMP_B > $TESTTMP/goto.out 2>&1
  $ map_rev_names < $TESTTMP/goto.out
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ workspace_state
  sl_workingcopy_parent: bump frameworks/b
  manifests HEAD: refs/heads/main REV_AFTER_BUMP_B
  static.xml index: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C2_REV
  static.xml disk: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C2_REV
  manifests: M  static/static.xml
  vendor/a: A2_REV
  frameworks/b: B2_REV
  vendor/a/sub/c: C_REV

Uncommitted change in `frameworks/b`. The goto keeps the revision of this
project.

Default flags. Expected: keep the change.
clean=False, updatecheck="noconflict"

  $ reset_workspace
  $ echo "uncommitted change" > frameworks/b/README
  $ sl goto $REV_AFTER_BUMP_B > $TESTTMP/goto.out 2>&1
  $ map_rev_names < $TESTTMP/goto.out
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ workspace_state
  sl_workingcopy_parent: bump frameworks/b
  manifests HEAD: refs/heads/main REV_AFTER_BUMP_B
  static.xml index: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C2_REV
  static.xml disk: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C2_REV
  manifests: M  static/static.xml
  vendor/a: A2_REV
  frameworks/b: B2_REV
   M README
  vendor/a/sub/c: C_REV

`--check`. Expected: `sl status` is empty, so the check passes. Keep the
change.
clean=False, updatecheck="abort"

  $ reset_workspace
  $ echo "uncommitted change" > frameworks/b/README
  $ sl goto --check $REV_AFTER_BUMP_B > $TESTTMP/goto.out 2>&1
  $ map_rev_names < $TESTTMP/goto.out
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ workspace_state
  sl_workingcopy_parent: bump frameworks/b
  manifests HEAD: refs/heads/main REV_AFTER_BUMP_B
  static.xml index: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C_REV
  static.xml disk: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C2_REV
  manifests:  M static/static.xml
  vendor/a: A2_REV
  frameworks/b: B2_REV
   M README
  vendor/a/sub/c: C_REV

`--merge`. Expected: keep the change.
clean=False, updatecheck="none"

  $ reset_workspace
  $ echo "uncommitted change" > frameworks/b/README
  $ sl goto --merge $REV_AFTER_BUMP_B > $TESTTMP/goto.out 2>&1
  $ map_rev_names < $TESTTMP/goto.out
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ workspace_state
  sl_workingcopy_parent: bump frameworks/b
  manifests HEAD: refs/heads/main REV_AFTER_BUMP_B
  static.xml index: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C_REV
  static.xml disk: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C2_REV
  manifests:  M static/static.xml
  vendor/a: A2_REV
  frameworks/b: B2_REV
   M README
  vendor/a/sub/c: C_REV

`--clean`. Expected: throw away the change.
clean=True

  $ reset_workspace
  $ echo "uncommitted change" > frameworks/b/README
  $ sl goto --clean $REV_AFTER_BUMP_B > $TESTTMP/goto.out 2>&1
  $ map_rev_names < $TESTTMP/goto.out
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ workspace_state
  sl_workingcopy_parent: bump frameworks/b
  manifests HEAD: refs/heads/main REV_AFTER_BUMP_B
  static.xml index: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C2_REV
  static.xml disk: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C2_REV
  manifests: M  static/static.xml
  vendor/a: A2_REV
  frameworks/b: B2_REV
  vendor/a/sub/c: C_REV

Local commit in `vendor/a/sub/c`. The goto updates the revision of this
project. Sapling shows the project as modified.

Default flags. Expected: refuse, because both the local commit and the goto
change this project.
(bad: goto checks out `C_REV` and drops `C_LOCAL_REV` without a word.)
clean=False, updatecheck="noconflict"

  $ reset_workspace
  $ git -C vendor/a/sub/c checkout -q --detach $C_LOCAL_REV
  $ sl goto $REV_AFTER_BUMP_B > $TESTTMP/goto.out 2>&1
  $ map_rev_names < $TESTTMP/goto.out
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ workspace_state
  sl_workingcopy_parent: bump frameworks/b
  manifests HEAD: refs/heads/main REV_AFTER_BUMP_B
  static.xml index: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C2_REV
  static.xml disk: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C2_REV
  manifests: M  static/static.xml
  vendor/a: A2_REV
  frameworks/b: B2_REV
  vendor/a/sub/c: C_REV

`--check`. Expected: refuse before changing anything.
clean=False, updatecheck="abort"

  $ reset_workspace
  $ git -C vendor/a/sub/c checkout -q --detach $C_LOCAL_REV
  $ sl goto --check $REV_AFTER_BUMP_B > $TESTTMP/goto.out 2>&1
  [255]
  $ map_rev_names < $TESTTMP/goto.out
  abort: uncommitted changes
  $ workspace_state
  sl_workingcopy_parent: bump vendor/a/sub/c
  manifests HEAD: refs/heads/main REV_AFTER_BUMP_C
  static.xml index: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C2_REV
  static.xml disk: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C2_REV
  vendor/a: A2_REV
  frameworks/b: B2_REV
  vendor/a/sub/c: C_LOCAL_REV
  sl status: M vendor/a/sub/c

`--merge`. Expected: refuse or report a conflict, because two revisions of a
project can't be merged.
(bad: goto checks out `C_REV` and drops `C_LOCAL_REV` without a word.)
clean=False, updatecheck="none"

  $ reset_workspace
  $ git -C vendor/a/sub/c checkout -q --detach $C_LOCAL_REV
  $ sl goto --merge $REV_AFTER_BUMP_B > $TESTTMP/goto.out 2>&1
  $ map_rev_names < $TESTTMP/goto.out
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ workspace_state
  sl_workingcopy_parent: bump frameworks/b
  manifests HEAD: refs/heads/main REV_AFTER_BUMP_B
  static.xml index: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C_REV
  static.xml disk: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C2_REV
  manifests:  M static/static.xml
  vendor/a: A2_REV
  frameworks/b: B2_REV
  vendor/a/sub/c: C_REV

`--clean`. Expected: check out `C_REV`. `C_LOCAL_REV` is still in the project's
Git repo.
clean=True

  $ reset_workspace
  $ git -C vendor/a/sub/c checkout -q --detach $C_LOCAL_REV
  $ sl goto --clean $REV_AFTER_BUMP_B > $TESTTMP/goto.out 2>&1
  $ map_rev_names < $TESTTMP/goto.out
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ workspace_state
  sl_workingcopy_parent: bump frameworks/b
  manifests HEAD: refs/heads/main REV_AFTER_BUMP_B
  static.xml index: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C2_REV
  static.xml disk: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C2_REV
  manifests: M  static/static.xml
  vendor/a: A2_REV
  frameworks/b: B2_REV
  vendor/a/sub/c: C_REV

Local commit in `frameworks/b`. The goto keeps the revision of this
project. Sapling shows the project as modified.

Default flags. Expected: keep `frameworks/b` at `B_LOCAL_REV`.
(bad: goto resets it to `B2_REV` and drops `B_LOCAL_REV` without a word.)
clean=False, updatecheck="noconflict"

  $ reset_workspace
  $ git -C frameworks/b checkout -q --detach $B_LOCAL_REV
  $ sl goto $REV_AFTER_BUMP_B > $TESTTMP/goto.out 2>&1
  $ map_rev_names < $TESTTMP/goto.out
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ workspace_state
  sl_workingcopy_parent: bump frameworks/b
  manifests HEAD: refs/heads/main REV_AFTER_BUMP_B
  static.xml index: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C2_REV
  static.xml disk: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C2_REV
  manifests: M  static/static.xml
  vendor/a: A2_REV
  frameworks/b: B2_REV
  vendor/a/sub/c: C_REV

`--check`. Expected: refuse before changing anything.
clean=False, updatecheck="abort"

  $ reset_workspace
  $ git -C frameworks/b checkout -q --detach $B_LOCAL_REV
  $ sl goto --check $REV_AFTER_BUMP_B > $TESTTMP/goto.out 2>&1
  [255]
  $ map_rev_names < $TESTTMP/goto.out
  abort: uncommitted changes
  $ workspace_state
  sl_workingcopy_parent: bump vendor/a/sub/c
  manifests HEAD: refs/heads/main REV_AFTER_BUMP_C
  static.xml index: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C2_REV
  static.xml disk: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C2_REV
  vendor/a: A2_REV
  frameworks/b: B_LOCAL_REV
  vendor/a/sub/c: C2_REV
  sl status: M frameworks/b

`--merge`. Expected: keep `frameworks/b` at `B_LOCAL_REV`.
(bad: same as the default flags. It even counts the reset as "1 files
updated".)
clean=False, updatecheck="none"

  $ reset_workspace
  $ git -C frameworks/b checkout -q --detach $B_LOCAL_REV
  $ sl goto --merge $REV_AFTER_BUMP_B > $TESTTMP/goto.out 2>&1
  $ map_rev_names < $TESTTMP/goto.out
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ workspace_state
  sl_workingcopy_parent: bump frameworks/b
  manifests HEAD: refs/heads/main REV_AFTER_BUMP_B
  static.xml index: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C_REV
  static.xml disk: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C2_REV
  manifests:  M static/static.xml
  vendor/a: A2_REV
  frameworks/b: B2_REV
  vendor/a/sub/c: C_REV

`--clean`. Expected: reset `frameworks/b` to `B2_REV`. `B_LOCAL_REV` is still in
the project's Git repo.
clean=True

  $ reset_workspace
  $ git -C frameworks/b checkout -q --detach $B_LOCAL_REV
  $ sl goto --clean $REV_AFTER_BUMP_B > $TESTTMP/goto.out 2>&1
  $ map_rev_names < $TESTTMP/goto.out
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ workspace_state
  sl_workingcopy_parent: bump frameworks/b
  manifests HEAD: refs/heads/main REV_AFTER_BUMP_B
  static.xml index: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C2_REV
  static.xml disk: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C2_REV
  manifests: M  static/static.xml
  vendor/a: A2_REV
  frameworks/b: B2_REV
  vendor/a/sub/c: C_REV

`goto` tests: dirty manifest

Every test starts clean at `$REV_AFTER_BUMP_C`. It edits the revision of one
project in `static.xml` on disk. It does not commit the edit. Then it runs
`sl goto $REV_AFTER_BUMP_B`. The goto updates the revision of only
`vendor/a/sub/c`, both in `static.xml` and the checked out submodule.
The edit is to one of two lines:
- The `vendor/a/sub/c` line. The goto also changes this line.
- The `frameworks/b` line. The goto keeps this line.
The edited project is in one of two states:
- It stays at its old revision.
- It is checked out at the new revision from the edit.

Sapling compares each project with `static.xml` in the commit. It does not read
`static.xml` on disk. So the edit alone leaves `sl status` empty. A project
checked out at the new revision shows as modified, like a local commit.

Expected: treat the `static.xml` edit like an uncommitted file change in the
manifests repo.
- Default flags: if the goto keeps the edited line, keep the edit. If the goto
changes the same line, refuse before changing anything.
- `--check`: refuse before changing anything.
- `--merge`: merge the edit into `static.xml` of the target. If the goto changes
the same line, report a conflict.
- `--clean`: throw away the edit. `static.xml` on disk matches the target.
Projects follow the same rules as in the dirty project tests.

Today no goto reads or writes `static.xml` on disk. So `static.xml` on disk
keeps the revisions of the source plus the edit. The manifests HEAD moves to
the target. On the Python path, the manifests index moves too. The problems
from the clean tests show up here too. We do not mark them again.

Edit to the `vendor/a/sub/c` line. The project stays at `C2_REV`.

Default flags. Expected: refuse, because the goto changes the same line.
(bad: Sapling can't see the edit, so goto does not refuse. The edit stays in
`static.xml` on disk. `vendor/a/sub/c` is checked out at `C_REV`. The two do
not match.)
clean=False, updatecheck="noconflict"

  $ reset_workspace
  $ sed -i "s/$C2_REV/$C_LOCAL_REV/" .repo/manifests/static/static.xml
  $ sl goto $REV_AFTER_BUMP_B > $TESTTMP/goto.out 2>&1
  $ map_rev_names < $TESTTMP/goto.out
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ workspace_state
  sl_workingcopy_parent: bump frameworks/b
  manifests HEAD: refs/heads/main REV_AFTER_BUMP_B
  static.xml index: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C2_REV
  static.xml disk: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C_LOCAL_REV
  manifests: MM static/static.xml
  vendor/a: A2_REV
  frameworks/b: B2_REV
  vendor/a/sub/c: C_REV

`--check`. Expected: refuse before changing anything.
(bad: Sapling can't see the edit, so it does not refuse. The rest is the same
as the default flags.)
clean=False, updatecheck="abort"

  $ reset_workspace
  $ sed -i "s/$C2_REV/$C_LOCAL_REV/" .repo/manifests/static/static.xml
  $ sl goto --check $REV_AFTER_BUMP_B > $TESTTMP/goto.out 2>&1
  $ map_rev_names < $TESTTMP/goto.out
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ workspace_state
  sl_workingcopy_parent: bump frameworks/b
  manifests HEAD: refs/heads/main REV_AFTER_BUMP_B
  static.xml index: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C_REV
  static.xml disk: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C_LOCAL_REV
  manifests:  M static/static.xml
  vendor/a: A2_REV
  frameworks/b: B2_REV
  vendor/a/sub/c: C_REV

`--merge`. Expected: report a conflict on the `vendor/a/sub/c` line.
(bad: no conflict is reported. The edit stays in `static.xml` on disk.)
clean=False, updatecheck="none"

  $ reset_workspace
  $ sed -i "s/$C2_REV/$C_LOCAL_REV/" .repo/manifests/static/static.xml
  $ sl goto --merge $REV_AFTER_BUMP_B > $TESTTMP/goto.out 2>&1
  $ map_rev_names < $TESTTMP/goto.out
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ workspace_state
  sl_workingcopy_parent: bump frameworks/b
  manifests HEAD: refs/heads/main REV_AFTER_BUMP_B
  static.xml index: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C_REV
  static.xml disk: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C_LOCAL_REV
  manifests:  M static/static.xml
  vendor/a: A2_REV
  frameworks/b: B2_REV
  vendor/a/sub/c: C_REV

`--clean`. Expected: throw away the edit.
(bad: the edit stays in `static.xml` on disk.)
clean=True

  $ reset_workspace
  $ sed -i "s/$C2_REV/$C_LOCAL_REV/" .repo/manifests/static/static.xml
  $ sl goto --clean $REV_AFTER_BUMP_B > $TESTTMP/goto.out 2>&1
  $ map_rev_names < $TESTTMP/goto.out
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ workspace_state
  sl_workingcopy_parent: bump frameworks/b
  manifests HEAD: refs/heads/main REV_AFTER_BUMP_B
  static.xml index: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C2_REV
  static.xml disk: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C_LOCAL_REV
  manifests: MM static/static.xml
  vendor/a: A2_REV
  frameworks/b: B2_REV
  vendor/a/sub/c: C_REV

Edit to the `frameworks/b` line. The project stays at `B2_REV`.

Default flags. Expected: keep the edit on top of `static.xml` of the target.
(bad: the edit stays only because goto never rewrites `static.xml` on disk. So
the `vendor/a/sub/c` line on disk still says `C2_REV`.)
clean=False, updatecheck="noconflict"

  $ reset_workspace
  $ sed -i "s/$B2_REV/$B_LOCAL_REV/" .repo/manifests/static/static.xml
  $ sl goto $REV_AFTER_BUMP_B > $TESTTMP/goto.out 2>&1
  $ map_rev_names < $TESTTMP/goto.out
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ workspace_state
  sl_workingcopy_parent: bump frameworks/b
  manifests HEAD: refs/heads/main REV_AFTER_BUMP_B
  static.xml index: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C2_REV
  static.xml disk: vendor/a=A2_REV frameworks/b=B_LOCAL_REV vendor/a/sub/c=C2_REV
  manifests: MM static/static.xml
  vendor/a: A2_REV
  frameworks/b: B2_REV
  vendor/a/sub/c: C_REV

`--check`. Expected: refuse before changing anything.
(bad: Sapling can't see the edit, so it does not refuse. The rest is the same
as the default flags.)
clean=False, updatecheck="abort"

  $ reset_workspace
  $ sed -i "s/$B2_REV/$B_LOCAL_REV/" .repo/manifests/static/static.xml
  $ sl goto --check $REV_AFTER_BUMP_B > $TESTTMP/goto.out 2>&1
  $ map_rev_names < $TESTTMP/goto.out
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ workspace_state
  sl_workingcopy_parent: bump frameworks/b
  manifests HEAD: refs/heads/main REV_AFTER_BUMP_B
  static.xml index: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C_REV
  static.xml disk: vendor/a=A2_REV frameworks/b=B_LOCAL_REV vendor/a/sub/c=C2_REV
  manifests:  M static/static.xml
  vendor/a: A2_REV
  frameworks/b: B2_REV
  vendor/a/sub/c: C_REV

`--merge`. Expected: merge the edit into `static.xml` of the target.
(bad: same as the default flags.)
clean=False, updatecheck="none"

  $ reset_workspace
  $ sed -i "s/$B2_REV/$B_LOCAL_REV/" .repo/manifests/static/static.xml
  $ sl goto --merge $REV_AFTER_BUMP_B > $TESTTMP/goto.out 2>&1
  $ map_rev_names < $TESTTMP/goto.out
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ workspace_state
  sl_workingcopy_parent: bump frameworks/b
  manifests HEAD: refs/heads/main REV_AFTER_BUMP_B
  static.xml index: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C_REV
  static.xml disk: vendor/a=A2_REV frameworks/b=B_LOCAL_REV vendor/a/sub/c=C2_REV
  manifests:  M static/static.xml
  vendor/a: A2_REV
  frameworks/b: B2_REV
  vendor/a/sub/c: C_REV

`--clean`. Expected: throw away the edit.
(bad: the edit stays in `static.xml` on disk.)
clean=True

  $ reset_workspace
  $ sed -i "s/$B2_REV/$B_LOCAL_REV/" .repo/manifests/static/static.xml
  $ sl goto --clean $REV_AFTER_BUMP_B > $TESTTMP/goto.out 2>&1
  $ map_rev_names < $TESTTMP/goto.out
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ workspace_state
  sl_workingcopy_parent: bump frameworks/b
  manifests HEAD: refs/heads/main REV_AFTER_BUMP_B
  static.xml index: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C2_REV
  static.xml disk: vendor/a=A2_REV frameworks/b=B_LOCAL_REV vendor/a/sub/c=C2_REV
  manifests: MM static/static.xml
  vendor/a: A2_REV
  frameworks/b: B2_REV
  vendor/a/sub/c: C_REV

Edit to the `vendor/a/sub/c` line. The project is checked out at the new
revision `C_LOCAL_REV`. Sapling shows the project as modified.

Default flags. Expected: refuse, because the goto changes the same line.
(bad: goto checks out `C_REV` and drops `C_LOCAL_REV` without a word.
`static.xml` on disk still says `C_LOCAL_REV`.)
clean=False, updatecheck="noconflict"

  $ reset_workspace
  $ sed -i "s/$C2_REV/$C_LOCAL_REV/" .repo/manifests/static/static.xml
  $ git -C vendor/a/sub/c checkout -q --detach $C_LOCAL_REV
  $ sl goto $REV_AFTER_BUMP_B > $TESTTMP/goto.out 2>&1
  $ map_rev_names < $TESTTMP/goto.out
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ workspace_state
  sl_workingcopy_parent: bump frameworks/b
  manifests HEAD: refs/heads/main REV_AFTER_BUMP_B
  static.xml index: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C2_REV
  static.xml disk: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C_LOCAL_REV
  manifests: MM static/static.xml
  vendor/a: A2_REV
  frameworks/b: B2_REV
  vendor/a/sub/c: C_REV

`--check`. Expected: refuse before changing anything.
clean=False, updatecheck="abort"

  $ reset_workspace
  $ sed -i "s/$C2_REV/$C_LOCAL_REV/" .repo/manifests/static/static.xml
  $ git -C vendor/a/sub/c checkout -q --detach $C_LOCAL_REV
  $ sl goto --check $REV_AFTER_BUMP_B > $TESTTMP/goto.out 2>&1
  [255]
  $ map_rev_names < $TESTTMP/goto.out
  abort: uncommitted changes
  $ workspace_state
  sl_workingcopy_parent: bump vendor/a/sub/c
  manifests HEAD: refs/heads/main REV_AFTER_BUMP_C
  static.xml index: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C2_REV
  static.xml disk: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C_LOCAL_REV
  manifests:  M static/static.xml
  vendor/a: A2_REV
  frameworks/b: B2_REV
  vendor/a/sub/c: C_LOCAL_REV
  sl status: M vendor/a/sub/c

`--merge`. Expected: report a conflict on the `vendor/a/sub/c` line.
(bad: no conflict is reported. The rest is the same as the default flags.)
clean=False, updatecheck="none"

  $ reset_workspace
  $ sed -i "s/$C2_REV/$C_LOCAL_REV/" .repo/manifests/static/static.xml
  $ git -C vendor/a/sub/c checkout -q --detach $C_LOCAL_REV
  $ sl goto --merge $REV_AFTER_BUMP_B > $TESTTMP/goto.out 2>&1
  $ map_rev_names < $TESTTMP/goto.out
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ workspace_state
  sl_workingcopy_parent: bump frameworks/b
  manifests HEAD: refs/heads/main REV_AFTER_BUMP_B
  static.xml index: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C_REV
  static.xml disk: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C_LOCAL_REV
  manifests:  M static/static.xml
  vendor/a: A2_REV
  frameworks/b: B2_REV
  vendor/a/sub/c: C_REV

`--clean`. Expected: throw away the edit and check out `C_REV`.
(bad: `vendor/a/sub/c` is checked out at `C_REV`, but the edit stays.
`static.xml` on disk still says `C_LOCAL_REV`.)
clean=True

  $ reset_workspace
  $ sed -i "s/$C2_REV/$C_LOCAL_REV/" .repo/manifests/static/static.xml
  $ git -C vendor/a/sub/c checkout -q --detach $C_LOCAL_REV
  $ sl goto --clean $REV_AFTER_BUMP_B > $TESTTMP/goto.out 2>&1
  $ map_rev_names < $TESTTMP/goto.out
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ workspace_state
  sl_workingcopy_parent: bump frameworks/b
  manifests HEAD: refs/heads/main REV_AFTER_BUMP_B
  static.xml index: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C2_REV
  static.xml disk: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C_LOCAL_REV
  manifests: MM static/static.xml
  vendor/a: A2_REV
  frameworks/b: B2_REV
  vendor/a/sub/c: C_REV

Edit to the `frameworks/b` line. The project is checked out at the new revision
`B_LOCAL_REV`. Sapling shows the project as modified.

Default flags. Expected: keep the edit. Keep `frameworks/b` at `B_LOCAL_REV`.
(bad: goto resets `frameworks/b` to `B2_REV`. `static.xml` on disk still says
`B_LOCAL_REV`.)
clean=False, updatecheck="noconflict"

  $ reset_workspace
  $ sed -i "s/$B2_REV/$B_LOCAL_REV/" .repo/manifests/static/static.xml
  $ git -C frameworks/b checkout -q --detach $B_LOCAL_REV
  $ sl goto $REV_AFTER_BUMP_B > $TESTTMP/goto.out 2>&1
  $ map_rev_names < $TESTTMP/goto.out
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ workspace_state
  sl_workingcopy_parent: bump frameworks/b
  manifests HEAD: refs/heads/main REV_AFTER_BUMP_B
  static.xml index: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C2_REV
  static.xml disk: vendor/a=A2_REV frameworks/b=B_LOCAL_REV vendor/a/sub/c=C2_REV
  manifests: MM static/static.xml
  vendor/a: A2_REV
  frameworks/b: B2_REV
  vendor/a/sub/c: C_REV

`--check`. Expected: refuse before changing anything.
clean=False, updatecheck="abort"

  $ reset_workspace
  $ sed -i "s/$B2_REV/$B_LOCAL_REV/" .repo/manifests/static/static.xml
  $ git -C frameworks/b checkout -q --detach $B_LOCAL_REV
  $ sl goto --check $REV_AFTER_BUMP_B > $TESTTMP/goto.out 2>&1
  [255]
  $ map_rev_names < $TESTTMP/goto.out
  abort: uncommitted changes
  $ workspace_state
  sl_workingcopy_parent: bump vendor/a/sub/c
  manifests HEAD: refs/heads/main REV_AFTER_BUMP_C
  static.xml index: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C2_REV
  static.xml disk: vendor/a=A2_REV frameworks/b=B_LOCAL_REV vendor/a/sub/c=C2_REV
  manifests:  M static/static.xml
  vendor/a: A2_REV
  frameworks/b: B_LOCAL_REV
  vendor/a/sub/c: C2_REV
  sl status: M frameworks/b

`--merge`. Expected: keep the edit. Keep `frameworks/b` at `B_LOCAL_REV`.
(bad: same as the default flags. It even counts the reset as "1 files
updated".)
clean=False, updatecheck="none"

  $ reset_workspace
  $ sed -i "s/$B2_REV/$B_LOCAL_REV/" .repo/manifests/static/static.xml
  $ git -C frameworks/b checkout -q --detach $B_LOCAL_REV
  $ sl goto --merge $REV_AFTER_BUMP_B > $TESTTMP/goto.out 2>&1
  $ map_rev_names < $TESTTMP/goto.out
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ workspace_state
  sl_workingcopy_parent: bump frameworks/b
  manifests HEAD: refs/heads/main REV_AFTER_BUMP_B
  static.xml index: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C_REV
  static.xml disk: vendor/a=A2_REV frameworks/b=B_LOCAL_REV vendor/a/sub/c=C2_REV
  manifests:  M static/static.xml
  vendor/a: A2_REV
  frameworks/b: B2_REV
  vendor/a/sub/c: C_REV

`--clean`. Expected: throw away the edit and reset `frameworks/b` to `B2_REV`.
(bad: `frameworks/b` is reset to `B2_REV`, but the edit stays. `static.xml` on
disk still says `B_LOCAL_REV`.)
clean=True

  $ reset_workspace
  $ sed -i "s/$B2_REV/$B_LOCAL_REV/" .repo/manifests/static/static.xml
  $ git -C frameworks/b checkout -q --detach $B_LOCAL_REV
  $ sl goto --clean $REV_AFTER_BUMP_B > $TESTTMP/goto.out 2>&1
  $ map_rev_names < $TESTTMP/goto.out
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ workspace_state
  sl_workingcopy_parent: bump frameworks/b
  manifests HEAD: refs/heads/main REV_AFTER_BUMP_B
  static.xml index: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C2_REV
  static.xml disk: vendor/a=A2_REV frameworks/b=B_LOCAL_REV vendor/a/sub/c=C2_REV
  manifests: MM static/static.xml
  vendor/a: A2_REV
  frameworks/b: B2_REV
  vendor/a/sub/c: C_REV

`goto` tests: dirty manifest and dirty project

Every test starts clean at `$REV_AFTER_BUMP_C` and makes three changes to one
project:
- check out the project at its local revision (`*_LOCAL_REV`).
- edit the revision of the project in `static.xml` on disk to match.
- change a file in the project without committing it.
Then it runs `sl goto $REV_AFTER_BUMP_B`. The goto updates the revision of only
`vendor/a/sub/c`, both in `static.xml` and the checked out submodule.
The dirty project is one of:
- `vendor/a/sub/c`. The goto updates the revision of this project.
- `frameworks/b`. The goto keeps the revision of this project.

Expected (the dirty project and dirty manifest rules combined):
- Default flags and `--merge`: if the goto keeps the project's revision, keep
all three changes. If the goto updates it, refuse before changing anything.
- `--check`: refuse before changing anything.
- `--clean`: throw away all three changes and check out the target.
The problems from the clean tests show up here too. We do not mark them again.

Dirty project `vendor/a/sub/c`. The goto updates the revision of this project.

Default flags. Expected: refuse before changing anything.
(bad: goto does not refuse. Git refuses halfway. By then `.` and `main` have
already moved to `$REV_AFTER_BUMP_B`. `vendor/a/sub/c` stays at
`C_LOCAL_REV`.)
clean=False, updatecheck="noconflict"

  $ reset_workspace
  $ git -C vendor/a/sub/c checkout -q --detach $C_LOCAL_REV
  $ sed -i "s/$C2_REV/$C_LOCAL_REV/" .repo/manifests/static/static.xml
  $ echo "uncommitted change" > vendor/a/sub/c/README
  $ sl goto $REV_AFTER_BUMP_B > $TESTTMP/goto.out 2>&1
  [255]
  $ map_rev_names < $TESTTMP/goto.out
  abort: Command exited with code 1
    git --git-dir=$TESTTMP/repodir/vendor/a/sub/c/.git checkout -d --recurse-submodules C_REV
      error: Your local changes to the following files would be overwritten by checkout:
      	README
      Please commit your changes or stash them before you switch branches.
      Aborting
  $ workspace_state
  sl_workingcopy_parent: bump frameworks/b
  manifests HEAD: refs/heads/main REV_AFTER_BUMP_B
  static.xml index: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C2_REV
  static.xml disk: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C_LOCAL_REV
  manifests: MM static/static.xml
  vendor/a: A2_REV
  frameworks/b: B2_REV
  vendor/a/sub/c: C_LOCAL_REV
   M README
  sl status: M vendor/a/sub/c

`--check`. Expected: refuse before changing anything.
clean=False, updatecheck="abort"

  $ reset_workspace
  $ git -C vendor/a/sub/c checkout -q --detach $C_LOCAL_REV
  $ sed -i "s/$C2_REV/$C_LOCAL_REV/" .repo/manifests/static/static.xml
  $ echo "uncommitted change" > vendor/a/sub/c/README
  $ sl goto --check $REV_AFTER_BUMP_B > $TESTTMP/goto.out 2>&1
  [255]
  $ map_rev_names < $TESTTMP/goto.out
  abort: uncommitted changes
  $ workspace_state
  sl_workingcopy_parent: bump vendor/a/sub/c
  manifests HEAD: refs/heads/main REV_AFTER_BUMP_C
  static.xml index: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C2_REV
  static.xml disk: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C_LOCAL_REV
  manifests:  M static/static.xml
  vendor/a: A2_REV
  frameworks/b: B2_REV
  vendor/a/sub/c: C_LOCAL_REV
   M README
  sl status: M vendor/a/sub/c

`--merge`. Expected: refuse before changing anything.
(bad: it fails halfway, same as the default flags.)
clean=False, updatecheck="none"

  $ reset_workspace
  $ git -C vendor/a/sub/c checkout -q --detach $C_LOCAL_REV
  $ sed -i "s/$C2_REV/$C_LOCAL_REV/" .repo/manifests/static/static.xml
  $ echo "uncommitted change" > vendor/a/sub/c/README
  $ sl goto --merge $REV_AFTER_BUMP_B > $TESTTMP/goto.out 2>&1
  [255]
  $ map_rev_names < $TESTTMP/goto.out
  abort: Command exited with code 1
    git --git-dir=$TESTTMP/repodir/vendor/a/sub/c/.git checkout -d --recurse-submodules C_REV
      error: Your local changes to the following files would be overwritten by checkout:
      	README
      Please commit your changes or stash them before you switch branches.
      Aborting
  $ workspace_state
  sl_workingcopy_parent: bump frameworks/b
  manifests HEAD: refs/heads/main REV_AFTER_BUMP_B
  static.xml index: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C_REV
  static.xml disk: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C_LOCAL_REV
  manifests:  M static/static.xml
  vendor/a: A2_REV
  frameworks/b: B2_REV
  vendor/a/sub/c: C_LOCAL_REV
   M README
  sl status: M vendor/a/sub/c

`--clean`. Expected: throw away all three changes and check out `C_REV`.
(bad: the file change is thrown away and `vendor/a/sub/c` is checked out at
`C_REV`. But the `static.xml` edit stays. `static.xml` on disk still says
`C_LOCAL_REV`.)
clean=True

  $ reset_workspace
  $ git -C vendor/a/sub/c checkout -q --detach $C_LOCAL_REV
  $ sed -i "s/$C2_REV/$C_LOCAL_REV/" .repo/manifests/static/static.xml
  $ echo "uncommitted change" > vendor/a/sub/c/README
  $ sl goto --clean $REV_AFTER_BUMP_B > $TESTTMP/goto.out 2>&1
  $ map_rev_names < $TESTTMP/goto.out
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ workspace_state
  sl_workingcopy_parent: bump frameworks/b
  manifests HEAD: refs/heads/main REV_AFTER_BUMP_B
  static.xml index: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C2_REV
  static.xml disk: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C_LOCAL_REV
  manifests: MM static/static.xml
  vendor/a: A2_REV
  frameworks/b: B2_REV
  vendor/a/sub/c: C_REV

Dirty project `frameworks/b`. The goto keeps the revision of this project.

Default flags. Expected: keep all three changes.
(bad: goto tries to reset `frameworks/b` to `B2_REV`. Git refuses halfway,
because of the file change. By then `.` and `main` have already moved.
`vendor/a/sub/c` stays at `C2_REV`, so Sapling now shows it as modified too.)
clean=False, updatecheck="noconflict"

  $ reset_workspace
  $ git -C frameworks/b checkout -q --detach $B_LOCAL_REV
  $ sed -i "s/$B2_REV/$B_LOCAL_REV/" .repo/manifests/static/static.xml
  $ echo "uncommitted change" > frameworks/b/README
  $ sl goto $REV_AFTER_BUMP_B > $TESTTMP/goto.out 2>&1
  [255]
  $ map_rev_names < $TESTTMP/goto.out
  abort: Command exited with code 1
    git --git-dir=$TESTTMP/repodir/frameworks/b/.git checkout -d --recurse-submodules B2_REV
      error: Your local changes to the following files would be overwritten by checkout:
      	README
      Please commit your changes or stash them before you switch branches.
      Aborting
  $ workspace_state
  sl_workingcopy_parent: bump frameworks/b
  manifests HEAD: refs/heads/main REV_AFTER_BUMP_B
  static.xml index: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C2_REV
  static.xml disk: vendor/a=A2_REV frameworks/b=B_LOCAL_REV vendor/a/sub/c=C2_REV
  manifests: MM static/static.xml
  vendor/a: A2_REV
  frameworks/b: B_LOCAL_REV
   M README
  vendor/a/sub/c: C2_REV
  sl status: M frameworks/b
  sl status: M vendor/a/sub/c

`--check`. Expected: refuse before changing anything.
clean=False, updatecheck="abort"

  $ reset_workspace
  $ git -C frameworks/b checkout -q --detach $B_LOCAL_REV
  $ sed -i "s/$B2_REV/$B_LOCAL_REV/" .repo/manifests/static/static.xml
  $ echo "uncommitted change" > frameworks/b/README
  $ sl goto --check $REV_AFTER_BUMP_B > $TESTTMP/goto.out 2>&1
  [255]
  $ map_rev_names < $TESTTMP/goto.out
  abort: uncommitted changes
  $ workspace_state
  sl_workingcopy_parent: bump vendor/a/sub/c
  manifests HEAD: refs/heads/main REV_AFTER_BUMP_C
  static.xml index: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C2_REV
  static.xml disk: vendor/a=A2_REV frameworks/b=B_LOCAL_REV vendor/a/sub/c=C2_REV
  manifests:  M static/static.xml
  vendor/a: A2_REV
  frameworks/b: B_LOCAL_REV
   M README
  vendor/a/sub/c: C2_REV
  sl status: M frameworks/b

`--merge`. Expected: keep all three changes.
(bad: it fails halfway, same as the default flags.)
clean=False, updatecheck="none"

  $ reset_workspace
  $ git -C frameworks/b checkout -q --detach $B_LOCAL_REV
  $ sed -i "s/$B2_REV/$B_LOCAL_REV/" .repo/manifests/static/static.xml
  $ echo "uncommitted change" > frameworks/b/README
  $ sl goto --merge $REV_AFTER_BUMP_B > $TESTTMP/goto.out 2>&1
  [255]
  $ map_rev_names < $TESTTMP/goto.out
  abort: Command exited with code 1
    git --git-dir=$TESTTMP/repodir/frameworks/b/.git checkout -d --recurse-submodules B2_REV
      error: Your local changes to the following files would be overwritten by checkout:
      	README
      Please commit your changes or stash them before you switch branches.
      Aborting
  $ workspace_state
  sl_workingcopy_parent: bump frameworks/b
  manifests HEAD: refs/heads/main REV_AFTER_BUMP_B
  static.xml index: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C_REV
  static.xml disk: vendor/a=A2_REV frameworks/b=B_LOCAL_REV vendor/a/sub/c=C2_REV
  manifests:  M static/static.xml
  vendor/a: A2_REV
  frameworks/b: B_LOCAL_REV
   M README
  vendor/a/sub/c: C2_REV
  sl status: M frameworks/b
  sl status: M vendor/a/sub/c

`--clean`. Expected: throw away all three changes and reset `frameworks/b` to
`B2_REV`.
(bad: the file change is thrown away and `frameworks/b` is reset to `B2_REV`.
But the `static.xml` edit stays. `static.xml` on disk still says
`B_LOCAL_REV`.)
clean=True

  $ reset_workspace
  $ git -C frameworks/b checkout -q --detach $B_LOCAL_REV
  $ sed -i "s/$B2_REV/$B_LOCAL_REV/" .repo/manifests/static/static.xml
  $ echo "uncommitted change" > frameworks/b/README
  $ sl goto --clean $REV_AFTER_BUMP_B > $TESTTMP/goto.out 2>&1
  $ map_rev_names < $TESTTMP/goto.out
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ workspace_state
  sl_workingcopy_parent: bump frameworks/b
  manifests HEAD: refs/heads/main REV_AFTER_BUMP_B
  static.xml index: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C2_REV
  static.xml disk: vendor/a=A2_REV frameworks/b=B_LOCAL_REV vendor/a/sub/c=C2_REV
  manifests: MM static/static.xml
  vendor/a: A2_REV
  frameworks/b: B2_REV
  vendor/a/sub/c: C_REV

`goto` tests: untracked files

Every test starts clean at `$REV_AFTER_BUMP_C` and adds one untracked file.
Then it runs `sl goto`. `sl status` does not show untracked files in projects
or in the manifests repo. `reset_workspace` does not remove them. So each test
removes its file at the end.

Expected: the same rules as for untracked files in Sapling's own tree.
- If the target has no file at the untracked path: every flag keeps the file.
- If the target adds a file at that path: for the default flags, `--check` and
`--merge`, the project is a conflicting project, as in the dirty project tests.
`--clean` replaces the file with the target's file.
The problems from the clean tests show up here too. We do not mark them again.

Untracked file in `vendor/a/sub/c`. The goto updates the revision of this
project. The target has no file at the untracked path.

Default flags. Expected: keep the file.
clean=False, updatecheck="noconflict"

  $ reset_workspace
  $ echo "untracked" > vendor/a/sub/c/untracked
  $ sl goto $REV_AFTER_BUMP_B > $TESTTMP/goto.out 2>&1
  $ map_rev_names < $TESTTMP/goto.out
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ workspace_state
  sl_workingcopy_parent: bump frameworks/b
  manifests HEAD: refs/heads/main REV_AFTER_BUMP_B
  static.xml index: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C2_REV
  static.xml disk: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C2_REV
  manifests: M  static/static.xml
  vendor/a: A2_REV
  frameworks/b: B2_REV
  vendor/a/sub/c: C_REV
  $ git -C vendor/a/sub/c status --porcelain
  ?? untracked
  $ cat vendor/a/sub/c/untracked
  untracked
  $ rm vendor/a/sub/c/untracked

`--check`. Expected: keep the file.
clean=False, updatecheck="abort"

  $ reset_workspace
  $ echo "untracked" > vendor/a/sub/c/untracked
  $ sl goto --check $REV_AFTER_BUMP_B > $TESTTMP/goto.out 2>&1
  $ map_rev_names < $TESTTMP/goto.out
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ workspace_state
  sl_workingcopy_parent: bump frameworks/b
  manifests HEAD: refs/heads/main REV_AFTER_BUMP_B
  static.xml index: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C_REV
  static.xml disk: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C2_REV
  manifests:  M static/static.xml
  vendor/a: A2_REV
  frameworks/b: B2_REV
  vendor/a/sub/c: C_REV
  $ git -C vendor/a/sub/c status --porcelain
  ?? untracked
  $ cat vendor/a/sub/c/untracked
  untracked
  $ rm vendor/a/sub/c/untracked

`--merge`. Expected: keep the file.
clean=False, updatecheck="none"

  $ reset_workspace
  $ echo "untracked" > vendor/a/sub/c/untracked
  $ sl goto --merge $REV_AFTER_BUMP_B > $TESTTMP/goto.out 2>&1
  $ map_rev_names < $TESTTMP/goto.out
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ workspace_state
  sl_workingcopy_parent: bump frameworks/b
  manifests HEAD: refs/heads/main REV_AFTER_BUMP_B
  static.xml index: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C_REV
  static.xml disk: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C2_REV
  manifests:  M static/static.xml
  vendor/a: A2_REV
  frameworks/b: B2_REV
  vendor/a/sub/c: C_REV
  $ git -C vendor/a/sub/c status --porcelain
  ?? untracked
  $ cat vendor/a/sub/c/untracked
  untracked
  $ rm vendor/a/sub/c/untracked

`--clean`. Expected: keep the file.
clean=True

  $ reset_workspace
  $ echo "untracked" > vendor/a/sub/c/untracked
  $ sl goto --clean $REV_AFTER_BUMP_B > $TESTTMP/goto.out 2>&1
  $ map_rev_names < $TESTTMP/goto.out
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ workspace_state
  sl_workingcopy_parent: bump frameworks/b
  manifests HEAD: refs/heads/main REV_AFTER_BUMP_B
  static.xml index: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C2_REV
  static.xml disk: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C2_REV
  manifests: M  static/static.xml
  vendor/a: A2_REV
  frameworks/b: B2_REV
  vendor/a/sub/c: C_REV
  $ git -C vendor/a/sub/c status --porcelain
  ?? untracked
  $ cat vendor/a/sub/c/untracked
  untracked
  $ rm vendor/a/sub/c/untracked

Untracked file in the manifests repo.

Default flags. Expected: keep the file.
clean=False, updatecheck="noconflict"

  $ reset_workspace
  $ echo "untracked" > .repo/manifests/untracked
  $ sl goto $REV_AFTER_BUMP_B > $TESTTMP/goto.out 2>&1
  $ map_rev_names < $TESTTMP/goto.out
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ workspace_state
  sl_workingcopy_parent: bump frameworks/b
  manifests HEAD: refs/heads/main REV_AFTER_BUMP_B
  static.xml index: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C2_REV
  static.xml disk: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C2_REV
  manifests: M  static/static.xml
  manifests: ?? untracked
  vendor/a: A2_REV
  frameworks/b: B2_REV
  vendor/a/sub/c: C_REV
  $ cat .repo/manifests/untracked
  untracked
  $ rm .repo/manifests/untracked

`--check`. Expected: keep the file.
clean=False, updatecheck="abort"

  $ reset_workspace
  $ echo "untracked" > .repo/manifests/untracked
  $ sl goto --check $REV_AFTER_BUMP_B > $TESTTMP/goto.out 2>&1
  $ map_rev_names < $TESTTMP/goto.out
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ workspace_state
  sl_workingcopy_parent: bump frameworks/b
  manifests HEAD: refs/heads/main REV_AFTER_BUMP_B
  static.xml index: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C_REV
  static.xml disk: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C2_REV
  manifests:  M static/static.xml
  manifests: ?? untracked
  vendor/a: A2_REV
  frameworks/b: B2_REV
  vendor/a/sub/c: C_REV
  $ cat .repo/manifests/untracked
  untracked
  $ rm .repo/manifests/untracked

`--merge`. Expected: keep the file.
clean=False, updatecheck="none"

  $ reset_workspace
  $ echo "untracked" > .repo/manifests/untracked
  $ sl goto --merge $REV_AFTER_BUMP_B > $TESTTMP/goto.out 2>&1
  $ map_rev_names < $TESTTMP/goto.out
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ workspace_state
  sl_workingcopy_parent: bump frameworks/b
  manifests HEAD: refs/heads/main REV_AFTER_BUMP_B
  static.xml index: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C_REV
  static.xml disk: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C2_REV
  manifests:  M static/static.xml
  manifests: ?? untracked
  vendor/a: A2_REV
  frameworks/b: B2_REV
  vendor/a/sub/c: C_REV
  $ cat .repo/manifests/untracked
  untracked
  $ rm .repo/manifests/untracked

`--clean`. Expected: keep the file.
clean=True

  $ reset_workspace
  $ echo "untracked" > .repo/manifests/untracked
  $ sl goto --clean $REV_AFTER_BUMP_B > $TESTTMP/goto.out 2>&1
  $ map_rev_names < $TESTTMP/goto.out
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ workspace_state
  sl_workingcopy_parent: bump frameworks/b
  manifests HEAD: refs/heads/main REV_AFTER_BUMP_B
  static.xml index: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C2_REV
  static.xml disk: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C2_REV
  manifests: M  static/static.xml
  manifests: ?? untracked
  vendor/a: A2_REV
  frameworks/b: B2_REV
  vendor/a/sub/c: C_REV
  $ cat .repo/manifests/untracked
  untracked
  $ rm .repo/manifests/untracked

Untracked file in `vendor/a/sub/c` at a path the target adds.

No revision above adds a path. So the setup below makes two new commits:
- `C3_REV` in project `c`. It adds the file `added` on top of `C2_REV`.
- `$REV_AFTER_ADD_C` in the manifests repo, on top of `$REV_AFTER_BUMP_C`. Its
`static.xml` sets `vendor/a/sub/c` to `C3_REV`.
`reset_workspace` moves `refs/heads/main` back. The `add-c` branch keeps
`$REV_AFTER_ADD_C` visible to Sapling after that. `map_rev_names` does not know
the two new hashes. So the tests rename them with an extra `sed`.

  $ echo "added content" > $TESTTMP/project-c/added
  $ git -C $TESTTMP/project-c add added
  $ git -C $TESTTMP/project-c commit -qm 'add file to c'
  $ C3_REV=$(git -C $TESTTMP/project-c rev-parse HEAD)
  $ git -C vendor/a/sub/c fetch -q origin
  $ reset_workspace
  $ sed -i "s/$C2_REV/$C3_REV/" .repo/manifests/static/static.xml
  $ git -C .repo/manifests commit -qam 'add file to vendor/a/sub/c'
  $ REV_AFTER_ADD_C=$(git -C .repo/manifests rev-parse HEAD)
  $ git -C .repo/manifests branch add-c

Default flags. Expected: `vendor/a/sub/c` is a conflicting project. `.` and
`main` move to `$REV_AFTER_ADD_C`. `vendor/a/sub/c` stays at `C2_REV` and keeps
the file. goto reports that `vendor/a/sub/c` failed.
(bad: the error is the raw Git error.)
clean=False, updatecheck="noconflict"

  $ reset_workspace
  $ echo "untracked" > vendor/a/sub/c/added
  $ sl goto $REV_AFTER_ADD_C > $TESTTMP/goto.out 2>&1
  [255]
  $ map_rev_names < $TESTTMP/goto.out | sed -e "s/$C3_REV/C3_REV/g" -e "s/$REV_AFTER_ADD_C/REV_AFTER_ADD_C/g"
  abort: Command exited with code 1
    git --git-dir=$TESTTMP/repodir/vendor/a/sub/c/.git checkout -d --recurse-submodules C3_REV
      error: The following untracked working tree files would be overwritten by checkout:
      	added
      Please move or remove them before you switch branches.
      Aborting
  $ workspace_state | sed -e "s/$C3_REV/C3_REV/g" -e "s/$REV_AFTER_ADD_C/REV_AFTER_ADD_C/g"
  sl_workingcopy_parent: add file to vendor/a/sub/c
  manifests HEAD: refs/heads/main REV_AFTER_ADD_C
  static.xml index: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C2_REV
  static.xml disk: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C2_REV
  manifests: M  static/static.xml
  vendor/a: A2_REV
  frameworks/b: B2_REV
  vendor/a/sub/c: C2_REV
  sl status: M vendor/a/sub/c
  $ git -C vendor/a/sub/c status --porcelain
  ?? added
  $ cat vendor/a/sub/c/added
  untracked
  $ rm vendor/a/sub/c/added

`--check`. Expected: `sl status` does not show the file, so the check passes.
Then the result is the same as for the default flags.
(bad: same as the default flags.)
clean=False, updatecheck="abort"

  $ reset_workspace
  $ echo "untracked" > vendor/a/sub/c/added
  $ sl goto --check $REV_AFTER_ADD_C > $TESTTMP/goto.out 2>&1
  [255]
  $ map_rev_names < $TESTTMP/goto.out | sed -e "s/$C3_REV/C3_REV/g" -e "s/$REV_AFTER_ADD_C/REV_AFTER_ADD_C/g"
  abort: Command exited with code 1
    git --git-dir=$TESTTMP/repodir/vendor/a/sub/c/.git checkout -d --recurse-submodules C3_REV
      error: The following untracked working tree files would be overwritten by checkout:
      	added
      Please move or remove them before you switch branches.
      Aborting
  $ workspace_state | sed -e "s/$C3_REV/C3_REV/g" -e "s/$REV_AFTER_ADD_C/REV_AFTER_ADD_C/g"
  sl_workingcopy_parent: add file to vendor/a/sub/c
  manifests HEAD: refs/heads/main REV_AFTER_ADD_C
  static.xml index: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C3_REV
  static.xml disk: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C2_REV
  manifests:  M static/static.xml
  vendor/a: A2_REV
  frameworks/b: B2_REV
  vendor/a/sub/c: C2_REV
  sl status: M vendor/a/sub/c
  $ git -C vendor/a/sub/c status --porcelain
  ?? added
  $ cat vendor/a/sub/c/added
  untracked
  $ rm vendor/a/sub/c/added

`--merge`. Expected: the same result as the default flags.
(bad: same as the default flags.)
clean=False, updatecheck="none"

  $ reset_workspace
  $ echo "untracked" > vendor/a/sub/c/added
  $ sl goto --merge $REV_AFTER_ADD_C > $TESTTMP/goto.out 2>&1
  [255]
  $ map_rev_names < $TESTTMP/goto.out | sed -e "s/$C3_REV/C3_REV/g" -e "s/$REV_AFTER_ADD_C/REV_AFTER_ADD_C/g"
  abort: Command exited with code 1
    git --git-dir=$TESTTMP/repodir/vendor/a/sub/c/.git checkout -d --recurse-submodules C3_REV
      error: The following untracked working tree files would be overwritten by checkout:
      	added
      Please move or remove them before you switch branches.
      Aborting
  $ workspace_state | sed -e "s/$C3_REV/C3_REV/g" -e "s/$REV_AFTER_ADD_C/REV_AFTER_ADD_C/g"
  sl_workingcopy_parent: add file to vendor/a/sub/c
  manifests HEAD: refs/heads/main REV_AFTER_ADD_C
  static.xml index: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C3_REV
  static.xml disk: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C2_REV
  manifests:  M static/static.xml
  vendor/a: A2_REV
  frameworks/b: B2_REV
  vendor/a/sub/c: C2_REV
  sl status: M vendor/a/sub/c
  $ git -C vendor/a/sub/c status --porcelain
  ?? added
  $ cat vendor/a/sub/c/added
  untracked
  $ rm vendor/a/sub/c/added

`--clean`. Expected: replace the file with the target's `added`. Check out
`vendor/a/sub/c` at `C3_REV`.
clean=True

  $ reset_workspace
  $ echo "untracked" > vendor/a/sub/c/added
  $ sl goto --clean $REV_AFTER_ADD_C > $TESTTMP/goto.out 2>&1
  $ map_rev_names < $TESTTMP/goto.out | sed -e "s/$C3_REV/C3_REV/g" -e "s/$REV_AFTER_ADD_C/REV_AFTER_ADD_C/g"
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ workspace_state | sed -e "s/$C3_REV/C3_REV/g" -e "s/$REV_AFTER_ADD_C/REV_AFTER_ADD_C/g"
  sl_workingcopy_parent: add file to vendor/a/sub/c
  manifests HEAD: refs/heads/main REV_AFTER_ADD_C
  static.xml index: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C2_REV
  static.xml disk: vendor/a=A2_REV frameworks/b=B2_REV vendor/a/sub/c=C2_REV
  manifests: M  static/static.xml
  vendor/a: A2_REV
  frameworks/b: B2_REV
  vendor/a/sub/c: C3_REV
  $ git -C vendor/a/sub/c status --porcelain
  $ cat vendor/a/sub/c/added
  added content
