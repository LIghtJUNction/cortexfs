import { existsSync } from "node:fs";
import { readFile, rename, writeFile } from "node:fs/promises";
import { basename, dirname, join, resolve } from "node:path";
import { randomUUID } from "node:crypto";

type Transport = "file" | "http";

type RouteSnapshot = {
  ctxHome: string;
  mount: string;
  format: string;
  provider: string;
  model: string;
  reason: string;
  localBaseUrl: string;
};

type ChatBody = {
  model?: string;
  messages: Array<{ role: "user"; content: string }>;
};

const DEFAULT_MOUNT = "/ctx";
const DEFAULT_FORMAT = "openai.chat";
const DEFAULT_PROMPT = "Reply with exactly: cortexfs-ok";
const DEFAULT_BASE_URL = "http://127.0.0.1:6185/v1";

async function main() {
  const [command = "chat", ...args] = process.argv.slice(2);
  const route = await discoverRoute();

  if (command === "route") {
    printJson(route);
    return;
  }

  if (command === "models") {
    printJson(await discoverModels(route));
    return;
  }

  if (command !== "chat") {
    throw new Error(`unknown command: ${command}`);
  }

  const prompt = args.join(" ").trim() || process.env.CORTEXFS_PROMPT || DEFAULT_PROMPT;
  const transport = parseTransport(process.env.CORTEXFS_TRANSPORT);
  const body = chatBody(prompt, route.model);
  const result =
    transport === "http"
      ? await submitHttp(route, body)
      : await submitFile(route, body);
  printJson(result);
}

function parseTransport(value: string | undefined): Transport {
  if (value === undefined || value === "" || value === "file") {
    return "file";
  }
  if (value === "http") {
    return "http";
  }
  throw new Error(`invalid CORTEXFS_TRANSPORT: ${value}`);
}

async function discoverRoute(): Promise<RouteSnapshot> {
  const ctxHome = resolve(ctxHomePath());
  const mount = resolve(mountPath(ctxHome));
  const format = process.env.CORTEXFS_FORMAT || DEFAULT_FORMAT;
  const routeDir = join(ctxHome, "route", format);
  const provider = await readSmall(join(routeDir, "provider"));
  const model = await readSmall(join(routeDir, "model"));
  const reason = await readSmall(join(routeDir, "reason"));
  const localBaseUrl = await discoverLocalBaseUrl(ctxHome);

  return {
    ctxHome,
    mount,
    format,
    provider,
    model,
    reason,
    localBaseUrl,
  };
}

function ctxHomePath(): string {
  if (process.env.CTX_HOME) {
    return process.env.CTX_HOME;
  }
  const mount = process.env.CORTEXFS_MOUNT || DEFAULT_MOUNT;
  const uid = process.env.CORTEXFS_UID || currentUid();
  return join(mount, "home", uid);
}

function mountPath(ctxHome: string): string {
  const marker = "/home/";
  const index = ctxHome.lastIndexOf(marker);
  if (index >= 0) {
    return ctxHome.slice(0, index);
  }
  return process.env.CORTEXFS_MOUNT || DEFAULT_MOUNT;
}

function currentUid(): string {
  const maybeProcess = process as typeof process & { getuid?: () => number };
  return String(maybeProcess.getuid?.() ?? 1000);
}

async function discoverLocalBaseUrl(ctxHome: string): Promise<string> {
  if (process.env.CORTEXFS_BASE_URL) {
    return trimTrailingSlash(process.env.CORTEXFS_BASE_URL);
  }
  const listen = await readSmall(join(ctxHome, "api", "http", "listen"));
  if (listen) {
    return `http://${listen}/v1`;
  }
  return DEFAULT_BASE_URL;
}

async function discoverModels(route: RouteSnapshot) {
  const list = await readSmall(join(route.ctxHome, "model", "list"));
  return {
    selected: {
      provider: route.provider,
      model: route.model,
      reason: route.reason,
    },
    models: list ? list.split("\n").filter(Boolean) : [],
  };
}

function chatBody(prompt: string, model: string): ChatBody {
  const body: ChatBody = {
    messages: [{ role: "user", content: prompt }],
  };
  if (model) {
    body.model = model;
  }
  return body;
}

async function submitFile(route: RouteSnapshot, body: ChatBody) {
  const id = requestId();
  const apiDir = join(route.ctxHome, "api", route.format);
  const inbox = join(apiDir, "inbox");
  const outbox = join(apiDir, "outbox");
  const tmp = join(inbox, `${id}.tmp`);
  const req = join(inbox, `${id}.req.json`);

  if (!existsSync(inbox) || !existsSync(outbox)) {
    throw new Error(
      `CortexFS ${route.format} inbox/outbox not found under ${apiDir}; mount CortexFS first or set CTX_HOME`,
    );
  }

  await writeFile(tmp, `${JSON.stringify(body)}\n`, "utf8");
  await rename(tmp, req);

  const drain = join(route.mount, "control", "drain");
  if (existsSync(drain)) {
    await writeFile(drain, "1\n", "utf8");
  }

  const responsePath = join(outbox, `${id}.resp.json`);
  const errorPath = join(outbox, `${id}.error`);
  const routePath = join(outbox, `${id}.route.json`);
  const fingerprintPath = join(outbox, `${id}.fingerprint`);

  return {
    transport: "file",
    requestId: id,
    request: relativeDisplay(req, route.mount),
    route: await readJsonFile(routePath),
    fingerprint: await readSmall(fingerprintPath),
    response: await readJsonFile(responsePath),
    error: await readJsonFile(errorPath),
  };
}

async function submitHttp(route: RouteSnapshot, body: ChatBody) {
  const endpoint = `${trimTrailingSlash(route.localBaseUrl)}/chat/completions`;
  const headers: Record<string, string> = {
    "Content-Type": "application/json",
    Accept: "application/json",
  };
  if (process.env.CORTEXFS_API_KEY) {
    headers.Authorization = `Bearer ${process.env.CORTEXFS_API_KEY}`;
  }
  const response = await fetch(endpoint, {
    method: "POST",
    headers,
    body: JSON.stringify(body),
  });
  const text = await response.text();

  return {
    transport: "http",
    endpoint,
    status: response.status,
    ok: response.ok,
    response: parseJsonOrText(text),
  };
}

function requestId(): string {
  const configured = process.env.CORTEXFS_REQUEST_ID?.trim();
  if (configured) {
    return safeFileStem(configured);
  }
  return `bun-${Date.now()}-${randomUUID().slice(0, 8)}`;
}

function safeFileStem(value: string): string {
  return value.replace(/[^A-Za-z0-9._-]/g, "-");
}

async function readSmall(path: string): Promise<string> {
  try {
    return (await readFile(path, "utf8")).trim();
  } catch {
    return "";
  }
}

async function readJsonFile(path: string): Promise<unknown> {
  const content = await readSmall(path);
  if (!content) {
    return null;
  }
  return parseJsonOrText(content);
}

function parseJsonOrText(content: string): unknown {
  try {
    return JSON.parse(content);
  } catch {
    return content;
  }
}

function trimTrailingSlash(value: string): string {
  return value.replace(/\/+$/, "");
}

function relativeDisplay(path: string, root: string): string {
  const normalizedRoot = trimTrailingSlash(root);
  if (path.startsWith(`${normalizedRoot}/`)) {
    return path.slice(normalizedRoot.length + 1);
  }
  return basename(dirname(path)) + "/" + basename(path);
}

function printJson(value: unknown) {
  console.log(JSON.stringify(value, null, 2));
}

main().catch((error: unknown) => {
  const message = error instanceof Error ? error.message : String(error);
  console.error(message);
  process.exitCode = 1;
});
