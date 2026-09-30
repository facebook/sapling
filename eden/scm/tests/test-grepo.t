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
