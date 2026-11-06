// Builds data/cities.txt from the GeoNames "cities15000" dump (CC BY 4.0, see
// THIRD_PARTY_NOTICES.md). Usage: node scripts/build-cities.mjs path/to/cities15000.txt
// One line per city: name|country|state (US only, else empty)|lat|lon  (coordinates to 2 dp).
import { readFileSync, writeFileSync } from "node:fs";

const [, , input] = process.argv;
if (!input) { console.error("usage: node scripts/build-cities.mjs cities15000.txt"); process.exit(2); }
const lines = readFileSync(input, "utf8").split("\n").filter(Boolean);
const rows = lines.map((line) => {
  const f = line.split("\t");
  const [name, lat, lon, , , country, , admin1] = [f[1], f[4], f[5], f[6], f[7], f[8], f[9], f[10]];
  const state = country === "US" ? admin1 : "";
  return `${name.replace(/[|\n]/g, " ")}|${country}|${state}|${Number(lat).toFixed(2)}|${Number(lon).toFixed(2)}`;
});
writeFileSync(new URL("../data/cities.txt", import.meta.url), rows.join("\n") + "\n");
console.log(`${rows.length} cities`);
