import process from "node:process";
import console from "node:console";
import { chromium } from "playwright-core";
import { execSync } from "node:child_process";
const REPO = process.env.REPO, URL = process.env.URL;
const git = (a) => execSync(`git -C ${REPO} ${a}`, {encoding:"utf8"});
const b = await chromium.launch({ executablePath: process.env.CHROME, args:["--no-sandbox"] });
const p = await b.newPage({viewport:{width:1400,height:900}});
p.on("pageerror",e=>console.log("PAGEERR",e.message));
let retried = 0; p.on("request", r => { if (r.url().includes("/api/")) retried++; });
await p.goto(URL); await p.waitForSelector("button:has-text('Retry access check')");
console.log("start diff:", JSON.stringify(git("diff --stat -- frontend/src")));
const panel = p.locator(".p");
await p.locator("button.t").click();
console.log("toggle:", await p.locator("button.t").textContent());
// 1. clicking a button in edit mode must not fire it
const before = retried;
await p.locator("button:has-text('Retry access check')").click({force:true}); await p.waitForTimeout(800);
console.log("button fired in edit mode:", retried !== before, "(api requests", before, "->", retried + ")");
console.log("panel src:", await panel.locator("h3").textContent());
// 2. button colour
console.log("PANEL:", (await panel.innerText()).replace(/\n/g," | "));
await panel.locator("label:has-text('Background') + .sw i[title='card']").click();
console.log("save enabled after swatch:", await panel.locator("button.save").isEnabled());
await panel.locator("button.save").click(); await p.waitForTimeout(2500);
console.log("status:", await panel.locator(".ok, .err").allTextContents());
// 3. heading text
await p.locator("h1").click({force:true}); await p.waitForTimeout(500);
console.log("heading src:", await panel.locator("h3").textContent());
console.log("save disabled before edit:", await panel.locator("button.save").isDisabled());
await panel.locator("input[type=text]").first().fill("StagePilot Beta");
console.log("live preview:", await p.locator("h1").textContent());
await panel.locator("button.save").click(); await p.waitForTimeout(2500);
console.log("after HMR h1:", await p.locator("h1").textContent());
console.log("DIFF:\n" + git("diff -U0 -- frontend/src"));
console.log("files:", git("diff --name-only").trim().split("\n").join(", "));
// 4. revert
await panel.locator("button.revert").click(); await p.waitForTimeout(2500);
console.log("revert status:", await panel.locator(".ok, .err").allTextContents());
console.log("diff after revert:", JSON.stringify(git("diff --stat -- frontend/src")));
await p.screenshot({path: process.env.HOME+"/edit-mode.png"});
await b.close();
