#debugruntest-compatible

#require no-eden

#inprocess-hg-incompatible

Message is optional
  $ SL_LOG=foo=debug hg dbsh -c 'sapling.tracing.debug(target="foo", hello="there"); sapling.tracing.debug("message", target="foo", hello="there")'
  DEBUG foo: hello="there"
  DEBUG foo: message hello="there"

Test value types
  $ SL_LOG=foo=debug hg dbsh -c 'sapling.tracing.debug(target="foo", str="str", int=123, bool=True, none=None)'
  DEBUG foo: bool=true int=123 str="str"
