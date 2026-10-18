// Copies the version in package.json (the single source of truth) to the Rust app crate.
// tauri.conf.json reads package.json itself. Runs as npm's "version" script, so
// `npm version minor` bumps everything, commits and tags in one go (see README, Releasing).
import { readFileSync, writeFileSync } from "node:fs";

const { version } = JSON.parse(readFileSync("package.json", "utf8"));

function replace(file, pattern, label) {
  const text = readFileSync(file, "utf8");
  if (!pattern.test(text)) throw new Error(`${file}: ${label} not found`);
  writeFileSync(file, text.replace(pattern, (_, head) => `${head}"${version}"`));
}

// The first `version =` in src-tauri/Cargo.toml is the [package] one.
replace("src-tauri/Cargo.toml", /^(version = )"[^"]*"/m, "package version");
replace("Cargo.lock", /(name = "steam-controller-app"\r?\nversion = )"[^"]*"/, "steam-controller-app entry");
console.log(`Version ${version}`);
