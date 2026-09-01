import assert from "node:assert/strict";
import test from "node:test";

import { detectGitHubPage, rawGlossUrl } from "../src/github.js";
import { classifyArtifact, glossToSourcePath, isGlossPath, sourceToGlossPath } from "../src/paths.js";

test("source and gloss paths round-trip through sibling .gloss directories", () => {
  assert.equal(sourceToGlossPath("src/foo.ex"), "src/.gloss/foo.ex.gloss");
  assert.equal(glossToSourcePath("src/.gloss/foo.ex.gloss"), "src/foo.ex");
  assert.equal(sourceToGlossPath("README.md"), ".gloss/README.md.gloss");
  assert.equal(glossToSourcePath(".gloss/README.md.gloss"), "README.md");
  assert.equal(sourceToGlossPath("src/.gloss/foo.ex.gloss"), "src/.gloss/.gloss/foo.ex.gloss.gloss");
  assert.equal(isGlossPath("src/.annotations/foo.ex.gloss"), true);
  assert.equal(glossToSourcePath("src/.annotations/foo.ex.gloss"), null);
  assert.equal(classifyArtifact("src/.annotations/foo.ex.gloss"), "gloss");
});

test("GitHub page detection returns blob, PR files, and tree models and rejects unsupported URLs", () => {
  assert.deepEqual(detectGitHubPage("https://github.com/acme/widgets/blob/main/src/foo.ex"), {
    kind: "blob", owner: "acme", repo: "widgets", ref: "main", path: "src/foo.ex",
  });
  assert.deepEqual(detectGitHubPage("https://github.com/acme/widgets/pull/42/files", { ref: "feature/gloss" }), {
    kind: "pull-files", owner: "acme", repo: "widgets", pullNumber: 42, ref: "feature/gloss", path: null,
  });
  assert.deepEqual(detectGitHubPage("https://github.com/acme/widgets/tree/main/src"), {
    kind: "tree", owner: "acme", repo: "widgets", ref: "main", path: "src",
  });
  assert.deepEqual(
    detectGitHubPage("https://github.com/acme/widgets/blob/feature/gloss/src/foo.ex", {
      ref: "feature/gloss", path: "src/foo.ex",
    }),
    { kind: "blob", owner: "acme", repo: "widgets", ref: "feature/gloss", path: "src/foo.ex" },
  );
  assert.equal(detectGitHubPage("https://gitlab.com/acme/widgets/blob/main/src/foo.ex"), null);
  assert.equal(detectGitHubPage("https://github.com/acme/widgets/issues/1"), null);
});

test("same-ref raw gloss URLs use the detected repository and sibling gloss path", () => {
  const page = detectGitHubPage("https://github.com/acme/widgets/blob/main/src/foo.ex");
  const glossPath = sourceToGlossPath(page.path);
  assert.equal(
    rawGlossUrl(page, glossPath),
    "https://github.com/acme/widgets/raw/main/src/.gloss/foo.ex.gloss",
  );
});
