# (c) Meta Platforms, Inc. and affiliates. Confidential and proprietary.

  $ . "${TEST_FIXTURES}/library.sh"
  $ . "${TEST_FIXTURES}/library-push-redirector.sh"

Setup config repo:
  $ setup_configerator_configs
  $ INFINITEPUSH_ALLOW_WRITES=true \
  >   INFINITEPUSH_NAMESPACE_REGEX='^scratch/.+$' \
  >   create_large_small_repo
  Adding synced mapping entry
  $ cd "$TESTTMP/mononoke-config"
  $ cat > "$PUSHREDIRECT_CONF/enable" <<EOF
  > {
  > "per_repo": {
  >   "1": {
  >      "draft_push": false,
  >      "public_push": true
  >    }
  >   }
  > }
  > EOF

  $ start_large_small_repo
  Starting Mononoke server
  $ init_local_large_small_clones

  $ cd "$TESTTMP/small-hg-client"
  $ export REPONAME=small-mon
  $ hgedenapi debugapi -e committranslateids -i "[{'Bonsai': '$SMALL_MASTER_BONSAI'}]" -i "'Hg'"
  [{"commit": {"Bonsai": bin("1ba347e63a4bf200944c22ade8dbea038dd271ef97af346ba4ccfaaefb10dd4d")},
    "translated": {"Hg": bin("11f848659bfcf77abd04f947883badd8efa88d26")}}]
