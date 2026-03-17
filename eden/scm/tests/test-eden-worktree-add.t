
#require eden

  $ setconfig worktree.enabled=true

setup backing repo

  $ newclientrepo myrepo
  $ touch file.txt
  $ hg add file.txt
  $ hg commit -m "init"

test worktree add - basic

  $ hg worktree add $TESTTMP/linked1
  created linked worktree at $TESTTMP/linked1

test worktree add - with label

  $ hg worktree add $TESTTMP/linked2 --label "feature-x"
  created linked worktree at $TESTTMP/linked2

test worktree add - from a subdirectory of the repo

  $ mkdir -p subdir/nested
  $ cd subdir/nested
  $ hg worktree add $TESTTMP/linked_from_subdir
  created linked worktree at $TESTTMP/linked_from_subdir
  $ cd $TESTTMP/myrepo

test worktree add - missing PATH argument

  $ hg worktree add
  abort: usage: sl worktree add PATH
  [255]

test worktree add - destination exists

  $ mkdir $TESTTMP/existing
  $ hg worktree add $TESTTMP/existing
  abort: destination path '$TESTTMP/existing' already exists
  [255]
  $ rmdir $TESTTMP/existing

test worktree add - linked checkout has same files

  $ ls $TESTTMP/linked1/file.txt
  $TESTTMP/linked1/file.txt

test worktree add - linked checkout is on the same commit as main checkout

  $ main_hash=$(hg log -r . -T '{node}')
  $ linked_hash=$(cd $TESTTMP/linked1 && hg log -r . -T '{node}')
  $ test "$main_hash" = "$linked_hash"
  $ echo "main: $main_hash, linked: $linked_hash"
  main: *, linked: * (glob)

test worktree add - edensparse filters are copied to linked checkout

  $ enable edensparse
  $ cd $TESTTMP
  $ newrepo sparse_server
  $ echo content > included.txt
  $ echo other > excluded.txt
  $ cat > my-filter <<EOF
  > [include]
  > included.txt
  > my-filter
  > [exclude]
  > excluded.txt
  > EOF
  $ hg commit -Aqm 'add files and filter'
  $ hg book master

  $ cd $TESTTMP
  $ hg clone -q --eden test:sparse_server sparse_client --config clone.eden-sparse-filter=my-filter
  $ cd sparse_client

verify main checkout has sparse config

  $ hg filteredfs show
  Enabled Profiles:
  
      ~ my-filter

create linked worktree and verify sparse config is copied

  $ hg worktree add $TESTTMP/sparse_linked
  created linked worktree at $TESTTMP/sparse_linked
  $ cd $TESTTMP/sparse_linked
  $ hg filteredfs show
  Enabled Profiles:
  
      ~ my-filter
  $ cd $TESTTMP/sparse_client

verify both checkouts have the same sparse config content

  $ cmp .hg/sparse $TESTTMP/sparse_linked/.hg/sparse

test worktree add - prefetch profiles are copied to linked checkout

  $ cd $TESTTMP
  $ newrepo prefetch_server
  $ echo content > file.txt
  $ hg commit -Aqm 'add file'
  $ hg book master

  $ cd $TESTTMP
  $ hg clone -q --eden test:prefetch_server prefetch_client
  $ cd prefetch_client

activate a prefetch profile in the main checkout

  $ eden prefetch-profile activate trees
  $ eden prefetch-profile list --checkout .
  trees

create linked worktree and verify prefetch profile is copied

  $ hg worktree add $TESTTMP/prefetch_linked
  created linked worktree at $TESTTMP/prefetch_linked
  $ eden prefetch-profile list --checkout $TESTTMP/prefetch_linked
  trees

test worktree add - redirections are copied to linked checkout

  $ cd $TESTTMP
  $ newrepo redirect_server
  $ echo content > file.txt
  $ hg commit -Aqm 'add file'
  $ hg book master

  $ cd $TESTTMP
  $ hg clone -q --eden test:redirect_server redirect_client
  $ cd redirect_client

add a symlink redirection in the main checkout

  $ mkdir build_output
  $ eden redirect add build_output symlink
  $ eden redirect list --json --mount . | grep -o '"repo_path":"build_output"'
  "repo_path":"build_output"

create linked worktree and verify redirection is copied

  $ hg worktree add $TESTTMP/redirect_linked
  created linked worktree at $TESTTMP/redirect_linked
  $ eden redirect list --json --mount $TESTTMP/redirect_linked | grep -o '"repo_path":"build_output"'
  "repo_path":"build_output"

clean up redirections

  $ eden redirect del build_output
  $ eden redirect del --mount $TESTTMP/redirect_linked build_output

test worktree add - enable-windows-symlinks is propagated from source checkout

On Windows, enable-windows-symlinks defaults to true in the source checkout's
EdenFS config.toml. worktree add reads this value and passes
--enable-windows-symlinks to eden clone so the linked worktree matches.
Verify the linked worktree has the same setting as the source.

#if windows
  $ cd $TESTTMP
  $ newrepo symlinks_server
  $ echo content > file.txt
  $ hg commit -Aqm 'add file'
  $ hg book master

  $ cd $TESTTMP
  $ hg clone -q --eden test:symlinks_server symlinks_client
  $ cd symlinks_client

find the eden client dir (parse .eden/config on Windows)

  $ source_client=$(python -c '
  > for l in open(".eden/config"):
  >     if l.strip().startswith("client"):
  >         v = l.split(chr(34))[1]
  >         print(v.replace(chr(92)*2, chr(92)).replace(chr(92), "/"))
  >         break
  > ')
  $ source_config="$source_client/config.toml"

verify source checkout has enable-windows-symlinks = true (Windows default)

  $ grep 'enable-windows-symlinks' "$source_config"
  enable-windows-symlinks = true

create linked worktree and verify enable-windows-symlinks is propagated

  $ hg worktree add $TESTTMP/symlinks_linked
  created linked worktree at $TESTTMP/symlinks_linked

  $ cd $TESTTMP/symlinks_linked
  $ linked_client=$(python -c '
  > for l in open(".eden/config"):
  >     if l.strip().startswith("client"):
  >         v = l.split(chr(34))[1]
  >         print(v.replace(chr(92)*2, chr(92)).replace(chr(92), "/"))
  >         break
  > ')
  $ cd $TESTTMP/symlinks_client
  $ linked_config="$linked_client/config.toml"
  $ grep 'enable-windows-symlinks' "$linked_config"
  enable-windows-symlinks = true
#endif
