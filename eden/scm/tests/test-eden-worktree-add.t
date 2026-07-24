
#require eden

  $ setconfig worktree.enabled=true

setup backing repo

  $ newclientrepo myrepo
  $ touch file.txt
  $ sl add file.txt
  $ sl commit -m "init"

test worktree add - basic

  $ sl worktree add $TESTTMP/linked1
  created linked worktree at $TESTTMP/linked1

test worktree add - with label

  $ sl worktree add $TESTTMP/linked2 --label "feature-x"
  created linked worktree at $TESTTMP/linked2

test worktree add - from a subdirectory of the repo

  $ mkdir -p subdir/nested
  $ cd subdir/nested
  $ sl worktree add $TESTTMP/linked_from_subdir
  created linked worktree at $TESTTMP/linked_from_subdir
  $ cd $TESTTMP/myrepo

test worktree add - missing PATH argument and no path-generator

  $ sl worktree add
  abort: worktree.path-generator is not configured; pass a PATH argument
  [255]

test worktree --rev is rejected for other subcommands

  $ sl worktree list --rev .
  abort: --rev can only be used with 'worktree add'
  [255]

test worktree add - require-generated-path without a generator

  $ sl worktree add --config worktree.require-generated-path=true
  abort: worktree.path-generator is required when worktree.require-generated-path=true
  [255]

test worktree add - destination exists

  $ mkdir $TESTTMP/existing
  $ sl worktree add $TESTTMP/existing
  abort: destination path '$TESTTMP/existing' already exists
  [255]
  $ rmdir $TESTTMP/existing

test worktree add - linked checkout has same files

  $ ls $TESTTMP/linked1/file.txt
  $TESTTMP/linked1/file.txt

test worktree add - linked checkout is on the same commit as main checkout

  $ main_hash=$(sl log -r . -T '{node}')
  $ linked_hash=$(cd $TESTTMP/linked1 && sl log -r . -T '{node}')
  $ test "$main_hash" = "$linked_hash"
  $ echo "main: $main_hash, linked: $linked_hash"
  main: *, linked: * (glob)

test worktree add - revision flag checks out requested revision and accepts bookmark names

  $ base_hash=$(sl log -r . -T '{node}')
  $ sl bookmark -i -r "$base_hash" base
  $ echo more >> file.txt
  $ sl commit -m "second"
  $ tip_hash=$(sl log -r . -T '{node}')
  $ test "$base_hash" != "$tip_hash"
  $ sl worktree add $TESTTMP/linked_revision --rev base
  created linked worktree at $TESTTMP/linked_revision
  $ linked_revision_hash=$(cd $TESTTMP/linked_revision && sl log -r . -T '{node}')
  $ test "$linked_revision_hash" = "$base_hash"
  $ test "$linked_revision_hash" != "$tip_hash"

test worktree add - invalid revision does not create a worktree

  $ sl worktree add $TESTTMP/invalid_rev --rev does-not-exist
  abort: unknown revision 'does-not-exist'
  [255]
  $ test -d $TESTTMP/invalid_rev
  [1]

test worktree add - revision flag accepts remote-only commit hashes

  $ cd $TESTTMP
  $ newrepo remote_server
  $ echo base > file.txt
  $ sl commit -Aqm "base"
  $ sl clone -q --eden test:remote_server $TESTTMP/remote_client
  $ cd $TESTTMP/remote_server
  $ echo remote > file.txt
  $ sl commit -Aqm "remote-only"
  $ remote_hash=$(sl log -r . -T '{node}')
  $ cd $TESTTMP/remote_client
  $ sl worktree add $TESTTMP/remote_linked --rev "$remote_hash"
  created linked worktree at $TESTTMP/remote_linked
  $ remote_linked_hash=$(cd $TESTTMP/remote_linked && sl log -r . -T '{node}')
  $ test "$remote_linked_hash" = "$remote_hash"
  $ cd $TESTTMP/myrepo

test worktree add - writes .sl/worktreename marker (basename when no --label)

  $ cat $TESTTMP/linked1/.sl/worktreename
  linked1 (no-eol)

test worktree add - marker reflects --label when provided

  $ cat $TESTTMP/linked2/.sl/worktreename
  feature-x (no-eol)

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
  $ sl commit -Aqm 'add files and filter'
  $ sl book master

  $ cd $TESTTMP
  $ sl clone -q --eden test:sparse_server sparse_client --config clone.eden-sparse-filter=my-filter
  $ cd sparse_client

verify main checkout has sparse config

  $ sl filteredfs show
  Enabled Profiles:
  
      ~ my-filter

create linked worktree and verify sparse config is copied

  $ sl worktree add $TESTTMP/sparse_linked
  created linked worktree at $TESTTMP/sparse_linked
  $ cd $TESTTMP/sparse_linked
  $ sl filteredfs show
  Enabled Profiles:
  
      ~ my-filter
  $ cd $TESTTMP/sparse_client

verify both checkouts have the same sparse config content

  $ cmp .sl/sparse $TESTTMP/sparse_linked/.sl/sparse

test worktree add - sl status works in linked filteredfs worktree

  $ cd $TESTTMP/sparse_client
  $ sl worktree add $TESTTMP/sparse_linked_status
  created linked worktree at $TESTTMP/sparse_linked_status
  $ cd $TESTTMP/sparse_linked_status
  $ sl status

  $ cd $TESTTMP/sparse_client

test worktree add - prefetch profiles are copied to linked checkout

  $ cd $TESTTMP
  $ newrepo prefetch_server
  $ echo content > file.txt
  $ sl commit -Aqm 'add file'
  $ sl book master

  $ cd $TESTTMP
  $ sl clone -q --eden test:prefetch_server prefetch_client
  $ cd prefetch_client

activate a prefetch profile in the main checkout

  $ eden prefetch-profile activate trees
  $ eden prefetch-profile list --checkout .
  trees

create linked worktree and verify prefetch profile is copied

  $ sl worktree add $TESTTMP/prefetch_linked
  created linked worktree at $TESTTMP/prefetch_linked
  $ eden prefetch-profile list --checkout $TESTTMP/prefetch_linked
  trees

test worktree add - redirections are copied to linked checkout

  $ cd $TESTTMP
  $ newrepo redirect_server
  $ echo content > file.txt
  $ sl commit -Aqm 'add file'
  $ sl book master

  $ cd $TESTTMP
  $ sl clone -q --eden test:redirect_server redirect_client
  $ cd redirect_client

add a symlink redirection in the main checkout

  $ mkdir build_output
  $ eden redirect add build_output symlink
  $ eden redirect list --json --mount . | grep -o '"repo_path":"build_output"'
  "repo_path":"build_output"

create linked worktree and verify redirection is copied

  $ sl worktree add $TESTTMP/redirect_linked
  created linked worktree at $TESTTMP/redirect_linked
  $ eden redirect list --json --mount $TESTTMP/redirect_linked | grep -o '"repo_path":"build_output"'
  "repo_path":"build_output"

clean up redirections

  $ eden redirect del build_output
  $ eden redirect del --mount $TESTTMP/redirect_linked build_output

test worktree add - post-worktree-add hook fires with correct env vars

  $ cd $TESTTMP
  $ newclientrepo hook_repo
  $ touch file.txt
  $ sl add file.txt
  $ sl commit -m "init"
#if windows
  $ setconfig hooks.post-worktree-add="echo PATH:%HG_PATH% SOURCE:%HG_SOURCE%"
  $ sl worktree add $TESTTMP/hook_linked
  created linked worktree at $TESTTMP/hook_linked
  PATH:$TESTTMP?hook_linked SOURCE:$TESTTMP?hook_repo\r (esc) (glob)
#else
  $ setconfig hooks.post-worktree-add="echo PATH:\$HG_PATH SOURCE:\$HG_SOURCE"
  $ sl worktree add $TESTTMP/hook_linked
  created linked worktree at $TESTTMP/hook_linked
  PATH:$TESTTMP/hook_linked SOURCE:$TESTTMP/hook_repo
#endif

test worktree add - post-worktree-add hook failure does not abort command

#if windows
  $ setconfig "hooks.post-worktree-add=cmd /c exit 1"
  $ sl worktree add $TESTTMP/hook_linked_fail
  created linked worktree at $TESTTMP/hook_linked_fail
#else
  $ setconfig hooks.post-worktree-add=false
  $ sl worktree add $TESTTMP/hook_linked_fail
  created linked worktree at $TESTTMP/hook_linked_fail
#endif
  $ test -d $TESTTMP/hook_linked_fail

test worktree add - pre-worktree-add hook fires with correct env vars

#if windows
  $ setconfig hooks.pre-worktree-add="echo PATH:%HG_PATH% SOURCE:%HG_SOURCE%"
  $ sl worktree add $TESTTMP/pre_hook_linked
  PATH:$TESTTMP?pre_hook_linked SOURCE:$TESTTMP?hook_repo\r (esc) (glob)
  created linked worktree at $TESTTMP/pre_hook_linked
#else
  $ setconfig hooks.pre-worktree-add="echo PATH:\$HG_PATH SOURCE:\$HG_SOURCE"
  $ sl worktree add $TESTTMP/pre_hook_linked
  PATH:$TESTTMP/pre_hook_linked SOURCE:$TESTTMP/hook_repo
  created linked worktree at $TESTTMP/pre_hook_linked
#endif

test worktree add - pre-worktree-add hook failure aborts command

#if windows
  $ setconfig "hooks.pre-worktree-add=cmd /c exit 1"
  $ sl worktree add $TESTTMP/pre_hook_blocked
  abort: pre-worktree-add hook exited with status 1
  [255]
#else
  $ setconfig hooks.pre-worktree-add=false
  $ sl worktree add $TESTTMP/pre_hook_blocked
  abort: pre-worktree-add hook exited with status 1
  [255]
#endif
  $ test -d $TESTTMP/pre_hook_blocked
  [1]

test worktree add - path-generator produces path when PATH omitted

  $ cd $TESTTMP
  $ newclientrepo gen_repo
  $ touch file.txt
  $ sl add file.txt
  $ sl commit -m "init"
  $ setconfig worktree.path-generator="echo $TESTTMP/generated_wt"
  $ sl worktree add
  created linked worktree at $TESTTMP/generated_wt
  $ test -d $TESTTMP/generated_wt

test worktree add - specified revision with generated path

  $ echo generated-rev > generated-rev.txt
  $ sl commit -Aqm "generated rev"
  $ generated_rev=$(sl log -r . -T '{node}')
  $ sl goto -q .^
  $ setconfig worktree.path-generator="echo $TESTTMP/generated_rev_wt"
  $ sl worktree add --rev "$generated_rev"
  created linked worktree at $TESTTMP/generated_rev_wt
  $ linked_hash=$(cd $TESTTMP/generated_rev_wt && sl log -r . -T '{node}')
  $ test "$generated_rev" = "$linked_hash"

test worktree add - path-generator receives correct env vars

#if windows
  $ setconfig worktree.path-generator="echo SOURCE:%HG_SOURCE% LABEL:%HG_LABEL% SL_SOURCE:%SL_SOURCE% SL_LABEL:%SL_LABEL%>$TESTTMP/gen_env_out& echo $TESTTMP/gen_envcheck"
  $ sl worktree add --label my-feature
  created linked worktree at $TESTTMP/gen_envcheck
  $ cat $TESTTMP/gen_env_out
  SOURCE:$TESTTMP?gen_repo LABEL:my-feature SL_SOURCE:$TESTTMP?gen_repo SL_LABEL:my-feature\r (esc) (glob)
#else
  $ setconfig worktree.path-generator="echo SOURCE:\$HG_SOURCE LABEL:\$HG_LABEL SL_SOURCE:\$SL_SOURCE SL_LABEL:\$SL_LABEL > $TESTTMP/gen_env_out; echo $TESTTMP/gen_envcheck"
  $ sl worktree add --label my-feature
  created linked worktree at $TESTTMP/gen_envcheck
  $ cat $TESTTMP/gen_env_out
  SOURCE:$TESTTMP/gen_repo LABEL:my-feature SL_SOURCE:$TESTTMP/gen_repo SL_LABEL:my-feature
#endif

test worktree add - path-generator failure aborts command

#if windows
  $ setconfig "worktree.path-generator=cmd /c exit 1"
  $ sl worktree add
  abort: worktree.path-generator exited with exit code: 1* (glob)
  [255]
#else
  $ setconfig worktree.path-generator=false
  $ sl worktree add
  abort: worktree.path-generator exited with exit status: 1
  [255]
#endif

test worktree add - path-generator empty output aborts command

#if windows
  $ setconfig worktree.path-generator=echo.
  $ sl worktree add
  abort: worktree.path-generator returned empty output
  [255]
#else
  $ setconfig worktree.path-generator="echo ''"
  $ sl worktree add
  abort: worktree.path-generator returned empty output
  [255]
#endif

test worktree add - path-generator empty output aborts command (empty config)

  $ setconfig worktree.path-generator=
  $ sl worktree add
  abort: worktree.path-generator returned empty output
  [255]

test worktree add - path-generator extra stdout aborts command

#if windows
  $ setconfig worktree.path-generator="echo %TESTTMP%\\gen_multiline& echo noise"
  $ sl worktree add
  abort: worktree.path-generator must write exactly one path to stdout
  [255]
#else
  $ setconfig worktree.path-generator="printf '$TESTTMP/gen_multiline\nnoise\n'"
  $ sl worktree add
  abort: worktree.path-generator must write exactly one path to stdout
  [255]
#endif

test worktree add - path-generator relative output aborts command

  $ setconfig worktree.path-generator="echo relative_wt"
  $ sl worktree add
  abort: worktree.path-generator must return an absolute path, got 'relative_wt'
  [255]

test worktree add - path-generator invalid path aborts command

#if windows
  $ setconfig 'worktree.path-generator=python -c "import os,sys; sys.stdout.buffer.write((os.environ[\"TESTTMP\"].replace(\"\\\\\", \"/\") + \"/gen_bad_\" + chr(0) + \"path\").encode())"'
  $ sl worktree add
  abort: worktree.path-generator returned invalid path '$TESTTMP/gen_bad_\0path': contains NUL byte
  [255]
#else
  $ setconfig worktree.path-generator="printf '/tmp/gen_bad_\0path'"
  $ sl worktree add
  abort: worktree.path-generator returned invalid path '/tmp/gen_bad_\0path': contains NUL byte
  [255]
#endif

test worktree add - require-generated-path rejects user PATH

  $ setconfig worktree.require-generated-path=true worktree.path-generator="echo $TESTTMP/gen_required"
  $ sl worktree add $TESTTMP/custom_path
  abort: custom worktree paths are not allowed (worktree.require-generated-path is set); run without a path argument to use the configured path generator
  [255]
  $ test -d $TESTTMP/custom_path
  [1]

test worktree add - require-generated-path allows omitted PATH with generator

  $ sl worktree add
  created linked worktree at $TESTTMP/gen_required
  $ test -d $TESTTMP/gen_required

test worktree add - Windows ANSI path output is accepted

#if windows
  $ setconfig 'worktree.path-generator=python -c "import ctypes, os, sys; cp = ctypes.windll.kernel32.GetACP(); path = os.environ[\"TESTTMP\"].replace(\"\\\\\", \"/\") + \"/gen_\u00fc\"; sys.stdout.buffer.write(path.encode(f\"cp{cp}\"))"'
  $ sl worktree add
  created linked worktree at $TESTTMP/gen_* (glob)
  $ python -c "import os; expected = 'gen_' + chr(0x00fc); assert os.path.isdir(os.path.join(os.environ['TESTTMP'], expected)), os.listdir(os.environ['TESTTMP'])"
#endif
