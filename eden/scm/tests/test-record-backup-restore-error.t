  $ enable amend
  $ setconfig diff.git=true

Extension to inject two errors:
- Applying patch to working copy fails.
- Restoring the first backup file fails.
  $ cat > $TESTTMP/inject_errors.py <<EOF
  > from sapling import error, extensions, patch, util
  > 
  > first = True
  > def wrap_copyfile(orig, *args, **kwargs):
  >   global first
  >   if "record-backups" in args[0] and first:
  >     first = False
  >     raise error.Abort("copyfile error")
  >   return orig(*args, **kwargs)
  > 
  > def wrap_internalpatch(orig, *args, **kwargs):
  >   raise error.Abort("internalpatch error")
  > 
  > def uisetup(ui):
  >   extensions.wrapfunction(util, "copyfile", wrap_copyfile)
  >   extensions.wrapfunction(patch, "internalpatch", wrap_internalpatch)
  > EOF

  $ newclientrepo
  $ echo foo > foo
  $ echo bar > bar
  $ hg commit -Aqm one
  $ echo foo >> foo
  $ echo bar >> bar
  $ setconfig extensions.copyfileerror=$TESTTMP/inject_errors.py
  $ cat <<EOS | hg amend -i --config ui.interactive=true
  > y
  > y
  > y
  > y
  > EOS
  diff --git a/bar b/bar
  1 hunks, 1 lines changed
  examine changes to 'bar'? [Ynesfdaq?] y
  
  @@ -1,1 +1,2 @@
   bar
  +bar
  record change 1/2 to 'bar'? [Ynesfdaq?] y
  
  diff --git a/foo b/foo
  1 hunks, 1 lines changed
  examine changes to 'foo'? [Ynesfdaq?] y
  
  @@ -1,1 +1,2 @@
   foo
  +foo
  record change 2/2 to 'foo'? [Ynesfdaq?] y
  
  error restoring $TESTTMP/repo1/.hg/record-backups/* to bar: copyfile error (glob)
  abort: internalpatch error
  [255]

We only lost the change for the one file that failed to restore ("bar"):
  $ hg st
  M foo

Backup file still contains content for "bar":
  $ cat $TESTTMP/repo1/.hg/record-backups/*
  bar
  bar

  $ hg show
  commit:      1435984fdceb
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       bar foo
  description:
  one
  
  
  diff --git a/bar b/bar
  new file mode 100644
  --- /dev/null
  +++ b/bar
  @@ -0,0 +1,1 @@
  +bar
  diff --git a/foo b/foo
  new file mode 100644
  --- /dev/null
  +++ b/foo
  @@ -0,0 +1,1 @@
  +foo
