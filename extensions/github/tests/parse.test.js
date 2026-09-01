import assert from "node:assert/strict";
import test from "node:test";

import { parseGlossFile, renderGlossFile } from "../src/parse.js";

const FIXTURE = "version: 1\nupdated: 2026-08-28T18:42:11Z\neditor: codex\n\n0198f5cf-4807-7ac3-a42a-938ff9b78220 42:58 2026-08-28T18:41:53Z calvin codex sess_123 Explain the intent.\n";

test("Gloss format v1 parses and renders the Rust fixture record", () => {
  const parsed = parseGlossFile(FIXTURE);
  assert.equal(parsed.version, 1);
  assert.equal(parsed.updated.toISOString(), "2026-08-28T18:42:11.000Z");
  assert.deepEqual(parsed.records[0].range, { start: 42, end: 58 });
  assert.equal(parsed.records[0].explanation, "Explain the intent.");
  assert.equal(renderGlossFile(parsed), FIXTURE);
});

test("Gloss format v1 accepts header-only files", () => {
  const parsed = parseGlossFile("version: 1\nupdated: 2026-08-28T18:42:11Z\neditor: codex\n\n");
  assert.deepEqual(parsed.records, []);
});

test("Gloss format v1 rejects malformed headers, UUIDs, and blank record lines", () => {
  assert.throws(() => parseGlossFile(""), /version header/u);
  assert.throws(() => parseGlossFile(FIXTURE.replace("updated: ", "changed: ")), /updated/u);
  assert.throws(() => parseGlossFile(FIXTURE.replace("0198f5cf-4807-7ac3-a42a-938ff9b78220", "not-a-uuid")), /UUID/u);
  assert.throws(() => parseGlossFile(FIXTURE.replace("\n0198", "\n\n0198")), /blank line/u);
});

test("Gloss format v1 normalizes UUID forms accepted by Rust", () => {
  const compact = FIXTURE.replace("0198f5cf-4807-7ac3-a42a-938ff9b78220", "0198f5cf48077ac3a42a938ff9b78220");
  assert.equal(renderGlossFile(parseGlossFile(compact)), FIXTURE);
});

test("Gloss format v1 enforces Rust range, timestamp, and field separators", () => {
  assert.throws(() => parseGlossFile(FIXTURE.replace("42:58", "0:58")), /line range/u);
  assert.throws(() => parseGlossFile(FIXTURE.replace("42:58", "42:4294967296")), /line range/u);
  assert.throws(() => parseGlossFile(FIXTURE.replace("2026-08-28T18:42:11Z", "2026-02-30T18:42:11Z")), /RFC 3339/u);
  assert.throws(() => parseGlossFile(FIXTURE.replace("2026-08-28T18:42:11Z", "2026-08-28T18:42:11.1234567890Z")), /RFC 3339/u);
  assert.throws(() => parseGlossFile(FIXTURE.replace(" 42:58", "  42:58")), /invalid record/u);
});

test("Gloss rendering preserves Rust timestamp precision and normalizes offsets", () => {
  const input = FIXTURE
    .replace("2026-08-28T18:42:11Z", "2026-08-28T20:42:11.123456789+02:00")
    .replace("2026-08-28T18:41:53Z", "2026-08-28T18:41:53.123456Z");
  const expected = FIXTURE
    .replace("2026-08-28T18:42:11Z", "2026-08-28T18:42:11.123456789Z")
    .replace("2026-08-28T18:41:53Z", "2026-08-28T18:41:53.123456Z");
  assert.equal(renderGlossFile(parseGlossFile(input)), expected);
});

test("Gloss format v1 accepts RFC 3339 leap seconds supported by Chrono", () => {
  const input = FIXTURE.replace("2026-08-28T18:41:53Z", "2015-02-18T23:59:60.234567+05:00");
  const expected = FIXTURE.replace("2026-08-28T18:41:53Z", "2015-02-18T18:59:60.234567Z");
  assert.equal(renderGlossFile(parseGlossFile(input)), expected);
});

test("Gloss format v1 validates leap days without JavaScript's 1900 year coercion", () => {
  const input = FIXTURE.replace("2026-08-28T18:41:53Z", "0000-02-29T18:41:53Z");
  const expected = FIXTURE.replace("2026-08-28T18:41:53Z", "0000-02-29T18:41:53Z");
  assert.equal(renderGlossFile(parseGlossFile(input)), expected);
});
