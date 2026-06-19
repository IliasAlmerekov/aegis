"use strict";

const crypto = require("node:crypto");
const fs = require("node:fs");
const https = require("node:https");
const path = require("node:path");

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

function download(url, destination) {
  return new Promise((resolve, reject) => {
    const file = fs.createWriteStream(destination, { mode: 0o755 });
    https.get(url, response => {
      if (response.statusCode !== 200) {
        file.close();
        fs.rmSync(destination, { force: true });
        reject(new Error(`download failed with HTTP ${response.statusCode}: ${url}`));
        return;
      }

      response.pipe(file);
      file.on("finish", () => {
        file.close(resolve);
      });
    }).on("error", error => {
      file.close();
      fs.rmSync(destination, { force: true });
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
}

main().catch(error => {
  fail(error.message);
});