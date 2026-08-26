#require no-eden

  $ setconfig agent.ignore-quiet=true
  $ newclientrepo
  $ echo a > a

Agent --quiet is ignored with a note:

  $ CODING_AGENT_METADATA=id=test_agent sl commit -Aqm first
  note: --quiet ignored for agents since output can contain important details (pass --quiet --quiet to force quiet)
  adding a

Agent --quiet --quiet forces quiet:

  $ echo b > b
  $ CODING_AGENT_METADATA=id=test_agent sl commit -A -q -q -m second

Humans keep single --quiet:

  $ echo c > c
  $ sl commit -Aqm third

Plain-mode automation keeps single --quiet:

  $ echo d > d
  $ HGPLAIN=1 CODING_AGENT_METADATA=id=test_agent sl commit -Aqm fourth

A polarity flip restarts the count, so --no-quiet -q is a single --quiet:

  $ echo f > f
  $ CODING_AGENT_METADATA=id=test_agent sl commit --no-quiet -Aqm sixth
  note: --quiet ignored for agents since output can contain important details (pass --quiet --quiet to force quiet)
  adding f

The config can restore the old behavior:

  $ echo e > e
  $ CODING_AGENT_METADATA=id=test_agent sl commit --config agent.ignore-quiet=false -Aqm fifth
