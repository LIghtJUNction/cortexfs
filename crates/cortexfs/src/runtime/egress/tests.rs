use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::AtomicUsize;
use std::time::Instant;

use super::*;

fn fixture() -> Result<(tempfile::TempDir, PathBuf), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let control = root.path().join("control");
    fs::create_dir_all(&control)?;
    fs::set_permissions(&control, fs::Permissions::from_mode(0o711))?;
    Ok((root, control))
}

fn write_model(root: &Path, model: &str, base_url: &str) -> io::Result<()> {
    let (provider, name) = model
        .split_once('/')
        .ok_or_else(|| io::Error::other("invalid test model"))?;
    let control = root.join("model").join(provider).join(format!("{name}.d"));
    fs::create_dir_all(&control)?;
    fs::write(control.join("default"), format!("base_url={base_url}\n"))
}

fn local_connect_timeout_supported() -> io::Result<bool> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let address = listener.local_addr()?;
    let connected = TcpStream::connect_timeout(&address, Duration::from_millis(100)).is_ok();
    if connected {
        let _accepted = listener.accept()?;
    }
    Ok(connected)
}

#[test]
fn connect_budget_is_absolute_across_addresses_and_shutdown()
-> Result<(), Box<dyn std::error::Error>> {
    let addresses = (1..=32)
        .map(|suffix| format!("203.0.113.{suffix}:9").parse())
        .collect::<Result<Vec<SocketAddr>, _>>()?;
    let shutdown = Arc::new(AtomicBool::new(false));
    let stop = Arc::clone(&shutdown);
    let started = Instant::now();
    let connector = thread::spawn(move || connect_target(&addresses, &stop));
    thread::sleep(Duration::from_millis(20));
    shutdown.store(true, Ordering::Release);
    let _result = connector.join().map_err(|_panic| "connector panicked")?;
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "connect exceeded its absolute budget"
    );
    Ok(())
}

#[test]
fn egress_relays_to_fixed_target_and_cleans_up() -> Result<(), Box<dyn std::error::Error>> {
    if !local_connect_timeout_supported()? {
        return Ok(());
    }
    let (root, control) = fixture()?;
    let tcp = TcpListener::bind("127.0.0.1:0")?;
    let tcp_address = tcp.local_addr()?;
    write_model(
        root.path(),
        "fixture/chat",
        &format!("http://{tcp_address}/v1"),
    )?;
    let targets = plan_targets(root.path(), "fixture/chat")?;
    let target = targets.first().ok_or("missing target")?;
    assert_eq!(target.addresses, [tcp_address]);
    let server = thread::spawn(move || -> io::Result<()> {
        let (mut stream, _address) = tcp.accept()?;
        let mut input = Vec::new();
        stream.read_to_end(&mut input)?;
        stream.write_all(&input)
    });
    let uid = nix::unistd::geteuid().as_raw();
    let gid = nix::unistd::getegid().as_raw();
    let directory;
    {
        let egress =
            ProviderEgress::create(&control, root.path(), "fixture/chat", uid, gid, "run1")?;
        directory = egress.host_dir().to_owned();
        let socket = egress.socket("fixture").ok_or("missing socket")?;
        let metadata = fs::symlink_metadata(socket)?;
        assert_eq!(metadata.mode() & 0o7777, 0o600);
        let mut client = UnixStream::connect(socket)?;
        client.write_all(b"ping")?;
        client.shutdown(Shutdown::Write)?;
        let mut output = Vec::new();
        client.read_to_end(&mut output)?;
        assert_eq!(output, b"ping");
    }
    server.join().map_err(|_panic| "server panicked")??;
    assert!(!directory.exists());
    Ok(())
}

#[test]
fn plan_resolves_alias_fallback_and_deduplicates_provider() -> Result<(), Box<dyn std::error::Error>>
{
    let root = tempfile::tempdir()?;
    let tcp = TcpListener::bind("127.0.0.1:0")?;
    let base_url = format!("http://{}/v1", tcp.local_addr()?);
    write_model(root.path(), "fixture/primary", &base_url)?;
    write_model(root.path(), "fixture/fallback", &base_url)?;
    fs::write(
        root.path().join("model/fixture/primary.d/fallback"),
        "fixture/fallback\n",
    )?;
    fs::create_dir_all(root.path().join("model"))?;
    std::os::unix::fs::symlink("/ctx/model/fixture/primary", root.path().join("model/main"))?;
    let targets = plan_targets(root.path(), "main")?;
    assert_eq!(targets.len(), 1);
    assert_eq!(targets.first().ok_or("missing target")?.provider, "fixture");
    Ok(())
}

#[test]
fn provider_model_detection_resolves_provider_alias() -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    write_model(root.path(), "fixture/chat", "http://127.0.0.1:8001/v1")?;
    fs::create_dir_all(root.path().join("model"))?;
    std::os::unix::fs::symlink("/ctx/model/fixture/chat", root.path().join("model/main"))?;

    assert!(is_provider_model(root.path(), "main")?);
    Ok(())
}

#[test]
fn provider_model_detection_skips_debug_alias() -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    fs::create_dir_all(root.path().join("model/debug/echo.d"))?;
    std::os::unix::fs::symlink("/ctx/model/debug/echo", root.path().join("model/main"))?;

    assert!(!is_provider_model(root.path(), "main")?);
    Ok(())
}

#[test]
fn plan_rejects_conflicting_provider_authorities_before_resources()
-> Result<(), Box<dyn std::error::Error>> {
    let (root, control) = fixture()?;
    write_model(root.path(), "fixture/primary", "http://127.0.0.1:8001/v1")?;
    write_model(root.path(), "fixture/fallback", "http://127.0.0.1:8002/v1")?;
    fs::write(
        root.path().join("model/fixture/primary.d/fallback"),
        "fixture/fallback\n",
    )?;
    let before = fs::read_dir(&control)?.count();
    let result = ProviderEgress::create(
        &control,
        root.path(),
        "fixture/primary",
        nix::unistd::geteuid().as_raw(),
        nix::unistd::getegid().as_raw(),
        "run1",
    );
    assert!(matches!(
        result,
        Err(ProviderEgressError::AuthorityConflict)
    ));
    assert_eq!(fs::read_dir(&control)?.count(), before);
    Ok(())
}

#[test]
fn plan_skips_orphan_fallback_but_requires_primary_control()
-> Result<(), Box<dyn std::error::Error>> {
    let (root, control) = fixture()?;
    write_model(root.path(), "fixture/primary", "http://127.0.0.1:8001/v1")?;
    fs::write(
        root.path().join("model/fixture/primary.d/fallback"),
        "orphan/missing\n",
    )?;
    let targets = plan_targets(root.path(), "fixture/primary")?;
    assert_eq!(targets.len(), 1);

    let result = ProviderEgress::create(
        &control,
        root.path(),
        "orphan/missing",
        nix::unistd::geteuid().as_raw(),
        nix::unistd::getegid().as_raw(),
        "run1",
    );
    assert!(matches!(result, Err(ProviderEgressError::MissingControl)));
    assert_eq!(fs::read_dir(control)?.count(), 0);
    Ok(())
}

#[test]
fn run_directory_keeps_runtime_owner_when_agent_differs_if_root()
-> Result<(), Box<dyn std::error::Error>> {
    let (root, control) = fixture()?;
    let tcp = TcpListener::bind("127.0.0.1:0")?;
    write_model(
        root.path(),
        "fixture/chat",
        &format!("http://{}/v1", tcp.local_addr()?),
    )?;
    let runtime_owner = (
        nix::unistd::geteuid().as_raw(),
        nix::unistd::getegid().as_raw(),
    );
    let agent = if runtime_owner.0 == 0 {
        (65_534, 65_534)
    } else {
        runtime_owner
    };
    let egress = ProviderEgress::create(
        &control,
        root.path(),
        "fixture/chat",
        agent.0,
        agent.1,
        "run1",
    )?;
    let directory = fs::symlink_metadata(egress.host_dir())?;
    let socket = fs::symlink_metadata(egress.socket("fixture").ok_or("missing socket")?)?;
    assert_eq!((directory.uid(), directory.gid()), runtime_owner);
    assert_eq!(directory.mode() & 0o7777, 0o711);
    assert_eq!((socket.uid(), socket.gid()), agent);
    assert_eq!(socket.mode() & 0o7777, 0o600);
    Ok(())
}

#[test]
fn root_agent_uid_can_roundtrip_through_owned_socket() -> Result<(), Box<dyn std::error::Error>> {
    if nix::unistd::geteuid().as_raw() != 0 {
        return Ok(());
    }
    if !local_connect_timeout_supported()? {
        return Ok(());
    }
    let (root, control) = fixture()?;
    fs::set_permissions(root.path(), fs::Permissions::from_mode(0o711))?;
    let tcp = TcpListener::bind("127.0.0.1:0")?;
    write_model(
        root.path(),
        "fixture/chat",
        &format!("http://{}/v1", tcp.local_addr()?),
    )?;
    let upstream = thread::spawn(move || -> io::Result<()> {
        let (mut stream, _address) = tcp.accept()?;
        let mut input = Vec::new();
        stream.read_to_end(&mut input)?;
        stream.write_all(&input)
    });
    let egress = ProviderEgress::create(
        &control,
        root.path(),
        "fixture/chat",
        65_534,
        65_534,
        "run1",
    )?;
    let status = Command::new(std::env::current_exe()?)
        .arg("--exact")
        .arg("runtime::egress::tests::cross_uid_child_helper")
        .arg("--ignored")
        .arg("--nocapture")
        .env(
            "CORTEXFS_EGRESS_CROSS_UID_SOCKET",
            egress.socket("fixture").ok_or("missing socket")?,
        )
        .status()?;
    assert!(status.success());
    upstream.join().map_err(|_panic| "upstream panicked")??;
    Ok(())
}

#[test]
#[ignore = "re-exec helper for root cross-uid egress test"]
fn cross_uid_child_helper() -> Result<(), Box<dyn std::error::Error>> {
    let Some(socket) = std::env::var_os("CORTEXFS_EGRESS_CROSS_UID_SOCKET") else {
        return Ok(());
    };
    nix::unistd::setgroups(&[])?;
    nix::unistd::setgid(nix::unistd::Gid::from_raw(65_534))?;
    nix::unistd::setuid(nix::unistd::Uid::from_raw(65_534))?;
    let mut stream = UnixStream::connect(socket)?;
    stream.write_all(b"cross-uid")?;
    stream.shutdown(Shutdown::Write)?;
    let mut response = Vec::new();
    stream.read_to_end(&mut response)?;
    assert_eq!(response, b"cross-uid");
    Ok(())
}

#[test]
fn drop_preserves_replacement_socket_on_receipt_conflict() -> Result<(), Box<dyn std::error::Error>>
{
    let (root, control) = fixture()?;
    let tcp = TcpListener::bind("127.0.0.1:0")?;
    write_model(
        root.path(),
        "fixture/chat",
        &format!("http://{}/v1", tcp.local_addr()?),
    )?;
    let egress = ProviderEgress::create(
        &control,
        root.path(),
        "fixture/chat",
        nix::unistd::geteuid().as_raw(),
        nix::unistd::getegid().as_raw(),
        "run1",
    )?;
    let directory = egress.host_dir().to_owned();
    let socket = egress.socket("fixture").ok_or("missing socket")?.to_owned();
    fs::remove_file(&socket)?;
    let replacement = UnixListener::bind(&socket)?;
    drop(egress);
    assert!(socket.exists());
    assert!(directory.exists());
    drop(replacement);
    fs::remove_file(socket)?;
    fs::remove_dir(directory)?;
    Ok(())
}

#[test]
fn listener_rejects_wrong_peer_before_connecting() -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let tcp = TcpListener::bind("127.0.0.1:0")?;
    tcp.set_nonblocking(true)?;
    let target = tcp.local_addr()?;
    let accepts = Arc::new(AtomicUsize::new(0));
    let upstream_stop = Arc::new(AtomicBool::new(false));
    let upstream_accepts = Arc::clone(&accepts);
    let upstream_shutdown = Arc::clone(&upstream_stop);
    let upstream = thread::spawn(move || {
        while !upstream_shutdown.load(Ordering::Acquire) {
            match tcp.accept() {
                Ok((_stream, _address)) => {
                    upstream_accepts.fetch_add(1, Ordering::AcqRel);
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    thread::sleep(ACCEPT_PAUSE);
                }
                Err(_error) => return,
            }
        }
    });
    let socket = root.path().join("egress.sock");
    let listener = UnixListener::bind(&socket)?;
    let shutdown = Arc::new(AtomicBool::new(false));
    let stop = Arc::clone(&shutdown);
    let denied_uid = nix::unistd::getuid().as_raw().wrapping_add(1);
    let server = thread::spawn(move || serve(listener, vec![target], denied_uid, stop));
    let mut client = UnixStream::connect(&socket)?;
    client.write_all(b"denied")?;
    thread::sleep(Duration::from_millis(100));
    shutdown.store(true, Ordering::Release);
    server.join().map_err(|_panic| "server panicked")?;
    upstream_stop.store(true, Ordering::Release);
    upstream.join().map_err(|_panic| "upstream panicked")?;
    assert_eq!(accepts.load(Ordering::Acquire), 0);
    Ok(())
}

#[test]
fn drop_cancels_continuous_relay_within_fixed_bound() -> Result<(), Box<dyn std::error::Error>> {
    if !local_connect_timeout_supported()? {
        return Ok(());
    }
    let (root, control) = fixture()?;
    let tcp = TcpListener::bind("127.0.0.1:0")?;
    write_model(
        root.path(),
        "fixture/chat",
        &format!("http://{}/v1", tcp.local_addr()?),
    )?;
    let accepted = Arc::new(AtomicBool::new(false));
    let did_accept = Arc::clone(&accepted);
    let upstream = thread::spawn(move || -> io::Result<()> {
        let (mut stream, _address) = tcp.accept()?;
        stream.set_read_timeout(Some(RELAY_TIMEOUT))?;
        stream.set_write_timeout(Some(RELAY_TIMEOUT))?;
        did_accept.store(true, Ordering::Release);
        let mut buffer = [0_u8; 1024];
        loop {
            match stream.read(&mut buffer) {
                Ok(0) => return Ok(()),
                Ok(count) => stream.write_all(
                    buffer
                        .get(..count)
                        .ok_or_else(|| io::Error::other("invalid test relay range"))?,
                )?,
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
                    ) => {}
                Err(error) => return Err(error),
            }
        }
    });
    let egress = ProviderEgress::create(
        &control,
        root.path(),
        "fixture/chat",
        nix::unistd::geteuid().as_raw(),
        nix::unistd::getegid().as_raw(),
        "run1",
    )?;
    let mut client = UnixStream::connect(egress.socket("fixture").ok_or("missing socket")?)?;
    client.set_read_timeout(Some(RELAY_TIMEOUT))?;
    client.set_write_timeout(Some(RELAY_TIMEOUT))?;
    let traffic_stop = Arc::new(AtomicBool::new(false));
    let traffic_shutdown = Arc::clone(&traffic_stop);
    let traffic = thread::spawn(move || {
        let mut response = [0_u8; 4];
        while !traffic_shutdown.load(Ordering::Acquire) {
            let _written = client.write_all(b"load");
            let _read = client.read_exact(&mut response);
        }
    });
    let deadline = Instant::now() + Duration::from_secs(1);
    while !accepted.load(Ordering::Acquire) && Instant::now() < deadline {
        thread::sleep(ACCEPT_PAUSE);
    }
    assert!(accepted.load(Ordering::Acquire));
    let started = Instant::now();
    drop(egress);
    let elapsed = started.elapsed();
    traffic_stop.store(true, Ordering::Release);
    traffic.join().map_err(|_panic| "traffic panicked")?;
    upstream.join().map_err(|_panic| "upstream panicked")??;
    assert!(elapsed < Duration::from_secs(1), "drop took {elapsed:?}");
    Ok(())
}
