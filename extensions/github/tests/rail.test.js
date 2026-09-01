import assert from "node:assert/strict";
import test from "node:test";

import { rangesOverlap, recordsToCards, rowsOverlappingRange } from "../src/rail.js";

test("maps parsed Gloss records to cards with metadata, labels, and high-risk state", () => {
  const cards = recordsToCards([
    {
      editId: "edit-high",
      range: { start: 42, end: 58 },
      explanation: "Keep parsing separate from validation.",
      agent: "codex",
      user: "calvin",
      labels: ["parser", "api-boundary"],
      risk: "high",
    },
    {
      editId: "edit-medium",
      range: { start: 60, end: 60 },
      explanation: "Preserve this fallback.",
      agent: "claude",
      user: "maya",
      labels: ["compatibility"],
      risk: "medium",
    },
  ]);

  assert.deepEqual(cards[0], {
    id: "edit-high",
    explanation: "Keep parsing separate from validation.",
    agent: "codex",
    user: "calvin",
    range: { start: 42, end: 58 },
    labels: ["parser", "api-boundary"],
    highRisk: true,
  });
  assert.equal(cards[1].highRisk, false, "non-high risk values are not marked as high-risk");
});

test("matches inclusive record ranges against available right-side lines", () => {
  assert.equal(rangesOverlap({ start: 10, end: 12 }, { start: 12, end: 15 }), true);
  assert.equal(rangesOverlap({ start: 10, end: 11 }, { start: 12, end: 15 }), false);

  const line10 = { side: "right", number: 10 };
  const line12 = { side: "right", number: 12 };
  const line15 = { side: "right", number: 15 };
  const availableRightRows = new Map([[10, line10], [12, line12], [15, line15]]);

  assert.deepEqual(rowsOverlappingRange(availableRightRows, { start: 11, end: 15 }), [line12, line15]);
  assert.deepEqual(rowsOverlappingRange(availableRightRows, { start: 16, end: 20 }), []);
});
