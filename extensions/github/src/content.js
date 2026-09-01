(() => {
  const moduleUrl = (path) => chrome.runtime.getURL(path);
  let navigation = 0;

  function findCodeLocation(value) {
    if (!value || typeof value !== "object") return null;
    if (typeof value.path === "string" && typeof value.refInfo?.name === "string") {
      return { ref: value.refInfo.name, path: value.path };
    }
    for (const child of Object.values(value)) {
      const location = findCodeLocation(child);
      if (location) return location;
    }
    return null;
  }

  function embeddedCodeLocation() {
    for (const script of document.querySelectorAll('script[data-target="react-app.embeddedData"]')) {
      try {
        const location = findCodeLocation(JSON.parse(script.textContent));
        if (location) return location;
      } catch {
        // Ignore unrelated or transient embedded-data scripts during navigation.
      }
    }
    return null;
  }

  function pageMetadata() {
    const embedded = embeddedCodeLocation();
    const commits = document.querySelectorAll("[data-commit]");
    const pullHeadCommit = commits.item(commits.length - 1)?.dataset.commit ?? null;
    return {
      ref: document.querySelector("[data-gloss-ref]")?.dataset.glossRef
        ?? embedded?.ref
        ?? pullHeadCommit,
      path: document.querySelector("[data-gloss-path]")?.dataset.glossPath
        ?? embedded?.path
        ?? null,
    };
  }

  async function boot() {
    const currentNavigation = ++navigation;
    const [{ parseGlossFile }, { sourceToGlossPath }, { detectGitHubPage, rawGlossUrl }] = await Promise.all([
      import(moduleUrl("src/parse.js")),
      import(moduleUrl("src/paths.js")),
      import(moduleUrl("src/github.js")),
    ]);

    const page = detectGitHubPage(location.href, pageMetadata());
    const glossPath = page?.kind === "blob" && page.path ? sourceToGlossPath(page.path) : null;
    const url = glossPath ? rawGlossUrl(page, glossPath) : null;
    const result = { page, glossPath, url, gloss: null, status: url ? "loading" : "idle" };
    globalThis.__glossGitHub = result;

    if (!url) return result;
    try {
      const response = await fetch(url, { credentials: "include" });
      if (currentNavigation !== navigation) return result;
      if (response.status === 404) {
        result.status = "missing";
      } else if (!response.ok) {
        throw new Error(`Gloss fetch failed with HTTP ${response.status}`);
      } else {
        result.gloss = parseGlossFile(await response.text());
        result.status = "loaded";
      }
    } catch (error) {
      if (currentNavigation === navigation) {
        result.status = "error";
        result.error = error instanceof Error ? error.message : String(error);
      }
    }
    return result;
  }

  const navigate = () => void boot();
  document.addEventListener("turbo:load", navigate);
  document.addEventListener("pjax:end", navigate);
  void boot();
})();
