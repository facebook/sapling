/**
 * This software contains information and intellectual property that is
 * confidential and proprietary to Facebook, Inc. and its affiliates.
 *
 * @generated
 */

/*
 * This file is synced between fbcode/eden/fs/facebook/prototypes/node-edenfs-notifications-client/example.js.
 * The authoritative copy is the one in eden/fs/.
 * Use `yarn sync-edenfs-notifications` to perform the sync.
 *
 * This file is intended to be self contained so it may be copied/referenced from other extensions,
 * which is why it should not import anything and why it reimplements many types.
 */

/**
 * Example usage of the EdenFS Notify JavaScript interface
 */

const {EdenFSNotificationsClient, EdenFSUtils} = require('./index.js');

async function basicExample() {
  console.log('=== Basic EdenFS Notify Example ===');

  // Create a client instance
  const client = new EdenFSNotificationsClient({
    mountPoint: null,
    timeout: 1000, // 1 second timeout
    edenBinaryPath: process.env.EDEN_PATH ? process.env.EDEN_PATH : 'eden',
  });

  try {
    // Get current journal position
    console.log('Getting current journal position...');
    const position = await client.getPosition();
    console.log('Current position:', position);
  } catch (error) {
    console.error('Error:', error.message);
  }
}

async function waitReadyExample() {
  console.log('\n=== EdenFS Wait Ready Example ===');

  // Create a client instance with a short timeout for demonstration
  const client = new EdenFSNotificationsClient({
    mountPoint: null,
    timeout: 5000, // 5 second default timeout
    edenBinaryPath: process.env.EDEN_PATH ? process.env.EDEN_PATH : 'eden',
  });

  try {
    // Wait for EdenFS to be ready (useful after restart or initial setup)
    console.log('Waiting for EdenFS to be ready...');
    const isReady = await client.waitReady({
      timeout: 10000, // Wait up to 10 seconds
    });

    if (isReady) {
      console.log('EdenFS is ready!');
      // Now safe to perform operations
      const position = await client.getPosition();
      console.log('Current position:', position);
    } else {
      console.log('EdenFS did not become ready within timeout');
    }
  } catch (error) {
    console.error('Error:', error.message);
  }
}

async function changesExample(position) {
  console.log('=== EdenFS Notify changesSince Example ===');

  // Create a client instance
  const client = new EdenFSNotificationsClient({
    mountPoint: null,
    timeout: 1000, // 1 second timeout
    edenBinaryPath: process.env.EDEN_PATH ? process.env.EDEN_PATH : 'eden',
  });

  try {
    // Get changes since a specific position (if you have one)
    console.log('\nGetting recent changes...');
    const changes = await client.getChangesSince({
      position: position, // Start from current position
    });
    console.log('Changes:', JSON.stringify(changes, null, 2));

    // Extract file paths from changes
    if (changes.changes && changes.changes.length > 0) {
      const paths = EdenFSUtils.extractPaths(changes.changes);
      console.log('Changed files:', paths);
    }
  } catch (error) {
    console.error('Error:', error.message);
  }
}

async function subscriptionExample() {
  console.log('\n=== Subscription Example ===');

  const client = new EdenFSNotificationsClient({
    // mountPoint: '/path/to/your/eden/mount' // Replace with your actual mount point
    edenBinaryPath: process.env.EDEN_PATH ? process.env.EDEN_PATH : 'eden',
  });

  try {
    // Create a subscription for real-time changes
    const subscription = client.subscribe(
      {
        throttle: 100, // 100ms throttle between events
        includedSuffixes: ['.js', '.ts', '.py'], // Only watch specific file types
        excludedRoots: ['node_modules', '.git'], // Exclude certain directories
        deferredStates: ['test'], // Wait for these states to be deasserted
      },
      (error, resp) => {
        if (error) {
          console.error('Subscription error:', error.message);
          return;
        } else if (resp === null) {
          console.error('Subscription closed');
          return;
        } else {
          console.log('\n--- File System Change Detected ---');
          if (resp.to_position) {
            console.log('Position:', resp.to_position);
          } else if (resp.position) {
            console.log('Position:', resp.position);
          } else {
            console.error('Unknown response');
          }

          if (resp.changes && resp.changes.length > 0) {
            resp.changes.forEach(change => {
              const changeType = EdenFSUtils.getChangeType(change);
              if (change.SmallChange) {
                const paths = EdenFSUtils.extractPaths([change]);
                console.log(`${changeType.toUpperCase()}: ${paths.join(', ')}`);
              } else if (change.LargeChange) {
                if (changeType == 'directory renamed') {
                  console.log(
                    `${changeType.toUpperCase()}: From ${EdenFSUtils.bytesToPath(change.LargeChange.DirectoryRenamed.from)} to ${EdenFSUtils.bytesToPath(change.LargeChange.DirectoryRenamed.to)}`,
                  );
                } else if (changeType == 'commit transition') {
                  console.log(
                    `${changeType.toUpperCase()}: From ${EdenFSUtils.bytesToHex(change.LargeChange.CommitTransition.from)} to ${EdenFSUtils.bytesToHex(change.LargeChange.CommitTransition.to)}`,
                  );
                } else if (changeType == 'lost changes') {
                  console.log(
                    `${changeType.toUpperCase()}: ${change.LargeChange.LostChanges.reason}`,
                  );
                } else {
                  console.log(`Unknown large change: ${JSON.stringify(change)}`);
                }
              }
            });
          } else if (resp.state) {
            console.log(`State change: ${resp.event_type} ${resp.state}`);
          } else {
            console.error(`Unknown response: ${JSON.stringify(resp)}`);
          }
        }
      },
    );

    // Start the subscription
    console.log('Starting subscription...');
    await subscription.start();
    console.log('Subscription active. Make some file changes to see events.');
    console.log('Press Ctrl+C to stop.');

    // Keep the process running
    process.on('SIGINT', async () => {
      console.log('\nStopping subscription...');
      // Wait until the subscription has fully exited before terminating
      subscription.on('exit', () => {
        console.log('Subscription exited');
        process.exit(0);
      });
      subscription.stop();
    });

    // Prevent the script from exiting
    await new Promise(() => {});
  } catch (error) {
    console.error('Subscription error:', error.message);
  }
}

async function stateExample() {
  console.log('\n=== State Management Example ===');

  const client = new EdenFSNotificationsClient({
    // mountPoint: '/path/to/your/eden/mount' // Replace with your actual mount point
    edenBinaryPath: process.env.EDEN_PATH ? process.env.EDEN_PATH : 'eden',
  });

  try {
    // Enter a state for 10 seconds
    console.log('Entering "build" state for 10 seconds...');
    await client.enterState('build', {duration: 10});
    console.log('State entered successfully');
  } catch (error) {
    console.error('State error:', error.message);
  }
}

async function advancedSubscriptionExample() {
  console.log('\n=== Advanced Subscription with States ===');

  const client = new EdenFSNotificationsClient({
    // mountPoint: '/path/to/your/eden/mount' // Replace with your actual mount point
    edenBinaryPath: process.env.EDEN_PATH ? process.env.EDEN_PATH : 'eden',
  });

  try {
    // Create a subscription that waits for certain states to be deasserted
    const subscription = client.subscribe(
      {
        deferredStates: ['build', 'test'], // Wait for these states to be deasserted
        throttle: 50,
        includedSuffixes: ['.js', '.ts', '.json'],
      },
      (error, resp) => {
        if (error) {
          console.error('Subscription error:', error.message);
          return;
        }

        console.log('\n--- File System Change Detected ---');
        if (resp.to_position) {
          console.log('Position:', resp.to_position);
        } else if (resp.position) {
          console.log('Position:', resp.position);
        } else {
          console.error('Unknown response: ', resp);
        }

        if (resp.changes) {
          if (resp.changes.length === 0) {
            console.log('no changes');
          }
          resp.changes.forEach(change => {
            const changeType = EdenFSUtils.getChangeType(change);
            if (change.SmallChange) {
              const paths = EdenFSUtils.extractPaths([change]);
              console.log(`${changeType.toUpperCase()}: ${paths.join(', ')}`);
            } else if (change.LargeChange) {
              if (changeType == 'directory renamed') {
                console.log(
                  `${changeType.toUpperCase()}: From ${EdenFSUtils.bytesToPath(change.LargeChange.DirectoryRenamed.from)} to ${EdenFSUtils.bytesToPath(change.LargeChange.DirectoryRenamed.to)}`,
                );
              } else if (changeType == 'commit transition') {
                console.log(
                  `${changeType.toUpperCase()}: From ${EdenFSUtils.bytesToHex(change.LargeChange.CommitTransition.from)} to ${EdenFSUtils.bytesToHex(change.LargeChange.CommitTransition.to)}`,
                );
              } else if (changeType == 'lost changes') {
                console.log(
                  `${changeType.toUpperCase()}: ${change.LargeChange.LostChanges.reason}`,
                );
              } else {
                console.log(`Unknown large change: ${JSON.stringify(change)}`);
              }
            }
          });
        } else if (resp.state) {
          console.log(`State change: ${resp.event_type} ${resp.state}`);
        } else {
          console.error(`Unknown response: ${JSON.stringify(resp)}`);
        }
      },
    );

    await subscription.start();
    console.log('Advanced subscription started. Try entering/exiting states.');

    // Simulate entering and exiting states
    setTimeout(async () => {
      console.log('\nEntering build state...');
      await client.enterState('build', {duration: 5});
    }, 2000);

    setTimeout(async () => {
      console.log('\nEntering test state...');
      await client.enterState('test', {duration: 3});
    }, 8000);

    // Keep running for demo
    setTimeout(() => {
      subscription.stop();
      console.log('\nDemo completed');
      process.exit(0);
    }, 15000);
  } catch (error) {
    console.error('Advanced subscription error:', error.message);
  }
}

async function utilityExample() {
  console.log('\n=== Utility Functions Example ===');

  // Example change data (as returned by EdenFS)
  const exampleChanges = [
    {
      SmallChange: {
        Added: {
          file_type: 'Regular',
          path: [104, 101, 108, 108, 111, 46, 116, 120, 116], // "hello.txt" in bytes
        },
      },
    },
    {
      SmallChange: {
        Modified: {
          file_type: 'Regular',
          path: [119, 111, 114, 108, 100, 46, 106, 115], // "world.js" in bytes
        },
      },
    },
    {
      SmallChange: {
        Renamed: {
          file_type: 'Regular',
          from: [111, 108, 100, 46, 116, 120, 116], // "old.txt" in bytes
          to: [110, 101, 119, 46, 116, 120, 116], // "new.txt" in bytes
        },
      },
    },
    {
      LargeChange: {
        CommitTransition: {
          from: [111, 108, 100, 46, 116, 120, 116], // "old.txt" in bytes
          to: [110, 101, 119, 46, 116, 120, 116], // "new.txt" in bytes
        },
      },
    },
    {
      StateChange: {
        StateEntered: {
          name: 'meerkat',
        },
      },
    },
  ];

  // Extract paths from changes
  console.log('Extracting single path');
  const [path1, path2] = EdenFSUtils.extractPath(exampleChanges[0].SmallChange);
  console.log('Extracted paths:', path1, path2);
  console.log('Extracting multiple paths:');
  const paths = EdenFSUtils.extractPaths(exampleChanges);
  console.log('Extracted paths:', paths);
  console.log('Extracting types:');
  const type = EdenFSUtils.extractFileType(exampleChanges[0].SmallChange);
  console.log('Extracted file type:', type);

  // Get change types
  exampleChanges.forEach((change, index) => {
    const changeType = EdenFSUtils.getChangeType(change);
    console.log(`Change ${index + 1} type:`, changeType);
  });
}

// Run examples
async function runExamples() {
  console.log('EdenFS Notify JavaScript Interface Examples');
  console.log('==========================================');

  // Note: Update the mount point in each example before running
  console.log('\nNOTE: Please update the mount point paths in the examples before running!');

  try {
    await basicExample();
    await waitReadyExample();
    if (process.argv.length > 2) {
      await changesExample(process.argv[2]);
    }
    await utilityExample();

    // Uncomment these to run interactive examples:
    // await subscriptionExample();
    // await stateExample();
    // await advancedSubscriptionExample();
  } catch (error) {
    console.error('Example error:', error.message);
  }
}

// Run if this file is executed directly
if (require.main === module) {
  runExamples();
}
