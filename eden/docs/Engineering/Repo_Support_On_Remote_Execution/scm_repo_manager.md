# Scm Repo Manager

Scm Repo Manager is a Thrift Service implementing Resource Manager APIs written in Python and Rust.

## Main Responsibilities of the SCM Repo Manager

* Setting up EdenFS dynamic config updater.
* Maintain the life cycle of EdenFS process including memory watchdog and health checks.
* Creating repo checkouts for a revision (global rev, snapshot) for use in an action's unit via bind-mounting.
* Removing the checkouts and provide robust handling for failed removals.
* Map errors: USER/INFRA


## Detailed breakdown per  Endpoint


| Endpoint Name of the RM protocol | Source Control operations **Foreground** | Source Control operations **Background** | Contribute to action duration? |
|----------------------------------|-----------------------------------------|------------------------------------------|-------------------------------|
| **getResourceConfig** | Retrieves a worker ID. | None | No |
| **setup** | Initializes Source Control support by starting the EdenFS dynamic config updater, the EdenFS daemon, and cloning the Sapling backing repo. | Prepares the mount for the first lease by triggering `eden clone`. | No |
| **beforeLease** | **Symlinks** the latest prepared mount, checks out the revision or snapshot, and initializes the [EdenFS Action Wrapper](https://www.internalfb.com/code/fbsource/fbcode/one_world/resource_managers/scm_manager/rust/edenfs_thrift_client/src/eden_fs_action_wrapper.rs) serving to collect perf counter per action and monitor EdenFS daemon's memory. | Prepares a new mount for the next lease by triggering another `eden clone`. | Yes  |
| **afterLease** | Removes the **Symlinks**, logs the perf counters, and EdenFS daemon's memory stats obtained via the [EdenFS Action Wrapper](https://www.internalfb.com/code/fbsource/fbcode/one_world/resource_managers/scm_manager/rust/edenfs_thrift_client/src/eden_fs_action_wrapper.rs). | Removes the mount by triggering an `eden rm` operation. | Yes |
| **cleanup** | Terminates background tasks, removes remaining mounts, stops the EdenFS daemon, cleans up the EdenFS state (`.eden`) and removes the Sapling backing repo. | None | No |
| **Unexpected shutdown/ Signal handler** | Terminates the EdenFS daemon (`SIGKILL`), cleans up the EdenFS state (`.eden`), and removes the Sapling backing repo. | None | No |


## Pipeline

![](px/6CDV6)

## Key Features and Configuration of EdenFS

* **Eden Light** Configuration: EdenFS is configured with an in-memory cache and rocksdb cache disabled, reducing memory usage and enabling multiple EdenFS daemons to run on the same Tupperware task. Sapling cache is only being used for storing files and trees metadata.
* **CASC** Enabled by Default: CASC is enabled by default, utilizing local CASd caches. EdenFS and Sapling rely on the Wdb CASd running on a physical host outside the Tupperware container, ensuring data persistence across container restarts.
* **Performance Counters**: Relevant EdenFS performance counters are collected on a per-action basis by snapshotting them before and after action execution.
* **Memory Watchdog**: A memory watchdog monitors peak RSS memory during action execution (for the lifetime of the mount in SCM Repo Manager).

## Health Checks and Pipeline

The next lease can be served in two ways: either by calling the `beforeLease` again or by going through a combination of `cleanup`, `setup`, and `beforeLease` sequence. 
This decision is influenced by multiple factors, including exceptions in `setup` and `beforeLease` and **explicit cleanup request** as part of the  [afterLease response](https://www.internalfb.com/code/fbsource/[f380e55a14e8ef68381312af3b5d696d05f209da]/fbcode/one_world/resource_managers/scm_manager/manager.py?lines=208).

There are several scenarios where the explicit cleanup may be requested:
* If the EdenFS daemon's health check via a thrift call fails
* If too many `eden rm` calls are fail
* If EdenFS exceeds the configured memory limit after an action has been completed (which is considered idle)



## Limitations

* Access to a repository implies full access to the working copy (including via EdenFS thrift intercace if required), but Sapling invocations are currently not supported.
* Future plans include supporting read-only Sapling commands. The platform's offerings are designed with read-only access to source code in mind.

## Additional Resources
* [Separate Dashboard](https://www.internalfb.com/intern/unidash/dashboard/scmunidash/scm_repo_manager_scm_on_re/)
* [Defined SLOs](https://www.internalfb.com/slick?service=scm%2Fscm_repo_manager&aggregation=DAY&heat_map_period=WEEK)

Check out our [resources](https://www.internalfb.com/wiki/Source_Control/Engineering/Repo_Support_On_Remote_Execution/resources) for helpful links.
