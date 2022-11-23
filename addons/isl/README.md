# Interactive Smartlog

Interactive Smartlog (ISL) is an embeddable, web-based GUI for Sapling.
[See user documentation here](https://sapling-scm.com/docs/addons/isl).

The code for ISL lives in the addons folder:

| folder          | use                                                        |
| --------------- | ---------------------------------------------------------- |
| isl             | Front end UI written with React and Recoil                 |
| isl-sever       | Back end, which runs sl commands / interacts with the repo |
| isl-sever/proxy | `sl web` CLI and server management                         |
| shared          | Utils shared by reviewstack and isl                        |
| vscode          | VS Code extension for Sapling, including ISL as a webview  |

## Development

As always, first run `yarn` to make sure all of the Node dependencies are installed.
Then launch the following three components in order:

### Client

**In the isl folder, run `yarn start`**.
This will make a development build with [Create React App](https://create-react-app.dev/).
This watches for changes to the front end and incrementally re-compiles.
Unlike most CRA apps, this will not yet open the browser,
because we need to open it using a token from when we start the server.

### Server

**In the `isl-server/` folder, run `yarn watch` and leave it running.**
This watches for changes to the server side back end and incrementally re-compiles.
This ensures the server code is bundled into a js file that runs a proxy
(in `isl-server/dist/run-proxy.js`) to handle requests.

### Proxy

We launch a WebSocket Server to proxy requests between the server and the
client. The entry point code lives in the `isl-server/proxy/` folder and is a
simple HTTP server that processes `upgrade` requests and forwards
them to the WebSocket Server that expects connections at `/ws`.

**In the `isl-server/` folder, run `yarn serve --dev` to start the proxy and open the browser**.
You will have to manually restart it in order to pick up server changes.
This is the development mode equivalent of running `sl web`.

Note: When the server is started, it creates a token to prevent unwanted access.
`--dev` opens the browser on the port used by CRA in `yarn start`
to ensure the client connects with the right token.

See `../vscode/CONTRIBUTING.md` for build instructions for the vscode extension.

## Production builds

`isl/release.js` is a script to build production bundles and
package them into a single self-contained directory that can be distributed.

# Goals

ISL is designed to be an opinionated UI. It does not implement every single feature or argument that the CLI supports.
Rather, it implements an intuitive UI by leverage a subset of features of the `sl` CLI.

ISL aims to optimize common workflows and provide an intuitive UX around some advanced workflows.

- **Opinionated**: ISL is opinionated about the "right" way to work.
  This includes using stacks, amending commits, using one-commit-per-PR, rebasing to merge.
- **Simple**: ISL hides unnecessary details and aims to be beginner-friendly.
  Each new button added to the UI makes it more intimidating to new users.
- **User concepts, not machine concepts**:
  ISL hides implementation details to present source control in a way a human would understand it.
  The salient example of this is not showing commit hashes in the UI by default.
  Hashes are needed to refer to commits when typing in a CLI, but
  ISL prefers being able to just click directly on commits, thus we don't need to show the hash by default.
  Other examples of this include drag & drop to rebase, and showing PR info directly under a commit by leaning on one-PR-per-commit.
- **Previews & Smoothness**: The UI should let you preview what action you'll take. It shows an optimistic
  version of the result of each command so the UI feels instant. We aim to avoid the UI _jumping_ between
  states as a result of async data fetches
- **Docuemntation & Transparency**: The UI uses tooltips and other signals to show you what every button will do.
  It always confirms before running dangerous commands. It shows exactly what CLI command is being run, so you
  could do it yourself and trust what it's doing.

# Internals

The following sections describe how ISL is implemented.

## Build / Bundling

- All parts of ISL (client, server, vscode extension) are built with webpack, which produces javascript/css bundles.
  This includes node_modules inside the bundle, which means we don't need to worry about including node_modules in builds.
- `sl web` is a normal `sl` python command, which invokes the latest ISL built CLI.
  `isl-server/proxy/run-proxy.ts` is the typescript entry point which is spawned by Python via `node`.
  In development mode, you interact directly with `run-proxy` rather than dealing with `sl web`.
  Note: there are slightly differences between the python `sl web` CLI args and the `run-proxy` CLI args.
  In general, `run-proxy` exposes more options, most of which aren't needed by normal `sl web` users.

## Architecture

ISL uses an embeddable Client / Server architecture.

- The Client runs in a browser-like context (web browser, VS Code webview, Electron renderer)
- The Server runs in a node-like context (node server from `sl web`, VS Code extension host, Electron main)

The server serves the client's static (html/js/css) files via HTTP.
The client JavaScript then connects back to the server via WebSocket,
where both sides can send and receive messages to communicate.

### Client

The client renders the UI and asks the server to actually do stuff. The client has no direct access
to the filesystem or repository. The client can make normal web requests, but does not have access tokens
to make authenticated requests to GitHub.

The client uses React (for rendering the UI) and Recoil (for state management).

### Server

The server is able to interact with the file system, spawn processes, run `sl commands`,
and make authenticated network requests to GitHub.
The server is also responsible for watching the repository for changes.
This will optionally use Watchman if it's installed.
If not, the server falls back to a polling mechanism, which polls on a variable frequency
which depends on if the UI is focused and visible.

The server shells out to the `gh` CLI to make authenticated requests to GitHub.

Most of the server's work is done by the `Repository` object, which represents a single Sapling repository.
This object also delegates to manage Watchman subscriptions and GitHub fetching.

### Server reuse and sharing

To support running `sl web` in multiple repos / cwds at the same time, ISL supports reusing server instances.
When spawning an ISL server, if the port is already in use by an ISL server, that server will be reused.

Since the server acts like a normal http web server, it supports multiple clients connecting at the same time,
both the static resources and WebSocket connections.

`Repository` instances inside the server are cached per repo root.
`RepositoryCache` manages Repositories by reference counting.
A `Repository` does not have its own cwd set. Rather, each reference to a `Repository`
via `RepositoryCache` has an associated cwd. This way, A single `Repository` instance is reused
even if accessed from multiple cwds within the same repo.
We treat each WebSocket connection as its own cwd, and each WebSocket connections has one reference
to a shared Repository via RepositoryCache.

Connecting multiple clients to the same sever at the same cwd is also supported.
Server-side fetched data is sent to all relevant (same repo) clients, not just the one that made a request.
Note that client-side cached data is not shared, which means optimistic state may not work as well
in a second window for operations triggered in a different window.

After all clients are disconnected, the server auto-shutdowns after one minute with no remaining repositories
which helps ensure that old ISL servers aren't reused.

Note that ISL exposes `--kill` and `--force` options to kill old servers and force a fresh server, to make
it easy to work around unexpectedly reusing old ISL servers.

### Security

The client sends messages to the server to run `sl` commands.
We must authenticate clients to ensure arbitrary websites or XSS attacks can't connect on localhost:3011 to run commands.
The approach we take is to generate a cryptographic token when a server is started.
Connecting via WebsOcket to the server requires this token.
The token is included in the url generated by `sl web`, which allows URLs from `sl web` to connect successfully.

Because of this token, restarting the ISL server requires clicking a fresh link to use the new token.
Once an ISL server stops running, its token is no longer valid.

In order to support reusing ISL servers, we must persist the server's token to disk,
so that later `sl web` invocations can find the right token to use.
This persisted data includes the token but also some other metadata about the server,
which is written to a permission-restricted file.

Detail: we have a second token we use to verify that a server running on a port
is actually an ISL server, to prevent misleading/phising "reuses" of a server.

## Embedding

ISL is designed to be embedded in multiple contexts. `sl web` is the default,
which is also the most complicated due to server reuse and managing tokens.

The Sapling VS Code extension's ISL webview is another example of an embedding.
Other embeddings are possible, such as an Electron / Tauri standalone app, or
other IDE extensions such as Android Studio.

### Platform

To support running in multiple contexts, ISL has the notion of a Platform,
on both the client and server, which contains embedding-specific implementations
of a common API.

This includes things like opening a file. In the browser, the best we can do is use the OS default.
Inside the VS Code extension, we always want to open with VS Code.
Each platform can implement this to match their UX best.
The Client's platform is where platform-specific code first runs. Some embeddings
have their client platform send platform-specific messages to the server platform.

The "default" platform is the BrowserPlatform, used by `sl web`.

Custom platforms can be implemented either by:

- including platform code in the build process (the VS Code extension does this)
- adding a new platform to isl-server for use by `run-proxy`'s `--platform` option (android studio does this)

## Syncing repository state

ISL started as a way to automatically re-run `sl status` and `sl smartlog` in a loop.
The UI should always feel up-to-date, even though it needs to run these commands
to actually fetch the data.
The client subscribes to this data, which the server is in charge of fetching automatically.
The server uses Watchman (if installed) to detect when:

- the `.sl/dirstate` has changed to indicate the list of commits has changed, so we should re-run `sl log`.
- any normal file in the repository has changed, so we should re-run `sl status` to look for uncommitted changes.
  If Watchman is not installed, `sl log` and `sl status` are polled on an interval by `WatchForChanges`.

Similarly, the server fetches new data from GitHub when the list of PRs changes, and refreshes by polling.

## Running Operations

ISL defines an "Operation" as any mutating `sl` command, such as `sl pull`, `sl rebase`, `sl goto`, `sl amend`, `sl add`, etc. Non-examples include `sl status`, `sl log`, `sl cat`, `sl diff`.

The lifecycle of an operation looks like this:

```
Ready to run -> Preview -> Queued -> Running -> Optimistic state -> Completed
```

### Preview Appliers

Critically, fetching data via `sl log` and `sl status` is separate from running operations.
We only get the "new" state of the world after _both_ the operation has completed _AND_
`sl log` / `sl status` has run to provide us with the latest data.

This would cause the UI to appear laggy and out of date.
Thus, we support using previews and optimistic to update the UI immediately.

To support this, ISL defines a "`preview applier`" function for every operation.
The preview applier function describes how the tree of commits and uncommitted changes
would change as a result of running this operation.
(Detail: there's actually a separate preview applier function for uncommitted changes and the commit tree
to ensure UI smoothness if `sl log` and `sl status` return data at different times)

This supports both:

- **previews**: What would the tree look like if I ran this command?
  - e.g. Drag & drop rebase preview before clicking "run rebase"
- **optimistic state**: How should we pretend the tree looks while this command is running?
  - e.g. showing result of a rebase while rebase command is running

Because `sl log` and `sl status` are run separately from an operation running,
the optimistic state preview applier must be used not just while the operation is running,
but also _after_ it finishes up until we get new data from `sl log` / `sl status`.

### Queued commands

Preview Appliers are functions which take a commit tree and return a new commit tree.
This allows us to stack the result of preview appliers on top of each other.
This trivially enables _Queued Commands_, which work like `&&` on the CLI.

If an operation is ongoing, and we click a button to run another,
it is queued up by the server to run next.
The client then renders the tree resulting from first running Operation 1's preview applier,
then running Operation 2's preview applier.

Important detail here: if an operation references a commit hash, the queued version
of that operation will not yet know the new hash after the previous operation finishes.
For example, `sl amend` in the middle of a stack, then `sl goto` the top of the stack.
Thus, when telling the server to run an Operation we tag which args are revsets,
so they are replaced with `max(sucessors(${revset}))` so the hash is replaced
with the latest successor hash.

## Internationalization

ISL has a built-in i18n system, however the only language currently implemented is `en-US` English.
`t()` and `<T>` functions convert English strings or keys into values for other languages in the `isl/i18n/${languageCode}` folders. To add support for a new langauage, add a new `isl/i18n/${languageCode}/common.js`
and provide translations for all the strings found by grepping for `t()` and `<T>` in `isl`.
This system can be improved later as new languages are supported.
