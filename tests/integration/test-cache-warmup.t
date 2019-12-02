  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ export CACHE_WARMUP_BOOKMARK="master_bookmark"
  $ setup_common_config
  $ cd $TESTTMP

setup repo

  $ hg init repo-hg

setup hg server repo
  $ cd repo-hg
  $ setup_hg_server
  $ echo a > a && hg add a && hg ci -m a

create master bookmark

  $ hg bookmark master_bookmark -r tip

blobimport them into Mononoke storage and start Mononoke
  $ cd ..
  $ blobimport repo-hg/.hg repo

start mononoke

  $ mononoke
  $ wait_for_mononoke
  $ wait_for_mononoke_cache_warmup
