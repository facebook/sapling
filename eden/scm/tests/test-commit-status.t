
#require no-eden

  $ eagerepo
  $ enable amend
  $ setconfig commit.status-path-limit=4

Agent commit currently does not show the created commit or its files:

  $ newrepo
  $ mkdir docs src tests
  $ echo docs > docs/readme
  $ echo a > src/a
  $ echo b > src/b
  $ echo c > src/c
  $ echo test > tests/test
  $ sl addremove -q
  $ CODING_AGENT_METADATA=id=test_agent sl commit -m initial

Agent amend currently does not show the rewritten commit or its files:

  $ echo changed >> src/a
  $ mkdir surprise
  $ echo surprise > surprise/file
  $ CODING_AGENT_METADATA=id=test_agent sl amend -A
  adding surprise/file
