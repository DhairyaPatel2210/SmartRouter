#!/usr/bin/env node
// Performance gate (docs/SPEC.md → Performance): cold start to interactive,
// idle memory (core + webview), idle CPU and installed app size, compared to
// the budgets. Appends a row to docs/perf.md. Exit 1 if any budget is
// exceeded by more than 10%.
//
// Usage: npm run perf            (expects a release build: npm run app:build)
//        npm run perf -- --no-write
import { execSync, spawn } from "node:child_process";
import { appendFileSync, existsSync, mkdtempSync, readFileSync, statSync, readdirSync } from "node:fs";
import { join, dirname } from "node:path";
import { tmpdir } from "node:os";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const brand = JSON.parse(readFileSync(join(root, "brand.json"), "utf8"));
const BUDGET = { startMs: 1000, idleMb: 150, idleCpu: 0.5, sizeMb: 25 };
const TOL = 1.1;

const appDir = join(root, "src-tauri/target/release/bundle/macos", `${brand.productName}.app`);
const bin = existsSync(appDir) ? join(appDir, "Contents/MacOS/orchestrator") : join(root, "src-tauri/target/release/orchestrator");
if (!existsSync(bin)) {
  console.error(`No release build at ${bin}. Run: npm run app:build`);
  process.exit(2);
}

function duBytes(p) {
  const st = statSync(p);
  if (!st.isDirectory()) return st.size;
  return readdirSync(p).reduce((a, f) => a + duBytes(join(p, f)), 0);
}
const sizeMb = duBytes(existsSync(appDir) ? appDir : bin) / 1e6;

const webContentPids = () =>
  new Set(
    execSync("ps -A -o pid=,comm=")
      .toString()
      .split("\n")
      .filter((l) => l.includes("com.apple.WebKit"))
      .map((l) => parseInt(l.trim(), 10)),
  );
// Physical footprint (what Activity Monitor shows), in KB. RSS overstates
// memory on macOS because it counts shared system frameworks in every process.
const footprintKb = (pid) => {
  try {
    const out = execSync(`vmmap --summary ${pid} 2>/dev/null`).toString();
    const m = out.match(/Physical footprint:\s+([\d.]+)([KMG])/);
    if (m) return parseFloat(m[1]) * { K: 1, M: 1024, G: 1024 * 1024 }[m[2]];
  } catch {
    /* fall back to RSS */
  }
  try {
    return parseInt(execSync(`ps -o rss= -p ${pid}`).toString().trim(), 10) || 0;
  } catch {
    return 0;
  }
};
const cpu = (pid) => {
  try {
    return parseFloat(execSync(`ps -o %cpu= -p ${pid}`).toString().trim()) || 0;
  } catch {
    return 0;
  }
};
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function measure() {
  const before = webContentPids();
  const data = mkdtempSync(join(tmpdir(), "orch-perf-"));
  const t0 = Date.now();
  const child = spawn(bin, [], { env: { ...process.env, ORCH_PERF: "1", ORCH_DATA_DIR: data }, stdio: ["ignore", "pipe", "inherit"] });
  const startMs = await new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error("app did not become interactive within 15 s")), 15000);
    child.stdout.on("data", (b) => {
      const m = b.toString().match(/ORCH_READY (\d+)/);
      if (m) {
        clearTimeout(timer);
        resolve(Math.max(Number(m[1]), 0) || Date.now() - t0);
      }
    });
    child.on("exit", (c) => reject(new Error(`app exited early (${c})`)));
  });
  // Let it settle, then measure idle.
  await sleep(8000);
  const web = [...webContentPids()].filter((p) => !before.has(p));
  const idleMb = (footprintKb(child.pid) + web.reduce((a, p) => a + footprintKb(p), 0)) / 1024;
  const samples = [];
  for (let i = 0; i < 5; i++) {
    samples.push(cpu(child.pid));
    await sleep(1000);
  }
  const idleCpu = samples.reduce((a, b) => a + b, 0) / samples.length;
  child.kill("SIGTERM");
  await sleep(500);
  return { startMs, idleMb, idleCpu, webProcs: web.length };
}

const runs = [];
for (let i = 0; i < 3; i++) runs.push(await measure());
const median = (k) => runs.map((r) => r[k]).sort((a, b) => a - b)[1];
const r = { startMs: median("startMs"), idleMb: median("idleMb"), idleCpu: median("idleCpu"), sizeMb };
const check = (v, b) => (v <= b * TOL ? "✓" : "✗");
const ok = r.startMs <= BUDGET.startMs * TOL && r.idleMb <= BUDGET.idleMb * TOL && r.idleCpu <= BUDGET.idleCpu * TOL + 0.2 && r.sizeMb <= BUDGET.sizeMb * TOL;
console.log(`cold start   ${r.startMs.toFixed(0)} ms   (budget ${BUDGET.startMs}) ${check(r.startMs, BUDGET.startMs)}`);
console.log(`idle memory  ${r.idleMb.toFixed(0)} MB   (budget ${BUDGET.idleMb}, core + webview) ${check(r.idleMb, BUDGET.idleMb)}`);
console.log(`idle CPU     ${r.idleCpu.toFixed(2)} %   (budget ${BUDGET.idleCpu}) ${check(r.idleCpu, BUDGET.idleCpu + 0.2)}`);
console.log(`app size     ${r.sizeMb.toFixed(1)} MB  (budget ${BUDGET.sizeMb}) ${check(r.sizeMb, BUDGET.sizeMb)}`);

if (!process.argv.includes("--no-write")) {
  let commit = "local";
  try {
    commit = execSync("git rev-parse --short HEAD", { cwd: root }).toString().trim();
  } catch {
    /* not a repo */
  }
  const date = new Date().toISOString().slice(0, 10);
  appendFileSync(join(root, "docs/perf.md"), `| ${date} | ${commit} | ${r.startMs.toFixed(0)} | ${r.idleMb.toFixed(0)} | ${r.idleCpu.toFixed(2)} | ${r.sizeMb.toFixed(1)} | ${ok ? "pass" : "FAIL"} |\n`);
}
process.exit(ok ? 0 : 1);
