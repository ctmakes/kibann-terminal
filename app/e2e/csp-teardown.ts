// After the run: the violations the `webkit-csp` project's pages reported
// (vite.config.ts serves them the release CSP). Any fails the run: the
// release build would drop that style or script.

import { existsSync, readFileSync } from "node:fs";
import { CSP_REPORTS } from "../vite.config";

export default function teardown(): void {
  if (!existsSync(CSP_REPORTS)) return;
  const reports = readFileSync(CSP_REPORTS, "utf8").trim();
  if (!reports) return;
  const seen = new Set(
    reports.split("\n").map((line) => {
      const r = JSON.parse(line)["csp-report"] ?? {};
      const where = r["source-file"] ? ` at ${r["source-file"]}:${r["line-number"]}` : "";
      return `${r["violated-directive"]} blocked ${r["blocked-uri"] || "inline"} on ${r["document-uri"]}${where}`;
    }),
  );
  throw new Error(`The release CSP blocked:\n${[...seen].join("\n")}`);
}
