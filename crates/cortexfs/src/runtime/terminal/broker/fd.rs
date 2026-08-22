use std::io::{self, IoSlice, IoSliceMut};
use std::mem::MaybeUninit;
use std::os::fd::{AsFd, OwnedFd};
use std::os::unix::net::UnixStream;

use rustix::net::{
    RecvAncillaryBuffer, RecvAncillaryMessage, RecvFlags, SendAncillaryBuffer,
    SendAncillaryMessage, SendFlags, recvmsg, sendmsg,
};

use super::BrokerProtocolError;

pub fn send_fd(control: &UnixStream, offered: &UnixStream) -> Result<(), BrokerProtocolError> {
    let marker = *b"F";
    let vectors = [IoSlice::new(&marker)];
    let rights = [offered.as_fd()];
    let mut space = [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(1))];
    let mut ancillary = SendAncillaryBuffer::new(&mut space);
    if !ancillary.push(SendAncillaryMessage::ScmRights(&rights)) {
        return Err(BrokerProtocolError::Protocol);
    }
    let sent = sendmsg(control, &vectors, &mut ancillary, SendFlags::NOSIGNAL).map_err(errno_io)?;
    if sent != marker.len() {
        return Err(BrokerProtocolError::Protocol);
    }
    Ok(())
}

pub fn receive_fd(control: &UnixStream) -> Result<OwnedFd, BrokerProtocolError> {
    let mut marker = [0_u8; 1];
    let mut vectors = [IoSliceMut::new(&mut marker)];
    let mut space = [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(1))];
    let mut ancillary = RecvAncillaryBuffer::new(&mut space);
    let received = recvmsg(
        control,
        &mut vectors,
        &mut ancillary,
        RecvFlags::CMSG_CLOEXEC,
    )
    .map_err(errno_io)?;
    if received.bytes != 1 || marker != *b"F" {
        return Err(BrokerProtocolError::Protocol);
    }
    for message in ancillary.drain() {
        if let RecvAncillaryMessage::ScmRights(mut rights) = message {
            return rights.next().ok_or(BrokerProtocolError::Protocol);
        }
    }
    Err(BrokerProtocolError::Protocol)
}

fn errno_io(error: rustix::io::Errno) -> BrokerProtocolError {
    io::Error::from_raw_os_error(error.raw_os_error()).into()
}
