# Standalone ISL

This is a standalone [Tauri](https://tauri.app/) application that shows the Interactive Smartlog (ISL) UI for Sapling.
This is similar to the browser-based `sl web`, but as an OS application.

## Development

First run `yarn` to install dependencies.
Compile the ISL client & server code and leave these watching processes running:

- `cd ../isl && yarn start`
- `cd ../isl-server && yarn watch`

Start the tauri app (from `isl-standalone/`):

- `./dev.sh`

Note: this can't be a `yarn` sub-command because tauri isn't happy about our configured node binary.
Manually invoking this script works around this.

The tauri dev script will recompile the rust code whenever it detects changes.

## Building for production

`yarn --no-default-rc tauri build` or `./node_modules/.bin/tauri build` will create a production build of the app,
which will end up in ./src-tauri/target/release.

## How the Tauri ISL platform works

When you start the Tauri app, it runs `sl web --json --platform standalone`, to start an ISL server with the given platform.
It will then launch and connect to the same server URL given by the `sl web` output.

The Tauri application bundle itself does not contain the ISL client or server code, since it just uses
`sl web`.

The `standalone` platform defines how the client & server JS use Standalone-specific implementations of features.
This is largely the same as the browser platform implementation.
