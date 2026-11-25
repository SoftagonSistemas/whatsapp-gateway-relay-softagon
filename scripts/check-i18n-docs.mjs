#!/usr/bin/env node
/**
 * Automated validation for Softagon Relay i18n catalogs, version consistency,
 * PII hygiene, and localized documentation screenshots.
 * Executed automatically by `.githooks/pre-push`.
 */
import fs from 'node:fs';
import path from 'node:path';

const LOCALES = ['pt-BR', 'en', 'es', 'fr', 'id', 'bew', 'su', 'jv', 'bbc', 'ms'];
const PII_DENYLIST = ['HermesPessoal', '558791508478', '87 99200-5656'];

let errors = 0;
function fail(msg) {
  console.error(`[check-i18n-docs] FAIL: ${msg}`);
  errors++;
}

function shapeKeys(obj, prefix = '') {
  if (Array.isArray(obj)) {
    return obj.flatMap((item, i) => shapeKeys(item, `${prefix}[${i}]`));
  }
  if (obj && typeof obj === 'object') {
    return Object.keys(obj)
      .sort()
      .flatMap((k) => shapeKeys(obj[k], prefix ? `${prefix}.${k}` : k));
  }
  return [prefix];
}

// 1. Version parity between Cargo.toml and console/package.json
const cargoToml = fs.readFileSync('Cargo.toml', 'utf8');
const cargoVersion = cargoToml.match(/^version\s*=\s*"([^"]+)"/m)?.[1];
const pkgVersion = JSON.parse(fs.readFileSync('console/package.json', 'utf8')).version;
if (!cargoVersion || cargoVersion !== pkgVersion) {
  fail(`Version mismatch: Cargo.toml=${cargoVersion} vs console/package.json=${pkgVersion}`);
}

// 2. i18n JSON key parity, placeholders, Intl.NumberFormat, and PII checks
const base = JSON.parse(fs.readFileSync('console/src/i18n/en.json', 'utf8'));
const baseKeys = shapeKeys(base);

for (const code of LOCALES) {
  const file = `console/src/i18n/${code}.json`;
  if (!fs.existsSync(file)) {
    fail(`Missing locale file ${file}`);
    continue;
  }
  const raw = fs.readFileSync(file, 'utf8');
  for (const forbidden of PII_DENYLIST) {
    if (raw.includes(forbidden)) {
      fail(`${file} contains disallowed PII/personal sample string "${forbidden}"`);
    }
  }

  let data;
  try {
    data = JSON.parse(raw);
  } catch (err) {
    fail(`${file} is not valid JSON: ${err.message}`);
    continue;
  }

  const keys = shapeKeys(data);
  const missing = baseKeys.filter((k) => !keys.includes(k));
  const extra = keys.filter((k) => !baseKeys.includes(k));
  if (missing.length || extra.length) {
    fail(`${file} key mismatch: missing=[${missing.join(', ')}] extra=[${extra.join(', ')}]`);
  }

  if (!/\b3\b/.test(data.common?.health ?? '')) {
    fail(`${file}: common.health must contain standalone "3" for dynamic session count replacement`);
  }
  if (!(data.home?.eyebrow ?? '').includes('{version}')) {
    fail(`${file}: home.eyebrow must include "{version}" interpolation placeholder`);
  }
  if (
    !(data.apiReference?.yourUrl ?? '').includes("{'{'}") ||
    !(data.apiReference?.yourUrl ?? '').includes("{'}'}")
  ) {
    fail(`${file}: apiReference.yourUrl must escape braces for vue-i18n v11`);
  }

  try {
    const formatted = new Intl.NumberFormat(code).format(3);
    if (!formatted) fail(`${file}: Intl.NumberFormat("${code}") returned empty string`);
  } catch (err) {
    fail(`${file}: Intl.NumberFormat("${code}") threw: ${err.message}`);
  }
}

// 3. Documentation & localized screenshot verification
if (!fs.existsSync('docs/images/demo.gif')) {
  fail('Missing docs/images/demo.gif');
}

for (const code of LOCALES) {
  const docPath = `docs/${code}/README.md`;
  if (!fs.existsSync(docPath)) {
    fail(`Missing documentation guide ${docPath}`);
    continue;
  }
  const doc = fs.readFileSync(docPath, 'utf8');
  const imgs = [...doc.matchAll(/!\[[^\]]*\]\(([^)]+)\)/g)].map((m) => m[1]);
  if (imgs.length < 6) {
    fail(`${docPath} expected at least 6 screenshots, found ${imgs.length}`);
  }
  for (const img of imgs) {
    if (!img.startsWith(`../images/${code}/`)) {
      fail(`${docPath} image "${img}" does not point to localized "../images/${code}/"`);
    }
    const resolved = path.resolve(path.dirname(docPath), img);
    if (!fs.existsSync(resolved)) {
      fail(`${docPath} broken image link: ${img}`);
    }
  }
}

if (errors > 0) {
  console.error(`[check-i18n-docs] Failed with ${errors} error(s).`);
  process.exit(1);
}

console.log(
  `[check-i18n-docs] OK: v${cargoVersion}, ${LOCALES.length} locales (${baseKeys.length} leaf keys), 0 PII hits, ${LOCALES.length * 6} localized screenshots + demo.gif verified.`,
);
