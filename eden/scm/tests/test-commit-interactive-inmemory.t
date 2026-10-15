#require execbit

  $ eagerepo
  $ enable amend
  $ setconfig ui.interactive=true ui.interface.chunkselector=repl diff.git=true
  $ newclientrepo repo

Interactive commit and amend build commits from the selected chunks without
rewriting the working copy.

  $ cat > a <<'EOF'
  > one
  > two
  > three
  > EOF
  $ echo base > b
  $ echo moveme > moveme
  $ printf 'script\n' > exe
  $ sl commit -Aqm base
  $ cat > a <<'EOF'
  > ONE
  > two
  > THREE
  > EOF
  $ echo added >> b
  $ sl mv -q moveme moved
  $ chmod +x exe
  $ printf 'new\nfile\n' > new
  $ sl add -q new

Record the identity of every file in the working copy so we can tell whether
any of them were rewritten.

  >>> import os
  >>> def snapshot():
  ...     return {f: (os.stat(f).st_ino, os.stat(f).st_mtime_ns, open(f, "rb").read()) for f in sorted(os.listdir(".")) if not f.startswith(".")}
  >>> before = snapshot()

Select the first replacement in a, the rename, the mode change and the first
line of the new file.

  $ sl commit -i -m partial >/dev/null <<'EOF'
  > select F1.H1,F3,F4,F5.H1.L1
  > confirm
  > EOF

  >>> snapshot() == before
  True
  $ test -d .sl/record-backups
  [1]

  $ sl log -r . -T '{desc}: {files}\n'
  partial: a exe moved moveme new
  $ sl diff --git -r .^ -r .
  diff --git a/a b/a
  --- a/a
  +++ b/a
  @@ -1,3 +1,3 @@
  -one
  +ONE
   two
   three
  diff --git a/exe b/exe
  old mode 100644
  new mode 100755
  diff --git a/moveme b/moved
  rename from moveme
  rename to moved
  diff --git a/new b/new
  new file mode 100644
  --- /dev/null
  +++ b/new
  @@ -0,0 +1,1 @@
  +new

The unselected changes remain in the working copy.

  $ sl status
  M a
  M b
  M new
  $ sl diff --git
  diff --git a/a b/a
  --- a/a
  +++ b/a
  @@ -1,3 +1,3 @@
   ONE
   two
  -three
  +THREE
  diff --git a/b b/b
  --- a/b
  +++ b/b
  @@ -1,1 +1,2 @@
   base
  +added
  diff --git a/new b/new
  --- a/new
  +++ b/new
  @@ -1,1 +1,2 @@
   new
  +file

Amend folds the selected chunk into the commit and leaves the rest in place.

  $ sl amend -i >/dev/null <<'EOF'
  > select F3
  > confirm
  > EOF

  >>> snapshot() == before
  True

  $ sl diff --git -r .^ -r . new
  diff --git a/new b/new
  new file mode 100644
  --- /dev/null
  +++ b/new
  @@ -0,0 +1,2 @@
  +new
  +file
  $ sl status
  M a
  M b

Selecting everything leaves a clean working copy.

  $ sl commit -i -m rest >/dev/null <<'EOF'
  > select all
  > confirm
  > EOF
  $ sl status
  >>> snapshot() == before
  True

The working copy is also untouched when selection is aborted or the commit
fails.

  $ echo more >> a
  $ sl commit -i -m aborted >/dev/null 2>&1 <<'EOF'
  > select all
  > abort
  > EOF
  [255]
  $ sl status
  M a

  $ sl commit -i -m failed --config hooks.pretxncommit=false >/dev/null 2>&1 <<'EOF'
  > select all
  > confirm
  > EOF
  [255]
  $ sl status
  M a
  $ sl log -r . -T '{desc}\n'
  rest

Amend to an ancestor uses the same selection.

  $ echo extra > c
  $ sl add c
  $ sl amend -i --to .^ >/dev/null <<'EOF'
  > select F2
  > confirm
  > EOF
  $ sl log -r .^ -T '{desc}: {files}\n'
  partial: a c exe moved moveme new
  $ sl status
  M a

A copy with a mode change has no content change, so the patch applier must
keep the copied content.

  $ sl cp -q b bcopy
  $ chmod +x bcopy
  $ sl commit -i -m copy >/dev/null <<'EOF'
  > select F2
  > confirm
  > EOF
  $ sl diff --git -r .^ -r .
  diff --git a/b b/bcopy
  old mode 100644
  new mode 100755
  copy from b
  copy to bcopy
  $ sl cat -r . bcopy
  base
  added
  $ sl status
  M a

The legacy implementation is still available.

  $ sl commit -i -m legacy --config record.inmemory=false >/dev/null <<'EOF'
  > select all
  > confirm
  > EOF
  $ sl status
