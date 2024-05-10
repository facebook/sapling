# Integration tests

ISL integration tests create a real `sl` repo and run actual `sl` commands, to validate the end-to-end workflow.

That said, integration tests differ from "real" use of ISL in a few ways:

- Integration tests run in a single process, using a fake MessageBus. This means the client and server are in the same process.
- Integration tests are not built using vite, but just run directly by jest. This may have different import ordering properties than production builds.
- Integration tests have some mocks, such as tracking (to avoid actually writing tracking data)
- Integration test repos are obviously synthetic and may behave different from "normal" clones.

Integration tests run one at a time, to avoid overlapping causing issues.

To run Integration tests:

```sh
yarn integration
```

You can use other jest CLI args like normal:

```sh
yarn integration testName
```

## Writing integration tests

See existing tests for examples of how to write an integration test.
Generally:

- Write tests in the `integrationTests/` directory, with `.test.tsx` names.
- Use react testing library to make assertions about the UI like normal
- Currently expect one test per file, though multiple may be possible with careful repo setup and management
- Call `await initRepo()` at the start of the test. It sets up the real repo and provides various utils
- Call `await act(cleanup)` at the end of the test
- **Important:** Do not import files at the top level that use ISL dependencies. Instead, use the `await import()` syntax. This is important since `initRepo()` sets up mocks that MUST happen before ANY other `isl/src` imports
