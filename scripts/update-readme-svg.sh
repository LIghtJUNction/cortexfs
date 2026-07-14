#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
ASSETS="$ROOT/docs/assets"
mkdir -p "$ASSETS"

CTX_BIN=${CTX_BIN:-/usr/bin/ctx}
RUNS=${RUNS:-30}
BENCHMARK_SUMMARY=${BENCHMARK_SUMMARY:-}
STAMP=$(date -u '+%Y-%m-%d %H:%M UTC')
HOST=$(uname -srmo | sed 's/&/\&amp;/g; s/</\&lt;/g; s/>/\&gt;/g')
PKG=$(pacman -Q cortexfs-git 2>/dev/null || printf 'cortexfs-git not installed')
PKG=$(printf '%s' "$PKG" | sed 's/&/\&amp;/g; s/</\&lt;/g; s/>/\&gt;/g')

svg_escape() {
    sed 's/&/\&amp;/g; s/</\&lt;/g; s/>/\&gt;/g'
}

write_hero_svg() {
    cat >"$ASSETS/cortexfs-hero.svg" <<'SVG'
<svg xmlns="http://www.w3.org/2000/svg" width="1200" height="520" viewBox="0 0 1200 520" role="img" aria-labelledby="title desc">
  <title id="title">CortexFS — Agents, as files.</title>
  <desc id="desc">A warm paper editorial hero for CortexFS, showing three executable object classes under /ctx: model, agent, and tool. Durable sessions are stored under user agent homes.</desc>
  <defs>
    <style>
      .serif{font-family:Georgia,"Times New Roman",serif}.sans{font-family:Inter,ui-sans-serif,system-ui,-apple-system,"Segoe UI",sans-serif}.mono{font-family:ui-monospace,SFMono-Regular,Menlo,Consolas,monospace}.ink{fill:#111312}.slate{fill:#66716c}.mint{fill:#2a8f73}.paper{fill:#fffdfa}.coal-muted{fill:#fffdfa;fill-opacity:.68}.line{stroke:#ded8ca}
    </style>
  </defs>

  <rect width="1200" height="520" fill="#f7f5f1"/>
  <path d="M56 48H1144M56 472H1144" class="line" stroke-width="1"/>

  <g transform="translate(72 72)">
    <g aria-label="CortexFS">
      <rect width="52" height="52" rx="6" fill="#fff" stroke="#ded8ca"/>
      <path class="ink" d="M12 15h15l4 5h10v11H12V15Zm0 21h11v11H12V36Zm24 2h11v5h-3v4h-5v-4h-3v-5Z"/>
      <path class="mint" d="M31 9h6L22 43h-6L31 9Z"/>
      <text class="serif ink" x="68" y="38" font-size="34" font-weight="700">Cor</text>
      <text class="serif mint" x="126" y="38" font-size="34" font-weight="700" font-style="italic">TeX</text>
      <text class="serif ink" x="184" y="38" font-size="34" font-weight="700">fs</text>
    </g>

    <text class="sans mint" y="120" font-size="12" font-weight="900" letter-spacing="2.4">FUSE FILESYSTEM INTERFACE</text>
    <text class="serif ink" y="193" font-size="68" font-weight="600">Agents, as files.</text>
    <text class="sans slate" y="239" font-size="17">A small Unix ABI for durable agent runtimes.</text>

    <g transform="translate(0 291)">
      <path d="M0 0H428" class="line"/>
      <text class="mono ink" y="32" font-size="13" font-weight="800">write request</text>
      <text class="mono mint" x="124" y="32" font-size="13" font-weight="900">→</text>
      <text class="mono ink" x="154" y="32" font-size="13" font-weight="800">atomic rename</text>
      <text class="mono mint" x="285" y="32" font-size="13" font-weight="900">→</text>
      <text class="mono ink" x="315" y="32" font-size="13" font-weight="800">read result</text>
    </g>
  </g>

  <g transform="translate(654 64)">
    <rect width="474" height="392" rx="8" fill="#181b19"/>
    <path d="M0 56H474" stroke="#fffdfa" stroke-opacity=".12"/>
    <circle cx="28" cy="28" r="4" fill="#d8ff66"/>
    <text class="mono paper" x="46" y="33" font-size="13" font-weight="850">/ctx</text>
    <text class="mono coal-muted" x="390" y="33" font-size="12">mounted</text>

    <g class="mono" font-size="14" font-weight="800">
      <text class="paper" x="34" y="92">/ctx</text>
      <text class="coal-muted" x="34" y="121">├── <tspan class="paper">status</tspan></text>
      <text class="coal-muted" x="34" y="150">├── <tspan class="paper">bin/</tspan></text>
      <text class="coal-muted" x="34" y="179">├── <tspan class="paper">model/</tspan></text>
      <text class="coal-muted" x="34" y="208">├── <tspan class="paper">agent/</tspan></text>
      <text class="coal-muted" x="34" y="237">├── <tspan class="paper">tool/</tspan></text>
      <text class="coal-muted" x="34" y="266">├── <tspan class="paper">home/</tspan></text>
      <text class="coal-muted" x="34" y="295">└── <tspan class="paper">shared/</tspan></text>
    </g>

    <path d="M270 91V290" stroke="#fffdfa" stroke-opacity=".12"/>
    <text class="sans coal-muted" x="304" y="109" font-size="12" font-weight="900" letter-spacing="1.6">UNIX-NATIVE</text>
    <text class="serif paper" x="304" y="158" font-size="26" font-weight="600">Inspect.</text>
    <text class="serif paper" x="304" y="194" font-size="26" font-weight="600">Script.</text>
    <text class="serif paper" x="304" y="230" font-size="26" font-weight="600">Compose.</text>

    <path d="M34 320H440" stroke="#fffdfa" stroke-opacity=".12"/>
    <text class="mono coal-muted" x="34" y="355" font-size="12">$</text>
    <text class="mono paper" x="54" y="355" font-size="12" font-weight="800">ls /ctx/agent</text>
    <rect x="170" y="346" width="7" height="12" fill="#d8ff66"/>
  </g>
</svg>
SVG
}

write_abi_svg() {
    cat >"$ASSETS/cortexfs-abi-map.svg" <<'SVG'
<svg xmlns="http://www.w3.org/2000/svg" width="1200" height="960" viewBox="0 0 1200 960" role="img" aria-labelledby="title desc">
  <title id="title">CortexFS v1 ABI and durable session map</title>
  <desc id="desc">The frozen /ctx root, three executable object classes, four default agents, and session durability: raw messages and events, rebuildable context, and archive-first garbage collection.</desc>
  <defs><filter id="shadow" x="-15%" y="-15%" width="130%" height="130%"><feDropShadow dx="0" dy="18" stdDeviation="20" flood-color="#1e180e" flood-opacity=".12"/></filter><marker id="arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse"><path d="M0 0 L10 5 L0 10 z" fill="#8a948f"/></marker><style>.serif{font-family:Georgia,"Times New Roman",serif}.sans{font-family:Inter,ui-sans-serif,system-ui,-apple-system,"Segoe UI",sans-serif}.mono{font-family:ui-monospace,SFMono-Regular,Menlo,Consolas,monospace}.bg{fill:#f7f5f1}.panel{fill:#fffdfa;stroke:#ded8ca;stroke-width:1.4}.ink{fill:#111312}.muted{fill:#66716c}.mint{fill:#2a8f73}.lime{fill:#d8ff66}.coal{fill:#181b19}.line{fill:none;stroke:#8a948f;stroke-width:2.2;stroke-linecap:round;stroke-linejoin:round;marker-end:url(#arrow)}</style></defs>
  <rect class="bg" width="1200" height="960"/><g transform="translate(58 50)"><text class="serif ink" x="0" y="0" font-size="42" font-weight="650">CortexFS v1 ABI</text><text class="sans muted" x="0" y="36" font-size="19" font-weight="600">Frozen root, executable objects, durable sessions.</text></g>
  <g transform="translate(58 126)" filter="url(#shadow)"><rect class="coal" width="300" height="472" rx="8"/><text class="mono lime" x="26" y="44" font-size="18" font-weight="950">/ctx</text><line x1="34" y1="66" x2="34" y2="398" stroke="#d8ff66" stroke-opacity=".65" stroke-width="2"/><g class="mono" font-size="18" font-weight="850"><rect x="66" y="80" width="186" height="38" rx="6" fill="#fffdfa" fill-opacity=".07" stroke="#fffdfa" stroke-opacity=".11"/><text fill="#fffdfa" x="86" y="105">status</text><rect x="66" y="130" width="186" height="38" rx="6" fill="#fffdfa" fill-opacity=".07" stroke="#fffdfa" stroke-opacity=".11"/><text fill="#fffdfa" x="86" y="155">bin/</text><rect x="66" y="180" width="186" height="38" rx="6" fill="#d8ff66" fill-opacity=".12" stroke="#d8ff66" stroke-opacity=".28"/><text fill="#fffdfa" x="86" y="205">model/</text><rect x="66" y="230" width="186" height="38" rx="6" fill="#d8ff66" fill-opacity=".12" stroke="#d8ff66" stroke-opacity=".28"/><text fill="#fffdfa" x="86" y="255">agent/</text><rect x="66" y="280" width="186" height="38" rx="6" fill="#d8ff66" fill-opacity=".12" stroke="#d8ff66" stroke-opacity=".28"/><text fill="#fffdfa" x="86" y="305">tool/</text><rect x="66" y="330" width="186" height="38" rx="6" fill="#fffdfa" fill-opacity=".07" stroke="#fffdfa" stroke-opacity=".11"/><text fill="#fffdfa" x="86" y="355">home/</text><rect x="66" y="380" width="186" height="38" rx="6" fill="#fffdfa" fill-opacity=".07" stroke="#fffdfa" stroke-opacity=".11"/><text fill="#fffdfa" x="86" y="405">shared/</text></g></g>
  <g transform="translate(438 126)"><rect class="panel" width="296" height="126" rx="8"/><text class="sans mint" x="24" y="38" font-size="13" font-weight="950" letter-spacing="2">OBJECT CLASS</text><text class="serif ink" x="24" y="76" font-size="34" font-weight="650">model</text><text class="sans muted" x="24" y="104" font-size="17" font-weight="600">pure inference endpoint</text><rect class="panel" y="162" width="296" height="126" rx="8"/><text class="sans mint" x="24" y="200" font-size="13" font-weight="950" letter-spacing="2">OBJECT CLASS</text><text class="serif ink" x="24" y="238" font-size="34" font-weight="650">agent</text><text class="sans muted" x="24" y="266" font-size="17" font-weight="600">policy-bound runtime</text><rect class="panel" y="324" width="296" height="126" rx="8"/><text class="sans mint" x="24" y="362" font-size="13" font-weight="950" letter-spacing="2">OBJECT CLASS</text><text class="serif ink" x="24" y="400" font-size="34" font-weight="650">tool</text><text class="sans muted" x="24" y="428" font-size="17" font-weight="600">executable capability</text></g>
  <path class="line" d="M358 325 C392 325 400 190 438 190"/><path class="line" d="M358 325 C396 325 400 352 438 352"/><path class="line" d="M358 325 C392 325 400 514 438 514"/>
  <g transform="translate(816 126)" filter="url(#shadow)"><rect class="panel" width="316" height="472" rx="8"/><text class="sans mint" x="26" y="46" font-size="13" font-weight="950" letter-spacing="2">EVERY OBJECT CAN EXPOSE</text><g class="mono" font-size="18" font-weight="850"><text class="ink" x="26" y="104">name</text><text class="muted" x="26" y="134">exec or metadata endpoint</text><line x1="26" y1="164" x2="290" y2="164" stroke="#ded8ca"/><text class="ink" x="26" y="214">name.sock</text><text class="muted" x="26" y="244">stateful JSONL stream</text><line x1="26" y1="274" x2="290" y2="274" stroke="#ded8ca"/><text class="ink" x="26" y="324">name.d/</text><text class="muted" x="26" y="354">small control files</text></g><rect x="26" y="398" width="264" height="44" rx="6" fill="#181b19"/><text class="mono lime" x="44" y="426" font-size="15" font-weight="900">*.req.json -> outbox</text></g>
  <g transform="translate(58 620)"><rect class="panel" width="1074" height="104" rx="8"/><text class="sans mint" x="20" y="28" font-size="12" font-weight="950" letter-spacing="2">FOUR DEFAULT AGENTS</text><g transform="translate(20 42)"><rect width="240" height="46" rx="6" fill="#181b19"/><text class="mono lime" x="14" y="29" font-size="14" font-weight="900">architect</text><text class="sans" x="104" y="29" font-size="12" font-weight="700" fill="#bfb6a8">plan + coordinate</text><rect x="258" width="240" height="46" rx="6" fill="#181b19"/><text class="mono lime" x="272" y="29" font-size="14" font-weight="900">coder</text><text class="sans" x="332" y="29" font-size="12" font-weight="700" fill="#bfb6a8">primary implementation</text><rect x="516" width="240" height="46" rx="6" fill="#181b19"/><text class="mono lime" x="530" y="29" font-size="14" font-weight="900">worker</text><text class="sans" x="596" y="29" font-size="12" font-weight="700" fill="#bfb6a8">bounded Spark tasks</text><rect x="774" width="240" height="46" rx="6" fill="#181b19"/><text class="mono lime" x="788" y="29" font-size="14" font-weight="900">reviewer</text><text class="sans" x="862" y="29" font-size="12" font-weight="700" fill="#bfb6a8">verify + guard</text></g></g>
  <g transform="translate(58 742)"><rect class="coal" width="1074" height="132" rx="8"/><text class="sans lime" x="20" y="29" font-size="12" font-weight="950" letter-spacing="2">SESSION DURABILITY</text><g transform="translate(20 46)"><rect width="330" height="66" rx="6" fill="#fffdfa" fill-opacity=".07" stroke="#fffdfa" stroke-opacity=".12"/><text class="mono lime" x="18" y="28" font-size="15" font-weight="900">messages + events</text><text class="sans" x="18" y="51" font-size="14" font-weight="700" fill="#bfb6a8">raw history</text><rect x="342" width="330" height="66" rx="6" fill="#fffdfa" fill-opacity=".07" stroke="#fffdfa" stroke-opacity=".12"/><text class="mono lime" x="360" y="28" font-size="15" font-weight="900">context</text><text class="sans" x="360" y="51" font-size="14" font-weight="700" fill="#bfb6a8">rebuildable</text><rect x="684" width="330" height="66" rx="6" fill="#fffdfa" fill-opacity=".07" stroke="#fffdfa" stroke-opacity=".12"/><text class="mono lime" x="702" y="28" font-size="15" font-weight="900">gc</text><text class="sans" x="702" y="51" font-size="14" font-weight="700" fill="#bfb6a8">archive first</text></g></g>
  <g transform="translate(58 892)"><rect width="1074" height="44" rx="8" fill="#181b19"/><text class="sans" x="22" y="29" font-size="16" font-weight="850" fill="#fffdfa">Boundary:</text><text class="sans" x="112" y="29" font-size="16" font-weight="650" fill="#bfb6a8">provider, workflow, MCP, vector, hook, job, and audit internals are not stable root ABI.</text></g>
</svg>
SVG
}

run_ms() {
    local label=$1
    shift
    local i elapsed start end status sample_label
    for ((i = 0; i < RUNS; i++)); do
        start=$(date +%s%N)
        if "$@" >/dev/null 2>&1; then
            status=0
        else
            status=$?
        fi
        end=$(date +%s%N)
        elapsed=$(((end - start) / 1000000))
        sample_label=$label
        if ((status != 0)); then
            sample_label="$label [exit $status]"
        fi
        printf '%s\t%s\n' "$sample_label" "$elapsed"
    done
}

service_snapshot() {
    local pid rss hwm threads cpu current peak
    pid=$(systemctl show cortexfs.service -p MainPID --value 2>/dev/null || true)
    [[ -n "$pid" ]] || pid="n/a"
    rss="n/a"
    hwm="n/a"
    threads="n/a"
    cpu="n/a"
    if [[ "$pid" =~ ^[1-9][0-9]*$ && -r "/proc/$pid/status" ]]; then
        rss=$(awk '/^VmRSS:/ {printf "%.1f MiB", $2 / 1024}' "/proc/$pid/status")
        hwm=$(awk '/^VmHWM:/ {printf "%.1f MiB", $2 / 1024}' "/proc/$pid/status")
        threads=$(awk '/^Threads:/ {print $2}' "/proc/$pid/status")
        cpu=$(ps -p "$pid" -o %cpu= 2>/dev/null | awk '{$1=$1; if ($1 != "") print $1 "%"}' || true)
        [[ -n "$cpu" ]] || cpu="n/a"
    fi
    current=$(systemctl show cortexfs.service -p MemoryCurrent --value 2>/dev/null || true)
    peak=$(systemctl show cortexfs.service -p MemoryPeak --value 2>/dev/null || true)
    if [[ "$current" =~ ^[0-9]+$ ]]; then
        current=$(awk -v bytes="$current" 'BEGIN {printf "%.1f MiB", bytes / 1048576}')
    else
        current="n/a"
    fi
    if [[ "$peak" =~ ^[0-9]+$ ]]; then
        peak=$(awk -v bytes="$peak" 'BEGIN {printf "%.1f MiB", bytes / 1048576}')
    else
        peak="n/a"
    fi
    printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\n' "$pid" "$rss" "$hwm" "$threads" "$current" "$peak" "$cpu"
}

write_bench_svg() {
    local tmp summary mount_line service_pid rss hwm threads memory_current memory_peak cpu
    local benchmark_status runtime_success exact_accuracy latency_p50 latency_p95 ttft_p50 ttft_p95
    local requests runtime_successes exact_matches token_samples archived cleanup_total dataset_samples repeat roles route run_id
    local success_count exact_count archive_count token_count provenance
    tmp=$(mktemp)
    IFS=$'\t' read -r service_pid rss hwm threads memory_current memory_peak cpu < <(service_snapshot)
    if findmnt /ctx >/dev/null 2>&1 && command -v "$CTX_BIN" >/dev/null 2>&1; then
        {
            run_ms "ctx status" "$CTX_BIN" status
            run_ms "ctx ls" "$CTX_BIN" ls
            run_ms "ctx ls tool" "$CTX_BIN" ls tool
            run_ms "cat /ctx/status" cat /ctx/status
            run_ms "ctx doctor" "$CTX_BIN" doctor
        } >>"$tmp"
    fi

    if [[ -s "$tmp" ]]; then
        summary=$(awk -F '\t' '
            { key=$1; value=$2; count[key]++; sum[key]+=value; if (!(key in min) || value<min[key]) min[key]=value; if (value>max[key]) max[key]=value; order[++n]=key }
            END { for (i=1; i<=n; i++) if (!seen[order[i]]++) { key=order[i]; mean=sum[key]/count[key]; printf "%s\t%.2f\t%d\t%d\t%d\n", key, mean, min[key], max[key], count[key]; } }
        ' "$tmp")
    else
        summary=$'runtime probes unavailable\t0\t0\t0\t0'
    fi

    mount_line=$(findmnt -no TARGET,SOURCE,FSTYPE,OPTIONS /ctx 2>/dev/null || true)
    if [[ "$mount_line" == *" /ctx "* ]]; then
        mount_line=${mount_line#* /ctx }
    fi
    [[ -n "$mount_line" ]] || mount_line="/ctx not mounted"
    mount_line=$(printf '%s' "$mount_line" | svg_escape)

    benchmark_status="No benchmark summary supplied"
    runtime_success="n/a"
    exact_accuracy="n/a"
    latency_p50="n/a"
    latency_p95="n/a"
    ttft_p50="n/a"
    ttft_p95="n/a"
    requests="n/a"
    runtime_successes="n/a"
    exact_matches="n/a"
    token_samples="n/a"
    archived="n/a"
    cleanup_total="n/a"
    dataset_samples="n/a"
    repeat="n/a"
    roles="n/a"
    route="n/a"
    run_id="n/a"
    provenance="not supplied"
    if [[ -n "$BENCHMARK_SUMMARY" && -r "$BENCHMARK_SUMMARY" ]] && command -v jq >/dev/null 2>&1; then
        if jq -e '. as $r |
            .schema == "cortexfs.readme-benchmark/v1" and
            (.run_id | type == "string" and length > 0) and
            (.timestamp | type == "string" and length > 0) and
            (.runtime.commit | type == "string" and length > 0) and
            (.runtime.package | type == "string" and length > 0) and
            (.runtime.model_routes | type == "object" and length > 0 and all(.[]; type == "string" and length > 0)) and
            (.dataset.ids | type == "array" and length > 0 and all(.[]; type == "string" and length > 0)) and
            (.dataset.sha256 | type == "string" and test("^[0-9a-f]{64}$")) and
            (.dataset.samples | type == "number" and floor == . and . > 0 and . == ($r.dataset.ids | length)) and
            (.agents | type == "array" and length > 0 and all(.[]; type == "string" and length > 0)) and
            (.repeat | type == "number" and floor == . and . > 0) and
            (.timeout_seconds | type == "number" and . > 0) and
            (.overall.requests | type == "number" and floor == . and . > 0) and
            (.overall.runtime_successes | type == "number" and floor == . and . >= 0 and . <= $r.overall.requests) and
            (.overall.runtime_success_rate | type == "number" and . >= 0 and . <= 1 and . == ($r.overall.runtime_successes / $r.overall.requests)) and
            (.overall.exact_matches | type == "number" and floor == . and . >= 0 and . <= $r.overall.requests) and
            (.overall.exact_accuracy | type == "number" and . >= 0 and . <= 1 and . == ($r.overall.exact_matches / $r.overall.requests)) and
            (.overall.latency_ms.p50 | type == "number" and . >= 0) and
            (.overall.latency_ms.p95 | type == "number" and . >= $r.overall.latency_ms.p50) and
            (.overall.ttft_ms.p50 | type == "number" and . >= 0) and
            (.overall.ttft_ms.p95 | type == "number" and . >= $r.overall.ttft_ms.p50) and
            (.overall.latency_samples | type == "number" and floor == . and . >= 0 and . <= $r.overall.requests) and
            (.overall.ttft_samples | type == "number" and floor == . and . >= 0 and . <= $r.overall.requests) and
            (.overall.token_samples | type == "number" and floor == . and . >= 0 and . <= $r.overall.requests) and
            (.overall.errors | type == "object") and
            (.cleanup.archived | type == "number" and floor == . and . >= 0) and
            (.cleanup.total | type == "number" and floor == . and . >= $r.cleanup.archived) and
            (.preflight.status_ok | type == "boolean") and
            (.preflight.doctor_ok | type == "boolean") and
            (.preflight.fuse.target | type == "string") and
            (.preflight.fuse.source | type == "string") and
            (.preflight.fuse.fstype | type == "string") and
            (.preflight.fuse.default_permissions | type == "boolean") and
            (.preflight.fuse.allow_other | type == "boolean") and
            (.preflight.pong_agents | type == "array" and all(.[]; type == "string" and length > 0)) and
            ($r.overall.requests == (($r.dataset.samples) * ($r.repeat) * ($r.agents | length))) and
            ($r.cleanup.total == $r.overall.requests)
        ' "$BENCHMARK_SUMMARY" >/dev/null 2>&1; then
            benchmark_status="Inspect AI agent benchmark"
            runtime_success=$(jq -r '(.overall.runtime_success_rate * 100 | tostring) + "%"' "$BENCHMARK_SUMMARY")
            exact_accuracy=$(jq -r '(.overall.exact_accuracy * 100 | tostring) + "%"' "$BENCHMARK_SUMMARY")
            latency_p50=$(jq -r '.overall.latency_ms.p50 | @text' "$BENCHMARK_SUMMARY")
            latency_p95=$(jq -r '.overall.latency_ms.p95 | @text' "$BENCHMARK_SUMMARY")
            ttft_p50=$(jq -r '.overall.ttft_ms.p50 | @text' "$BENCHMARK_SUMMARY")
            ttft_p95=$(jq -r '.overall.ttft_ms.p95 | @text' "$BENCHMARK_SUMMARY")
            requests=$(jq -r '.overall.requests' "$BENCHMARK_SUMMARY")
            runtime_successes=$(jq -r '.overall.runtime_successes' "$BENCHMARK_SUMMARY")
            exact_matches=$(jq -r '.overall.exact_matches' "$BENCHMARK_SUMMARY")
            token_samples=$(jq -r '.overall.token_samples' "$BENCHMARK_SUMMARY")
            archived=$(jq -r '.cleanup.archived' "$BENCHMARK_SUMMARY")
            cleanup_total=$(jq -r '.cleanup.total' "$BENCHMARK_SUMMARY")
            dataset_samples=$(jq -r '.dataset.samples' "$BENCHMARK_SUMMARY")
            repeat=$(jq -r '.repeat' "$BENCHMARK_SUMMARY")
            roles=$(jq -r '.agents | length' "$BENCHMARK_SUMMARY")
            route=$(jq -r '[.runtime.model_routes[]] | unique | if length == 1 then .[0] else "mixed" end' "$BENCHMARK_SUMMARY")
            run_id=$(jq -r '.run_id' "$BENCHMARK_SUMMARY")
            provenance=${BENCHMARK_SUMMARY#"$ROOT/"}
            latency_p50=$(awk -v value="$latency_p50" 'BEGIN {printf "%.2f ms", value}')
            latency_p95=$(awk -v value="$latency_p95" 'BEGIN {printf "%.2f ms", value}')
            ttft_p50=$(awk -v value="$ttft_p50" 'BEGIN {printf "%.2f ms", value}')
            ttft_p95=$(awk -v value="$ttft_p95" 'BEGIN {printf "%.2f ms", value}')
        else
            benchmark_status="Benchmark summary invalid"
        fi
    elif [[ -n "$BENCHMARK_SUMMARY" ]]; then
        benchmark_status="Benchmark summary unavailable"
    fi

    success_count="n/a"
    exact_count="n/a"
    archive_count="n/a"
    token_count="n/a"
    if [[ "$requests" =~ ^[0-9]+$ ]]; then
        success_count="$runtime_successes/$requests"
        exact_count="$exact_matches/$requests"
        token_count="$token_samples/$requests"
    fi
    if [[ "$archived" =~ ^[0-9]+$ && "$cleanup_total" =~ ^[0-9]+$ ]]; then
        archive_count="$archived/$cleanup_total"
    fi
    benchmark_status=$(printf '%s' "$benchmark_status" | svg_escape)
    route=$(printf '%s' "$route" | svg_escape)
    run_id=$(printf '%s' "$run_id" | svg_escape)
    provenance=$(printf '%s' "$provenance" | svg_escape)

    {
        cat <<SVG
<svg xmlns="http://www.w3.org/2000/svg" width="1200" height="760" viewBox="0 0 1200 760" role="img" aria-labelledby="title desc">
  <title id="title">CortexFS measured runtime</title>
  <desc id="desc">A live FUSE service snapshot, Inspect AI agent benchmark results, and local CLI probe latency.</desc>
  <defs><style>.bg{fill:#111211}.panel{fill:#181b19;stroke:#fffdfa;stroke-opacity:.12;stroke-width:1.2}.title{font:650 38px Georgia,"Times New Roman",serif;fill:#fffdfa}.label{font:900 12px Inter,ui-sans-serif,system-ui,sans-serif;fill:#2a8f73;letter-spacing:2px}.text{font:650 16px Inter,ui-sans-serif,system-ui,sans-serif;fill:#bfb6a8}.value{font:700 29px Georgia,"Times New Roman",serif;fill:#fffdfa}.mono{font:850 15px ui-monospace,SFMono-Regular,Menlo,Consolas,monospace;fill:#fffdfa}.small{font:650 13px Inter,ui-sans-serif,system-ui,sans-serif;fill:#8a948f}.lime{fill:#d8ff66}.mint{fill:#2a8f73}.line{stroke:#fffdfa;stroke-opacity:.1}</style></defs>
  <rect class="bg" width="1200" height="760"/>
  <text class="title" x="56" y="60">CortexFS runtime, measured in place</text>
  <text class="text" x="56" y="92">Service values are a live snapshot. Agent values are one recorded benchmark run. No cross-tool speed claim.</text>
  <rect x="56" y="112" width="1088" height="38" rx="6" fill="#0f1110" stroke="#fffdfa" stroke-opacity=".12"/>
  <text class="mono" x="76" y="137">$mount_line</text>
  <rect class="panel" x="56" y="170" width="520" height="240" rx="8"/>
  <text class="label" x="80" y="202">LIVE FUSE SERVICE SNAPSHOT</text>
  <text class="value" x="80" y="252">$rss</text><text class="small" x="80" y="274">mount process RSS</text>
  <text class="value" x="270" y="252">$hwm</text><text class="small" x="270" y="274">process RSS high-water</text>
  <text class="value" x="80" y="326">$memory_current</text><text class="small" x="80" y="348">systemd cgroup current</text>
  <text class="value" x="270" y="326">$memory_peak</text><text class="small" x="270" y="348">systemd cgroup peak</text>
  <line class="line" x1="80" y1="368" x2="552" y2="368"/>
  <text class="mono" x="80" y="394">threads $threads · lifetime CPU $cpu · PID $service_pid</text>
  <rect class="panel" x="596" y="170" width="548" height="240" rx="8"/>
  <text class="label" x="620" y="202">$benchmark_status · LOWER LATENCY IS BETTER</text>
  <text class="small" x="620" y="224">runtime success</text><text class="value" x="620" y="258">$runtime_success</text><text class="small" x="710" y="256">$success_count · higher is better</text>
  <text class="small" x="885" y="224">exact accuracy</text><text class="value" x="885" y="258">$exact_accuracy</text><text class="small" x="955" y="256">$exact_count · higher is better</text>
  <text class="mono" x="620" y="316">latency  p50 $latency_p50 · p95 $latency_p95</text>
  <text class="mono" x="620" y="346">TTFT     p50 $ttft_p50 · p95 $ttft_p95</text>
  <line class="line" x1="620" y1="368" x2="1120" y2="368"/>
  <text class="small" x="620" y="392">route $route · dataset $dataset_samples · roles $roles · repeat $repeat · archived $archive_count · token samples $token_count</text>
  <rect class="panel" x="56" y="430" width="1088" height="228" rx="8"/>
  <text class="label" x="80" y="462">LOCAL CLI PROBES · MEAN WALL TIME · LOWER IS BETTER · N=$RUNS EACH</text>
SVG
        local row=0
        while IFS=$'\t' read -r label mean min max count; do
            local y escaped
            y=$((492 + row * 31))
            escaped=$(printf '%s' "$label" | svg_escape)
            cat <<SVG
  <text class="mono" x="80" y="$y">$escaped</text>
  <text class="mono lime" x="340" y="$y">${mean} ms mean</text>
  <text class="small" x="540" y="$y">min ${min} ms · max ${max} ms · n=$count</text>
SVG
            row=$((row + 1))
        done <<<"$summary"
        cat <<SVG
  <rect x="56" y="680" width="1088" height="42" rx="6" fill="#0f1110" stroke="#fffdfa" stroke-opacity=".12"/>
  <text class="small" x="76" y="702">Updated $STAMP · $HOST · $PKG · benchmark $run_id</text>
  <text class="small" x="76" y="716">provenance $provenance</text>
</svg>
SVG
    } >"$ASSETS/cortexfs-performance.svg"
    rm -f "$tmp"
}

write_hero_svg
write_abi_svg
write_bench_svg
printf 'updated %s\n' "$ASSETS/cortexfs-hero.svg"
printf 'updated %s\n' "$ASSETS/cortexfs-abi-map.svg"
printf 'updated %s\n' "$ASSETS/cortexfs-performance.svg"
