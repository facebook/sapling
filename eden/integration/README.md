> Note: `sl` is the new command name to replace `hg`

# Integration Test

Eden integration tests are python tests located under `fbsource/fbcode/eden/integration/`.

## Eden Sapling Integration Test

Tests for sl with Eden are located under fbsource/fbcode/eden/integration/hg.

e.g. `status_test.py` is testing sl status works correctly with Eden.

### Write a New Integration Test

If you are starting a whole new group of new tests for testing a Sapling command, then you want to implement a new test class based on top of EdenHgTestCase.

Otherwise, just identify the right testing file and testing class that matches your testing purpose and simply add a new test case for that.

```python
@hg_cached_status_test
class StatusTest(EdenHgTestCase):
```

The decorator @hg_cached_status_test is used so the same test can be replicated into variant setup.

**Initial Repo Setup**
You define the initial directory/file structure for your test by implementing this function

```python
    def populate_backing_repo(self, repo: hgrepo.HgRepository) -> None:
        raise NotImplementedError(
            "individual test classes must implement " "populate_backing_repo()"
        )
```

**Trigger Sapling Command**

You call `sl` command from the test by using this helper function

```python
    def hg(
        self,
        *args: str,
        encoding: str = "utf-8",
        input: Optional[str] = None,
        hgeditor: Optional[str] = None,
        cwd: Optional[str] = None,
        check: bool = True,
    ) -> str:
```

**Run an Integration Test**

See `.claude/CLAUDE.md` → "Verification" for the `buck2 test` command.

### Debug an Integration Test

**DBG Level**

To tune DBG level for integration tests, just override this method for your testing class

```python
    def edenfs_logging_settings(self) -> Dict[str, str]:
        return {
            "eden.fs.inodes.TreeInode": "DBG5",
            "eden.fs.inodes.CheckoutAction": "DBG5",
            "eden.fs.inodes.CheckoutContext": "DBG5",
        }
```

### Test Framework Repo Objects

A **backing repo** is the on-disk Sapling/Hg repository that EdenFS reads data from (see `eden/fs/docs/Glossary.md` → "Backing Repository"). EdenFS only reads the object store — it ignores the backing repo's working copy.

The two test base classes use **different naming conventions** for the same concepts:

| Concept | `EdenRepoTest` | `EdenHgTestCase` | Use for |
|---------|---------------|------------------|---------|
| **Backing repo** | `self.repo` | `self.backing_repo` | Adding commits the mount will serve |
| **EdenFS mount** | `self.eden_repo` | `self.repo` | `hg update`, `hg status`, commands through EdenFS |
| **Mount path** (`str`) | `self.mount` | `self.mount` | `os.listdir`, `open()`, `os.stat` |

`EdenRepoTest`: override `populate_repo()`. `EdenHgTestCase`: override `populate_backing_repo()`.

> ⚠️ **`self.repo` means different things in each base class.** In `EdenRepoTest` it's the backing repo; in `EdenHgTestCase` it's the EdenFS mount.

### Common Pitfalls

- **Updating in the wrong repo:** `self.repo.hg("update", commit)` in an `EdenRepoTest` subclass updates the backing repo's working copy, which EdenFS ignores — use `self.eden_repo.hg("update", commit)` instead.
- **Bookmark behavior:** `eden clone` checks out the backing repo's active bookmark, not the working-copy parent. If `populate_repo` sets a bookmark, the mount starts there. Explicitly call `self.eden_repo.hg("update", target_commit)` to be safe.
