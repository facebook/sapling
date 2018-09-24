// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

/* Request Example
{
  "operation": "download",
  "objects": [
    {
      "oid": "12345678",
      "size": 123,
    }
  ]
}
*/

#[derive(Debug, Serialize, Deserialize)]
enum OperationType {
    #[serde(rename = "upload")] Upload,
    #[serde(rename = "download")] Download,
}

#[derive(Debug, Serialize, Deserialize)]
struct RequestObject {
    oid: String,
    size: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BatchRequest {
    operation: OperationType,
    objects: Vec<RequestObject>,
}

//Response Example
/*
{
  "transfer": "basic",
  "objects": [
    {
      "oid": "1111111",
      "size": 123,
      "actions": {
        "download": {
          "href": "https://some-download.com",
          "expires_at": "2016-11-10T15:29:07Z",
        }
      }
    }
  ]
}
*/

#[derive(Debug, Serialize, Deserialize)]
struct ActionDesc {
    href: String,
    expires_at: String,
}

#[derive(Debug, Serialize, Deserialize)]
enum Action {
    #[serde(rename = "upload")] Upload(ActionDesc),
    #[serde(rename = "download")] Download(ActionDesc),
}

#[derive(Debug, Serialize, Deserialize)]
struct ResponseObject {
    oid: String,
    size: u64,
    actions: Action,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BatchResponse {
    transfer: String,
    objects: Vec<ResponseObject>,
}

fn get_upload_obj(repo: &str, oid: &str) -> Action {
    let action_desc = ActionDesc {
        // TODO(anastasiyaz): T34243344 Config for mononoke API server to have the opportunity to return the link to itself.
        href: format!("http://127.0.0.1:8000/{}/lfs/upload/{}", repo, oid),
        // TODO(anastasiya): T34243344 Infinite expiration time of the link
        expires_at: "2030-11-10T15:29:07Z".to_string(),
    };
    Action::Upload(action_desc)
}

fn get_download_obj(repo: &str, oid: &str) -> Action {
    let action_desc = ActionDesc {
        // TODO(anastasiyaz): T34243344 Config for mononoke API server to have the opportunity to return the link to itself.
        href: format!("http://127.0.0.1:8000/{}/lfs/download/{}", repo, oid),
        // TODO(anastasiya): T34243344 Infinite expiration time of the link
        expires_at: "2030-11-10T15:29:07Z".to_string(),
    };
    Action::Download(action_desc)
}

pub fn build_response(repo: String, req: BatchRequest) -> BatchResponse {
    let response_objects = req.objects
        .iter()
        .map(|file| match req.operation {
            OperationType::Upload => ResponseObject {
                oid: file.oid.clone(),
                size: file.size,
                actions: get_upload_obj(&repo.clone(), &file.oid),
            },
            OperationType::Download => ResponseObject {
                oid: file.oid.clone(),
                size: file.size,
                actions: get_download_obj(&repo, &file.oid),
            },
        })
        .collect();

    let response = BatchResponse {
        transfer: "basic".to_string(),
        objects: response_objects,
    };
    response
}
