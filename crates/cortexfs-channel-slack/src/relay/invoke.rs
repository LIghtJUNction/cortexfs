use cortexfs_channels::{ChannelCommandResult, MessageTarget};
use reqwest::Client;
use serde_json::Value;

use crate::{api, config::Config, error::Result, socket::Session};

#[expect(
    clippy::too_many_arguments,
    reason = "the frame bridge keeps socket correlation fields explicit"
)]
pub(super) async fn handle(
    client: &Client,
    config: &Config,
    session: &Session,
    request_id: String,
    session_id: String,
    command_id: String,
    target: MessageTarget,
    name: String,
    payload: Value,
) -> Result<()> {
    let result = ChannelCommandResult::from_value_result(
        api::invoke(client, config, &target, &name, &payload).await,
    );
    session.command_result(
        crate::socket::CommandReply {
            request_id,
            session: session_id,
            command_id,
        },
        result,
    )
}
