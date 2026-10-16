// Audited producer pins, rather than a generic shell rewrite or user-installation edit.
// 2.1.1: gitHead 68d7d2d5ddfc0ed5746f9f6130892dda685e65dd.
// 2.2.2: gitHead 202e66e63343ec4f5a5e35ab590251d813d129ca.
import childProcess from "node:child_process";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { syncBuiltinESMExports } from "node:module";
import { dirname, isAbsolute, join } from "node:path";
import { pathToFileURL } from "node:url";

const producers = new Map([
  ["2.1.1", "4b76310393d756a0f111687cd9df899720f36f7c59b1eb1d86429484034ff91b"],
  ["2.2.2", "6b1bb4e7e9caaf4aeca1042ae4aec2dc2433b0b0e4b1614b38a4d41699902aaa"],
]);

try {
  const adapter = process.argv[2];
  const launcher = process.env.CODEX_PATH;
  if (!adapter || !isAbsolute(adapter) || !launcher || !isAbsolute(launcher)
    || process.platform !== "win32" || !process.env.HIROUTE_NATIVE_LAUNCH_DESCRIPTOR) {
    throw new Error("invalid managed launch");
  }
  const manifest = JSON.parse(readFileSync(join(dirname(adapter), "..", "package.json"), "utf8"));
  const digest = createHash("sha256").update(readFileSync(adapter)).digest("hex");
  if (manifest.name !== "@agentclientprotocol/codex-acp" || producers.get(manifest.version) !== digest) {
    throw new Error("unrecognized adapter producer");
  }
  const original = childProcess.spawn;
  childProcess.spawn = function (command, args, options) {
    if (manifest.version === "2.1.1" && command === `"${launcher}" app-server`) {
      if (!args || Array.isArray(args) || args.shell !== true) throw new Error("unexpected launch form");
      return original.call(this, launcher, ["app-server"], { ...args, shell: false });
    }
    if (command === launcher) {
      if (!Array.isArray(args) || args.length !== 1 || args[0] !== "app-server" || options?.shell) {
        throw new Error("unexpected launch arguments");
      }
      return original.call(this, launcher, args, { ...options, shell: false });
    }
    return original.call(this, command, args, options);
  };
  syncBuiltinESMExports();
  process.argv = [process.argv[0], adapter, ...process.argv.slice(3)];
  await import(pathToFileURL(adapter).href);
} catch {
  process.stderr.write("HiRoute Codex managed Windows adapter launch failed\n");
  process.exitCode = 1;
}
