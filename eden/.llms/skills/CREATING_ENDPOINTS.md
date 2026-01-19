---
oncalls: ['source_control']
description: "This guide explains how to create new EdenAPI/SLAPI endpoints, covering both streaming and non-streaming patterns."
---

# Creating EdenAPI Endpoints

This guide explains how to create new EdenAPI/SLAPI endpoints, covering both streaming and non-streaming patterns.

## Overview

EdenAPI is a REST-like API that connects Sapling clients to the Mononoke server. Creating a new endpoint involves three main steps:

1. **Define Types** - Request and response types in `edenapi_types`
2. **Server Handler** - Implement the handler in `slapi_service`
3. **Client Implementation** - Add client-side code (lands after server)

## Key Files Reference

| Component | Location |
|-----------|----------|
| Types | `fbcode/eden/scm/lib/edenapi/types/src/` |
| Server Handler Trait | `fbcode/eden/mononoke/servers/slapi/slapi_service/src/handlers/handler.rs` |
| Server Handlers | `fbcode/eden/mononoke/servers/slapi/slapi_service/src/handlers/*.rs` |
| Router Registration | `fbcode/eden/mononoke/servers/slapi/slapi_service/src/handlers.rs` |
| Client API Trait | `fbcode/eden/scm/lib/edenapi/trait/src/api.rs` |
| Client Implementation | `fbcode/eden/scm/lib/edenapi/src/client.rs` |

---

## Step 1: Define Request/Response Types

**Location**: `fbcode/eden/scm/lib/edenapi/types/src/`

Types need the `#[auto_wire]` macro to generate wire format serialization and the `ToWire` trait implementation. All parameters should be sent in the request body, not in the URL path.

### Required Attributes

```rust
#[auto_wire]
#[derive(Clone, Debug, Default, Eq, PartialEq)]
#[derive(Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
pub struct MyRequest {
    #[id(1)]
    pub field_one: String,
    #[id(2)]
    pub field_two: Option<u64>,
}
```

Key points:
- `#[auto_wire]` generates wire format code
- `#[id(N)]` assigns a stable field ID for serialization (use sequential numbers)
- Include `Serialize`, `Deserialize` for serde support
- `Arbitrary` derive enables property-based testing
- **All parameters go in the request struct**, not in URL path

### Non-streaming Example

See `EphemeralPrepareRequest`/`EphemeralPrepareResponse` in `types/src/commit.rs:555-570`:

```rust
#[auto_wire]
#[derive(Clone, Default, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct EphemeralPrepareRequest {
    #[id(1)]
    pub custom_duration_secs: Option<u64>,
    #[id(2)]
    pub labels: Option<Vec<String>>,
}

// Response doesn't need #[auto_wire] if not batched
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct EphemeralPrepareResponse {
    pub bubble_id: NonZeroU64,
    pub expiration_timestamp: Option<i64>,
}
```

### Streaming Example

See `CommitMutationsRequest`/`CommitMutationsResponse` in `types/src/commit.rs:609-623`:

```rust
#[auto_wire]
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct CommitMutationsRequest {
    #[id(1)]
    pub commits: Vec<HgId>,
}

#[auto_wire]
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CommitMutationsResponse {
    #[id(1)]
    pub mutation: HgMutationEntryContent,
}
```

---

## Step 2: Create Server-Side Handler

**Location**: `fbcode/eden/mononoke/servers/slapi/slapi_service/src/handlers/`

### 2a. Add Method to SaplingRemoteApiMethod Enum

In `handlers.rs`, add your method to the enum:

```rust
pub enum SaplingRemoteApiMethod {
    // ... existing methods
    MyNewMethod,
}
```

And update the `Display` impl:

```rust
impl fmt::Display for SaplingRemoteApiMethod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            // ... existing matches
            Self::MyNewMethod => "my_new_method",
        };
        write!(f, "{}", name)
    }
}
```

### 2b. Implement the Handler

Create a handler struct and implement `SaplingRemoteApiHandler`:

```rust
pub struct MyNewHandler;

#[async_trait]
impl SaplingRemoteApiHandler for MyNewHandler {
    type Request = MyRequest;
    type Response = MyResponse;

    const HTTP_METHOD: hyper::Method = hyper::Method::POST;
    const API_METHOD: SaplingRemoteApiMethod = SaplingRemoteApiMethod::MyNewMethod;
    const ENDPOINT: &'static str = "/my/endpoint";  // Without /:repo prefix

    async fn handler(
        ectx: SaplingRemoteApiContext<Self::PathExtractor, Self::QueryStringExtractor, Repo>,
        request: Self::Request,
    ) -> HandlerResult<'async_trait, Self::Response> {
        let repo = ectx.repo();
        // ... implementation
    }
}
```

### Hg vs Git Support

By default, handlers only support Hg clients (`SUPPORTED_FLAVOURS` defaults to `[SlapiCommitIdentityScheme::Hg]`). **Most new endpoints should be Hg-only.**

> ⚠️ **Avoid adding new Git SLAPI methods.** Git support requires additional work in the location service and is discouraged for new endpoints. If you need Git support, consult with the team first.


### Non-streaming Pattern

For single-response endpoints, use `stream::once`:

```rust
async fn handler(
    ectx: SaplingRemoteApiContext<Self::PathExtractor, Self::QueryStringExtractor, Repo>,
    request: Self::Request,
) -> HandlerResult<'async_trait, Self::Response> {
    let repo = ectx.repo();

    Ok(stream::once(async move {
        // Do work...
        let result = do_something(&repo, &request).await?;
        Ok(MyResponse { data: result })
    }).boxed())
}
```

See `EphemeralPrepareHandler` at `handlers/commit.rs:862-892`.

### Streaming Pattern

For endpoints returning multiple items, use `stream::iter` or `try_stream!`:

```rust
async fn handler(
    ectx: SaplingRemoteApiContext<Self::PathExtractor, Self::QueryStringExtractor, Repo>,
    request: Self::Request,
) -> HandlerResult<'async_trait, Self::Response> {
    let repo = ectx.repo();

    let results = fetch_multiple_items(&repo, request.items).await?;

    let responses = results
        .into_iter()
        .map(|item| Ok(MyResponse { data: item }));

    Ok(stream::iter(responses).boxed())
}
```

See `CommitMutationsHandler` at `handlers/commit.rs:1117-1152`.

### 2c. Register the Handler

In `handlers.rs`, add to `build_router()`:

```rust
pub fn build_router<R: Send + Sync + Clone + 'static>(ctx: ServerContext<R>) -> Router {
    // ...
    gotham_build_router(chain, pipelines, |route| {
        // ... existing handlers
        Handlers::setup::<MyNewHandler>(route);
    })
}
```

---

## Step 3: Add Client-Side Implementation

**Location**: `fbcode/eden/scm/lib/edenapi/`

> ⚠️ **IMPORTANT**: Do not land client-side changes until the server-side diff has been deployed. The server must be available first, otherwise clients will call an endpoint that doesn't exist yet.

### 3a. Add Endpoint Path

In `src/client.rs`, add to `mod paths`:

```rust
pub mod paths {
    // ... existing paths
    pub const MY_ENDPOINT: &str = "my/endpoint";
}
```

### 3b. Add Trait Method

In `trait/src/api.rs`, add the method signature to `SaplingRemoteApi`:

```rust
#[async_trait]
pub trait SaplingRemoteApi: Send + Sync + 'static {
    // ... existing methods

    async fn my_endpoint(
        &self,
        request: MyRequest,
    ) -> Result<MyResponse, SaplingRemoteApiError> {
        let _ = request;
        Err(SaplingRemoteApiError::NotSupported)
    }
}
```

### 3c. Implement in Client

In `src/client.rs`, add the implementation:

#### Non-streaming (single response)

```rust
async fn my_endpoint_attempt(
    &self,
    request: MyRequest,
) -> Result<MyResponse, SaplingRemoteApiError> {
    tracing::info!("Calling my_endpoint");
    self.request_single(paths::MY_ENDPOINT, request).await
}

// In impl SaplingRemoteApi for Client:
async fn my_endpoint(
    &self,
    request: MyRequest,
) -> Result<MyResponse, SaplingRemoteApiError> {
    self.with_retry(|this| this.my_endpoint_attempt(request.clone()).boxed())
        .await
}
```

See `ephemeral_prepare_attempt` and its trait impl at `client.rs:828-850` and `1931-1941`.

#### Streaming (multiple responses)

```rust
async fn my_endpoint(
    &self,
    items: Vec<ItemId>,
) -> Result<Vec<MyResponse>, SaplingRemoteApiError> {
    tracing::info!("Requesting {} items", items.len());

    let requests = self.prepare_requests(
        None,
        paths::MY_ENDPOINT,
        items,
        self.config().max_items_per_batch,  // or Some(N) for fixed batch size
        None,
        |items| {
            let req = MyRequest { items };
            self.log_request(&req, "my_endpoint");
            req
        },
        |url, _keys| url.clone(),
    )?;

    self.fetch_vec_with_retry::<MyResponse>(requests).await
}
```

See `commit_mutations` at `client.rs:1980-2001`.

---

## Landing Order

1. **First diff**: Server-side changes (types + handler + registration)
   - Land and wait for deployment

2. **Second diff**: Client-side changes
   - **Do not land until server-side is deployed**
   - Once server is deployed, land the client diff

---

## Optional: Query String Parameters

If your endpoint needs query parameters (rarely needed, prefer putting params in request body), define a custom extractor:

```rust
#[derive(Debug, Deserialize, StateData, StaticResponseExtender)]
pub struct MyQueryString {
    pub bubble_id: Option<NonZeroU64>,
}

impl SaplingRemoteApiHandler for MyHandler {
    type QueryStringExtractor = MyQueryString;
    // ...

    async fn handler(
        ectx: SaplingRemoteApiContext<Self::PathExtractor, Self::QueryStringExtractor, Repo>,
        request: Self::Request,
    ) -> HandlerResult<'async_trait, Self::Response> {
        let query = ectx.query();
        let bubble_id = query.bubble_id;
        // ...
    }
}
```

See `UploadBonsaiChangesetQueryString` at `handlers/commit.rs:151-153`.

---

## Testing

- Server-side unit tests can be added alongside handler code
- Integration tests are in `fbcode/eden/mononoke/tests/integration/`
- Client-side tests typically use mock servers or `eagerepo`
