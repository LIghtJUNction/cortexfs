use crate::*;

use cortexfs::runtime::terminal::broker::{
    BrokerProtocolError, BrokerReply, TerminalMode, read_frame, receive_fd, write_frame,
};

pub(crate) fn run_broker_control(
    mut control: UnixStream,
    pty_writer: &PtyWriter,
    clients: &Clients,
) -> Result<(), BrokerProtocolError> {
    loop {
        match read_frame(&mut control)? {
            BrokerReply::Offer { nonce, mode } => {
                handle_offer(&mut control, pty_writer, clients, &nonce, mode)?;
            }
            _ => return Err(BrokerProtocolError::Protocol),
        }
    }
}

fn handle_offer(
    control: &mut UnixStream,
    pty_writer: &PtyWriter,
    clients: &Clients,
    nonce: &str,
    mode: TerminalMode,
) -> Result<(), BrokerProtocolError> {
    let offered = receive_fd(control)?;
    let stream = UnixStream::from(offered);
    if client_limit_reached(clients)? {
        write_frame(
            control,
            &BrokerReply::Error {
                code: "client_limit".into(),
                message: "terminal client limit reached".into(),
            },
        )?;
        return Ok(());
    }
    let output = stream.try_clone()?;
    output.set_write_timeout(Some(CLIENT_WRITE_TIMEOUT))?;
    write_frame(
        control,
        &BrokerReply::Prepared {
            nonce: nonce.to_owned(),
        },
    )?;
    match read_frame(control)? {
        BrokerReply::Commit { nonce: committed } if committed == nonce => {}
        BrokerReply::Abort { nonce: aborted } if aborted == nonce => return Ok(()),
        _ => return Err(BrokerProtocolError::Protocol),
    }
    clients
        .lock()
        .map_err(|_error| BrokerProtocolError::Protocol)?
        .push(Arc::new(Mutex::new(output)));
    if mode == TerminalMode::Attach {
        let writer = Arc::clone(pty_writer);
        thread::spawn(move || {
            let _result = copy_stream_to_pty(stream, &writer);
        });
    }
    Ok(())
}

fn client_limit_reached(clients: &Clients) -> Result<bool, BrokerProtocolError> {
    clients
        .lock()
        .map(|clients| clients.len() >= MAX_CLIENTS)
        .map_err(|_error| BrokerProtocolError::Protocol)
}
