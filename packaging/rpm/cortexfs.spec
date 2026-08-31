Name:           cortexfs
Version:        0.1.20
Release:        1%{?dist}
Summary:        FUSE filesystem interface for agent runtimes
License:        MIT
URL:            https://github.com/LIghtJUNction/cortexfs
Source0:        %{name}-%{version}.tar.gz

BuildRequires:  cargo >= 1.91
BuildRequires:  rust >= 1.91
BuildRequires:  gcc
BuildRequires:  make
BuildRequires:  pkgconfig
BuildRequires:  fuse3-devel
BuildRequires:  libsecret-devel

Requires:       bubblewrap >= 0.10.0
Requires:       ca-certificates
Requires:       curl
Requires:       fuse3
Requires:       libsecret
Requires:       systemd
Requires:       util-linux
Conflicts:      cortexfs-git

%description
CortexFS exposes models, agents, tools, and durable sessions through a small
Linux filesystem interface mounted at /ctx.

%prep
%setup -q -n %{name}-%{version}

%build
cargo build --release --locked -p cortexfs --bins -p cortexfs-mcp --bin ctxmcp \
    -p cortexfs-channel-tools -p cortexfs-channel-nostr -p cortexfs-channel-amqp \
    -p cortexfs-channel-wecom-ws -p cortexfs-channel-wechat \
    -p cortexfs-channel-voice -p cortexfs-channel-slack \
    -p cortexfs-channel-mqtt -p cortexfs-agents -p cortexfs-futureagi

%install
install -d -m 0755 \
    %{buildroot}%{_bindir} \
    %{buildroot}%{_datadir}/doc/cortexfs \
    %{buildroot}%{_prefix}/lib/systemd/system \
    %{buildroot}%{_prefix}/lib/cortexfs \
    %{buildroot}%{_datadir}/doc/cortexfs/docs/spec \
    %{buildroot}%{_datadir}/licenses/cortexfs \
    %{buildroot}%{_sysconfdir}/cortexfs/providers.d \
    %{buildroot}%{_sysconfdir}/cortexfs/channels \
    %{buildroot}%{_sharedstatedir}/cortexfs/storage/generations
install -d -m 0700 %{buildroot}%{_sharedstatedir}/cortexfs/secrets
for binary in ctx ctxterm ctxchat tsh cortexfs-mount cortexfs-object-runner \
    cortexfs-terminal-broker cortexfs-agent-runtime cortexfs-auth-runner \
    cortexfs-channel cortexfs-channel-tool cortexfs-channel-nostr \
    cortexfs-channel-amqp cortexfs-channel-wecom-ws cortexfs-channel-wechat \
    cortexfs-channel-voice cortexfs-channel-slack cortexfs-channel-mqtt ctxmcp \
    cortexfs-agent-architect cortexfs-agent-executor cortexfs-agent-product-manager \
    cortexfs-futureagi; do
    install -m 0755 "target/release/$binary" "%{buildroot}%{_bindir}/$binary"
done
for unit in cortexfs.service cortexfs-agent@.service cortexfs-agent@.socket \
    cortexfs-terminal-broker.service cortexfs-terminal-broker.socket \
    cortexfs-channel@.service cortexfs-channel-bluesky.service \
    cortexfs-channel-driver@.service cortexfs-channel-nostr.service \
    cortexfs-channel-amqp.service cortexfs-channel-wecom-ws.service \
    cortexfs-channel-wechat.service cortexfs-channel-voice.service \
    cortexfs-channel-slack.service cortexfs-channel-telegram.service \
    cortexfs-channel-mqtt.service \
    cortexfs-channel-clawdtalk.service \
    cortexfs-channel-dingtalk.service \
    cortexfs-channel-email.service cortexfs-channel-gmail.service \
    cortexfs-channel-irc.service cortexfs-channel-matrix.service \
    cortexfs-channel-mattermost.service cortexfs-channel-mochat.service \
    cortexfs-channel-notion.service \
    cortexfs-channel-qq.service \
    cortexfs-channel-reddit.service cortexfs-channel-twitch.service \
    cortexfs-channel-twitter.service; do
    install -m 0644 "packaging/systemd/$unit" \
        "%{buildroot}%{_prefix}/lib/systemd/system/$unit"
done
install -m 0755 scripts/update-linux.sh %{buildroot}%{_prefix}/lib/cortexfs/update-linux
install -m 0644 README.md %{buildroot}%{_datadir}/doc/cortexfs/README.md
install -m 0644 docs/channels.md %{buildroot}%{_datadir}/doc/cortexfs/docs/channels.md
install -m 0644 docs/futureagi.md %{buildroot}%{_datadir}/doc/cortexfs/docs/futureagi.md
install -m 0644 LICENSE %{buildroot}%{_datadir}/licenses/cortexfs/LICENSE
install -m 0644 docs/spec/*.md %{buildroot}%{_datadir}/doc/cortexfs/docs/spec/

%post
if [ "${CORTEXFS_UPDATE_TRANSACTION:-0}" = 1 ]; then
    exit 0
fi
if [ "$1" -eq 1 ]; then
    /usr/bin/systemctl daemon-reload >/dev/null 2>&1 || :
    /usr/bin/systemctl enable cortexfs.service cortexfs-terminal-broker.socket >/dev/null 2>&1 || :
else
    /usr/bin/systemctl daemon-reload >/dev/null 2>&1 || :
    /usr/bin/systemctl enable --now cortexfs-terminal-broker.socket >/dev/null 2>&1 || :
    /usr/bin/systemctl try-restart cortexfs-terminal-broker.service >/dev/null 2>&1 || :
    if /usr/bin/systemctl is-active --quiet cortexfs.service; then
        /usr/bin/systemctl try-restart cortexfs.service >/dev/null 2>&1 || :
        for socket_path in /etc/systemd/system/sockets.target.wants/cortexfs-agent@*.socket; do
            [ -e "$socket_path" ] || continue
            socket=${socket_path##*/}
            /usr/bin/systemctl start "$socket" >/dev/null 2>&1 || :
        done
    fi
fi

%preun
if [ "$1" -eq 0 ]; then
    /usr/bin/systemctl disable --now cortexfs.service cortexfs-terminal-broker.service \
        cortexfs-terminal-broker.socket >/dev/null 2>&1 || :
fi

%postun
/usr/bin/systemctl daemon-reload >/dev/null 2>&1 || :

%files
%license %{_datadir}/licenses/cortexfs/LICENSE
%doc %{_datadir}/doc/cortexfs/README.md
%doc %{_datadir}/doc/cortexfs/docs/channels.md
%doc %{_datadir}/doc/cortexfs/docs/futureagi.md
%doc %{_datadir}/doc/cortexfs/docs/spec
%{_bindir}/ctx
%{_bindir}/ctxterm
%{_bindir}/ctxchat
%{_bindir}/tsh
%{_bindir}/cortexfs-mount
%{_bindir}/cortexfs-object-runner
%{_bindir}/cortexfs-terminal-broker
%{_bindir}/cortexfs-agent-runtime
%{_bindir}/cortexfs-auth-runner
%{_bindir}/cortexfs-channel
%{_bindir}/cortexfs-channel-tool
%{_bindir}/cortexfs-channel-nostr
%{_bindir}/cortexfs-channel-amqp
%{_bindir}/cortexfs-channel-wecom-ws
%{_bindir}/cortexfs-channel-wechat
%{_bindir}/cortexfs-channel-voice
%{_bindir}/cortexfs-channel-slack
%{_bindir}/cortexfs-channel-mqtt
%{_bindir}/ctxmcp
%{_bindir}/cortexfs-agent-architect
%{_bindir}/cortexfs-agent-executor
%{_bindir}/cortexfs-agent-product-manager
%{_bindir}/cortexfs-futureagi
%dir %{_prefix}/lib/cortexfs
%{_prefix}/lib/cortexfs/update-linux
%{_prefix}/lib/systemd/system/cortexfs.service
%{_prefix}/lib/systemd/system/cortexfs-agent@.service
%{_prefix}/lib/systemd/system/cortexfs-agent@.socket
%{_prefix}/lib/systemd/system/cortexfs-terminal-broker.service
%{_prefix}/lib/systemd/system/cortexfs-terminal-broker.socket
%{_prefix}/lib/systemd/system/cortexfs-channel@.service
%{_prefix}/lib/systemd/system/cortexfs-channel-bluesky.service
%{_prefix}/lib/systemd/system/cortexfs-channel-driver@.service
%{_prefix}/lib/systemd/system/cortexfs-channel-nostr.service
%{_prefix}/lib/systemd/system/cortexfs-channel-amqp.service
%{_prefix}/lib/systemd/system/cortexfs-channel-wecom-ws.service
%{_prefix}/lib/systemd/system/cortexfs-channel-wechat.service
%{_prefix}/lib/systemd/system/cortexfs-channel-voice.service
%{_prefix}/lib/systemd/system/cortexfs-channel-slack.service
%{_prefix}/lib/systemd/system/cortexfs-channel-telegram.service
%{_prefix}/lib/systemd/system/cortexfs-channel-mqtt.service
%{_prefix}/lib/systemd/system/cortexfs-channel-clawdtalk.service
%{_prefix}/lib/systemd/system/cortexfs-channel-dingtalk.service
%{_prefix}/lib/systemd/system/cortexfs-channel-email.service
%{_prefix}/lib/systemd/system/cortexfs-channel-gmail.service
%{_prefix}/lib/systemd/system/cortexfs-channel-irc.service
%{_prefix}/lib/systemd/system/cortexfs-channel-matrix.service
%{_prefix}/lib/systemd/system/cortexfs-channel-mattermost.service
%{_prefix}/lib/systemd/system/cortexfs-channel-mochat.service
%{_prefix}/lib/systemd/system/cortexfs-channel-notion.service
%{_prefix}/lib/systemd/system/cortexfs-channel-qq.service
%{_prefix}/lib/systemd/system/cortexfs-channel-reddit.service
%{_prefix}/lib/systemd/system/cortexfs-channel-twitch.service
%{_prefix}/lib/systemd/system/cortexfs-channel-twitter.service
%dir %{_sysconfdir}/cortexfs
%dir %{_sysconfdir}/cortexfs/providers.d
%attr(0700,root,root) %dir %{_sysconfdir}/cortexfs/channels
%dir %{_sharedstatedir}/cortexfs
%dir %{_sharedstatedir}/cortexfs/storage
%dir %{_sharedstatedir}/cortexfs/storage/generations
%attr(0700,root,root) %dir %{_sharedstatedir}/cortexfs/secrets

%changelog
* Fri Aug 21 2026 LIghtJUNction <lightjunction.me@gmail.com> - 0.1.20-1
- Preserve completed host tool calls in continuation context to prevent duplicate execution.
- Auto-mount the current workspace for bare ctx sessions when the agent policy permits it.
- Report the required provider network permission when an agent policy denies egress.
- Refresh the base64, fuser, getrandom, jsonschema, and reqwest dependency lines.

* Tue Aug 18 2026 LIghtJUNction <lightjunction.me@gmail.com> - 0.1.15-1
- Accept raw stdout from passthrough agent tools and preserve failure diagnostics.

* Tue Aug 18 2026 LIghtJUNction <lightjunction.me@gmail.com> - 0.1.14-1
- Use a unique capability handshake request ID for every hosted-agent continuation step.

* Tue Aug 18 2026 LIghtJUNction <lightjunction.me@gmail.com> - 0.1.13-1
- Preserve the first streamed tool-call index when providers emit unexpected parallel calls.

* Tue Aug 18 2026 LIghtJUNction <lightjunction.me@gmail.com> - 0.1.12-1
- Preserve the model error summary when the hosted Agent wrapper exits.

* Tue Aug 18 2026 LIghtJUNction <lightjunction.me@gmail.com> - 0.1.11-1
- Preserve bounded, redacted Agent process diagnostics in EIO responses.

* Tue Aug 18 2026 LIghtJUNction <lightjunction.me@gmail.com> - 0.1.10-1
- Enforce negotiated channel capabilities for live effects and commands.

* Tue Aug 18 2026 LIghtJUNction <lightjunction.me@gmail.com> - 0.1.9-1
- Add one-shot channel command reply callbacks.
