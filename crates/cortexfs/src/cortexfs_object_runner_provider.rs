use serde::Deserialize;
use serde_json::json;
use std::collections::BTreeMap;
use std::fmt::Write as FmtWrite;
use std::net::IpAddr;

include!("cortexfs_object_runner_provider/types.rs");
include!("cortexfs_object_runner_provider/completion.rs");
include!("cortexfs_object_runner_provider/config.rs");
include!("cortexfs_object_runner_provider/routes.rs");
include!("cortexfs_object_runner_provider/credentials.rs");
include!("cortexfs_object_runner_provider/context.rs");
include!("cortexfs_object_runner_provider/requests.rs");
include!("cortexfs_object_runner_provider/streaming.rs");
include!("cortexfs_object_runner_provider/curl.rs");
include!("cortexfs_object_runner_provider/responses.rs");
