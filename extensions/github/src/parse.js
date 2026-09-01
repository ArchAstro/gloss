const RFC3339 = /^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2}):(\d{2})(?:\.(\d+))?(?:Z|[+-](\d{2}):(\d{2}))$/;
const UUID_FORMS = /^(?:([0-9a-f]{32})|(?:urn:uuid:)?([0-9a-f]{8})-([0-9a-f]{4})-([0-9a-f]{4})-([0-9a-f]{4})-([0-9a-f]{12})|\{([0-9a-f]{8})-([0-9a-f]{4})-([0-9a-f]{4})-([0-9a-f]{4})-([0-9a-f]{12})\})$/i;
const TOKEN = /^\S+$/u;
const U32_MAX = 0xffff_ffff;
const NORMALIZED_TIMESTAMPS = new WeakMap();

function fail(message, line) {
  const suffix = line === undefined ? "" : ` at line ${line}`;
  throw new SyntaxError(`${message}${suffix}`);
}

function daysInMonth(year, month) {
  if (month === 2) {
    const leapYear = year % 4 === 0 && (year % 100 !== 0 || year % 400 === 0);
    return leapYear ? 29 : 28;
  }
  return [4, 6, 9, 11].includes(month) ? 30 : 31;
}

function parseTimestamp(value, message, line) {
  const match = RFC3339.exec(value);
  if (!match) fail(message, line);

  const [, yearText, monthText, dayText, hourText, minuteText, secondText, fraction = "", offsetHour = "0", offsetMinute = "0"] = match;
  const year = Number(yearText);
  const month = Number(monthText);
  const day = Number(dayText);
  const second = Number(secondText);
  const leapSecond = second === 60;
  const dateValue = leapSecond ? value.replace(":60", ":59") : value;
  const validDay = month > 0 && month <= 12 && day > 0 && day <= daysInMonth(year, month);
  if (!validDay || Number(hourText) > 23 || Number(minuteText) > 59 || second > 60
      || fraction.length > 9 || Number(offsetHour) > 23 || Number(offsetMinute) > 59 || Number.isNaN(Date.parse(dateValue))) {
    fail(message, line);
  }

  const timestamp = new Date(dateValue);
  const nanoseconds = fraction.padEnd(9, "0");
  const significantDigits = nanoseconds.endsWith("000000") ? 3
    : nanoseconds.endsWith("000") ? 6
      : 9;
  const normalizedFraction = Number(nanoseconds) === 0 ? "" : `.${nanoseconds.slice(0, significantDigits)}`;
  const utcWholeSeconds = timestamp.toISOString().slice(0, 19);
  const normalizedWholeSeconds = leapSecond ? `${utcWholeSeconds.slice(0, -2)}60` : utcWholeSeconds;
  NORMALIZED_TIMESTAMPS.set(timestamp, `${normalizedWholeSeconds}${normalizedFraction}Z`);
  return timestamp;
}

function parseRange(value, line) {
  const match = /^(\d+):(\d+)$/.exec(value);
  if (!match) fail("invalid line range", line);

  const start = Number(match[1]);
  const end = Number(match[2]);
  if (!Number.isSafeInteger(start) || !Number.isSafeInteger(end) || start === 0 || end < start
      || start > U32_MAX || end > U32_MAX) {
    fail("invalid line range", line);
  }
  return { start, end };
}

function parseUuid(value, line) {
  const match = UUID_FORMS.exec(value);
  if (!match) fail("invalid UUID", line);

  const compact = match[1] ?? (match[2] ? match.slice(2, 7).join("") : match.slice(7, 12).join(""));
  return `${compact.slice(0, 8)}-${compact.slice(8, 12)}-${compact.slice(12, 16)}-${compact.slice(16, 20)}-${compact.slice(20)}`.toLowerCase();
}

function parseRecord(line, lineNumber) {
  // Rust's splitn treats each whitespace character as one separator, so
  // repeated whitespace in the first six fields creates an empty token.
  const parts = line.match(/^(\S+)\s(\S+)\s(\S+)\s(\S+)\s(\S+)\s(\S+)\s(.+)$/u);
  if (!parts || !parts[7].trim()) fail("invalid record", lineNumber);

  const editId = parseUuid(parts[1], lineNumber);
  const range = parseRange(parts[2], lineNumber);
  const timestamp = parseTimestamp(parts[3], "invalid record timestamp", lineNumber);
  if (![parts[4], parts[5], parts[6]].every((value) => TOKEN.test(value))) {
    fail("metadata must use non-empty tokens", lineNumber);
  }

  return {
    editId,
    range,
    timestamp,
    user: parts[4],
    agent: parts[5],
    sessionId: parts[6],
    explanation: parts[7].trim(),
  };
}

/** Parse the on-disk Gloss format v1 contract implemented by src/format.rs. */
export function parseGlossFile(input) {
  if (typeof input !== "string") throw new TypeError("gloss input must be a string");

  const lines = input.split(/\r?\n/u);
  if (lines.at(-1) === "") lines.pop();

  const versionMatch = /^version: (\d+)$/.exec(lines[0] ?? "");
  if (!versionMatch) fail(lines.length ? "expected `version: <number>`" : "missing version header");
  const version = Number(versionMatch[1]);
  if (version !== 1) fail(`unsupported gloss version ${version}`);

  const updatedValue = lines[1]?.startsWith("updated: ") ? lines[1].slice(9) : null;
  if (updatedValue === null) fail(lines[1] === undefined ? "missing updated header" : "expected `updated: <timestamp>`");
  const updated = parseTimestamp(updatedValue, "updated must be an RFC 3339 timestamp");

  const editor = lines[2]?.startsWith("editor: ") ? lines[2].slice(8) : null;
  if (editor === null) fail(lines[2] === undefined ? "missing editor header" : "expected `editor: <token>`");
  if (!TOKEN.test(editor)) fail("editor must be one non-empty token");

  if (lines[3] !== "") fail("header must be followed by a blank line");

  const records = lines.slice(4).map((line, index) => {
    if (!line.trim()) fail("unexpected blank line", index + 5);
    return parseRecord(line, index + 5);
  });
  return { version, updated, editor, records };
}

function renderTimestamp(value) {
  if (!(value instanceof Date) || Number.isNaN(value.valueOf())) {
    throw new TypeError("timestamp must be a valid Date");
  }
  return NORMALIZED_TIMESTAMPS.get(value) ?? value.toISOString().replace(/\.000Z$/u, "Z");
}

/** Render the normalized representation used by GlossFile::render. */
export function renderGlossFile(gloss) {
  let output = `version: ${gloss.version}\nupdated: ${renderTimestamp(gloss.updated)}\neditor: ${gloss.editor}\n\n`;
  for (const record of gloss.records) {
    output += `${record.editId} ${record.range.start}:${record.range.end} ${renderTimestamp(record.timestamp)} ${record.user} ${record.agent} ${record.sessionId} ${record.explanation.trim()}\n`;
  }
  return output;
}
