  $ setconfig diff.git=true
  $ setconfig drawdag.defaultfiles=false

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


Can filter by paths "--to-path" space:
  $ hg diff -r $A -r $A --from-path foo --to-path bar bar/differs
  diff --git a/foo/differs b/bar/differs
  --- a/foo/differs
  +++ b/bar/differs
  @@ -1,2 +1,2 @@
   one
  -two
  +three

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


Can diff with working copy:
  $ newclientrepo
  $ drawdag <<EOS
  > A  # A/foo/file = cat\n
  >    # A/bar/file = cat\n
  > EOS
  $ hg go -q $A
  $ hg diff --from-path foo --to-path bar
  $ echo dog > bar/file
  $ hg diff --from-path foo --to-path bar
  diff --git a/foo/file b/bar/file
  --- a/foo/file
  +++ b/bar/file
  @@ -1,1 +1,1 @@
  -cat
  +dog
  $ hg diff -r . --from-path foo --to-path bar
  diff --git a/foo/file b/bar/file
  --- a/foo/file
  +++ b/bar/file
  @@ -1,1 +1,1 @@
  -cat
  +dog


Works with --only-files-in-revs:
  $ newclientrepo
  $ drawdag <<EOS
  > B  # B/bar/animal = giraffe\n
  > |
  > A  # A/foo/fruit = apple\n
  >    # A/foo/animal = cat\n
  >    # A/bar/fruit = banana\n
  >    # A/bar/animal = dog\n
  > EOS
  $ hg diff -r $B -r $B --from-path foo --to-path bar --only-files-in-revs
  diff --git a/foo/animal b/bar/animal
  --- a/foo/animal
  +++ b/bar/animal
  @@ -1,1 +1,1 @@
  -cat
  +giraffe
  $ hg diff -r $B -r $B --from-path bar --to-path foo --only-files-in-revs
  diff --git a/bar/animal b/foo/animal
  --- a/bar/animal
  +++ b/foo/animal
  @@ -1,1 +1,1 @@
  -giraffe
  +cat


Works with multiple grafts:
  $ newclientrepo
  $ drawdag <<EOS
  > B  # B/bar/animal = giraffe\n
  > |  # B/baz/food = sushi\n (renamed from baz/fruit)
  > |
  > A  # A/foo/fruit = apple\n
  >    # A/foo/animal = cat\n
  >    # A/bar/fruit = banana\n
  >    # A/bar/animal = dog\n
  >    # A/baz/fruit = orange\n
  >    # A/baz/animal = horse\n
  > EOS
  $ hg diff -r $A -r $B --from-path foo --to-path bar --from-path foo --to-path baz
  diff --git a/foo/animal b/bar/animal
  --- a/foo/animal
  +++ b/bar/animal
  @@ -1,1 +1,1 @@
  -cat
  +giraffe
  diff --git a/foo/fruit b/bar/fruit
  --- a/foo/fruit
  +++ b/bar/fruit
  @@ -1,1 +1,1 @@
  -apple
  +banana
  diff --git a/foo/animal b/baz/animal
  --- a/foo/animal
  +++ b/baz/animal
  @@ -1,1 +1,1 @@
  -cat
  +horse
  diff --git a/foo/fruit b/baz/food
  rename from foo/fruit
  rename to baz/food
  --- a/foo/fruit
  +++ b/baz/food
  @@ -1,1 +1,1 @@
  -apple
  +sushi
  $ hg diff -r $B -r $B --from-path foo --to-path bar --from-path foo --to-path baz --only-files-in-revs
  diff --git a/foo/animal b/bar/animal
  --- a/foo/animal
  +++ b/bar/animal
  @@ -1,1 +1,1 @@
  -cat
  +giraffe
  diff --git a/foo/fruit b/baz/food
  rename from foo/fruit
  rename to baz/food
  --- a/foo/fruit
  +++ b/baz/food
  @@ -1,1 +1,1 @@
  -apple
  +sushi
