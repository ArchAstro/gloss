function cleanRepositoryPath(path) {
  if (typeof path !== "string") return null;
  const normalized = path.replace(/^\/+|\/+$/gu, "");
  if (!normalized || normalized.split("/").some((part) => !part || part === "." || part === "..")) {
    return null;
  }
  return normalized;
}

export function sourceToGlossPath(sourcePath) {
  const source = cleanRepositoryPath(sourcePath);
  if (!source) return null;
  const slash = source.lastIndexOf("/");
  const parent = slash < 0 ? "" : `${source.slice(0, slash)}/`;
  const name = source.slice(slash + 1);
  return `${parent}.gloss/${name}.gloss`;
}

export function glossToSourcePath(glossPath) {
  const gloss = cleanRepositoryPath(glossPath);
  if (!gloss) return null;

  const parts = gloss.split("/");
  if (parts.length < 2 || parts.at(-2) !== ".gloss") return null;
  const name = parts.at(-1);
  if (!name.endsWith(".gloss") || name === ".gloss") return null;

  return [...parts.slice(0, -2), name.slice(0, -6)].join("/");
}

export function isGlossPath(path) {
  return glossToSourcePath(path) !== null;
}

export function classifyArtifact(path) {
  if (isGlossPath(path)) return "gloss";
  return cleanRepositoryPath(path) ? "source" : null;
}
