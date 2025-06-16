#require diff

  $ setconfig drawdag.defaultfiles=false
  $ enable extdiff

  $ newclientrepo

  $ cat <<EOF >> $HGRCPATH
  > [extdiff]
  > cmd.gdiff = diff
  > opts.gdiff = -ur
  > EOF

  $ drawdag <<EOS
  > A  # A/foo/differs = one\ntwo\n
  >    # A/foo/same = same
  >    # A/foo/onlyfoo = onlyfoo\n
  >    # A/bar/differs = one\nthree\n
  >    # A/bar/same = same
  >    # A/bar/onlybar = onlybar\n
  > EOS

validate from/to paths:
  $ hg subtree gdiff -r $A -r $A --from-path foo --to-path barbar
  abort: path 'barbar' does not exist in commit 112bacaa6bb9
  [255]
  $ hg subtree gdiff -r $A -r $A --from-path foofoo --to-path bar
  abort: path 'foofoo' does not exist in commit 112bacaa6bb9
  [255]

Basic diff with add, modify, and remove:
  $ hg subtree gdiff -r $A -r $A --from-path foo --to-path bar
  diff -ur repo1.112bacaa6bb9.1a/bar/differs repo1.112bacaa6bb9.2/bar/differs
  --- repo1.112bacaa6bb9.1a/bar/differs	* (glob)
  +++ repo1.112bacaa6bb9.2/bar/differs	* (glob)
  @@ -1,2 +1,2 @@
   one
  -two
  +three
  Only in repo1.112bacaa6bb9.2/bar: onlybar
  Only in repo1.112bacaa6bb9.1a/bar: onlyfoo
  [1]


Can filter by paths "--to-path" space:
  $ hg subtree gdiff -r $A -r $A --from-path foo --to-path bar bar/differs
  --- /tmp/extdiff.*/repo1.112bacaa6bb9.1a/bar/differs	* (glob)
  +++ repo1.112bacaa6bb9.2/bar/differs	* (glob)
  @@ -1,2 +1,2 @@
   one
  -two
  +three
  [1]

Can diff with working copy:
  $ newclientrepo
  $ drawdag <<EOS
  > A  # A/foo/file = cat\n
  >    # A/bar/file = cat\n
  > EOS
  $ hg go -q $A
  $ hg subtree gdiff --from-path foo --to-path bar
  $ echo dog > bar/file
  $ hg subtree gdiff --from-path foo --to-path bar
  --- /tmp/extdiff.*/repo2.669b16a60a5a.1a/bar/file	* (glob)
  +++ $TESTTMP/repo2/bar/file	* (glob)
  @@ -1 +1 @@
  -cat
  +dog
  [1]
  $ hg subtree gdiff -r . --from-path foo --to-path bar
  --- /tmp/extdiff.*/repo2.669b16a60a5a.1a/bar/file	* (glob)
  +++ $TESTTMP/repo2/bar/file	* (glob)
  @@ -1 +1 @@
  -cat
  +dog
  [1]

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
  $ hg subtree gdiff -r $A -r $B --from-path foo --to-path bar --from-path foo --to-path baz
  diff -ur repo3.19d6f42db3d3.1a/bar/animal repo3.7720cd095633.2/bar/animal
  --- repo3.19d6f42db3d3.1a/bar/animal	* (glob)
  +++ repo3.7720cd095633.2/bar/animal	* (glob)
  @@ -1 +1 @@
  -cat
  +giraffe
  diff -ur repo3.19d6f42db3d3.1a/bar/fruit repo3.7720cd095633.2/bar/fruit
  --- repo3.19d6f42db3d3.1a/bar/fruit	* (glob)
  +++ repo3.7720cd095633.2/bar/fruit	* (glob)
  @@ -1 +1 @@
  -apple
  +banana
  diff -ur repo3.19d6f42db3d3.1a/baz/animal repo3.7720cd095633.2/baz/animal
  --- repo3.19d6f42db3d3.1a/baz/animal	* (glob)
  +++ repo3.7720cd095633.2/baz/animal	* (glob)
  @@ -1 +1 @@
  -cat
  +horse
  Only in repo3.7720cd095633.2/baz: food
  Only in repo3.19d6f42db3d3.1a/baz: fruit
  [1]
