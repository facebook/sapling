# Interactive Smartlog

Prototype of a clean-room implementation of Interactive SmartLog (ISL) designed
to support open source EdenSCM.

## Development

As always, first run `yarn` to make sure all of the Node dependencies are installed.
Then launch the following three components in order:

### Client

**In the isl folder, run `yarn start`**.
This will make a development build with [Create React App](https://create-react-app.dev/).
Unlike most CRA apps, this will not yet open the browser,
because we need to open it using a token from when we start the server.

### Server

**In the `isl-server/` folder, run `yarn watch` and leave it running.**
The `isl-server/` folder is where our server code goes.
This ensures the server code is bundled into a js file that runs a proxy
(in `isl-server/dist/run-proxy.js`) to handle requests.

### Proxy

We launch a WebSocket Server to proxy requests between the server and the
client. The entry point code lives in the `proxy/` folder and is a
simple HTTP server that processes `upgrade` requests and forwards
them to the WebSocket Server that expects connections at `/ws`.

**In the `isl-server/` folder, run `yarn serve --dev` to start the proxy and open the browser**.
You will have to manually restart it in order to pick up server changes.

Note: When the server is started, it creates a token to prevent unwanted access.
`--dev` opens the browser on the port used by CRA in `yarn start`
to ensure the client connects with the right token.

## Production builds

`isl/release.js` is a script to build production bundles and
package them into a single self-contained directory that can be distributed.
