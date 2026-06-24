"use strict";

const crypto = require("node:crypto");
const fs = require("node:fs");
const https = require("node:https");
const os = require("node:os");
const path = require("node:path");
const { spawnSync } = require("node:child_process");

const packageRoot = path.join(__dirname, "..");
const vendorDir = path.join(packageRoot, "vendor");
const binaryPath = path.join(vendorDir, "aegis");
const checksumsPath = path.join(packageRoot, "checksums.json");

const platform = process.env.AEGIS_NPM_PLATFORM || process.platform;
const arch = process.env.AEGIS_NPM_ARCH || process.arch;

const assets = {
  "linux:x64": "aegis-linux-x86_64",
  "linux:arm64": "aegis-linux-aarch64",
  "darwin:x64": "aegis-macos-x86_64",
  "darwin:arm64": "aegis-macos-aarch64"
};

function fail(message) {
  console.error(message);
  process.exit(1);
}

// Binary-only install: never edit shell startup files here. Point users at the
// explicit opt-in `aegis setup-shell` command so enabling the shell proxy is a
// deliberate user action, not a side effect of `npm i -g`.
function printNextSteps() {
  console.log("Aegis installed.");
  console.log("");
  console.log("Test:");
  console.log("  aegis -c 'echo hello'");
  console.log("");
  console.log("Enable shell-proxy mode for tools that launch commands via $SHELL -c:");
  console.log("  aegis setup-shell");
  console.log("");
  console.log("Undo shell-proxy setup:");
  console.log("  aegis setup-shell --remove");
}

// Best-effort agent hook setup. Only runs `aegis install-hooks --all` when a
// supported agent config directory already exists; never creates one. Failures
// here must not fail the npm install — the binary is already in place and the
// user can run `aegis install-hooks --all` manually. Set AEGIS_NPM_SKIP_HOOKS=1
// to opt out (used by deterministic packaging tests).
function maybeInstallAgentHooks() {
  if (process.env.AEGIS_NPM_SKIP_HOOKS === "1") {
    return;
  }

  const home = os.homedir();
  if (!home) {
    return;
  }

  const claudeDir = path.join(home, ".claude");
  const codexDir = path.join(home, ".codex");
  const hasAgentDir = fs.existsSync(claudeDir) || fs.existsSync(codexDir);

  if (!hasAgentDir) {
    console.log("");
    console.log("If you install Claude Code or Codex later, run:");
    console.log("  aegis install-hooks --all");
    return;
  }

  try {
    const result = spawnSync(binaryPath, ["install-hooks", "--all"], {
      stdio: "inherit"
    });
    if (result.error) {
      console.log("");
      console.log("Could not auto-install agent hooks; run manually with:");
      console.log("  aegis install-hooks --all");
    }
  } catch (_error) {
    console.log("");
    console.log("Could not auto-install agent hooks; run manually with:");
    console.log("  aegis install-hooks --all");
  }
}

function readChecksums() {
  const raw = fs.readFileSync(checksumsPath, "utf8");
  return JSON.parse(raw);
}

function selectedAsset() {
  const asset = assets[`${platform}:${arch}`];
  if (!asset) {
    fail(`Unsupported platform or architecture: ${platform}/${arch}`);
  }
  return asset;
}

function releaseUrl(checksums, asset) {
  const repo = process.env.AEGIS_NPM_REPO || checksums.repo;
  const release = process.env.AEGIS_NPM_RELEASE || checksums.release;
  const baseUrl = process.env.AEGIS_NPM_BASE_URL || `https://github.com/${repo}/releases/download/${release}`;
  return `${baseUrl}/${asset}`;
}

const MAX_REDIRECTS = 5;

function download(url, destination, redirects = 0) {
  return new Promise((resolve, reject) => {
    if (redirects > MAX_REDIRECTS) {
      reject(new Error(`too many redirects for ${url}`));
      return;
    }

    https.get(url, response => {
      const status = response.statusCode;

      // GitHub release asset URLs respond with 301/302/303/307/308 and a
      // Location header pointing at release-assets.githubusercontent.com. Node's
      // https.get does not follow redirects automatically, so follow them here.
      if (status === 301 || status === 302 || status === 303 || status === 307 || status === 308) {
        const location = response.headers.location;
        response.resume();
        if (!location) {
          reject(new Error(`redirect ${status} without Location header from ${url}`));
          return;
        }
        const nextUrl = new URL(location, url).toString();
        download(nextUrl, destination, redirects + 1).then(resolve, reject);
        return;
      }

      if (status !== 200) {
        response.resume();
        reject(new Error(`download failed with HTTP ${status}: ${url}`));
        return;
      }

      const file = fs.createWriteStream(destination, { mode: 0o755 });
      response.pipe(file);
      file.on("finish", () => {
        file.close(resolve);
      });
      file.on("error", error => {
        fs.rmSync(destination, { force: true });
        reject(error);
      });
    }).on("error", error => {
      reject(error);
    });
  });
}

function sha256(filePath) {
  const hash = crypto.createHash("sha256");
  hash.update(fs.readFileSync(filePath));
  return hash.digest("hex");
}

async function main() {
  const checksums = readChecksums();
  const asset = selectedAsset();
  const expected = checksums.assets[asset];
  if (!expected) {
    fail(`missing SHA256 for ${asset}`);
  }

  fs.mkdirSync(vendorDir, { recursive: true });

  if (process.env.AEGIS_NPM_SKIP_DOWNLOAD === "1") {
    fs.writeFileSync(binaryPath, "#!/bin/sh\nprintf 'aegis test binary\\n'\n", { mode: 0o755 });
    printNextSteps();
    maybeInstallAgentHooks();
    return;
  }

  const tmpPath = `${binaryPath}.tmp`;
  await download(releaseUrl(checksums, asset), tmpPath);

  const actual = sha256(tmpPath);
  if (actual !== expected.toLowerCase()) {
    fs.rmSync(tmpPath, { force: true });
    fail(`SHA256 mismatch for ${asset}: expected ${expected}, got ${actual}`);
  }

  fs.renameSync(tmpPath, binaryPath);
  fs.chmodSync(binaryPath, 0o755);
  printNextSteps();
  maybeInstallAgentHooks();
}

main().catch(error => {
  fail(error.message);
});