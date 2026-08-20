use base64::{Engine as _, engine::general_purpose::STANDARD};
use lettre::{
    Message, SmtpTransport, Transport,
    message::{Attachment, MultiPart, SinglePart, header::ContentType},
    transport::smtp::authentication::Credentials,
};

use super::{EmailConfig, EmailError};

pub(super) struct Request<'a> {
    pub(super) recipient: &'a str,
    pub(super) subject: &'a str,
    pub(super) text: &'a str,
    pub(super) name: &'a str,
    pub(super) mime: &'a str,
    pub(super) encoded: &'a str,
}

pub(super) fn send(config: &EmailConfig, request: &Request<'_>) -> Result<(), EmailError> {
    let bytes = STANDARD
        .decode(request.encoded)
        .map_err(|error| EmailError::Smtp(format!("invalid attachment: {error}")))?;
    if bytes.len() > 8 * 1024 * 1024 {
        return Err(EmailError::Smtp("attachment exceeds 8 MiB".to_owned()));
    }
    let content_type = request
        .mime
        .parse::<ContentType>()
        .map_err(|error| EmailError::Smtp(format!("invalid attachment MIME type: {error}")))?;
    let email = Message::builder()
        .from(
            config
                .from
                .parse()
                .map_err(|error| EmailError::Smtp(format!("invalid sender: {error}")))?,
        )
        .to(request
            .recipient
            .parse()
            .map_err(|error| EmailError::Smtp(format!("invalid recipient: {error}")))?)
        .subject(request.subject)
        .multipart(
            MultiPart::mixed()
                .singlepart(SinglePart::plain(request.text.to_owned()))
                .singlepart(Attachment::new(request.name.to_owned()).body(bytes, content_type)),
        )
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
