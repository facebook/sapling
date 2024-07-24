  $ setconfig diff.git=true

  $ newclientrepo
  $ drawdag <<EOS
  > A  # A/foo/differs = one\ntwo\n
  >    # A/foo/same = same
  >    # A/foo/onlyfoo = onlyfoo\n
  >    # A/bar/differs = one\nthree\n
  >    # A/bar/same = same
  >    # A/bar/onlybar = onlybar\n
  > EOS

Basic diff with add, modify, and remove:
  $ hg diff -r $A -r $A --from-path foo --to-path bar
  diff --git a/foo/differs b/bar/differs
  --- a/foo/differs
  +++ b/bar/differs
  @@ -1,2 +1,2 @@
   one
  -two
  +three
  diff --git a/foo/onlybar b/bar/onlybar
  new file mode 100644
  --- /dev/null
  +++ b/bar/onlybar
  @@ -0,0 +1,1 @@
  +onlybar
  diff --git a/foo/onlyfoo b/bar/onlyfoo
  deleted file mode 100644
  --- a/foo/onlyfoo
  +++ /dev/null
  @@ -1,1 +0,0 @@
  -onlyfoo


Same diff, but in --reverse:
  $ hg diff --reverse -r $A -r $A --from-path foo --to-path bar --traceback --config devel.collapse-traceback=false
  diff --git a/bar/differs b/foo/differs
  --- a/bar/differs
  +++ b/foo/differs
  @@ -1,2 +1,2 @@
   one
  -three
  +two
  diff --git a/bar/onlybar b/foo/onlybar
  deleted file mode 100644
  --- a/bar/onlybar
  +++ /dev/null
  @@ -1,1 +0,0 @@
  -onlybar
  diff --git a/bar/onlyfoo b/foo/onlyfoo
  new file mode 100644
  --- /dev/null
  +++ b/foo/onlyfoo
  @@ -0,0 +1,1 @@
  +onlyfoo


Check copy tracing:
  $ newclientrepo
  $ drawdag <<EOS
  > B  # B/foo/rename = dog\n (renamed from foo/file)
  > |
  > A  # A/foo/file = cat\n
  >    # A/bar/file = cat\n
  > EOS
  $ hg diff -r $B --from-path foo --to-path bar -r $A
  diff --git a/foo/rename b/bar/file
  rename from foo/rename
  rename to bar/file
  --- a/foo/rename
  +++ b/bar/file
  @@ -1,1 +1,1 @@
  -dog
  +cat
