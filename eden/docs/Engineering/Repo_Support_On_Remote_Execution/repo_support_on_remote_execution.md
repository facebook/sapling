# Repo Support on Remote Execution

[Remote Execution](https://www.internalfb.com/wiki/Remote_Execution/) is the compute platform in Meta designed for rapid and parallel execution of small, stateless actions as outlined in the [DevInfra Compute Offerings Guideline](https://www.internalfb.com/wiki/DevInfra_Compute_offerings/What_execution_platform_to_use/).
The feature of Repo Support on Remote Execution facilitates the distributed and efficient execution of small granular actions on the Source Code, initially only enabled for the `fbsource` repo.
This optimization not only improves performance but also maximizes concurrency and minimizes setup overhead, thereby enhancing overall productivity and efficiency in handling small computes on a repositiry.

**Repo Support on RE offers a key advantage:** customers can include a revision in an action's inputs, granting access to the repository at that exact revision during the action.
The repository is accessible by default at `/fbsource` within the action, with the option to configure a different location if required. Global Revs and [Snapshots](https://www.internalfb.com/wiki/Source_Control/Admin/Snapshots/) are also supported.
Snapshots provide a means to save and share uncommitted changes without altering the working copy, and often used by automation.

The remote execution **action cache** is optionally available, providing caching for action runs that occur on the same revision.

**Capacity:** Although customers are responsible for funding capacity, the platform has demonstrated exceptional efficiency due to its ability to handle high concurrency. This approach is consistent with remote execution practices, where customers typically bear the costs of capacity while benefiting from optimized resource utilization.
For example, in the case of Tupperware specs compilation, our setup utilizes a worker-to-machine ratio of **25 workers per T10 machine**, showcasing an efficient allocation of resources for this specific use case.

**Repo Support on RE** is implemented using the [SCM Repo Manager Thrift Service](https://www.internalfb.com/wiki/Source_Control/Engineering/Repo_Support_On_Remote_Execution/scm_repo_manager) as an implementation of the Resource Manager APIs, allowing running RE actions alongside the resource.

## What is the Resource Manager?

The Resource Manager is responsible for managing resources required by RE actions alongside the workers. Resource in our case is a repository's working copy (a mount) at a requested revision, needed by an action to access the codebase files.
The Resource Manager ensures that these resources are properly set up, made available to the action, and cleaned up after execution. The RE agent is responsible for orchestrating the Resource Managers.
The agent acts as a coordinator that sets up and communicates with an individual Resource Manager for every worker.

## Execution Flow

![](px/6CJnR)


## SCM Repo Manager Thrift Service
The SCM Repo Manager Thrift Service is used as an implementation of the Resource Manager APIs. This service provides access to the repository's EdenFs mounts.

* There is one to one mapping between SCM Repo Manager Thrift Service and EdenFs process
* Resource Manager protocol consists of 4 main endpoints: `setup`, `beforeLease`, `afterLease`, `cleanup`
* Actions are sequential per Resource Manager, therefore sequential per EdenFs process
* The `eden clone` command creates a mount, whereas the `eden rm` command removes it. On the other hand, the EdenFs daemon is usually reused across multiple actions unless an exception or health check error occurs.
In such cases, the pipeline would cycle through `cleanup` and then `setup`.

![](px/6CDjx)

*More details provided in the [SCM Repo Manager](https://www.internalfb.com/wiki/Source_Control/Engineering/Repo_Support_On_Remote_Execution/scm_repo_manager) design overview.*

Check out our [resources](https://www.internalfb.com/wiki/Source_Control/Engineering/Repo_Support_On_Remote_Execution/resources) for helpful links.
