import json, os, sys, time
mode=os.environ.get("CTXMCP_MOCK")
if os.environ.get("PID_FILE"):
 open(os.environ["PID_FILE"],"w").write(str(os.getpid()))
if mode=="timeout":
 time.sleep(2); sys.exit(0)
if mode=="oversize":
 sys.stdout.write("x"*1048577); sys.stdout.flush(); sys.exit(0)
page=0
for line in sys.stdin:
 r=json.loads(line); m=r.get("method")
 if "id" not in r:
  if mode=="descendant":
   child=os.fork()
   if child==0:
    with open(os.environ["DESC_PID_FILE"],"w") as f: f.write(str(os.getpid()))
    while True: time.sleep(1)
   sys.exit(0)
  continue
 if m=="server/discover":
  meta=r.get("params",{}).get("_meta",{})
  if mode.startswith("modern"):
   expected={"io.modelcontextprotocol/protocolVersion":"2026-07-28",
             "io.modelcontextprotocol/clientInfo":{"name":"ctxmcp","version":os.environ["PKG_VERSION"]},
             "io.modelcontextprotocol/clientCapabilities":{}}
   if meta!=expected: sys.exit(7)
   if mode=="modernunsupported":
    error={"code":-32022,"message":"unsupported","data":{"supported":["2027-01-01"],"requested":"2026-07-28"}}
    print(json.dumps({"jsonrpc":"2.0","id":r["id"],"error":error}),flush=True); continue
   if mode=="modernmissingcap":
    result={"resultType":"complete","supportedVersions":["2026-07-28"],"capabilities":{}}
   elif mode=="modernbadresult":
    result={}
   else:
    result={"resultType":"complete","supportedVersions":["2026-07-28"],"capabilities":{"tools":{}}}
   print(json.dumps({"jsonrpc":"2.0","id":r["id"],"result":result}),flush=True); continue
  if mode=="legacyprobelate":
   time.sleep(.08)
  print(json.dumps({"jsonrpc":"2.0","id":r["id"],"error":{"code":-32601,"message":"unknown"}}),flush=True)
  continue
 if mode.startswith("modern") and m=="initialize":
  result={"protocolVersion":"2025-11-25","capabilities":{"tools":{}}}
  print(json.dumps({"jsonrpc":"2.0","id":r["id"],"result":result}),flush=True); continue
 if mode in ("stderrlimit","stderroverflow") and m=="initialize":
  size=1048576 if mode=="stderrlimit" else 1048577
  try:
   sys.stderr.buffer.write(b"x"*size); sys.stderr.buffer.flush()
  except BrokenPipeError:
   sys.stderr=open(os.devnull,"w")
 if mode=="slowframes" and m=="initialize":
  for _ in range(4):
   print(json.dumps({"jsonrpc":"2.0","method":"progress"}),flush=True); time.sleep(.1)
 if mode=="invalidnotification" and m=="initialize":
  print(json.dumps({"method":"progress"}),flush=True)
 if mode in ("pingstr","pingnum") and m=="initialize":
  ping_id="server-ping" if mode=="pingstr" else 7
  print(json.dumps({"jsonrpc":"2.0","id":ping_id,"method":"ping","params":{}}),flush=True)
  reply=json.loads(sys.stdin.readline())
  if reply!={"jsonrpc":"2.0","id":ping_id,"result":{}}: sys.exit(3)
 if mode=="unsupportedrequest" and m=="initialize":
  print(json.dumps({"jsonrpc":"2.0","id":"server-request","method":"roots/list","params":{}}),flush=True)
 if mode=="malformedping" and m=="initialize":
  print(json.dumps({"jsonrpc":"2.0","id":None,"method":"ping","params":{}}),flush=True)
 versions={"legacy20241105":"2024-11-05","stable20250326":"2025-03-26",
           "stable20250618":"2025-06-18","unknownversion":"unknown",
           "draftversion":"2025-11-25-draft","badversion":"2099-01-01"}
 version=versions.get(mode,"2025-11-25")
 if m=="initialize":
  capabilities={"tools":{}}
  if mode=="missingtools": capabilities={}
  if mode=="badtools": capabilities={"tools":[]}
  result={"protocolVersion":version,"capabilities":capabilities,"serverInfo":{"name":"mock","version":"1"}}
 elif m=="tools/list" and page==0:
  if mode.startswith("modern"):
   meta=r.get("params",{}).get("_meta",{})
   if meta.get("io.modelcontextprotocol/protocolVersion")!="2026-07-28": sys.exit(8)
   if meta.get("io.modelcontextprotocol/clientCapabilities")!={}: sys.exit(8)
   if meta.get("io.modelcontextprotocol/clientInfo",{}).get("name")!="ctxmcp": sys.exit(8)
  page=1
  tool={"name":"echo","title":"Echo title","description":"Echo",
        "icons":[{"src":"data:image/svg+xml,echo","mimeType":"image/svg+xml","sizes":["any"]}],
        "annotations":{"readOnlyHint":True},"inputSchema":{"type":"object"},
        "outputSchema":{"type":"object"},"execution":{"taskSupport":"forbidden"},
        "_meta":{"fixture":"current"}}
  if mode=="missingname": tool.pop("name")
  if mode=="missinginputschema": tool.pop("inputSchema")
  if mode=="missingschematype": tool["inputSchema"]={}
  if mode=="wrongschematype": tool["inputSchema"]={"type":"string"}
  if mode=="optionaltask": tool["execution"]={"taskSupport":"optional"}
  if mode=="requiredtask": tool["execution"]={"taskSupport":"required"}
  if mode=="badtask": tool["execution"]={"taskSupport":"unknown"}
  result={"tools":[tool],"nextCursor":7 if mode=="badcursor" else "next"}
  if mode.startswith("modern"): result["resultType"]="complete"
 elif m=="tools/list":
  result={"tools":[{"name":"echo" if mode=="duplicate" else "sum",
                    "description":"Sum","inputSchema":{"type":"object"}}]}
  if mode.startswith("modern"): result["resultType"]="complete"
 elif m=="tools/call":
  if mode.startswith("modern"):
   meta=r.get("params",{}).get("_meta",{})
   if meta.get("io.modelcontextprotocol/protocolVersion")!="2026-07-28": sys.exit(8)
  result={"content":[{"type":"text","text":"ok"}],"isError":False}
  if mode.startswith("modern"): result["resultType"]="complete"
 else: result={}
 if mode=="error" and m=="tools/list":
  print(json.dumps({"jsonrpc":"2.0","id":r["id"],"error":{"code":-32603,"message":"secret-value"}}),flush=True); continue
 if mode=="callerror" and m=="tools/call":
  print(json.dumps({"jsonrpc":"2.0","id":r["id"],"error":{"code":-32603,"message":"failed"}}),flush=True); continue
 print(json.dumps({"jsonrpc":"2.0","id":r["id"],"result":result}),flush=True)
