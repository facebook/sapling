#debugruntest-compatible

Message is optional
  $ SL_LOG=foo=debug hg dbsh -c 'sapling.tracing.debug(target="foo", hello="there"); sapling.tracing.debug("message", target="foo", hello="there")'
  DEBUG foo: hello="there"
  DEBUG foo: message hello="there"
