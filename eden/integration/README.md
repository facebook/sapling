> Note: `sl` is the new command name to replace `hg`

# Integration Test

Eden integration tests are python tests located under `fbsource/fbcode/eden/integration/`.

## Eden Sapling Integration Test

Tests for sl with Eden are located under fbsource/fbcode/eden/integration/hg.

e.g. `status_test.py` is testing sl status works correctly with Eden.

### Write a New Integration Test

If you are starting a whole new group of new tests for testing a new sapling command, e.g. sl awesome, then you want to implement a new test class based on top of EdenHgTestCase.

Otherwise, just identify the right testing file and testing class that matches your testing purpose and simply add a new test case for that.

```python
@hg_cached_status_test
class StatusTest(EdenHgTestCase):
```

The decorator @hg_cached_status_test is used so the same test can be replicated into variant setup.

**Initial Repo Setup**
You define the initial director/file structure for your test by implementing this function

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

We use buck test to trigger the integration tests.
To run all tests from a file for all variants

```bash
buck test '@fbcode//mode/opt' fbcode//eden/integration/hg:status --
```

**To run a specific test case for all variants** (use -r to match a regex)

```bash
buck test '@fbcode//mode/opt' fbcode//eden/integration/hg:status -- -r '.*test_status_thrift_apis.*'
```

**To run a specific test case for a specific variant**

```bash
buck2 test '@fbcode//mode/opt' fbcode//eden/integration:clone -- --exact 'eden/integration:clone - test_clone_should_start_daemon (eden.integration.clone_test.CloneTestFilteredHg)'
```

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
