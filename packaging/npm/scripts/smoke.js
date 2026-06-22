"use strict";

const { spawnSync } = require("node:child_process");
const path = require("node:path");

const shim = path.join(__dirname, "..", "bin", "aegis.js");
const result = spawnSync(process.execPath, [shim, "--version"], {
  encoding: "utf8"
});

if (result.status !== 0) {
  process.stderr.write(result.stderr);
  process.stdout.write(result.stdout);
  process.exit(result.status === null ? 1 : result.status);
}

if (!result.stdout.includes("aegis")) {
  process.stderr.write(`expected aegis version output, got: ${result.stdout}`);
  process.exit(1);
}