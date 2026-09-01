const SUPPORTED_KINDS = new Set(["blob", "tree"]);

function decodeParts(pathname) {
  try {
    return pathname.split("/").filter(Boolean).map(decodeURIComponent);
  } catch {
    return null;
  }
}

function cleanMetadataPath(value) {
  return typeof value === "string" ? value.replace(/^\/+|\/+$/gu, "") : null;
}

function refAndPath(rest, metadata) {
  const metadataRef = cleanMetadataPath(metadata?.ref);
  const metadataPath = cleanMetadataPath(metadata?.path);
  if (metadataRef) {
    const encoded = rest.join("/");
    if (encoded === metadataRef || encoded.startsWith(`${metadataRef}/`)) {
      return {
        ref: metadataRef,
        path: metadataPath ?? encoded.slice(metadataRef.length).replace(/^\//u, ""),
      };
    }
  }
  return { ref: rest[0] ?? null, path: rest.slice(1).join("/") };
}

/** Detect a supported GitHub page. DOM-derived ref/path metadata resolves slash refs. */
export function detectGitHubPage(url, metadata = {}) {
  let parsed;
  try {
    parsed = new URL(url);
  } catch {
    return null;
  }
  if (parsed.protocol !== "https:" || parsed.hostname !== "github.com") return null;

  const parts = decodeParts(parsed.pathname);
  if (!parts || parts.length < 3) return null;
  const [owner, repo, kind, ...rest] = parts;
  if (!owner || !repo) return null;

  if (SUPPORTED_KINDS.has(kind) && rest.length) {
    const location = refAndPath(rest, metadata);
    return { kind, owner, repo, ref: location.ref, path: location.path || null };
  }

  if (kind === "pull" && /^\d+$/u.test(rest[0] ?? "") && rest[1] === "files" && rest.length === 2) {
    return {
      kind: "pull-files",
      owner,
      repo,
      pullNumber: Number(rest[0]),
      ref: cleanMetadataPath(metadata.ref),
      path: cleanMetadataPath(metadata.path),
    };
  }
  return null;
}

export function rawGlossUrl(page, glossPath) {
  if (!page?.owner || !page.repo || !page.ref || !glossPath) return null;
  const segments = [page.owner, page.repo, "raw", page.ref, glossPath]
    .flatMap((value) => value.split("/"))
    .map(encodeURIComponent);
  return `https://github.com/${segments.join("/")}`;
}
