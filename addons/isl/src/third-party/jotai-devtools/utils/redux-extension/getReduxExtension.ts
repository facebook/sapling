// Original but incomplete type of the redux extension package
type Extension = NonNullable<typeof window.__REDUX_DEVTOOLS_EXTENSION__>;

export type ReduxExtension = {
  /** Create a connection to the extension.
   *  This will connect a store (like an atom) to the extension and
   *  display it within the extension tab.
   *
   *  @param options https://github.com/reduxjs/redux-devtools/blob/main/extension/docs/API/Arguments.md
   *  @returns https://github.com/reduxjs/redux-devtools/blob/main/extension/docs/API/Methods.md#connectoptions
   */
  connect: Extension['connect'];

  /** Disconnects all existing connections to the redux extension.
   *  Only use this when you are sure that no other connection exists
   *  or you want to remove all existing connections.
   */
  disconnect?: () => void;

  /** Have a look at the documentation for more methods:
   *  https://github.com/reduxjs/redux-devtools/blob/main/extension/docs/API/Methods.md
   */
};

/** Returns the global redux extension object if available */
export const getReduxExtension = (
  enabled = __DEV__,
): ReduxExtension | undefined => {
  if (!enabled) {
    return undefined;
  }

  const reduxExtension = window.__REDUX_DEVTOOLS_EXTENSION__;
  if (!reduxExtension && __DEV__) {
    console.warn('Please install/enable Redux devtools extension');
    return undefined;
  }

  return reduxExtension;
};
