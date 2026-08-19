use std::{
    io::{BufRead, BufReader},
    os::unix::net::UnixStream,
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

use cortexfs_channels::{
    ChannelActions, ChannelCapabilities, ChannelFrame, ChannelFrameBody, ChannelId, ConversationId,
    DeliveryReceipt, MessageBody, MessageTarget, OutboundMessage,
};

use super::DriverHub;

#[test]
fn hub_waits_for_driver_receipt() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let channel = ChannelId::from_static("telegram");
    let hub = DriverHub::default();
    let (runtime, adapter) = UnixStream::pair()?;
    let writer = Arc::new(Mutex::new(runtime));
    let _registration = hub.attach(
        &channel,
        Arc::clone(&writer),
        ChannelCapabilities::text(),
        ChannelActions::empty(),
    );
    let receipt_channel = channel.clone();
    let receipt_hub = hub.clone();
    let worker = thread::spawn(
        move || -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            let mut reader = BufReader::new(adapter);
            let mut line = String::new();
            reader.read_line(&mut line)?;
            let ChannelFrameBody::Outbound { request_id, .. } =
                ChannelFrame::decode(line.as_bytes())?.frame
            else {
                return Err("outbound frame missing".into());
            };
            let receipt = DeliveryReceipt {
                channel: receipt_channel,
                message_id: "remote-1".to_owned(),
                target: target()?,
                timestamp_ms: None,
            };
            assert!(receipt_hub.complete(&request_id, receipt));
            Ok(())
        },
    );
    let receipt = hub.send_and_wait(
        &channel,
        "send-1",
        OutboundMessage {
            target: target()?,
            body: MessageBody::text("hello")?,
            metadata: std::collections::BTreeMap::new(),
        },
        Duration::from_secs(1),
    )?;
    assert_eq!(receipt.message_id, "remote-1");
    worker
        .join()
        .map_err(|error| std::io::Error::other(format!("worker panicked: {error:?}")))??;
    Ok(())
}

fn target() -> Result<MessageTarget, cortexfs_channels::ChannelError> {
    Ok(MessageTarget {
        channel: ChannelId::from_static("telegram"),
        conversation: ConversationId::new("chat")?,
        thread: None,
        reply_to: None,
    })
}
