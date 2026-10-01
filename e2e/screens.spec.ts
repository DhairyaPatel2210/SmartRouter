import { test, expect, type Page } from "@playwright/test";
import { readFileSync } from "node:fs";

const mock = readFileSync(new URL("./tauri-mock.js", import.meta.url), "utf8");
const shots = process.env.SHOTS_DIR;

async function open(page: Page, hash = "") {
  await page.addInitScript(mock);
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(e.message));
  await page.goto(`/#${hash}`);
  return errors;
}

async function shot(page: Page, name: string) {
  if (shots) await page.screenshot({ path: `${shots}/${name}.png` });
}

test("home renders workspace, modes, pre-flight and status bar", async ({ page }) => {
  const errors = await open(page);
  await expect(page.getByText("acme-api").first()).toBeVisible();
  await expect(page.getByText("Balanced").first()).toBeVisible();
  await expect(page.getByText(/after load: Tight/)).toBeVisible();
  await expect(page.getByText(/uncommitted change/)).toBeVisible();
  await expect(page.getByText(/GB free/).first()).toBeVisible();
  await shot(page, "home");
  expect(errors).toEqual([]);
});

test("run view shows steps, tiers, totals and logs", async ({ page }) => {
  const errors = await open(page, "run");
  await page.getByText("Add input validation to the API with tests").first().click();
  await expect(page.getByText("Fix the auth token refresh race").first()).toBeVisible();
  await expect(page.getByText(/saved ~/)).toBeVisible();
  await expect(page.getByText(/Added a mutex/).first()).toBeVisible();
  await shot(page, "run");
  expect(errors).toEqual([]);
});

for (const [key, name, text] of [
  ["2", "library", "Conventional commits"],
  ["3", "agents", "Planner & reviewer"],
  ["4", "telemetry", "Memory over the last 5 minutes"],
  ["5", "workflows", "Templates"],
  ["6", "settings", "Appearance"],
] as const) {
  test(`${name} screen renders`, async ({ page }) => {
    const errors = await open(page);
    await expect(page.getByText("acme-api").first()).toBeVisible();
    await page.keyboard.press(`Meta+${key}`);
    await expect(page.getByText(text).first()).toBeVisible();
    await page.waitForTimeout(300);
    await shot(page, name);
    expect(errors).toEqual([]);
  });
}

test("onboarding first screen", async ({ page }) => {
  const errors = await open(page, "onboarding");
  await expect(page.getByText(/Welcome to/)).toBeVisible();
  await expect(page.getByText(/Scanned in/)).toBeVisible();
  await shot(page, "onboarding");
  await page.getByRole("button", { name: /Continue/ }).click();
  await expect(page.getByText("Pick your style")).toBeVisible();
  await page.getByRole("button", { name: /Continue/ }).click();
  await expect(page.getByText("Choose your executor")).toBeVisible();
  await shot(page, "onboarding-executor");
  expect(errors).toEqual([]);
});

test("approval dialog appears from a core event", async ({ page }) => {
  await open(page);
  await expect(page.getByText("acme-api").first()).toBeVisible();
  await page.evaluate(() =>
    (window as unknown as { __emit: (e: string, p: unknown) => void }).__emit("orch://events", [
      { type: "approval", request: { id: "a1", run_id: "r1", step_id: "s3", kind: "escalate", title: "Step 3 failed 4 times. Escalate to Claude Code?", body: "Cost mode asks before using the paid agent.", options: [{ id: "escalate", label: "Escalate to Claude Code", primary: true, danger: false }, { id: "stop", label: "Stop run", primary: false, danger: true }] } },
    ]),
  );
  await expect(page.getByText(/Escalate to Claude Code\?/)).toBeVisible();
  await shot(page, "approval");
});
