include "common/fb303/if/fb303.thrift"

struct MononokeGetRawParams {
  1: string changeset,
  2: string path,
}

service MononokeAPIService extends fb303.FacebookService {
  binary get_raw(1: MononokeGetRawParams params),
}
