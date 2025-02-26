# Repo Support on Remote Execution

[Remote Execution](https://www.internalfb.com/wiki/Remote_Execution/) is the compute platform in Meta designed for rapid and parallel execution of small, stateless actions as outlined in the [DevInfra Compute Offerings Guideline](https://www.internalfb.com/wiki/DevInfra_Compute_offerings/What_execution_platform_to_use/).
The feature of Repo Support on Remote Execution facilitates the distributed and efficient execution of small granular actions on the Source Code, initially only enabled for the `fbsource` repo.
This offering not only improves performance but also maximizes concurrency and minimizes setup overhead, thereby enhancing overall productivity and efficiency in handling small computes on a repository.

**Repo Support on RE offers a key advantage:** customers can include a revision in an action's inputs, granting access to the repository at that exact revision during the action.
The repository is accessible by default at `/fbsource` within the action, with the option to configure a different location if required. Global Revs and [Snapshots](https://www.internalfb.com/wiki/Source_Control/Admin/Snapshots/) are also supported.
Snapshots provide a means to save and share uncommitted changes without altering the working copy, and often used by automation.

The remote execution **action cache** is optionally available, providing caching for action runs that occur on the same revision.

**Efficiency:**
The `scm-repo-support` platform boasts high efficiency, thanks to several key features:
* Dynamic container sizing based on historical data allows for high concurrency
* Source Control Preps are excluded from action duration, except for "checkout revision"
* RE scheduling employs a push model, resulting in good affinities and a high local cache hit rate for EdenFS
* The RE is built using Rust/C++ (for CAS), minimizing overhead

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

# Onboarding

If you're interested in using our platform, please make a post in the [Source Control & Remote Execution XFN Group](https://fb.workplace.com/groups/538958065679523) to get started, providing detailed information about your use case, and we will normally respond within 1 business day.

**There are important considerations to take into account:**

* The platform focuses on compute capabilities and does not support use cases requiring just retrieving of Source Control data.
* Please, read carefully: [What execution platform to use?](https://www.internalfb.com/wiki/DevInfra_Compute_offerings/What_execution_platform_to_use/)
* Please, consider leveraging [SCMQuery](https://www.internalfb.com/wiki/ScmqueryGuide/) or the Source Control Service `shardmanager:mononoke.scs`, a thrift interface that provides answers to queries about Source Control, for Source Control access. While this can be effective for small-scale file access, refrain from using it to retrieve extensive file content data (measured in GB), as these services are not optimized for high-volume data transfer.
* Limitations:
    * Shelling out buck commands is not supported due to its high memory requirements and incompatibility with RE's execution model. Please, consider using Sandcastle.
    * Hack scripts are not currently supported as actions, but this feature may be added in the future.

# Demo

This demonstration showcases the execution of the `ls` command within a repository using frecli at the current revision of your repo checkout.
The SCM overhead is comprised of the `Before action` and `After action`.

```
 liuba ⛅️  ~/fbsource
 [🍊] → frecli --platform scm-repo-support -r "$(hg whereami)" exec command -- ls /fbsource
-------------------------------------Action-------------------------------------
RE Session ID        : reSessionID-b73a7fe8-4dfb-4c3b-a1e6-1af1c4e4effd
Action digest        : 01a2df1f57273856572f7d5d6555bd2341e967f151633d5653baf67108c5ccfd:195
Action result digest : e249dbc087874fd6def5325955ae27bc45b15300672f16ae664f29fb63a2b276:815
Action status        : success with exit code 0
-------------------------------------stdout-------------------------------------
arvr
buck-out
fbandroid
fbcode
fbobjc
genai
opsfiles
ovrsource-legacy
PACKAGE
rocket
third-party
tools
whatsapp
www
xplat

-----------------------------------NO stderr------------------------------------

----------------------------------Output files----------------------------------

------------------------------Output file digests-------------------------------

--------------------------------Output symlinks---------------------------------

-----------------------Output directories (tree digests)------------------------

------------------------------------Metadata------------------------------------
Worker               : tsp_cco/remote_execution/scm_agent.prod/1/23
Cached               : false
Execution Dir        : /mnt/remote_execution/execution/23/e5043c4aaeeb413bb49cfd7ae8a6d048
Queue                : 274.115893ms
Total time           : 1.110580071s
  Input fetch        : 2.199974ms
  Before action      : 966.563128ms
  Execution          : 130.832861ms
  After action       : 8.520264ms
  Output upload      : 1.674673ms
```


# Further Reading:
* [SCM Repo Manager](https://www.internalfb.com/wiki/Source_Control/Engineering/Repo_Support_On_Remote_Execution/scm_repo_manager)
* [Monitoring and Alerts](https://www.internalfb.com/wiki/Source_Control/Engineering/Repo_Support_On_Remote_Execution/monitoring_and_alerts)
* [Hacking on SCM Repository Manager](https://www.internalfb.com/wiki/Source_Control/Engineering/Repo_Support_On_Remote_Execution/development)
* [Client Success Story](https://fb.workplace.com/groups/1604648659652094/permalink/9258085384308345/)
* Check out our [resources](https://www.internalfb.com/wiki/Source_Control/Engineering/Repo_Support_On_Remote_Execution/resources) for helpful links

