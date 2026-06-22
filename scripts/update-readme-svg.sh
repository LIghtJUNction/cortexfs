#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
ASSETS="$ROOT/docs/assets"
mkdir -p "$ASSETS"

CTX_BIN=${CTX_BIN:-/usr/bin/ctx}
RUNS=${RUNS:-30}
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
  <title id="title">CortexFS turns AI runtimes into Unix-shaped files</title>
  <desc id="desc">A concise visual showing /ctx with model, agent, tool, home, and shared entries.</desc>
  <defs>
    <linearGradient id="bg" x1="0" x2="1" y1="0" y2="1">
      <stop offset="0" stop-color="#0b1320"/>
      <stop offset="0.55" stop-color="#10251f"/>
      <stop offset="1" stop-color="#111827"/>
    </linearGradient>
    <linearGradient id="accent" x1="0" x2="1">
      <stop offset="0" stop-color="#58d68d"/>
      <stop offset="1" stop-color="#38bdf8"/>
    </linearGradient>
    <filter id="soft" x="-20%" y="-20%" width="140%" height="140%">
      <feDropShadow dx="0" dy="16" stdDeviation="18" flood-color="#000" flood-opacity="0.32"/>
    </filter>
    <style>
      .title{font:700 58px system-ui,-apple-system,Segoe UI,sans-serif;fill:#f8fafc;letter-spacing:0}
      .sub{font:500 24px system-ui,-apple-system,Segoe UI,sans-serif;fill:#cbd5e1;letter-spacing:0}
      .mono{font:600 24px ui-monospace,SFMono-Regular,Menlo,Consolas,monospace;fill:#e5f7ef}
      .small{font:500 18px system-ui,-apple-system,Segoe UI,sans-serif;fill:#cbd5e1}
      .tag{font:700 16px system-ui,-apple-system,Segoe UI,sans-serif;fill:#07111f}
      .node{fill:#142033;stroke:#2d405f;stroke-width:1.4}
      .hot{fill:#102820;stroke:#58d68d;stroke-width:1.7}
      .line{stroke:#58d68d;stroke-width:3;stroke-linecap:round;opacity:.9}
      .muted{fill:#8aa0b8}
    </style>
  </defs>
  <rect width="1200" height="520" fill="url(#bg)"/>
  <path d="M0 436 C188 388 280 454 438 408 C612 357 710 389 851 335 C1005 277 1094 310 1200 248 L1200 520 L0 520 Z" fill="#0f1d2c" opacity=".9"/>
  <g transform="translate(70 66)">
    <text class="title" x="0" y="0">AI, exposed as files.</text>
    <text class="sub" x="0" y="48">No provider tree. No workflow DSL. Just a small Linux ABI at /ctx.</text>
    <g transform="translate(0 96)">
      <rect width="178" height="40" rx="6" fill="url(#accent)"/>
      <text class="tag" x="18" y="26">model as file</text>
      <rect x="196" width="178" height="40" rx="6" fill="#eab308"/>
      <text class="tag" x="216" y="26">agent as file</text>
      <rect x="392" width="164" height="40" rx="6" fill="#f97316"/>
      <text class="tag" x="412" y="26">tool as file</text>
    </g>
  </g>
  <g transform="translate(666 74)" filter="url(#soft)">
    <rect x="0" y="0" width="438" height="344" rx="8" fill="#0b1220" stroke="#2d405f"/>
    <text class="mono" x="30" y="50">/ctx</text>
    <line class="line" x1="55" y1="68" x2="55" y2="294"/>
    <g transform="translate(86 86)">
      <rect class="node" width="270" height="40" rx="6"/><text class="mono" x="18" y="27">status</text>
      <rect class="node" y="48" width="270" height="40" rx="6"/><text class="mono" x="18" y="75">bin/</text>
      <rect class="hot" y="96" width="270" height="40" rx="6"/><text class="mono" x="18" y="123">model/qwen</text>
      <rect class="hot" y="144" width="270" height="40" rx="6"/><text class="mono" x="18" y="171">agent/coder.sock</text>
      <rect class="hot" y="192" width="270" height="40" rx="6"/><text class="mono" x="18" y="219">tool/fs.read</text>
      <rect class="node" y="240" width="270" height="40" rx="6"/><text class="mono" x="18" y="267">home/ shared/</text>
    </g>
  </g>
  <g transform="translate(72 358)">
    <text class="small" x="0" y="0">Install, mount, inspect:</text>
    <rect x="0" y="18" width="488" height="54" rx="8" fill="#08111f" stroke="#26354f"/>
    <text class="mono" x="20" y="53">ctx mount  &amp;&amp;  ctx doctor</text>
  </g>
</svg>
SVG
}

write_abi_svg() {
    cat >"$ASSETS/cortexfs-abi-map.svg" <<'SVG'
<svg xmlns="http://www.w3.org/2000/svg" width="1200" height="720" viewBox="0 0 1200 720" role="img" aria-labelledby="title desc">
  <title id="title">CortexFS v1 ABI map</title>
  <desc id="desc">The stable /ctx root, object triple, socket traffic, control files, and durable sessions.</desc>
  <defs>
    <style>
      .bg{fill:#f8fafc}.ink{fill:#0f172a}.muted{fill:#475569}.label{font:700 22px system-ui,-apple-system,Segoe UI,sans-serif;letter-spacing:0}.text{font:500 18px system-ui,-apple-system,Segoe UI,sans-serif}.mono{font:600 17px ui-monospace,SFMono-Regular,Menlo,Consolas,monospace}.card{fill:#fff;stroke:#cbd5e1;stroke-width:1.4}.green{fill:#ecfdf5;stroke:#10b981}.blue{fill:#eff6ff;stroke:#3b82f6}.amber{fill:#fffbeb;stroke:#f59e0b}.red{fill:#fff1f2;stroke:#fb7185}.line{fill:none;stroke:#64748b;stroke-width:2.4;stroke-linecap:round;stroke-linejoin:round}.arrow{marker-end:url(#arrow)}
    </style>
    <marker id="arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse"><path d="M0 0 L10 5 L0 10 z" fill="#64748b"/></marker>
  </defs>
  <rect class="bg" width="1200" height="720"/>
  <text class="label ink" x="64" y="58" style="font-size:34px">CortexFS v1: one small mount, three executable object classes</text>
  <text class="text muted" x="64" y="92">Every endpoint is inspectable with shell tools. Provider details stay behind the runtime boundary.</text>
  <g transform="translate(64 136)">
    <rect class="card" width="260" height="444" rx="8"/>
    <text class="label ink" x="24" y="42">Stable root</text>
    <text class="mono ink" x="24" y="84">/ctx/status</text>
    <text class="mono ink" x="24" y="122">/ctx/bin/</text>
    <text class="mono ink" x="24" y="160">/ctx/model/</text>
    <text class="mono ink" x="24" y="198">/ctx/agent/</text>
    <text class="mono ink" x="24" y="236">/ctx/tool/</text>
    <text class="mono ink" x="24" y="274">/ctx/home/</text>
    <text class="mono ink" x="24" y="312">/ctx/shared/</text>
    <text class="text muted" x="24" y="370">No root-level provider,</text>
    <text class="text muted" x="24" y="396">workflow, cluster,</text>
    <text class="text muted" x="24" y="422">MCP, skill, or vector tree.</text>
  </g>
  <g transform="translate(424 136)">
    <rect class="green" width="292" height="142" rx="8"/>
    <text class="label ink" x="24" y="40">Executable</text>
    <text class="mono ink" x="24" y="78">agent/coder</text>
    <text class="text muted" x="24" y="112">one-shot exec endpoint</text>
    <rect class="blue" y="174" width="292" height="142" rx="8"/>
    <text class="label ink" x="24" y="214">Socket</text>
    <text class="mono ink" x="24" y="252">agent/coder.sock</text>
    <text class="text muted" x="24" y="286">JSONL stateful stream</text>
    <rect class="amber" y="348" width="292" height="142" rx="8"/>
    <text class="label ink" x="24" y="388">Control</text>
    <text class="mono ink" x="24" y="426">agent/coder.d/</text>
    <text class="text muted" x="24" y="460">small text/JSON files</text>
  </g>
  <path class="line arrow" d="M324 356 C360 356 378 207 424 207"/>
  <path class="line arrow" d="M324 356 C368 356 380 381 424 381"/>
  <path class="line arrow" d="M716 207 C766 207 784 207 834 207"/>
  <path class="line arrow" d="M716 381 C766 381 784 381 834 381"/>
  <g transform="translate(834 136)">
    <rect class="card" width="302" height="204" rx="8"/>
    <text class="label ink" x="24" y="42">Agent runtime view</text>
    <text class="mono ink" x="24" y="82">CTX_ROOT=/ctx</text>
    <text class="mono ink" x="24" y="114">CTX_PATH=/ctx/tool:...</text>
    <text class="mono ink" x="24" y="146">policy=default deny</text>
    <text class="text muted" x="24" y="180">visibility + execution + sharing</text>
    <rect class="card" y="238" width="302" height="204" rx="8"/>
    <text class="label ink" x="24" y="280">Durable session</text>
    <text class="mono ink" x="24" y="320">messages.jsonl</text>
    <text class="mono ink" x="24" y="352">events.jsonl</text>
    <text class="mono ink" x="24" y="384">latest.md</text>
    <text class="text muted" x="24" y="418">raw history stays inspectable</text>
  </g>
  <g transform="translate(64 628)">
    <rect class="red" width="1072" height="48" rx="8"/>
    <text class="label ink" x="22" y="31">Boundary:</text>
    <text class="text muted" x="142" y="31">Rig handles provider/API quirks. CortexFS keeps the Linux ABI small and scriptable.</text>
  </g>
</svg>
SVG
}

run_ms() {
    local label=$1
    shift
    local i elapsed start end
    for ((i = 0; i < RUNS; i++)); do
        start=$(date +%s%N)
        "$@" >/dev/null 2>&1
        end=$(date +%s%N)
        elapsed=$(( (end - start) / 1000000 ))
        printf '%s\t%s\n' "$label" "$elapsed"
    done
}

write_bench_svg() {
    local tmp summary max_mean
    tmp=$(mktemp)
    if findmnt /ctx >/dev/null 2>&1 && command -v "$CTX_BIN" >/dev/null 2>&1; then
        run_ms "ctx status" "$CTX_BIN" status >>"$tmp"
        run_ms "ctx ls" "$CTX_BIN" ls >>"$tmp"
        run_ms "ctx ls tool" "$CTX_BIN" ls tool >>"$tmp"
        run_ms "cat /ctx/status" cat /ctx/status >>"$tmp"
        run_ms "ctx doctor" "$CTX_BIN" doctor >>"$tmp"
    fi

    if [[ -s "$tmp" ]]; then
        summary=$(awk -F '\t' '
            { key=$1; value=$2; count[key]++; sum[key]+=value; if (!(key in min) || value<min[key]) min[key]=value; if (value>max[key]) max[key]=value; order[++n]=key }
            END {
                for (i=1; i<=n; i++) if (!seen[order[i]]++) {
                    key=order[i];
                    mean=sum[key]/count[key];
                    printf "%s\t%.2f\t%d\t%d\t%d\n", key, mean, min[key], max[key], count[key];
                }
            }
        ' "$tmp")
        max_mean=$(printf '%s\n' "$summary" | awk -F '\t' 'BEGIN{m=1}{if($2>m)m=$2}END{printf "%.2f",m}')
    else
        summary=$'not mounted\t0\t0\t0\t0'
        max_mean=1
    fi

    {
        cat <<SVG
<svg xmlns="http://www.w3.org/2000/svg" width="1200" height="560" viewBox="0 0 1200 560" role="img" aria-labelledby="title desc">
  <title id="title">CortexFS local benchmark</title>
  <desc id="desc">Automatically generated benchmark chart for README.</desc>
  <defs>
    <style>
      .bg{fill:#0f172a}.panel{fill:#111c31;stroke:#26354f;stroke-width:1.2}.grid{stroke:#26354f;stroke-width:1}.title{font:700 34px system-ui,-apple-system,Segoe UI,sans-serif;fill:#f8fafc;letter-spacing:0}.text{font:500 18px system-ui,-apple-system,Segoe UI,sans-serif;fill:#cbd5e1}.mono{font:600 17px ui-monospace,SFMono-Regular,Menlo,Consolas,monospace;fill:#e2e8f0}.small{font:500 14px system-ui,-apple-system,Segoe UI,sans-serif;fill:#94a3b8}.bar{fill:#38bdf8}.bar2{fill:#58d68d}.axis{stroke:#94a3b8;stroke-width:1.2}
    </style>
  </defs>
  <rect class="bg" width="1200" height="560"/>
  <text class="title" x="56" y="58">CortexFS overhead is visible and measurable</text>
  <text class="text" x="56" y="92">Generated by scripts/update-readme-svg.sh. Values are local wall-clock means over $RUNS runs.</text>
  <rect class="panel" x="56" y="126" width="1088" height="342" rx="8"/>
  <line class="axis" x1="258" y1="420" x2="1098" y2="420"/>
SVG
        local row=0
        while IFS=$'\t' read -r label mean min max count; do
            local y barw escaped
            y=$((170 + row * 54))
            barw=$(awk -v mean="$mean" -v max="$max_mean" 'BEGIN { w = 720 * mean / max; if (w < 3) w = 3; printf "%d", w }')
            escaped=$(printf '%s' "$label" | svg_escape)
            cat <<SVG
  <text class="mono" x="82" y="$((y + 22))">$escaped</text>
  <rect class="bar" x="258" y="$y" width="$barw" height="28" rx="5"/>
  <text class="mono" x="$((278 + barw))" y="$((y + 21))">${mean} ms</text>
  <text class="small" x="912" y="$((y + 20))">min ${min} ms / max ${max} ms / n=${count}</text>
SVG
            row=$((row + 1))
        done <<<"$summary"
        cat <<SVG
  <rect x="56" y="490" width="1088" height="34" rx="6" fill="#0b1220" stroke="#26354f"/>
  <text class="small" x="78" y="512">Updated: $STAMP · Host: $HOST · Package: $PKG · Lower is better.</text>
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
