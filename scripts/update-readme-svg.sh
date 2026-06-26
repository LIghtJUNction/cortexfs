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
  <desc id="desc">A branded CortexFS hero card showing the ctx logo, the frozen /ctx root, and an agent REPL surface.</desc>
  <defs>
    <linearGradient id="panel" x1="0" x2="1" y1="0" y2="1"><stop offset="0" stop-color="#181b19"/><stop offset="1" stop-color="#0f1110"/></linearGradient>
    <radialGradient id="glow" cx="76%" cy="22%" r="58%"><stop offset="0" stop-color="#64c8a9" stop-opacity=".24"/><stop offset="1" stop-color="#64c8a9" stop-opacity="0"/></radialGradient>
    <pattern id="grid" width="38" height="38" patternUnits="userSpaceOnUse"><path d="M38 0H0V38" fill="none" stroke="#d8ff66" stroke-opacity=".075"/></pattern>
    <filter id="shadow" x="-20%" y="-20%" width="140%" height="140%"><feDropShadow dx="0" dy="24" stdDeviation="26" flood-color="#000" flood-opacity=".3"/></filter>
    <style>.serif{font-family:Georgia,"Times New Roman",serif}.sans{font-family:Inter,ui-sans-serif,system-ui,-apple-system,"Segoe UI",sans-serif}.mono{font-family:ui-monospace,SFMono-Regular,Menlo,Consolas,monospace}.paper{fill:#fffdfa}.muted{fill:#bfb6a8}.mint{fill:#64c8a9}.lime{fill:#d8ff66}.ink{fill:#111312}.soft{fill:#fffdfa;fill-opacity:.06;stroke:#fffdfa;stroke-opacity:.1}</style>
  </defs>
  <rect width="1200" height="520" fill="#111211"/><rect width="1200" height="520" fill="url(#glow)"/><rect x="578" y="36" width="548" height="404" fill="url(#grid)" opacity=".85"/>
  <g transform="translate(72 58)">
    <path fill="#d8ff66" d="M38 4.7c.4-.2.8-.2 1.2 0l28.2 16.2c.4.2.6.6.6 1.1v32.5c0 .5-.2.9-.6 1.1L39.2 71.8c-.4.2-.8.2-1.2 0L9.8 55.6c-.4-.2-.6-.6-.6-1.1V22c0-.5.2-.9.6-1.1L38 4.7Z"/>
    <path fill="#111312" d="M42.7 17.3h8.6L34.4 60.4h-8.6l16.9-43.1Z"/>
    <path fill="#111312" fill-rule="evenodd" d="M18.2 22.8h13.5c.8 0 1.5.3 2.1 1l2.4 2.9h10.3c1.5 0 2.6 1.1 2.6 2.6v9.3c0 1.5-1.1 2.6-2.6 2.6H18.2c-1.5 0-2.6-1.1-2.6-2.6V25.4c0-1.5 1.1-2.6 2.6-2.6Zm0 27.1c0-1.3 1.1-2.5 2.5-2.5h7.9c1.3 0 2.5 1.2 2.5 2.5v7.9c0 1.3-1.2 2.5-2.5 2.5h-7.9c-1.4 0-2.5-1.2-2.5-2.5v-7.9Zm34.4 2.8a5.4 5.4 0 1 1 8.6 4.5v5.2h5.6v5.1H51.1v-5.1h5.2v-5.1a5.4 5.4 0 0 1-3.7-4.6Z" clip-rule="evenodd"/>
    <path fill="#fffdfa" d="M21.2 50.6H28v6.8h-6.8z"/>
    <text class="serif paper" x="96" y="47" font-size="44" font-weight="700">Cor</text><text class="serif mint" x="171" y="47" font-size="44" font-weight="700" font-style="italic">TeX</text><text class="serif paper" x="244" y="47" font-size="44" font-weight="700">fs</text>
  </g>
  <g transform="translate(72 168)"><text class="sans mint" x="0" y="0" font-size="17" font-weight="900" letter-spacing="3">FILESYSTEM AS AGENT OS</text><text class="serif paper" x="0" y="72" font-size="64" font-weight="650">Agent runtime</text><text class="serif paper" x="0" y="144" font-size="64" font-weight="650">as a Unix ABI</text><text class="sans muted" x="0" y="210" font-size="23">No provider tree. No workflow DSL. Just /ctx.</text><rect x="0" y="250" width="434" height="58" rx="8" fill="#fffdfa"/><text class="mono ink" x="24" y="286" font-size="18" font-weight="900">ctx agent repl coder</text></g>
  <g transform="translate(620 86)" filter="url(#shadow)"><rect width="424" height="332" rx="8" fill="url(#panel)" stroke="#fffdfa" stroke-opacity=".12"/><rect width="424" height="48" rx="8" fill="#101210"/><rect width="54" height="48" rx="8" fill="#d8ff66"/><text class="mono ink" x="17" y="31" font-size="14" font-weight="950">ctx</text><text class="mono paper" x="74" y="31" font-size="13" font-weight="850" fill-opacity=".86">/ctx/agent/coder</text><text class="mono paper" x="348" y="31" font-size="12" font-weight="850" fill-opacity=".5">repl</text><g transform="translate(28 72)"><text class="mono lime" x="0" y="17" font-size="15" font-weight="950">$</text><text class="mono paper" x="24" y="17" font-size="15" font-weight="900">ctx agent repl coder</text><rect class="soft" x="0" y="42" width="368" height="44" rx="6"/><text class="mono mint" x="16" y="70" font-size="13" font-weight="900">USER</text><text class="mono paper" x="86" y="70" font-size="13" font-weight="850" fill-opacity=".82">review docs/DESIGN.md</text><rect class="soft" x="0" y="96" width="368" height="44" rx="6"/><text class="mono mint" x="16" y="124" font-size="13" font-weight="900">TOOL</text><text class="mono paper" x="86" y="124" font-size="13" font-weight="850" fill-opacity=".82">tsh fs.read</text><rect x="0" y="150" width="368" height="56" rx="6" fill="#d8ff66" fill-opacity=".08" stroke="#d8ff66" stroke-opacity=".18"/><text class="mono lime" x="16" y="181" font-size="13" font-weight="900">COMMIT</text><text class="mono paper" x="96" y="181" font-size="13" font-weight="850" fill-opacity=".82">*.req.json -> outbox</text></g></g>
</svg>
SVG
}

write_abi_svg() {
    cat >"$ASSETS/cortexfs-abi-map.svg" <<'SVG'
<svg xmlns="http://www.w3.org/2000/svg" width="1200" height="720" viewBox="0 0 1200 720" role="img" aria-labelledby="title desc">
  <title id="title">CortexFS v1 ABI map</title>
  <desc id="desc">The frozen /ctx root, the three executable object classes, and object-local runtime state.</desc>
  <defs><filter id="shadow" x="-20%" y="-20%" width="140%" height="140%"><feDropShadow dx="0" dy="18" stdDeviation="20" flood-color="#1e180e" flood-opacity=".12"/></filter><marker id="arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse"><path d="M0 0 L10 5 L0 10 z" fill="#8a948f"/></marker><style>.serif{font-family:Georgia,"Times New Roman",serif}.sans{font-family:Inter,ui-sans-serif,system-ui,-apple-system,"Segoe UI",sans-serif}.mono{font-family:ui-monospace,SFMono-Regular,Menlo,Consolas,monospace}.bg{fill:#f7f5f1}.panel{fill:#fffdfa;stroke:#ded8ca;stroke-width:1.4}.ink{fill:#111312}.muted{fill:#66716c}.mint{fill:#2a8f73}.lime{fill:#d8ff66}.coal{fill:#181b19}.line{fill:none;stroke:#8a948f;stroke-width:2.2;stroke-linecap:round;stroke-linejoin:round;marker-end:url(#arrow)}</style></defs>
  <rect class="bg" width="1200" height="720"/><g transform="translate(58 50)"><text class="serif ink" x="0" y="0" font-size="42" font-weight="650">CortexFS v1 ABI</text><text class="sans muted" x="0" y="36" font-size="19" font-weight="600">One frozen mount, ordinary files, and object-local runtime state.</text></g>
  <g transform="translate(58 126)" filter="url(#shadow)"><rect class="coal" width="300" height="472" rx="8"/><text class="mono lime" x="26" y="44" font-size="18" font-weight="950">/ctx</text><line x1="34" y1="66" x2="34" y2="398" stroke="#d8ff66" stroke-opacity=".65" stroke-width="2"/><g class="mono" font-size="18" font-weight="850"><rect x="66" y="80" width="186" height="38" rx="6" fill="#fffdfa" fill-opacity=".07" stroke="#fffdfa" stroke-opacity=".11"/><text fill="#fffdfa" x="86" y="105">status</text><rect x="66" y="130" width="186" height="38" rx="6" fill="#fffdfa" fill-opacity=".07" stroke="#fffdfa" stroke-opacity=".11"/><text fill="#fffdfa" x="86" y="155">bin/</text><rect x="66" y="180" width="186" height="38" rx="6" fill="#d8ff66" fill-opacity=".12" stroke="#d8ff66" stroke-opacity=".28"/><text fill="#fffdfa" x="86" y="205">model/</text><rect x="66" y="230" width="186" height="38" rx="6" fill="#d8ff66" fill-opacity=".12" stroke="#d8ff66" stroke-opacity=".28"/><text fill="#fffdfa" x="86" y="255">agent/</text><rect x="66" y="280" width="186" height="38" rx="6" fill="#d8ff66" fill-opacity=".12" stroke="#d8ff66" stroke-opacity=".28"/><text fill="#fffdfa" x="86" y="305">tool/</text><rect x="66" y="330" width="186" height="38" rx="6" fill="#fffdfa" fill-opacity=".07" stroke="#fffdfa" stroke-opacity=".11"/><text fill="#fffdfa" x="86" y="355">home/</text><rect x="66" y="380" width="186" height="38" rx="6" fill="#fffdfa" fill-opacity=".07" stroke="#fffdfa" stroke-opacity=".11"/><text fill="#fffdfa" x="86" y="405">shared/</text></g></g>
  <g transform="translate(438 126)"><rect class="panel" width="296" height="126" rx="8"/><text class="sans mint" x="24" y="38" font-size="13" font-weight="950" letter-spacing="2">OBJECT CLASS</text><text class="serif ink" x="24" y="76" font-size="34" font-weight="650">model</text><text class="sans muted" x="24" y="104" font-size="17" font-weight="600">pure inference endpoint</text><rect class="panel" y="162" width="296" height="126" rx="8"/><text class="sans mint" x="24" y="200" font-size="13" font-weight="950" letter-spacing="2">OBJECT CLASS</text><text class="serif ink" x="24" y="238" font-size="34" font-weight="650">agent</text><text class="sans muted" x="24" y="266" font-size="17" font-weight="600">policy-bound runtime</text><rect class="panel" y="324" width="296" height="126" rx="8"/><text class="sans mint" x="24" y="362" font-size="13" font-weight="950" letter-spacing="2">OBJECT CLASS</text><text class="serif ink" x="24" y="400" font-size="34" font-weight="650">tool</text><text class="sans muted" x="24" y="428" font-size="17" font-weight="600">executable capability</text></g>
  <path class="line" d="M358 325 C392 325 400 190 438 190"/><path class="line" d="M358 325 C396 325 400 352 438 352"/><path class="line" d="M358 325 C392 325 400 514 438 514"/>
  <g transform="translate(816 126)" filter="url(#shadow)"><rect class="panel" width="316" height="472" rx="8"/><text class="sans mint" x="26" y="46" font-size="13" font-weight="950" letter-spacing="2">EVERY OBJECT CAN EXPOSE</text><g class="mono" font-size="18" font-weight="850"><text class="ink" x="26" y="104">name</text><text class="muted" x="26" y="134">exec or metadata endpoint</text><line x1="26" y1="164" x2="290" y2="164" stroke="#ded8ca"/><text class="ink" x="26" y="214">name.sock</text><text class="muted" x="26" y="244">stateful JSONL stream</text><line x1="26" y1="274" x2="290" y2="274" stroke="#ded8ca"/><text class="ink" x="26" y="324">name.d/</text><text class="muted" x="26" y="354">small control files</text></g><rect x="26" y="398" width="264" height="44" rx="6" fill="#181b19"/><text class="mono lime" x="44" y="426" font-size="15" font-weight="900">*.req.json -> outbox</text></g>
  <g transform="translate(58 644)"><rect width="1074" height="44" rx="8" fill="#181b19"/><text class="sans" x="22" y="29" font-size="16" font-weight="850" fill="#fffdfa">Boundary:</text><text class="sans" x="112" y="29" font-size="16" font-weight="650" fill="#bfb6a8">provider, workflow, MCP, vector, hook, job, and audit internals are not stable root ABI.</text></g>
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
        elapsed=$(((end - start) / 1000000))
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
            END { for (i=1; i<=n; i++) if (!seen[order[i]]++) { key=order[i]; mean=sum[key]/count[key]; printf "%s\t%.2f\t%d\t%d\t%d\n", key, mean, min[key], max[key], count[key]; } }
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
  <defs><style>.bg{fill:#111211}.panel{fill:#181b19;stroke:#fffdfa;stroke-opacity:.12;stroke-width:1.2}.title{font:650 36px Georgia,"Times New Roman",serif;fill:#fffdfa;letter-spacing:0}.text{font:600 18px Inter,ui-sans-serif,system-ui,-apple-system,"Segoe UI",sans-serif;fill:#bfb6a8}.mono{font:850 17px ui-monospace,SFMono-Regular,Menlo,Consolas,monospace;fill:#fffdfa}.small{font:600 14px Inter,ui-sans-serif,system-ui,-apple-system,"Segoe UI",sans-serif;fill:#8a948f}.bar{fill:#d8ff66}.axis{stroke:#8a948f;stroke-width:1.2}</style></defs>
  <rect class="bg" width="1200" height="560"/>
  <text class="title" x="56" y="62">CortexFS overhead is visible and measurable</text>
  <text class="text" x="56" y="98">Generated by scripts/update-readme-svg.sh. Values are local wall-clock means over $RUNS runs.</text>
  <rect class="panel" x="56" y="130" width="1088" height="342" rx="8"/>
  <line class="axis" x1="258" y1="424" x2="1098" y2="424"/>
SVG
        local row=0
        while IFS=$'\t' read -r label mean min max count; do
            local y barw escaped
            y=$((174 + row * 54))
            barw=$(awk -v mean="$mean" -v max="$max_mean" 'BEGIN { w = 600 * mean / max; if (w < 3) w = 3; printf "%d", w }')
            escaped=$(printf '%s' "$label" | svg_escape)
            cat <<SVG
  <text class="mono" x="82" y="$((y + 22))">$escaped</text>
  <rect class="bar" x="258" y="$y" width="$barw" height="28" rx="5"/>
  <text class="mono" x="$((278 + barw))" y="$((y + 21))">${mean} ms</text>
  <text class="small" x="970" y="$((y + 20))">min ${min} ms / max ${max} ms / n=${count}</text>
SVG
            row=$((row + 1))
        done <<<"$summary"
        cat <<SVG
  <rect x="56" y="494" width="1088" height="34" rx="6" fill="#0f1110" stroke="#fffdfa" stroke-opacity=".12"/>
  <text class="small" x="78" y="516">Updated: $STAMP · Host: $HOST · Package: $PKG · Lower is better.</text>
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
