import { isGlossPath } from "./paths.js";

const PATH_ATTRIBUTES = ["data-path", "data-file-path", "data-tagsearch-path", "data-tree-entry-path"];
const PATH_SELECTOR = PATH_ATTRIBUTES.map((attribute) => `[${attribute}]`).join(",");
const CANDIDATE_SELECTOR = `${PATH_SELECTOR},a[href*="/blob/"],a[href*="/tree/"]`;
const CONTAINER_SELECTOR = [
  "[data-gloss-file-container]",
  "[data-file-tree-item]",
  "[role=treeitem]",
  "[role=row]",
  ".js-file",
  "tr",
].join(",");

function pathFromHref(href, page) {
  if (typeof href !== "string") return null;
  try {
    const url = new URL(href, "https://github.com");
    const parts = url.pathname.split("/").filter(Boolean).map(decodeURIComponent);
    if (url.hostname !== "github.com" || parts[0] !== page?.owner || parts[1] !== page?.repo) return null;
    const kindIndex = parts.findIndex((part) => part === "blob" || part === "tree");
    if (kindIndex < 0) return null;
    const rest = parts.slice(kindIndex + 1).join("/");
    if (page.ref && (rest === page.ref || rest.startsWith(`${page.ref}/`))) {
      return rest.slice(page.ref.length).replace(/^\//u, "");
    }
    return parts.slice(kindIndex + 2).join("/");
  } catch {
    return null;
  }
}

export function artifactPath(element, page = null) {
  for (const attribute of PATH_ATTRIBUTES) {
    const path = element.getAttribute?.(attribute);
    if (path) return path;
  }
  return pathFromHref(element.getAttribute?.("href"), page);
}

function candidates(root) {
  const descendants = [...root.querySelectorAll(CANDIDATE_SELECTOR)];
  return root.matches?.(CANDIDATE_SELECTOR) ? [root, ...descendants] : descendants;
}

/** Hide file-level GitHub UI whose path identifies a generated Gloss artifact. */
export function hideGlossArtifacts(root = document, page = null) {
  let hidden = 0;
  for (const candidate of candidates(root)) {
    if (!isGlossPath(artifactPath(candidate, page))) continue;
    const container = candidate.closest(CONTAINER_SELECTOR);
    if (container && !container.hidden) {
      container.hidden = true;
      container.setAttribute?.("data-gloss-hidden", "");
      hidden += 1;
    }
  }
  return hidden;
}
