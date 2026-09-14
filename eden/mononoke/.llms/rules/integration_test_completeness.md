---
name: integration-test-completeness
metadata:
  oncalls: ['source_control']
  strict: true
  apply_to_path: 'eden/mononoke/.*(\.rs|\.t)$'
  apply_to_content: 'async fn |fn test_|pub async fn handle|SaplingRemoteApiHandler|service_method|thrift_method'
---

# Integration Test Completeness

**Severity: MEDIUM**

## What to Look For

- A new SCS method or EdenAPI handler with no `.t` integration test
- Integration tests that only cover the happy path -- e.g., only testing "changed" files, not "added" or "removed"; or only one input shape when the API accepts multiple
- A new git protocol feature without a test comparing Mononoke's output against native git's output for the same operation
- SCS integration tests placed in `tests/integration/` instead of `tests/integration/facebook/scs/` (breaks OSS builds)

## Do NOT Flag

- Unit tests for internal helper functions (those don't need integration tests)
- Changes that only modify existing test infrastructure
- Refactoring diffs that are covered by existing integration tests (with a note like "existing tests pass")
- New features behind a JustKnob that is off by default (tests can come in a follow-up)

## Examples

**BAD (only happy path tested):**
```python
# test-diff-service.t
# Only tests diffing two existing files
  $ scsc diff --repo repo --old-commit A --new-commit B --path file.txt
  - old content
  + new content
# Missing: what happens when a file is added? removed? binary? empty?
```

**GOOD (covers add/remove/modify/binary):**
```python
# test-diff-service.t

# Test modified file
  $ scsc diff ... --path modified.txt
  - old
  + new

# Test added file
  $ scsc diff ... --path added.txt
  + new content

# Test removed file
  $ scsc diff ... --path removed.txt
  - old content

# Test binary file
  $ scsc diff ... --path image.png
  Binary files differ
```

**BAD (no reference comparison for git protocol feature):**
```python
# test-shallow-clone.t
  $ mononoke_git_clone --depth 1 repo clone_dir
  $ ls clone_dir
  file1.txt
  file2.txt
# We don't know if this is correct! No comparison against native git.
```

**GOOD (comparison against reference implementation):**
```python
# test-shallow-clone.t
# Clone with native git for reference
  $ cd "$GIT_REPO" && git clone --depth 1 file://"$GIT_REPO" git_clone_dir

# Clone with Mononoke
  $ mononoke_git_clone --depth 1 repo mononoke_clone_dir

# Compare the results
  $ diff -r git_clone_dir/.git/shallow mononoke_clone_dir/.git/shallow
```

**BAD (SCS test in wrong directory):**
```
# tests/integration/test-scs-redaction.t
# This test uses SCS but is in the main integration dir.
# It won't work in OSS mode.
```

**GOOD (SCS test in facebook/scs):**
```
# tests/integration/facebook/scs/test-scs-redaction.t
```

## Recommendation

Every new API endpoint or protocol feature should have integration tests that cover: (1) the success case for each supported input type, (2) at least one error case (invalid input, missing resource), and (3) edge cases relevant to the feature (empty inputs, large inputs, binary content). For git protocol features, compare Mononoke's output against native git to validate correctness -- don't just check that "something is returned." Place SCS-specific tests in `tests/integration/facebook/scs/` for OSS compatibility.

## Evidence

- D57561919: Review comment -- "I'd like to see a more 'real-life' example, like shallow-cloning a large repo and comparing the `.git` directories for both to show we're doing exactly what git would have done."
- D81597780: Review comment -- "We should try to test all cases. The case of an added or removed file, although simple, is still worth testing."
- D82918207: Review comment on needing more generic ways of specifying both sides of the diff, not just one hard-coded case.
- D88865547: Review comment -- "scs tests need to go in the facebook/scs directory as they don't work in OSS mode."
