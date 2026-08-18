use std::{net::TcpStream, sync::Arc};

use rustls::{ClientConfig, ClientConnection, RootCertStore, StreamOwned, pki_types::ServerName};
use webpki_roots::TLS_SERVER_ROOTS;

pub(super) type Stream = StreamOwned<ClientConnection, TcpStream>;

pub(super) fn connect(stream: TcpStream, host: &str) -> Result<Stream, String> {
    let mut roots = RootCertStore::empty();
    roots.extend(TLS_SERVER_ROOTS.iter().cloned());
    let config = ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    let name = ServerName::try_from(host.to_owned()).map_err(|error| error.to_string())?;
    let connection =
        ClientConnection::new(Arc::new(config), name).map_err(|error| error.to_string())?;
    Ok(StreamOwned::new(connection, stream))
}
