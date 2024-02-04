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

  $ hg log -R $TESTTMP/small-hg-client -G -T '{node} {desc|firstline}\n'
  @  11f848659bfcf77abd04f947883badd8efa88d26 first post-move commit
  │
  o  fc7ae591de0e714dc3abfb7d4d8aa5f9e400dd77 pre-move commit
  

  $ hg log -R $TESTTMP/large-hg-client -G -T '{node} {desc|firstline}\n'
  @  bfcfb674663c5438027bcde4a7ae5024c838f76a first post-move commit
  │
  o  5a0ba980eee8c305018276735879efba05b3e988 move commit
  │
  o  fc7ae591de0e714dc3abfb7d4d8aa5f9e400dd77 pre-move commit
  

  $ cd "$TESTTMP/small-hg-client"
  $ export REPONAME=small-mon
  $ hgedenapi debugapi -e committranslateids -i "[{'Bonsai': '$SMALL_MASTER_BONSAI'}]" -i "'Hg'"
  [{"commit": {"Bonsai": bin("1ba347e63a4bf200944c22ade8dbea038dd271ef97af346ba4ccfaaefb10dd4d")},
    "translated": {"Hg": bin("11f848659bfcf77abd04f947883badd8efa88d26")}}]

  $ hgedenapi debugapi -e committranslateids -i "[{'Hg': '11f848659bfcf77abd04f947883badd8efa88d26'}]" -i "'Hg'" -i None -i "'large-mon'"
  [{"commit": {"Hg": bin("11f848659bfcf77abd04f947883badd8efa88d26")},
    "translated": {"Hg": bin("bfcfb674663c5438027bcde4a7ae5024c838f76a")}}]

  $ hgedenapi debugapi -e committranslateids -i "[{'Hg': 'bfcfb674663c5438027bcde4a7ae5024c838f76a'}]" -i "'Hg'" -i "'large-mon'"
  [{"commit": {"Hg": bin("bfcfb674663c5438027bcde4a7ae5024c838f76a")},
    "translated": {"Hg": bin("11f848659bfcf77abd04f947883badd8efa88d26")}}]

  $ hgedenapi log -r bfcfb67466 -T '{node}\n' --config 'megarepo.transparent-lookup=small-mon large-mon' --config extensions.megarepo=
  pulling 'bfcfb67466' from 'mononoke://$LOCALIP:$LOCAL_PORT/small-mon'
  pull failed: bfcfb67466 not found
  translated bfcfb674663c5438027bcde4a7ae5024c838f76a@large-mon to 11f848659bfcf77abd04f947883badd8efa88d26
  pulling '11f848659bfcf77abd04f947883badd8efa88d26' from 'mononoke://$LOCALIP:$LOCAL_PORT/small-mon'
  11f848659bfcf77abd04f947883badd8efa88d26

  $ hgedenapi log -r large-mon/master_bookmark -T '{node}\n' --config 'megarepo.transparent-lookup=large-mon' --config extensions.megarepo=
  translated bfcfb674663c5438027bcde4a7ae5024c838f76a@large-mon to 11f848659bfcf77abd04f947883badd8efa88d26
  pulling '11f848659bfcf77abd04f947883badd8efa88d26' from 'mononoke://$LOCALIP:$LOCAL_PORT/small-mon'
  11f848659bfcf77abd04f947883badd8efa88d26
