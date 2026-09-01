(() => {
  const moduleUrl = (path) => chrome.runtime.getURL(path);
  let navigation = 0;
  let hideArtifacts = null;
  let hidePage = null;
  const pendingHideRoots = new Set();
  let hideScheduled = false;
  let cleanup = () => {};

  function scheduleHide(mutations) {
    if (!hidePage || !hideArtifacts) return;
    for (const mutation of mutations) {
      for (const node of mutation.addedNodes) {
        if (node.nodeType === Node.ELEMENT_NODE) pendingHideRoots.add(node);
      }
    }
    if (!hideScheduled && pendingHideRoots.size) {
      hideScheduled = true;
      queueMicrotask(() => {
        hideScheduled = false;
        if (hidePage) {
          for (const root of pendingHideRoots) hideArtifacts(root, hidePage);
        }
        pendingHideRoots.clear();
      });
    }
  }

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

  async function fetchGloss(page, sourcePath, parseGlossFile, sourceToGlossPath, rawGlossUrl) {
    const glossPath = sourceToGlossPath(sourcePath);
    const url = glossPath ? rawGlossUrl(page, glossPath) : null;
    if (!url) return { glossPath, url, gloss: null, status: "idle" };
    try {
      const response = await fetch(url, { credentials: "include" });
      if (response.status === 404) return { glossPath, url, gloss: null, status: "missing" };
      if (!response.ok) throw new Error(`Gloss fetch failed with HTTP ${response.status}`);
      return { glossPath, url, gloss: parseGlossFile(await response.text()), status: "loaded" };
    } catch (error) {
      return {
        glossPath,
        url,
        gloss: null,
        status: "error",
        error: error instanceof Error ? error.message : String(error),
      };
    }
  }

  function filePath(root) {
    const direct = root.dataset.path ?? root.dataset.filePath ?? root.dataset.tagsearchPath;
    if (direct) return direct.replace(/^\/+/u, "");
    const pathElement = root.querySelector("[data-path], [data-file-path], [data-tagsearch-path]");
    const nested = pathElement?.dataset.path ?? pathElement?.dataset.filePath ?? pathElement?.dataset.tagsearchPath;
    if (nested) return nested.replace(/^\/+/u, "");
    const clipboard = root.querySelector('[data-copy-feedback="Copied!"], clipboard-copy[value]');
    return clipboard?.getAttribute("value")?.replace(/^\/+/u, "") ?? null;
  }

  function pullFiles() {
    const candidates = document.querySelectorAll(
      ".js-file, [data-file-path], [data-tagsearch-path], [data-testid^=diff-file]",
    );
    const seen = new Set();
    return [...candidates].flatMap((root) => {
      const path = filePath(root);
      if (!path || seen.has(path) || !root.querySelector("[data-line-number], .blob-num")) return [];
      seen.add(path);
      return [{ root, path }];
    });
  }

  function blobRoot() {
    return document.querySelector("[data-testid=code-viewer-container], .blob-wrapper, table.highlight")
      ?? document.querySelector("[data-line-number], td[id^=L]")?.closest("table, main");
  }

  async function boot() {
    cleanup();
    cleanup = () => {};
    const currentNavigation = ++navigation;
    const [
      { parseGlossFile },
      { sourceToGlossPath },
      { detectGitHubPage, rawGlossUrl },
      hide,
      { renderRail },
    ] = await Promise.all([
      import(moduleUrl("src/parse.js")),
      import(moduleUrl("src/paths.js")),
      import(moduleUrl("src/github.js")),
      import(moduleUrl("src/hide.js")),
      import(moduleUrl("src/rail.js")),
    ]);
    if (currentNavigation !== navigation) return null;

    hideArtifacts = hide.hideGlossArtifacts;
    const page = detectGitHubPage(location.href, pageMetadata());
    hidePage = page;
    if (hidePage) hideArtifacts(document, hidePage);
    const files = page?.kind === "blob" && page.path
      ? [{ root: blobRoot(), path: page.path, side: "blob" }]
      : page?.kind === "pull-files" ? pullFiles().map((file) => ({ ...file, side: "right" })) : [];
    const result = { page, files: [], status: files.length ? "loading" : "idle" };
    globalThis.__glossGitHub = result;
    if (!files.length) return result;

    const fetched = await Promise.all(files.map(async (file) => ({
      ...file,
      ...(await fetchGloss(page, file.path, parseGlossFile, sourceToGlossPath, rawGlossUrl)),
    })));
    if (currentNavigation !== navigation) return result;

    const rails = fetched.flatMap((file) => {
      result.files.push({ path: file.path, glossPath: file.glossPath, url: file.url, gloss: file.gloss, status: file.status });
      if (!file.root || file.status !== "loaded" || !file.gloss.records.length) return [];
      const rail = renderRail({ root: file.root, records: file.gloss.records, side: file.side });
      return rail ? [rail] : [];
    });
    result.status = fetched.some((file) => file.status === "error") ? "error" : "loaded";

    let frame = 0;
    const update = () => {
      if (frame) return;
      frame = requestAnimationFrame(() => {
        frame = 0;
        for (const rail of rails) rail.update();
      });
    };
    window.addEventListener("scroll", update, { passive: true });
    window.addEventListener("resize", update);
    cleanup = () => {
      if (frame) cancelAnimationFrame(frame);
      window.removeEventListener("scroll", update);
      window.removeEventListener("resize", update);
      for (const rail of rails) rail.destroy();
      for (const row of document.querySelectorAll(".gloss-line-highlight")) row.classList.remove("gloss-line-highlight");
    };
    return result;
  }

  const navigate = () => void boot();
  const leavePage = () => {
    hidePage = null;
  };
  document.addEventListener("turbo:before-render", leavePage);
  document.addEventListener("pjax:start", leavePage);
  document.addEventListener("turbo:load", navigate);
  document.addEventListener("pjax:end", navigate);
  new MutationObserver(scheduleHide).observe(document.documentElement, { childList: true, subtree: true });
  void boot();
})();
