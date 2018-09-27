// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use url::Url;

use errors::ErrorKind;

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

#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct ActionDesc {
    href: String,
    expires_at: String,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
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

fn get_upload_obj(repo: &str, oid: &str, address: &Url) -> Result<Action, ErrorKind> {
    address
        .join(&format!("{}/lfs/upload/{}", repo, oid))
        .map(|full_address| {
            let action_desc = ActionDesc {
                href: full_address.as_str().to_string(),
                // TODO(anastasiya): T34243344 Infinite expiration time of the link
                expires_at: "2030-11-10T15:29:07Z".to_string(),
            };
            Action::Upload(action_desc)
        })
        .map_err(|e| {
            ErrorKind::LFSNotFound(format!(
                "Incorrect base address joining on upload request, {}.
            Probably Apiserver has been started without flags
            (or with invalid flags) --http-host, --http-port",
                e.to_string()
            ))
        })
}

fn get_download_obj(repo: &str, oid: &str, address: &Url) -> Result<Action, ErrorKind> {
    address
        .join(&format!("{}/lfs/download/{}", repo, oid))
        .map(|full_address| {
            let action_desc = ActionDesc {
                href: full_address.as_str().to_string(),
                // TODO(anastasiya): T34243344 Infinite expiration time of the link
                expires_at: "2030-11-10T15:29:07Z".to_string(),
            };
            Action::Download(action_desc)
        })
        .map_err(|e| {
            ErrorKind::LFSNotFound(format!(
                "Incorrect base address joining on download request, {}.
            Probably Apiserver has been started without flags
            (or with invalid flags) --http-host, --http-port",
                e.to_string()
            ))
        })
}

fn get_response_obj(
    repo: &String,
    file: &RequestObject,
    address: &Url,
    get_action_obj_func: &Fn(&str, &str, &Url) -> Result<Action, ErrorKind>,
) -> Result<ResponseObject, ErrorKind> {
    get_action_obj_func(&repo, &file.oid, address).map(|action_desc| ResponseObject {
        oid: file.oid.clone(),
        size: file.size,
        actions: action_desc,
    })
}

pub fn build_response(
    repo: String,
    req: BatchRequest,
    address: Url,
) -> Result<BatchResponse, ErrorKind> {
    let response_objects: Result<Vec<ResponseObject>, ErrorKind> = req.objects
        .iter()
        .map(|file| match req.operation {
            OperationType::Upload => get_response_obj(&repo, file, &address, &get_upload_obj),
            OperationType::Download => get_response_obj(&repo, file, &address, &get_download_obj),
        })
        .collect();

    response_objects.map(|response_objects| BatchResponse {
        transfer: "basic".to_string(),
        objects: response_objects,
    })
}

#[test]
fn test_get_upload_link() {
    let address = Url::parse("https://localhost:8000").unwrap();
    let expected_action = Action::Upload(ActionDesc {
        href: "https://localhost:8000/test_repo/lfs/upload/123".to_string(),
        expires_at: "2030-11-10T15:29:07Z".to_string(),
    });
    assert_eq!(
        get_upload_obj("test_repo", "123", &address).unwrap(),
        expected_action
    );
}

#[test]
fn test_get_upload_link_with_additional_slash() {
    let address = Url::parse("https://localhost:8000/").unwrap();
    let expected_action = Action::Upload(ActionDesc {
        href: "https://localhost:8000/test_repo/lfs/upload/123".to_string(),
        expires_at: "2030-11-10T15:29:07Z".to_string(),
    });
    assert_eq!(
        get_upload_obj("test_repo", "123", &address).unwrap(),
        expected_action
    );
}

#[test]
fn test_get_download_link() {
    let address = Url::parse("https://localhost:8000").unwrap();
    let expected_action = Action::Download(ActionDesc {
        href: "https://localhost:8000/test_repo/lfs/download/123".to_string(),
        expires_at: "2030-11-10T15:29:07Z".to_string(),
    });
    assert_eq!(
        get_download_obj("test_repo", "123", &address).unwrap(),
        expected_action
    );
}

#[test]
fn test_get_download_link_with_additional_slash() {
    let address = Url::parse("https://localhost:8000/").unwrap();
    let expected_action = Action::Download(ActionDesc {
        href: "https://localhost:8000/test_repo/lfs/download/123".to_string(),
        expires_at: "2030-11-10T15:29:07Z".to_string(),
    });
    assert_eq!(
        get_download_obj("test_repo", "123", &address).unwrap(),
        expected_action
    );
}
