use super::{
    INTERACTION_V2_ABI, InteractionCorrelation as Corr, InteractionSide as Side,
    InteractionV2Error as Error, InteractionV2Frame as Frame, InteractionV2Kind as Kind,
    shape::{valid_id, validate_sequence, validate_shape},
};

const ATTACHMENT: u8 = 1;
const REQUEST: u8 = 2;
const RUN: u8 = 4;
const COMMAND: u8 = 8;

impl Frame {
    /// Validates ABI, correlation, negotiation, and durable sequence invariants.
    pub fn validate(&self) -> Result<(), Error> {
        if self.abi != INTERACTION_V2_ABI {
            return Err(Error::WrongAbi);
        }
        let (required, allowed) = correlations(self.event.kind);
        self.correlation.validate(required, allowed)?;
        validate_side(self.event.side, self.event.kind)?;
        validate_shape(self)?;
        validate_sequence(self)
    }
}

impl Corr {
    fn validate(&self, required: u8, allowed: u8) -> Result<(), Error> {
        valid_id("connection_id", &self.connection)?;
        let fields = [
            (ATTACHMENT, "attachment_id", &self.attachment),
            (REQUEST, "request_id", &self.request),
            (RUN, "run_id", &self.run),
            (COMMAND, "command_id", &self.command),
        ];
        for (bit, name, value) in fields {
            if required & bit != 0 && value.is_none() {
                return Err(Error::MissingCorrelation(name));
            }
            if allowed & bit == 0 && value.is_some() {
                return Err(Error::UnexpectedCorrelation(name));
            }
            if let Some(value) = value.as_deref() {
                valid_id(name, value)?;
            }
        }
        if self.command.is_some() && self.run.is_none() {
            return Err(Error::MissingCorrelation("run_id"));
        }
        if self.run.is_some() && self.request.is_none() {
            return Err(Error::MissingCorrelation("request_id"));
        }
        if self.request.is_some() && self.attachment.is_none() {
            return Err(Error::MissingCorrelation("attachment_id"));
        }
        Ok(())
    }
}

fn correlations(kind: Kind) -> (u8, u8) {
    let exact = match kind {
        Kind::Hello | Kind::Welcome => 0,
        Kind::Attach | Kind::Attached | Kind::Detach | Kind::Ack | Kind::Gap => ATTACHMENT,
        Kind::Input | Kind::Status => ATTACHMENT | REQUEST,
        Kind::Cancel | Kind::Accepted | Kind::Started | Kind::Event | Kind::Done => {
            ATTACHMENT | REQUEST | RUN
        }
        Kind::CommandResult | Kind::Command => ATTACHMENT | REQUEST | RUN | COMMAND,
        Kind::Error => return (0, 15),
    };
    (exact, exact)
}

fn validate_side(side: Side, kind: Kind) -> Result<(), Error> {
    let expected = match kind {
        Kind::Hello
        | Kind::Attach
        | Kind::Detach
        | Kind::Input
        | Kind::Cancel
        | Kind::CommandResult
        | Kind::Ack => Side::Master,
        Kind::Status => return Ok(()),
        _ => Side::Slave,
    };
    (side == expected).then_some(()).ok_or(Error::InvalidEvent)
}
