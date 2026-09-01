const CARD_WIDTH = 296;
const CARD_GAP = 10;

/** Return whether two inclusive line ranges overlap. */
export function rangesOverlap(left, right) {
  return Boolean(left && right && left.start <= right.end && right.start <= left.end);
}

/** Convert parser records into the presentation model used by the rail. */
export function recordsToCards(records) {
  return records.map((record) => ({
    id: record.editId,
    explanation: record.explanation,
    agent: record.agent,
    user: record.user,
    range: { ...record.range },
    labels: [...(record.labels ?? [])],
    highRisk: record.risk === "high",
  }));
}

function lineNumber(element) {
  const value = element.dataset.lineNumber ?? element.id.match(/^L(?:C)?(\d+)$/u)?.[1];
  const number = Number(value);
  return Number.isInteger(number) && number > 0 ? number : null;
}

function rowForLineElement(element) {
  return element.closest("tr, [role=row]") ?? element;
}

function isDeletedOnlyRow(row) {
  if (row.querySelector('[data-side="right"][data-line-number]')) return false;
  return Boolean(row.querySelector(".blob-num-deletion, .blob-code-deletion"))
    && !row.querySelector(".blob-num-addition, .blob-code-addition");
}

/** Collect line rows. PR mode intentionally returns only right/new-file lines. */
export function collectLineRows(root, side = "blob") {
  const rows = new Map();
  const selectors = side === "right"
    ? ['[data-side="right"][data-line-number]', "[data-line-number]"]
    : ["[data-line-number]", "td[id^=L]", "[id^=LC]"];

  for (const selector of selectors) {
    for (const element of root.querySelectorAll(selector)) {
      const row = rowForLineElement(element);
      if (side === "right") {
        const explicitSide = element.dataset.side;
        if (explicitSide && explicitSide !== "right") continue;
        const numbered = [...row.querySelectorAll("[data-line-number]")];
        if (!explicitSide && numbered.at(-1) !== element) continue;
        if (isDeletedOnlyRow(row)) continue;
      }
      const number = lineNumber(element);
      if (number && !rows.has(number)) rows.set(number, row);
    }
    if (rows.size) break;
  }
  return rows;
}

export function rowsOverlappingRange(rows, range) {
  return [...rows.entries()]
    .filter(([number]) => number >= range.start && number <= range.end)
    .sort(([left], [right]) => left - right)
    .map(([, row]) => row);
}

function textElement(document, className, text) {
  const element = document.createElement("span");
  element.className = className;
  element.textContent = text;
  return element;
}

function cardElement(document, card, focus) {
  const element = document.createElement("button");
  element.type = "button";
  element.className = "gloss-card";
  element.dataset.glossId = card.id;
  if (card.highRisk) element.classList.add("gloss-card--high-risk");

  const heading = document.createElement("span");
  heading.className = "gloss-card__heading";
  heading.append(textElement(document, "gloss-card__range", `${card.range.start}:${card.range.end}`));
  if (card.highRisk) heading.append(textElement(document, "gloss-card__risk", "High risk"));
  element.append(heading, textElement(document, "gloss-card__explanation", card.explanation));

  const metadata = document.createElement("span");
  metadata.className = "gloss-card__metadata";
  metadata.append(
    textElement(document, "gloss-card__agent", `Agent: ${card.agent}`),
    textElement(document, "gloss-card__user", `User: ${card.user}`),
  );
  element.append(metadata);

  if (card.labels.length) {
    const labels = document.createElement("span");
    labels.className = "gloss-card__labels";
    for (const label of card.labels) labels.append(textElement(document, "gloss-card__label", label));
    element.append(labels);
  }
  element.addEventListener("focus", focus);
  element.addEventListener("click", focus);
  return element;
}

function highlightRows(rows) {
  for (const row of document.querySelectorAll(".gloss-line-highlight")) {
    row.classList.remove("gloss-line-highlight");
  }
  for (const row of rows) row.classList.add("gloss-line-highlight");
  rows[0]?.scrollIntoView({ behavior: "smooth", block: "center" });
}

/** Render cards for one file and return a geometry updater. */
export function renderRail({ root, records, side = "blob" }) {
  const rows = collectLineRows(root, side);
  const mapped = recordsToCards(records)
    .map((card) => ({ card, rows: rowsOverlappingRange(rows, card.range) }))
    .filter(({ rows: matchingRows }) => matchingRows.length);
  if (!mapped.length) return null;

  const rail = document.createElement("div");
  rail.className = "gloss-rail";
  rail.setAttribute("aria-label", "Gloss explanations");
  document.body.append(rail);

  const entries = mapped.map(({ card, rows: matchingRows }) => {
    const element = cardElement(document, card, () => highlightRows(matchingRows));
    rail.append(element);
    return { element, rows: matchingRows };
  });

  const update = () => {
    const rootRect = root.getBoundingClientRect();
    const left = Math.min(rootRect.right + CARD_GAP, window.innerWidth - CARD_WIDTH - CARD_GAP);
    let bottom = 0;
    for (const entry of entries) {
      const anchor = entry.rows[0].getBoundingClientRect().top + window.scrollY;
      const top = Math.max(anchor, bottom ? bottom + CARD_GAP : anchor);
      entry.element.style.top = `${top}px`;
      entry.element.style.left = `${Math.max(CARD_GAP, left)}px`;
      bottom = top + entry.element.offsetHeight;
    }
  };
  update();
  return { rail, update, destroy: () => rail.remove() };
}
