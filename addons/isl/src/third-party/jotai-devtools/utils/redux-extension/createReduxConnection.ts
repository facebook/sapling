import { Message } from '../types';
import { ReduxExtension } from './getReduxExtension';

// Original but incomplete type of the redux extension package
type ConnectResponse = ReturnType<NonNullable<ReduxExtension>['connect']>;

export type Connection = {
  /** Mark the connection as not initiated, so it can be initiated before using it. */
  shouldInit?: boolean;

  /** Initiate the connection and add it to the extension connections.
   *  Should only be executed once in the live time of the connection.
   */
  init: ConnectResponse['init'];

  // FIXME https://github.com/reduxjs/redux-devtools/issues/1097
  /** Add a subscription to the connection.
   *  The provided listener will be executed when the user interacts with the extension
   *  with actions like time traveling, importing a state or the likes.
   *
   *  @param listener function to be executed when an action is submitted
   *  @returns function to unsubscribe the applied listener
   */
  subscribe: (listener: (message: Message) => void) => (() => void) | undefined;

  /** Send a new action to the connection to display the state change in the extension.
   *  For example when the value of the store changes.
   */
  send: ConnectResponse['send'];
};

/** Wrapper for creating connections to the redux extension
 *  Connections are used to display the stores value and value changes within the extension
 *  as well as reacting to extension actions like time traveling.
 **/
export const createReduxConnection = (
  extension: ReduxExtension | undefined,
  name: string,
) => {
  if (!extension) return undefined;
  const connection = extension.connect({ name });

  return Object.assign(connection, {
    shouldInit: true,
  }) as Connection;
};
