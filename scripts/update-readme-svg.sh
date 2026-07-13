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
<svg xmlns="http://www.w3.org/2000/svg" width="1200" height="560" viewBox="0 0 1200 560" role="img" aria-labelledby="title desc">
  <title id="title">CortexFS: Durable agents. One Unix ABI.</title>
  <desc id="desc">Four default agent roles share the frozen CortexFS Unix ABI, while a same-session retry with the same client ID and payload replays the recorded run instead of executing it again.</desc>
  <defs>
    <pattern id="grid" width="40" height="40" patternUnits="userSpaceOnUse"><path d="M40 0H0V40" fill="none" stroke="#d8ff66" stroke-opacity=".06"/></pattern>
    <filter id="shadow" x="-8%" y="-10%" width="116%" height="120%"><feDropShadow dx="0" dy="20" stdDeviation="22" flood-color="#000" flood-opacity=".28"/></filter>
    <style>.serif{font-family:Georgia,"Times New Roman",serif}.sans{font-family:Inter,ui-sans-serif,system-ui,-apple-system,"Segoe UI",sans-serif}.mono{font-family:ui-monospace,SFMono-Regular,Menlo,Consolas,monospace}.paper{fill:#fffdfa}.muted{fill:#bfb6a8}.mint{fill:#2a8f73}.lime{fill:#d8ff66}.ink{fill:#111312}.soft{fill:#fffdfa;fill-opacity:.06;stroke:#fffdfa;stroke-opacity:.11}</style>
  </defs>
  <rect width="1200" height="560" fill="#111312"/><rect x="604" width="596" height="560" fill="url(#grid)"/>
  <g transform="translate(64 48)">
    <rect width="68" height="68" rx="6" fill="#d8ff66"/><path fill="#111312" d="M41.5 13h8L31.8 57h-8l17.7-44Z"/><path fill="#111312" d="M15 20h18l5 6h15v14H15V20Zm0 26h14v14H15V46Zm31 4h15v6h-5v5h-6v-5h-4v-6Z"/>
    <text class="serif paper" x="88" y="46" font-size="42" font-weight="700">Cor</text><text class="serif mint" x="160" y="46" font-size="42" font-weight="700" font-style="italic">TeX</text><text class="serif paper" x="230" y="46" font-size="42" font-weight="700">fs</text>
  </g>
  <g transform="translate(64 164)">
    <text class="sans mint" x="0" y="0" font-size="15" font-weight="900" letter-spacing="3">FILESYSTEM AS AGENT OS</text>
    <text class="serif paper" x="0" y="72" font-size="62" font-weight="650">Durable agents.</text>
    <text class="serif paper" x="0" y="140" font-size="62" font-weight="650">One Unix ABI.</text>
    <text class="sans muted" x="0" y="190" font-size="21">Small enough to inspect. Ordinary enough to script.</text>
    <rect x="0" y="226" width="432" height="52" rx="8" fill="#fffdfa"/><text class="mono ink" x="22" y="259" font-size="16" font-weight="900">ctx agent start coder</text>
    <rect x="0" y="292" width="432" height="52" rx="8" fill="#181b19" stroke="#fffdfa" stroke-opacity=".14"/><text class="mono lime" x="22" y="325" font-size="16" font-weight="900">ctx agent chat coder</text>
  </g>
  <g transform="translate(638 56)" filter="url(#shadow)">
    <rect width="500" height="448" rx="8" fill="#181b19" stroke="#fffdfa" stroke-opacity=".13"/>
    <rect width="500" height="52" rx="8" fill="#0f1110"/><rect width="62" height="52" rx="8" fill="#d8ff66"/>
    <text class="mono ink" x="18" y="33" font-size="14" font-weight="950">ctx</text><text class="mono paper" x="84" y="33" font-size="14" font-weight="850">/ctx/agent</text><text class="mono muted" x="394" y="33" font-size="12" font-weight="850">default</text>
    <g transform="translate(24 76)">
      <rect class="soft" width="218" height="62" rx="6"/><text class="mono lime" x="16" y="25" font-size="15" font-weight="900">architect</text><text class="sans muted" x="16" y="47" font-size="13" font-weight="700">plan + coordinate</text>
      <rect class="soft" x="234" width="218" height="62" rx="6"/><text class="mono lime" x="250" y="25" font-size="15" font-weight="900">coder</text><text class="sans muted" x="250" y="47" font-size="13" font-weight="700">primary implementation</text>
      <rect class="soft" y="78" width="218" height="62" rx="6"/><text class="mono lime" x="16" y="103" font-size="15" font-weight="900">worker</text><text class="sans muted" x="16" y="125" font-size="13" font-weight="700">bounded Spark tasks</text>
      <rect class="soft" x="234" y="78" width="218" height="62" rx="6"/><text class="mono lime" x="250" y="103" font-size="15" font-weight="900">reviewer</text><text class="sans muted" x="250" y="125" font-size="13" font-weight="700">verify + guard</text>
      <text class="sans mint" x="0" y="188" font-size="12" font-weight="900" letter-spacing="2">RETRY-SAFE DURABLE RUN</text>
      <rect x="0" y="212" width="120" height="42" rx="6" fill="#fffdfa" fill-opacity=".07" stroke="#fffdfa" stroke-opacity=".12"/><text class="mono paper" x="31" y="239" font-size="14" font-weight="850">claim</text>
      <text class="mono muted" x="137" y="239" font-size="16" font-weight="850">→</text>
      <rect x="166" y="212" width="120" height="42" rx="6" fill="#2a8f73" fill-opacity=".22" stroke="#2a8f73"/><text class="mono paper" x="208" y="239" font-size="14" font-weight="850">run</text>
      <text class="mono muted" x="303" y="239" font-size="16" font-weight="850">→</text>
      <rect x="332" y="212" width="120" height="42" rx="6" fill="#d8ff66" fill-opacity=".12" stroke="#d8ff66" stroke-opacity=".4"/><text class="mono lime" x="364" y="239" font-size="14" font-weight="850">replay</text>
      <text class="sans muted" x="0" y="282" font-size="13" font-weight="700">same session + client id + payload</text>
      <rect x="0" y="304" width="452" height="42" rx="6" fill="#d8ff66" fill-opacity=".08" stroke="#d8ff66" stroke-opacity=".2"/><text class="mono paper" x="16" y="330" font-size="13" font-weight="850">retry = replay, not re-run</text>
    </g>
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
  <line class="axis" x1="330" y1="424" x2="1098" y2="424"/>
SVG
        local row=0
        while IFS=$'\t' read -r label mean min max count; do
            local y barw escaped
            y=$((174 + row * 54))
            barw=$(awk -v mean="$mean" -v max="$max_mean" 'BEGIN { w = 420 * mean / max; if (w < 3) w = 3; printf "%d", w }')
            escaped=$(printf '%s' "$label" | svg_escape)
            cat <<SVG
  <text class="mono" x="82" y="$((y + 22))">$escaped</text>
  <rect class="bar" x="330" y="$y" width="$barw" height="28" rx="5"/>
  <text class="mono" x="$((350 + barw))" y="$((y + 21))">${mean} ms</text>
  <text class="small" x="900" y="$((y + 20))">min ${min} ms / max ${max} ms / n=${count}</text>
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
