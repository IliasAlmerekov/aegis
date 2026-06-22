#!/usr/bin/env node
"use strict";

const { spawnSync } = require("node:child_process");
const path = require("node:path");

const binary = path.join(__dirname, "..", "vendor", process.platform === "win32" ? "aegis.exe" : "aegis");
const result = spawnSync(binary, process.argv.slice(2), {
  stdio: "inherit"
});

if (result.error) {
  console.error(`failed to run ${binary}: ${result.error.message}`);
  process.exit(127);
}

if (result.signal) {
  process.kill(process.pid, result.signal);
}

process.exit(result.status === null ? 1 : result.status);