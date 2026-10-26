// Builds the per-app helper and stages it where Tauri's externalBin expects it
// (src-tauri/binaries/overlook-helper-<host triple>[.exe]). macOS needs no helper.
import { execFileSync } from "node:child_process";
import { copyFileSync, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

if (process.platform === "darwin") process.exit(0);

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const run = (cmd, args) => execFileSync(cmd, args, { cwd: root, encoding: "utf8", stdio: ["ignore", "pipe", "inherit"] });
const triple = run("rustc", ["-vV"]).match(/^host: (\S+)$/m)[1];
const ext = process.platform === "win32" ? ".exe" : "";

run("cargo", ["build", "-p", "overlook-helper", "--release"]);
mkdirSync(join(root, "src-tauri/binaries"), { recursive: true });
copyFileSync(join(root, `target/release/overlook-helper${ext}`), join(root, `src-tauri/binaries/overlook-helper-${triple}${ext}`));
