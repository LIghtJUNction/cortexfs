use serde::Deserialize;
use serde_json::json;
use std::collections::BTreeMap;
use std::fmt::Write as FmtWrite;
use std::net::IpAddr;

include!("runner_provider/types.rs");
include!("runner_provider/completion.rs");
include!("runner_provider/config.rs");
include!("runner_provider/routes.rs");
include!("runner_provider/credentials.rs");
include!("runner_provider/context.rs");
include!("runner_provider/requests.rs");
include!("runner_provider/streaming.rs");
include!("runner_provider/curl.rs");
include!("runner_provider/responses.rs");
