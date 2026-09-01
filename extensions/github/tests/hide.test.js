import assert from "node:assert/strict";
import test from "node:test";

import { artifactPath, hideGlossArtifacts } from "../src/hide.js";
import { classifyArtifact, isGlossPath } from "../src/paths.js";

class FixtureContainer {
  constructor() {
    this.hidden = false;
    this.attributes = new Map();
  }

  setAttribute(name, value) {
    this.attributes.set(name, value);
  }
}

class FixtureCandidate {
  constructor(attributes, container) {
    this.attributes = attributes;
    this.container = container;
  }

  getAttribute(name) {
    return this.attributes[name] ?? null;
  }

  closest() {
    return this.container;
  }
}

class FixtureRoot {
  constructor(candidates = []) {
    this.candidates = candidates;
  }

  querySelectorAll() {
    return this.candidates;
  }
}

function fixture(attributes) {
  const container = new FixtureContainer();
  return { candidate: new FixtureCandidate(attributes, container), container };
}

test("classifies only .gloss path segments and .gloss filenames as artifacts", () => {
  for (const path of [".gloss", ".gloss/README.md.gloss", "src/.gloss", "src/.gloss/foo.js", "notes/design.gloss"]) {
    assert.equal(isGlossPath(path), true, path);
    assert.equal(classifyArtifact(path), "gloss", path);
  }
  for (const path of ["src/gloss/foo.js", "src/.glossary/foo.js", ".gitattributes", ".github/workflows/ci.yml", "src/app.js"]) {
    assert.equal(isGlossPath(path), false, path);
    assert.equal(classifyArtifact(path), "source", path);
  }
});

test("hides fixture tree rows and PR diff blocks for Gloss artifacts only", () => {
  const directory = fixture({ href: "/acme/repo/tree/main/src/.gloss" });
  const source = fixture({ href: "/acme/repo/blob/main/src/app.js" });
  const glossFile = fixture({ "data-file-path": "src/.gloss/app.js.gloss" });
  const setup = fixture({ "data-path": ".gitattributes" });
  const root = new FixtureRoot([directory.candidate, source.candidate, glossFile.candidate, setup.candidate]);

  assert.equal(hideGlossArtifacts(root, { owner: "acme", repo: "repo", ref: "main" }), 2);
  assert.equal(directory.container.hidden, true);
  assert.equal(glossFile.container.hidden, true);
  assert.equal(source.container.hidden, false);
  assert.equal(setup.container.hidden, false);
  assert.equal(artifactPath(glossFile.candidate), "src/.gloss/app.js.gloss");
});

test("re-applies hiding to artifact rows added after initial render", () => {
  const source = fixture({ "data-path": "src/app.js" });
  const root = new FixtureRoot([source.candidate]);
  const page = { owner: "acme", repo: "repo", ref: "feature/ref" };
  assert.equal(hideGlossArtifacts(root, page), 0);

  const addedGloss = fixture({ href: "/acme/repo/blob/feature/ref/src/app.js.gloss" });
  root.candidates.push(addedGloss.candidate);
  assert.equal(hideGlossArtifacts(root, page), 1);
  assert.equal(addedGloss.container.hidden, true);
  assert.equal(hideGlossArtifacts(root, page), 0);
});
