use super::{
    AttachmentMode, InteractionCapability as Cap, InteractionSide as Side,
    InteractionV2Error as Error, InteractionV2Frame as Frame, InteractionV2Kind as Kind,
};

pub(super) fn validate_shape(frame: &Frame) -> Result<(), Error> {
    let event = &frame.event;
    if !event.data.is_object() {
        return Err(Error::InvalidEvent);
    }
    let attaching = matches!(event.kind, Kind::Attach | Kind::Attached);
    if attaching != event.session.is_some() {
        return Err(Error::MissingCorrelation("session"));
    }
    if let Some(session) = event.session.as_deref() {
        valid_id("session", session)?;
    }
    if (event.kind == Kind::Input) != event.origin.is_some() {
        return Err(Error::InvalidEvent);
    }
    if let Some(origin) = event.origin.as_ref() {
        valid_id("transport", &origin.transport)?;
        for value in [
            &origin.endpoint,
            &origin.identity,
            &origin.conversation,
            &origin.thread,
        ]
        .into_iter()
        .flatten()
        {
            valid_id("origin", value)?;
        }
    }
    if attaching {
        valid_mode(event.mode, &event.capabilities)?;
    } else if matches!(event.kind, Kind::Hello | Kind::Welcome) {
        valid_caps(&event.capabilities)?;
        if event.mode.is_some() {
            return Err(Error::InvalidAttachmentMode);
        }
    } else if event.mode.is_some() || !event.capabilities.is_empty() {
        return Err(Error::InvalidCapabilities);
    }
    Ok(())
}

pub(super) fn validate_sequence(frame: &Frame) -> Result<(), Error> {
    if frame.session_seq == Some(0) {
        return Err(Error::InvalidSequence);
    }
    let event = &frame.event;
    let valid = match event.kind {
        Kind::Ack => !event.durable && frame.session_seq.is_some(),
        Kind::Accepted | Kind::Started | Kind::Done => event.durable && frame.session_seq.is_some(),
        Kind::Status if event.side == Side::Master => !event.durable && frame.session_seq.is_none(),
        Kind::Event | Kind::Status | Kind::Error => event.durable == frame.session_seq.is_some(),
        _ => !event.durable && frame.session_seq.is_none(),
    };
    valid.then_some(()).ok_or(Error::DurableSequenceMismatch)
}

pub(super) fn valid_id(name: &'static str, value: &str) -> Result<(), Error> {
    (!value.is_empty() && value.len() <= 128 && !value.chars().any(char::is_control))
        .then_some(())
        .ok_or(Error::InvalidIdentifier(name))
}

fn valid_caps(caps: &[Cap]) -> Result<(), Error> {
    (!caps.is_empty()
        && !caps
            .iter()
            .enumerate()
            .any(|(index, cap)| caps.iter().take(index).any(|seen| seen == cap)))
    .then_some(())
    .ok_or(Error::InvalidCapabilities)
}

fn valid_mode(mode: Option<AttachmentMode>, caps: &[Cap]) -> Result<(), Error> {
    valid_caps(caps)?;
    let has = |cap| caps.contains(&cap);
    let valid = match mode {
        Some(AttachmentMode::Observe) => {
            has(Cap::Observe)
                && !has(Cap::Input)
                && !has(Cap::Cancel)
                && !has(Cap::CommandResult)
                && !has(Cap::Invoke)
        }
        Some(AttachmentMode::Interact) => has(Cap::Input),
        None => false,
    };
    valid.then_some(()).ok_or(Error::InvalidAttachmentMode)
}
