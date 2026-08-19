#!/usr/bin/env bash
set -Eeuo pipefail

MIN_RUST=1.91.0
MIN_BWRAP=0.10.0
STATE_FILE=/var/lib/cortexfs/install-state
TTY_PATH=/dev/tty
LANGUAGE=en
FIRST_INSTALL=0
TEMP_DIR=
TTY_ECHO_OFF=0
SECRET_VALUE=''
ROOT_TEMP_FILES=()

setup_style() {
    if [[ -t 1 && -z ${NO_COLOR:-} ]]; then
        C_INK=$'\033[38;5;255m'
        C_COAL=$'\033[38;5;244m'
        C_MINT=$'\033[38;5;121m'
        C_SIGNAL=$'\033[38;5;214m'
        C_ERROR=$'\033[38;5;203m'
        C_RESET=$'\033[0m'
    else
        C_INK=''
        C_COAL=''
        C_MINT=''
        C_SIGNAL=''
        C_ERROR=''
        C_RESET=''
    fi
}

cleanup() {
    local file
    if (( TTY_ECHO_OFF )); then
        stty echo <"$TTY_PATH" 2>/dev/null || true
        TTY_ECHO_OFF=0
    fi
    for file in "${ROOT_TEMP_FILES[@]}"; do
        sudo rm -f -- "$file" >/dev/null 2>&1 || true
    done
    [[ -z $TEMP_DIR ]] || rm -rf -- "$TEMP_DIR"
}

on_signal() {
    cleanup
    trap - EXIT
    exit 130
}
trap cleanup EXIT
trap on_signal HUP INT TERM

say() {
    local en=$1 zh=$2
    if [[ $LANGUAGE == zh ]]; then printf '%s\n' "$zh"; else printf '%s\n' "$en"; fi
}

info() {
    printf '%s◆%s ' "$C_MINT" "$C_RESET"
    say "$1" "$2"
}

warn() {
    printf '%s!%s ' "$C_SIGNAL" "$C_RESET" >&2
    if [[ $LANGUAGE == zh ]]; then printf '%s\n' "$2" >&2; else printf '%s\n' "$1" >&2; fi
}

die() {
    printf '%s×%s ' "$C_ERROR" "$C_RESET" >&2
    if [[ $LANGUAGE == zh ]]; then printf '%s\n' "$2" >&2; else printf '%s\n' "$1" >&2; fi
    exit 1
}

card() {
    printf '\n%s━━ %s%s%s\n' "$C_COAL" "$C_INK" "$1" "$C_RESET"
}

locale_language() {
    local locale=${1:-}
    locale=${locale,,}
    [[ $locale == zh* || $locale == *_zh* ]] && printf 'zh\n' || printf 'en\n'
}

version_ge() {
    local actual=${1#v} minimum=${2#v}
    awk -v actual="$actual" -v minimum="$minimum" '
        function part(value, position, pieces, count) {
            count = split(value, pieces, ".")
            if (position > count) return 0
            sub(/[^0-9].*$/, "", pieces[position])
            return pieces[position] == "" ? 0 : pieces[position] + 0
        }
        BEGIN {
            for (i = 1; i <= 4; i++) {
                a = part(actual, i)
                b = part(minimum, i)
                if (a > b) exit 0
                if (a < b) exit 1
            }
            exit 0
        }'
}

os_value() {
    local key=$1 file=$2
    awk -F= -v wanted="$key" '
        $1 == wanted {
            value = substr($0, index($0, "=") + 1)
            if (value ~ /^".*"$/ || value ~ /^\047.*\047$/)
                value = substr(value, 2, length(value) - 2)
            print tolower(value)
            exit
        }' "$file"
}

detect_distro() {
    local file=$1 available=$2 id like family manager
    [[ -r $file ]] || return 1
    id=$(os_value ID "$file")
    like=$(os_value ID_LIKE "$file")
    case " $id $like " in
        *" arch "* | *" manjaro "*) family=arch; manager=pacman ;;
        *" debian "* | *" ubuntu "*) family=debian; manager=apt-get ;;
        *" fedora "* | *" rhel "* | *" centos "* | *" rocky "* | *" almalinux "*)
            family=fedora; manager=dnf ;;
        *" opensuse "* | *" suse "* | *" sles "*) family=suse; manager=zypper ;;
        *) return 1 ;;
    esac
    [[ " $available " == *" $manager "* ]] || return 1
    printf '%s|%s|%s\n' "$family" "$manager" "${id:-unknown}"
}

platform_supported() {
    local kernel=$1 pid1=$2 systemd_runtime=$3
    [[ $kernel == Linux && $pid1 == systemd && $systemd_runtime == yes ]]
}

read_state_language() {
    local file=$1 line schema='' language='' count=0
    [[ -f $file && ! -L $file ]] || return 1
    while IFS= read -r line || [[ -n $line ]]; do
        ((++count))
        case "$line" in
            schema=1) [[ -z $schema ]] || return 1; schema=1 ;;
            language=en) [[ -z $language ]] || return 1; language=en ;;
            language=zh) [[ -z $language ]] || return 1; language=zh ;;
            *) return 1 ;;
        esac
    done <"$file"
    [[ $count -eq 2 && $schema == 1 && -n $language ]] || return 1
    printf '%s\n' "$language"
}

classify_install() {
    local marker=$1 deployment=$2
    if [[ $marker == no && $deployment == no ]]; then
        printf 'first\n'
    elif [[ $marker == yes ]]; then
        printf 'managed\n'
    else
        printf 'existing\n'
    fi
}

exact_match() {
    [[ $1 == "$2" ]]
}

confirm() {
    local token=$1 en=$2 zh=$3 answer
    say "$en" "$zh"
    if [[ ${CORTEXFS_INSTALL_LIB:-0} == 1 && ${CORTEXFS_INSTALL_TEST_MODE:-0} == 1 ]]; then
        answer=${CORTEXFS_TEST_INPUT:-}
    else
        printf '%s›%s ' "$C_SIGNAL" "$C_RESET" >"$TTY_PATH"
        IFS= read -r answer <"$TTY_PATH" ||
            die "Confirmation input closed; nothing was changed." "确认输入已关闭；未执行任何变更。"
    fi
    exact_match "$answer" "$token" ||
        die "Confirmation did not match; stopped safely." "确认指令不匹配；已安全停止。"
}

command_list() {
    local name found=
    for name in pacman apt-get dnf zypper; do
        command -v "$name" >/dev/null 2>&1 && found+=" $name"
    done
    printf '%s\n' "${found# }"
}

check_host() {
    local kernel pid1 runtime=no version_text
    kernel=$(uname -s 2>/dev/null || true)
    pid1=$(tr -d '\n' </proc/1/comm 2>/dev/null || true)
    [[ -d /run/systemd/system ]] && runtime=yes
    platform_supported "$kernel" "$pid1" "$runtime" ||
        die "CortexFS requires a Linux host booted with systemd; WSL is supported only when systemd is active." \
            "CortexFS 需要以 systemd 启动的 Linux；WSL 仅在已启用 systemd 时支持。"
    [[ ${EUID:-$(id -u)} -ne 0 ]] ||
        die "Do not run this installer as root. Use a normal user with sudo." \
            "请勿以 root 运行安装器；请使用具有 sudo 权限的普通用户。"
    [[ -r $TTY_PATH && -w $TTY_PATH ]] ||
        die "A controlling terminal is required for explicit confirmation." \
            "安装器需要控制终端以获取明确确认。"
    command -v sudo >/dev/null 2>&1 ||
        die "sudo is required." "需要 sudo。"
    version_text=$(systemctl --version 2>/dev/null || true)
    [[ $version_text == systemd* ]] ||
        die "systemctl is unavailable." "systemctl 不可用。"
}

choose_language() {
    local default answer
    default=$(locale_language "${LC_ALL:-${LC_MESSAGES:-${LANG:-}}}")
    LANGUAGE=$default
    printf '\n%sCortexFS · Linux Installer%s\n' "$C_INK" "$C_RESET"
    printf 'Language / 语言 [1 English, 2 中文] (default / 默认: %s): ' \
        "$([[ $default == zh ]] && printf '2' || printf '1')" >"$TTY_PATH"
    IFS= read -r answer <"$TTY_PATH" || answer=
    case "$answer" in
        1) LANGUAGE=en ;;
        2) LANGUAGE=zh ;;
        "") LANGUAGE=$default ;;
        *) die "Invalid language choice." "语言选择无效。" ;;
    esac
}

deployment_present() {
    [[ -e /usr/bin/ctx || -e /usr/lib/systemd/system/cortexfs.service ||
        -e /etc/systemd/system/cortexfs.service || -d /var/lib/cortexfs/storage ]]
}

resolve_language_and_install_kind() {
    local marker=no deployed=no state_kind saved
    [[ -e $STATE_FILE ]] && marker=yes
    deployment_present && deployed=yes
    state_kind=$(classify_install "$marker" "$deployed")
    if [[ $state_kind == managed ]] && saved=$(read_state_language "$STATE_FILE"); then
        LANGUAGE=$saved
    else
        LANGUAGE=$(locale_language "${LC_ALL:-${LC_MESSAGES:-${LANG:-}}}")
        if [[ $state_kind == first ]]; then
            choose_language
            FIRST_INSTALL=1
        elif [[ $state_kind == existing ]]; then
            warn "Existing CortexFS deployment detected; preserving it and treating this as an upgrade." \
                "检测到现有 CortexFS 部署；将保留数据并按升级处理。"
        else
            warn "Installer state is invalid; preserving the deployment and rebuilding only the state marker." \
                "安装器状态文件无效；将保留部署，仅重建状态标记。"
        fi
    fi
}

package_plan() {
    case "$1" in
        arch)
            PACKAGES=(base-devel curl git ca-certificates pkgconf fuse3 bubblewrap util-linux libsecret)
            ;;
        debian)
            PACKAGES=(build-essential curl git ca-certificates pkg-config fuse3 libfuse3-dev bubblewrap util-linux libsecret-tools)
            ;;
        fedora)
            PACKAGES=(gcc gcc-c++ make curl git ca-certificates pkgconf-pkg-config fuse3 fuse3-devel bubblewrap util-linux libsecret)
            ;;
        suse)
            PACKAGES=(gcc gcc-c++ make curl git ca-certificates pkg-config fuse3 fuse3-devel bubblewrap util-linux libsecret-tools)
            ;;
        *) return 1 ;;
    esac
}

install_dependencies() {
    local family=$1 manager=$2
    package_plan "$family"
    card "$( [[ $LANGUAGE == zh ]] && printf '01 · 系统依赖' || printf '01 · System dependencies' )"
    say "Package manager: $manager" "包管理器：$manager"
    say "Packages: ${PACKAGES[*]}" "软件包：${PACKAGES[*]}"
    confirm "INSTALL DEPENDENCIES" \
        "Type INSTALL DEPENDENCIES to let sudo install or refresh these packages." \
        "输入 INSTALL DEPENDENCIES，允许 sudo 安装或刷新这些软件包。"
    info "Authenticating sudo before package installation..." "将在安装软件包前验证 sudo..."
    # shellcheck disable=SC2024 # Force sudo's prompt onto the controlling terminal.
    sudo -v <"$TTY_PATH"
    case "$manager" in
        pacman) sudo pacman -S --needed --noconfirm "${PACKAGES[@]}" ;;
        apt-get)
            sudo apt-get update
            sudo apt-get install -y --no-install-recommends "${PACKAGES[@]}"
            ;;
        dnf) sudo dnf install -y "${PACKAGES[@]}" ;;
        zypper) sudo zypper --non-interactive install --no-recommends "${PACKAGES[@]}" ;;
    esac
}

runtime_paths() {
    printf '%s\n' /usr/bin/bwrap /usr/bin/setpriv /usr/bin/setsid \
        /usr/bin/systemctl /usr/bin/systemd-run /usr/bin/curl /usr/bin/env \
        /usr/bin/id /usr/bin/sh /usr/bin/findmnt /usr/bin/umount /usr/bin/install
}

require_runtime_paths() {
    local path missing=()
    while IFS= read -r path; do
        [[ -x $path ]] || missing+=("$path")
    done < <(runtime_paths)
    ((${#missing[@]} == 0)) ||
        die "Required runtime paths are missing: ${missing[*]}. Check the distribution packages above." \
            "缺少运行时必需路径：${missing[*]}。请检查上面的发行版软件包。"
}

check_bwrap() {
    local version
    version=$(/usr/bin/bwrap --version 2>/dev/null | awk '{print $NF}')
    if [[ -z $version ]] || ! version_ge "$version" "$MIN_BWRAP"; then
        die "bubblewrap $MIN_BWRAP or newer is required (found: ${version:-unknown}). Upgrade it through your distribution; /usr/bin/bwrap will not be overwritten." \
            "需要 bubblewrap $MIN_BWRAP 或更高版本（当前：${version:-未知}）。请通过发行版升级；安装器不会覆盖 /usr/bin/bwrap。"
    fi
    info "bubblewrap $version satisfies the sandbox requirement." "bubblewrap $version 满足沙箱要求。"
}

fuse_ready() {
    [[ -c /dev/fuse ]] &&
        { grep -qw fuse /proc/filesystems 2>/dev/null || [[ -d /sys/module/fuse ]]; } &&
        { command -v fusermount3 >/dev/null 2>&1 || command -v fusermount >/dev/null 2>&1; } &&
        pkg-config --exists fuse3
}

audit_fuse() {
    card "$( [[ $LANGUAGE == zh ]] && printf '02 · FUSE 审计' || printf '02 · FUSE audit' )"
    if ! grep -qw fuse /proc/filesystems 2>/dev/null && [[ ! -d /sys/module/fuse ]]; then
        command -v modprobe >/dev/null 2>&1 ||
            die "The FUSE kernel module is not active and modprobe is unavailable." \
                "FUSE 内核模块未启用，且 modprobe 不可用。"
        confirm "LOAD FUSE" \
            "The FUSE kernel module is the remaining kernel step. Type LOAD FUSE to run: sudo modprobe fuse" \
            "尚需加载 FUSE 内核模块。输入 LOAD FUSE 执行：sudo modprobe fuse"
        sudo modprobe fuse
    fi
    fuse_ready ||
        die "FUSE is not ready. Required: /dev/fuse, kernel FUSE, fusermount3 (or fusermount), and pkg-config fuse3. Check container/device permissions and your kernel." \
            "FUSE 尚未就绪。需要 /dev/fuse、内核 FUSE、fusermount3（或 fusermount）以及 pkg-config fuse3；请检查容器/设备权限和内核。"
    info "FUSE device, kernel support, helper, and development metadata are ready." \
        "FUSE 设备、内核支持、辅助程序和开发元数据均已就绪。"
}

rust_version() {
    rustc --version 2>/dev/null | awk '{print $2}'
}

ensure_rust() {
    local current installer="$TEMP_DIR/rustup-init.sh"
    card "$( [[ $LANGUAGE == zh ]] && printf '03 · Rust 工具链' || printf '03 · Rust toolchain' )"
    if command -v rustc >/dev/null 2>&1 && command -v cargo >/dev/null 2>&1; then
        current=$(rust_version)
    fi
    if [[ -n $current ]] && version_ge "$current" "$MIN_RUST"; then
        info "Rust $current satisfies MSRV $MIN_RUST." "Rust $current 满足 MSRV $MIN_RUST。"
        return
    fi
    say "Current Rust: ${current:-not installed}; required: $MIN_RUST or newer." \
        "当前 Rust：${current:-未安装}；需要 $MIN_RUST 或更高版本。"
    say "Download: https://sh.rustup.rs → $installer" \
        "下载：https://sh.rustup.rs → $installer"
    confirm "INSTALL RUST" \
        "Type INSTALL RUST to download rustup-init, then install a user-level Rust toolchain without changing shell profiles." \
        "输入 INSTALL RUST，下载 rustup-init 并安装用户级 Rust 工具链（不修改 shell 配置）。"
    /usr/bin/curl -fL --retry 3 --connect-timeout 15 -o "$installer" https://sh.rustup.rs
    [[ -s $installer ]] ||
        die "The rustup installer download is empty." "rustup 安装器下载为空。"
    sh "$installer" -y --profile minimal --default-toolchain "$MIN_RUST" --no-modify-path
    export PATH="$HOME/.cargo/bin:$PATH"
    current=$(rust_version)
    if [[ -z $current ]] || ! version_ge "$current" "$MIN_RUST"; then
        die "Rust installation completed but MSRV $MIN_RUST is still unavailable." \
            "Rust 安装已完成，但仍无法使用 MSRV $MIN_RUST。"
    fi
}

validate_source() {
    local source=$1 required
    [[ $source == /* ]] ||
        die "--source must be an absolute path." "--source 必须是绝对路径。"
    for required in Cargo.toml README.md packaging/systemd/cortexfs.service \
        packaging/systemd/cortexfs-agent@.service packaging/systemd/cortexfs-agent@.socket; do
        [[ -f $source/$required ]] ||
            die "Source snapshot is missing $required." "源码快照缺少 $required。"
    done
}

build_cortexfs() {
    local source=$1
    card "$( [[ $LANGUAGE == zh ]] && printf '04 · Release 构建' || printf '04 · Release build' )"
    say "Source: $source" "源码：$source"
    say "Command: cargo build --release --locked -p cortexfs --bins -p cortexfs-channel-tools -p cortexfs-mcp --bin ctxmcp ..." \
        "命令：cargo build --release --locked -p cortexfs --bins -p cortexfs-channel-tools -p cortexfs-mcp --bin ctxmcp …"
    confirm "BUILD CORTEXFS" \
        "Type BUILD CORTEXFS to start the release build as your current user." \
        "输入 BUILD CORTEXFS，以当前用户开始 release 构建。"
    (
        cd "$source"
        CARGO_TARGET_DIR="$source/target" \
            cargo build --release --locked -p cortexfs --bins -p cortexfs-mcp --bin ctxmcp \
                -p cortexfs-channel-tools \
                -p cortexfs-channel-nostr -p cortexfs-channel-amqp \
                -p cortexfs-channel-wecom-ws -p cortexfs-channel-wechat \
                -p cortexfs-channel-voice -p cortexfs-channel-slack \
                -p cortexfs-channel-mqtt
    )
}

expected_binaries() {
    printf '%s\n' ctx ctxterm ctxchat tsh cortexfs-mount cortexfs-object-runner \
        cortexfs-agent-runtime cortexfs-channel cortexfs-channel-tool cortexfs-channel-nostr \
        cortexfs-channel-amqp cortexfs-channel-wecom-ws cortexfs-channel-wechat \
        cortexfs-channel-voice cortexfs-channel-slack cortexfs-channel-mqtt ctxmcp
}

verify_build() {
    local source=$1 binary
    while IFS= read -r binary; do
        [[ -x $source/target/release/$binary ]] ||
            die "Expected release binary is missing: $binary" "缺少预期 release 二进制：$binary"
    done < <(expected_binaries)
}

atomic_install() {
    local source=$1 destination=$2 mode=$3 temporary
    temporary="${destination}.cortexfs-new.$$"
    if [[ -f $destination ]] && cmp -s "$source" "$destination"; then
        return
    fi
    ROOT_TEMP_FILES+=("$temporary")
    sudo install -m "$mode" "$source" "$temporary"
    sudo mv -f "$temporary" "$destination"
}

write_state() {
    local local_state="$TEMP_DIR/install-state" staged="${STATE_FILE}.cortexfs-new.$$"
    printf 'schema=1\nlanguage=%s\n' "$LANGUAGE" >"$local_state"
    ROOT_TEMP_FILES+=("$staged")
    sudo install -m 0644 "$local_state" "$staged"
    sudo mv -f "$staged" "$STATE_FILE"
}

ensure_mountpoint() {
    if findmnt -rnM /ctx >/dev/null 2>&1; then
        return
    fi
    sudo install -d -m 0755 /ctx
}

deploy() {
    local source=$1 binary unit
    card "$( [[ $LANGUAGE == zh ]] && printf '05 · 原子部署' || printf '05 · Atomic deployment' )"
    say "Binaries: /usr/bin/{ctx,ctxterm,ctxchat,tsh,cortexfs-mount,cortexfs-object-runner,cortexfs-agent-runtime,cortexfs-channel,cortexfs-channel-tool,cortexfs-channel-slack,cortexfs-channel-mqtt,ctxmcp}" \
        "二进制：/usr/bin/{ctx,ctxterm,ctxchat,tsh,cortexfs-mount,cortexfs-object-runner,cortexfs-agent-runtime,cortexfs-channel,cortexfs-channel-tool,cortexfs-channel-slack,cortexfs-channel-mqtt,ctxmcp}"
    say "Units: /usr/lib/systemd/system/cortexfs*.{service,socket}" \
        "单元：/usr/lib/systemd/system/cortexfs*.{service,socket}"
    say "Preserved: /var/lib/cortexfs/{storage,secrets}, /etc/cortexfs/providers.d, existing *.env, and /ctx user state." \
        "保留：/var/lib/cortexfs/{storage,secrets}、/etc/cortexfs/providers.d、现有 *.env 与 /ctx 用户状态。"
    confirm "DEPLOY CORTEXFS" \
        "Type DEPLOY CORTEXFS to atomically install the build and restart cortexfs.service." \
        "输入 DEPLOY CORTEXFS，原子安装构建并重启 cortexfs.service。"
    # shellcheck disable=SC2024 # Force sudo's prompt onto the controlling terminal.
    sudo -v <"$TTY_PATH"
    sudo install -d -m 0755 /usr/lib/systemd/system /usr/share/doc/cortexfs \
        /etc/cortexfs /etc/cortexfs/providers.d /var/lib/cortexfs \
        /var/lib/cortexfs/storage /var/lib/cortexfs/storage/generations
    sudo install -d -m 0700 /etc/cortexfs/channels
    sudo install -d -m 0700 /var/lib/cortexfs/secrets
    ensure_mountpoint
    while IFS= read -r binary; do
        atomic_install "$source/target/release/$binary" "/usr/bin/$binary" 0755
    done < <(expected_binaries)
    for unit in cortexfs.service cortexfs-agent@.service cortexfs-agent@.socket \
        cortexfs-channel@.service cortexfs-channel-bluesky.service \
        cortexfs-channel-driver@.service cortexfs-channel-nostr.service \
        cortexfs-channel-amqp.service cortexfs-channel-wecom-ws.service \
        cortexfs-channel-wechat.service cortexfs-channel-voice.service \
        cortexfs-channel-slack.service cortexfs-channel-mqtt.service \
        cortexfs-channel-clawdtalk.service \
        cortexfs-channel-dingtalk.service \
        cortexfs-channel-email.service cortexfs-channel-gmail.service \
        cortexfs-channel-irc.service cortexfs-channel-matrix.service \
        cortexfs-channel-mattermost.service cortexfs-channel-mochat.service \
        cortexfs-channel-notion.service \
        cortexfs-channel-qq.service \
        cortexfs-channel-reddit.service cortexfs-channel-twitch.service \
        cortexfs-channel-twitter.service; do
        atomic_install "$source/packaging/systemd/$unit" "/usr/lib/systemd/system/$unit" 0644
    done
    atomic_install "$source/README.md" /usr/share/doc/cortexfs/README.md 0644
    info "Reloading systemd and enabling the CortexFS mount..." "正在重载 systemd 并启用 CortexFS 挂载..."
    sudo systemctl daemon-reload
    sudo systemctl enable cortexfs.service
    if systemctl is-active --quiet cortexfs.service; then
        sudo systemctl restart cortexfs.service
    else
        sudo systemctl start cortexfs.service
    fi
    for unit in /etc/systemd/system/sockets.target.wants/cortexfs-agent@*.socket; do
        [[ -e $unit ]] || continue
        sudo systemctl start "${unit##*/}"
    done
    systemctl is-active --quiet cortexfs.service ||
        die "cortexfs.service did not become active. Run: sudo systemctl status cortexfs.service" \
            "cortexfs.service 未进入 active。请运行：sudo systemctl status cortexfs.service"
    findmnt -n /ctx >/dev/null ||
        die "cortexfs.service is active, but /ctx is not mounted. Inspect the service journal." \
            "cortexfs.service 已启动，但 /ctx 未挂载；请检查服务日志。"
    write_state
}

read_secret() {
    local prompt=$1 value
    SECRET_VALUE=''
    printf '%s' "$prompt" >"$TTY_PATH"
    stty -echo <"$TTY_PATH"
    TTY_ECHO_OFF=1
    IFS= read -r value <"$TTY_PATH" || {
        stty echo <"$TTY_PATH"
        TTY_ECHO_OFF=0
        printf '\n' >"$TTY_PATH"
        return 1
    }
    stty echo <"$TTY_PATH"
    TTY_ECHO_OFF=0
    printf '\n' >"$TTY_PATH"
    [[ -n $value ]] || return 1
    SECRET_VALUE=$value
}

configure_api_provider() {
    local provider=$1 label=$2
    say "Plan: sudo ctx provider preset install $provider" \
        "计划：sudo ctx provider preset install $provider"
    say "Then: hidden input → sudo ctx provider secret set $provider" \
        "然后：隐藏输入 → sudo ctx provider secret set $provider"
    confirm "CONFIGURE AI" \
        "Type CONFIGURE AI to install the selected preset and store its key." \
        "输入 CONFIGURE AI，安装所选 preset 并保存密钥。"
    # shellcheck disable=SC2024 # Force sudo's prompt onto the controlling terminal.
    sudo -v <"$TTY_PATH"
    sudo ctx provider preset install "$provider"
    read_secret "$label: " ||
        die "No API key was entered." "未输入 API Key。"
    store_api_secret "$provider" "$SECRET_VALUE"
    SECRET_VALUE=''
}

store_api_secret() {
    local provider=$1 secret=$2
    printf '%s\n' "$secret" | sudo ctx provider secret set "$provider"
}

configure_codex() {
    say "Plan: sudo ctx provider preset install codex" \
        "计划：sudo ctx provider preset install codex"
    say "Then: sudo ctx provider oauth login codex --device" \
        "然后：sudo ctx provider oauth login codex --device"
    confirm "CONFIGURE AI" \
        "Type CONFIGURE AI to install the Codex preset and start device login." \
        "输入 CONFIGURE AI，安装 Codex preset 并开始设备登录。"
    # Codex tokens must enter the root-owned system credential store used by the runtime.
    # shellcheck disable=SC2024 # Force sudo's prompt onto the controlling terminal.
    sudo -v <"$TTY_PATH"
    sudo ctx provider preset install codex
    sudo ctx provider oauth login codex --device
}

onboard_ai() {
    local choice
    (( FIRST_INSTALL )) || return
    card "$( [[ $LANGUAGE == zh ]] && printf '06 · AI 接入（可选）' || printf '06 · AI onboarding (optional)' )"
    say "Choose a vendor-neutral provider path:" "请选择 provider 接入方式："
    say "  1 OpenAI API key    2 Codex OAuth device login" \
        "  1 OpenAI API Key    2 Codex OAuth 设备登录"
    say "  3 Anthropic API key 4 Google API key    5 Later [default]" \
        "  3 Anthropic API Key 4 Google API Key    5 稍后配置 [默认]"
    printf '%s›%s ' "$C_SIGNAL" "$C_RESET" >"$TTY_PATH"
    IFS= read -r choice <"$TTY_PATH" || choice=
    case "${choice:-5}" in
        1) configure_api_provider openai "OpenAI API key" ;;
        2) configure_codex ;;
        3) configure_api_provider anthropic "Anthropic API key" ;;
        4) configure_api_provider google "Google API key" ;;
        5) info "AI onboarding skipped. Run ctx provider preset list later." \
            "已跳过 AI 接入；稍后可运行 ctx provider preset list。" ;;
        *) die "Invalid onboarding choice." "AI 接入选项无效。" ;;
    esac
    if [[ ${choice:-5} != 5 ]]; then
        sudo systemctl restart cortexfs.service
        systemctl is-active --quiet cortexfs.service ||
            die "The service failed after provider configuration." \
                "provider 配置后服务启动失败。"
    fi
}

verify_installation() {
    card "$( [[ $LANGUAGE == zh ]] && printf '07 · 验证' || printf '07 · Verification' )"
    systemctl is-active --quiet cortexfs.service ||
        die "cortexfs.service is inactive." "cortexfs.service 未运行。"
    findmnt -n /ctx >/dev/null ||
        die "/ctx is not mounted." "/ctx 未挂载。"
    info "cortexfs.service is active and /ctx is mounted." "cortexfs.service 已运行，/ctx 已挂载。"
    ctx doctor || warn "ctx doctor reported optional readiness issues; review its output." \
        "ctx doctor 报告了可选就绪项问题；请检查其输出。"
    ctx status || warn "ctx status was not ready." "ctx status 尚未就绪。"
    ctx ls || warn "ctx ls was not ready." "ctx ls 尚未就绪。"
    systemctl --user show-environment >/dev/null 2>&1 ||
        warn "Your user systemd manager is unavailable; agent terminal commands may need a login session." \
            "当前用户的 systemd manager 不可用；agent 终端命令可能需要登录会话。"
}

finish() {
    card "$( [[ $LANGUAGE == zh ]] && printf '完成' || printf 'Complete' )"
    say "CortexFS turns agents, models, and tools into inspectable paths under /ctx." \
        "CortexFS 将 agent、模型与工具投射为 /ctx 下可检查的路径。"
    say "Try: ctx status · ctx ls · ctx doctor · ctx --help" \
        "可尝试：ctx status · ctx ls · ctx doctor · ctx --help"
    say "Re-running the installer is safe: binaries and units update; data and provider state stay in place." \
        "可安全重复运行：二进制和 unit 会更新，数据与 provider 状态保持不变。"
}

main() {
    local source distro family manager id
    setup_style
    [[ $# -eq 2 && $1 == --source ]] ||
        die "Usage: install-linux.sh --source ABSOLUTE_PATH" \
            "用法：install-linux.sh --source 绝对路径"
    source=$2
    check_host
    resolve_language_and_install_kind
    validate_source "$source"
    distro=$(detect_distro /etc/os-release "$(command_list)") ||
        die "Unsupported distribution or package manager. Supported families: Arch, Debian/Ubuntu, Fedora/RHEL, openSUSE/SLES." \
            "不支持当前发行版或包管理器。支持：Arch、Debian/Ubuntu、Fedora/RHEL、openSUSE/SLES 系。"
    IFS='|' read -r family manager id <<<"$distro"
    card "$( [[ $LANGUAGE == zh ]] && printf '安装计划' || printf 'Install plan' )"
    say "Detected: $id · $manager · systemd" "检测到：$id · $manager · systemd"
    say "Flow: dependencies → FUSE audit → Rust → release build → atomic deploy → verify" \
        "流程：依赖 → FUSE 审计 → Rust → release 构建 → 原子部署 → 验证"
    TEMP_DIR=$(mktemp -d "${TMPDIR:-/tmp}/cortexfs-linux.XXXXXX")
    install_dependencies "$family" "$manager"
    require_runtime_paths
    check_bwrap
    audit_fuse
    ensure_rust
    build_cortexfs "$source"
    verify_build "$source"
    deploy "$source"
    onboard_ai
    verify_installation
    finish
}

if [[ ${CORTEXFS_INSTALL_LIB:-0} != 1 ]]; then
    main "$@"
fi
