Name:           cortexfs
Version:        0.1.7
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
cargo build --release --locked -p cortexfs --bins -p cortexfs-mcp --bin ctxmcp

%install
install -d -m 0755 \
    %{buildroot}%{_bindir} \
    %{buildroot}%{_datadir}/doc/cortexfs \
    %{buildroot}%{_prefix}/lib/systemd/system \
    %{buildroot}%{_datadir}/doc/cortexfs/docs/spec \
    %{buildroot}%{_datadir}/licenses/cortexfs \
    %{buildroot}%{_sysconfdir}/cortexfs/providers.d \
    %{buildroot}%{_sysconfdir}/cortexfs/channels \
    %{buildroot}%{_sharedstatedir}/cortexfs/storage/generations
install -d -m 0700 %{buildroot}%{_sharedstatedir}/cortexfs/secrets
for binary in ctx ctxterm ctxchat tsh cortexfs-mount cortexfs-object-runner \
    cortexfs-agent-runtime cortexfs-channel ctxmcp; do
    install -m 0755 "target/release/$binary" "%{buildroot}%{_bindir}/$binary"
done
for unit in cortexfs.service cortexfs-agent@.service cortexfs-agent@.socket \
    cortexfs-channel@.service; do
    install -m 0644 "packaging/systemd/$unit" \
        "%{buildroot}%{_prefix}/lib/systemd/system/$unit"
done
install -m 0644 README.md %{buildroot}%{_datadir}/doc/cortexfs/README.md
install -m 0644 docs/channels.md %{buildroot}%{_datadir}/doc/cortexfs/docs/channels.md
install -m 0644 LICENSE %{buildroot}%{_datadir}/licenses/cortexfs/LICENSE
install -m 0644 docs/spec/*.md %{buildroot}%{_datadir}/doc/cortexfs/docs/spec/

%post
if [ "$1" -eq 1 ]; then
    /usr/bin/systemctl daemon-reload >/dev/null 2>&1 || :
    /usr/bin/systemctl enable cortexfs.service >/dev/null 2>&1 || :
else
    /usr/bin/systemctl daemon-reload >/dev/null 2>&1 || :
    if /usr/bin/systemctl is-active --quiet cortexfs.service; then
        /usr/bin/systemctl try-restart cortexfs.service >/dev/null 2>&1 || :
    fi
fi

%preun
if [ "$1" -eq 0 ]; then
    /usr/bin/systemctl disable --now cortexfs.service >/dev/null 2>&1 || :
fi

%postun
/usr/bin/systemctl daemon-reload >/dev/null 2>&1 || :

%files
%license %{_datadir}/licenses/cortexfs/LICENSE
%doc %{_datadir}/doc/cortexfs/README.md
%doc %{_datadir}/doc/cortexfs/docs/channels.md
%doc %{_datadir}/doc/cortexfs/docs/spec
%{_bindir}/ctx
%{_bindir}/ctxterm
%{_bindir}/ctxchat
%{_bindir}/tsh
%{_bindir}/cortexfs-mount
%{_bindir}/cortexfs-object-runner
%{_bindir}/cortexfs-agent-runtime
%{_bindir}/cortexfs-channel
%{_bindir}/ctxmcp
%{_prefix}/lib/systemd/system/cortexfs.service
%{_prefix}/lib/systemd/system/cortexfs-agent@.service
%{_prefix}/lib/systemd/system/cortexfs-agent@.socket
%{_prefix}/lib/systemd/system/cortexfs-channel@.service
%dir %{_sysconfdir}/cortexfs
%dir %{_sysconfdir}/cortexfs/providers.d
%attr(0700,root,root) %dir %{_sysconfdir}/cortexfs/channels
%dir %{_sharedstatedir}/cortexfs
%dir %{_sharedstatedir}/cortexfs/storage
%dir %{_sharedstatedir}/cortexfs/storage/generations
%attr(0700,root,root) %dir %{_sharedstatedir}/cortexfs/secrets

%changelog
* Sun Aug 16 2026 LIghtJUNction <lightjunction.me@gmail.com> - 0.1.7-1
- Add native Linux packages and systemd integration.
