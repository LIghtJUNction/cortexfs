use cortexfs_channels::OutboundMessage;
use lettre::{
    Message, SmtpTransport, Transport, message::header::ContentType,
    transport::smtp::authentication::Credentials,
};

use super::{EmailConfig, EmailError};

pub(super) fn send(config: &EmailConfig, message: &OutboundMessage) -> Result<(), EmailError> {
    let recipient = message
        .metadata
        .get("email.from")
        .map_or(message.target.conversation.as_str(), String::as_str);
    let subject = message
        .metadata
        .get("email.subject")
        .map_or("CortexFS reply", String::as_str);
    let mut builder = Message::builder()
        .from(
            config
                .from
                .parse()
                .map_err(|error| EmailError::Smtp(format!("invalid sender: {error}")))?,
        )
        .to(recipient
            .parse()
            .map_err(|error| EmailError::Smtp(format!("invalid recipient: {error}")))?)
        .subject(subject)
        .header(ContentType::TEXT_PLAIN);
    if let Some(reply) = message.target.reply_to.as_deref() {
        builder = builder.in_reply_to(reply.to_owned());
    }
    let email = builder
        .body(message.body.text.clone())
        .map_err(|error| EmailError::Smtp(error.to_string()))?;
    let mailer = SmtpTransport::starttls_relay(&config.smtp_host)
        .map_err(|error| EmailError::Smtp(error.to_string()))?
        .port(config.smtp_port)
        .credentials(Credentials::new(
            config.username.clone(),
            config.password.clone(),
        ))
        .build();
    mailer
        .send(&email)
        .map_err(|error| EmailError::Smtp(error.to_string()))?;
    Ok(())
}
