  $ enable amend

Agent: a -m title with a typed "\n" before a commit message field is rejected:

  $ newclientrepo
  $ echo a > a
  $ sl add a
  $ CODING_AGENT_METADATA=id=test_agent sl commit --config devel.print-metrics=commit.escapednewline -m 'Fix the loader\n\nSummary:\nRetry once.'
  abort: commit message title contains a '\n' escape sequence before 'Summary:'
  (use literal newlines instead of '\n' escape sequences, which Sapling does not interpret)
  commit.escapednewline.agent_rejected: 1
  [255]
  $ sl status
  A a

Following the hint, a heredoc gives literal newlines and the commit goes through:

  $ CODING_AGENT_METADATA=id=test_agent sl commit -q --logfile - <<'EOF'
  > Fix the loader
  > 
  > Summary:
  > Retry once.
  > EOF
  $ sl log -r . -T '{desc|firstline}\n'
  Fix the loader

A file passed to --logfile is checked like -m:

  $ cat > $TESTTMP/msg <<'EOF'
  > Fix the docs\n\nTest Plan: none
  > EOF
  $ CODING_AGENT_METADATA=id=test_agent sl commit -q --logfile $TESTTMP/msg
  abort: commit message title contains a '\n' escape sequence before 'Test Plan:'
  (use literal newlines instead of '\n' escape sequences, which Sapling does not interpret)
  [255]

A typed "\n" that does not run into a field is allowed:

  $ echo csv > csv
  $ CODING_AGENT_METADATA=id=test_agent sl commit -Aqm 'Handle \n in CSV export'

Only the title is checked, so the body may quote a typed "\n" (here the body
comes from a second -m):

  $ echo doc > doc
  $ CODING_AGENT_METADATA=id=test_agent sl commit -Aq -m 'Explain the guard' -m 'Agents run sl commit -m "Fix\n\nSummary: ..." by mistake.'
  $ sl log -r . -T '{desc|firstline}\n'
  Explain the guard

The title is the first non-empty line, as Sapling stores it:

  $ CODING_AGENT_METADATA=id=test_agent sl commit -q -m '' -m 'Fix\n\nSummary: x'
  abort: commit message title contains a '\n' escape sequence before 'Summary:'
  (use literal newlines instead of '\n' escape sequences, which Sapling does not interpret)
  [255]

amend, metaedit and fold check their -m too:

  $ CODING_AGENT_METADATA=id=test_agent sl amend -m 'Handle CSV\n\nTest Plan: ran it'
  abort: commit message title contains a '\n' escape sequence before 'Test Plan:'
  (use literal newlines instead of '\n' escape sequences, which Sapling does not interpret)
  [255]
  $ CODING_AGENT_METADATA=id=test_agent sl metaedit -m 'Handle CSV\n\nSummary: x'
  abort: commit message title contains a '\n' escape sequence before 'Summary:'
  (use literal newlines instead of '\n' escape sequences, which Sapling does not interpret)
  [255]
  $ CODING_AGENT_METADATA=id=test_agent sl fold --from .^ -m 'Fix and handle CSV\n\nSummary: x'
  abort: commit message title contains a '\n' escape sequence before 'Summary:'
  (use literal newlines instead of '\n' escape sequences, which Sapling does not interpret)
  [255]

Humans get a warning instead, once even though amend reads its message twice:

  $ echo b > b
  $ sl commit --config devel.print-metrics=commit.escapednewline -Aqm 'Add b\n\nSummary: human'
  warning: commit message title contains a '\n' escape sequence before 'Summary:'
  (use literal newlines instead of '\n' escape sequences, which Sapling does not interpret)
  commit.escapednewline.human_warned: 1
  $ sl amend -q -m 'Add b\n\nSummary: human, amended'
  warning: commit message title contains a '\n' escape sequence before 'Summary:'
  (use literal newlines instead of '\n' escape sequences, which Sapling does not interpret)

A message that amend fills in itself is not checked again:

  $ echo more >> b
  $ CODING_AGENT_METADATA=id=test_agent sl amend -q
  $ sl log -r . -T '{desc}\n'
  Add b\n\nSummary: human, amended

Automation (HGPLAIN) skips the check, and the check can be turned off:

  $ echo c > c
  $ HGPLAIN=1 CODING_AGENT_METADATA=id=test_agent sl commit --config devel.print-metrics=commit.escapednewline -Aqm 'Add c\n\nSummary: plain'
  commit.escapednewline.automation_allowed: 1
  $ echo d > d
  $ CODING_AGENT_METADATA=id=test_agent sl commit --config commit.check-escaped-newlines=false -Aqm 'Add d\n\nSummary: off'
