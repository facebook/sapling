
#require no-eden

Tests for the pytoolsim automerge file type, used by the checked-in toolsim
manifest genai/msl/rl/projects/toolsim/simulated_apps/cli_name_map_generated.py.
That file is generated from per-tool toolsim.toml markers (one per CLI dir), so
two engineers adding two different tools each insert an alphabetically-sorted
entry into the same `CLI_TO_SIM_TOOL_NAME` dict and `CONFIGURED_TOOLS` tuple.
sort-inserts merges those concurrent inserts instead of conflicting.

  $ export HGIDENTITY=sl
  $ configure modern
  $ enable rebase
  $ setconfig automerge.disable-for-noninteractive=False
  $ setconfig automerge.mode=accept
  $ setconfig automerge.merge-algos=sort-inserts

Configure pytoolsim for test.py. One import-pattern matches BOTH the dict-entry
("cli": "sim",) and tuple-entry ("cli",) line shapes; the key is the CLI name.
  $ setconfig filetype-patterns.test.py=pytoolsim
  $ setconfig 'automerge.import-pattern:pytoolsim=re:^\s*"[^"]+"(:\s*"[^"]+")?,\s*$'
  $ setconfig 'automerge.import-key-pattern:pytoolsim=re:^\s*"([^"]+)"'

=== DICT-ENTRY TESTS ("cli": "sim",) ===

Two concurrent adds of alphabetically-adjacent dict entries auto-merge:

  $ newrepo
  $ drawdag <<'EOS'
  > B C # C/test.py=    "aaa": "Aaa",\n    "ccc": "Ccc",\n
  > |/  # B/test.py=    "aaa": "Aaa",\n    "bbb": "Bbb",\n
  > A   # A/test.py=    "aaa": "Aaa",\n
  > EOS
  $ sl rebase -r $C -d $B
  rebasing * "C" (glob)
  merging test.py
   lines 2-3 have been resolved by automerge algorithms
  $ sl cat -r tip test.py
  "aaa": "Aaa",
      "bbb": "Bbb",
      "ccc": "Ccc",

=== TUPLE-ENTRY TESTS ("cli",) ===

Two concurrent adds of alphabetically-adjacent tuple entries auto-merge:

  $ newrepo
  $ drawdag <<'EOS'
  > B C # C/test.py=    "aaa",\n    "ccc",\n
  > |/  # B/test.py=    "aaa",\n    "bbb",\n
  > A   # A/test.py=    "aaa",\n
  > EOS
  $ sl rebase -r $C -d $B
  rebasing * "C" (glob)
  merging test.py
   lines 2-3 have been resolved by automerge algorithms
  $ sl cat -r tip test.py
  "aaa",
      "bbb",
      "ccc",
