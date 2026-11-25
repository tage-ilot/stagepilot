import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const BOOK_ID = "org.stagepilot.desktop.help";
const BOOK_FOLDER = "StagePilot.help";
const SKIPPED_DIRECTORIES = new Set([
  ".agents",
  ".codex",
  ".git",
  ".github",
  ".mypy_cache",
  ".pytest_cache",
  ".ruff_cache",
  ".stagepilot-backup",
  ".tools",
  "build",
  "dist",
  "generated-help",
  "graphify-out",
  "node_modules",
  "target",
]);

const escapeHtml = (value) =>
  value
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;");

const slug = (value) =>
  value
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-|-$/g, "") || "topic";

const inlineMarkdown = (value) => {
  let output = escapeHtml(value);
  output = output.replace(
    /\[([^\]]+)\]\((https?:\/\/[^)\s]+)\)/g,
    '<a href="$2">$1</a>',
  );
  output = output.replace(/`([^`]+)`/g, "<code>$1</code>");
  output = output.replace(/\*\*([^*]+)\*\*/g, "<strong>$1</strong>");
  output = output.replace(/\*([^*]+)\*/g, "<em>$1</em>");
  return output;
};

export function renderMarkdown(markdown) {
  const output = [];
  let paragraph = [];
  let list = null;
  let code = false;
  const usedAnchors = new Map();

  const closeParagraph = () => {
    if (paragraph.length) {
      output.push(`<p>${inlineMarkdown(paragraph.join(" "))}</p>`);
      paragraph = [];
    }
  };
  const closeList = () => {
    if (list) {
      output.push(`</${list}>`);
      list = null;
    }
  };

  for (const rawLine of markdown.replaceAll("\r\n", "\n").split("\n")) {
    const line = rawLine.trimEnd();
    if (line.startsWith("```")) {
      closeParagraph();
      closeList();
      output.push(code ? "</code></pre>" : "<pre><code>");
      code = !code;
      continue;
    }
    if (code) {
      output.push(`${escapeHtml(rawLine)}\n`);
      continue;
    }
    const heading = /^(#{1,6})\s+(.+)$/.exec(line);
    if (heading) {
      closeParagraph();
      closeList();
      const level = heading[1].length;
      const base = slug(heading[2]);
      const seen = usedAnchors.get(base) ?? 0;
      usedAnchors.set(base, seen + 1);
      const anchor = seen ? `${base}-${seen + 1}` : base;
      output.push(
        `<h${level} id="${anchor}">${inlineMarkdown(heading[2])}</h${level}>`,
      );
      continue;
    }
    const unordered = /^\s*[-*]\s+(.+)$/.exec(line);
    const ordered = /^\s*\d+\.\s+(.+)$/.exec(line);
    if (unordered || ordered) {
      closeParagraph();
      const nextList = unordered ? "ul" : "ol";
      if (list !== nextList) {
        closeList();
        output.push(`<${nextList}>`);
        list = nextList;
      }
      output.push(`<li>${inlineMarkdown((unordered ?? ordered)[1])}</li>`);
      continue;
    }
    if (/^\s*---+\s*$/.test(line)) {
      closeParagraph();
      closeList();
      output.push("<hr>");
      continue;
    }
    if (!line.trim()) {
      closeParagraph();
      closeList();
      continue;
    }
    paragraph.push(line.trim());
  }
  closeParagraph();
  closeList();
  if (code) output.push("</code></pre>");
  return output.join("\n");
}

function collectMarkdownFiles(sourceRoot) {
  const files = [];
  const visit = (directory) => {
    for (const entry of fs.readdirSync(directory, { withFileTypes: true })) {
      if (entry.name.startsWith(".") && entry.isDirectory()) continue;
      if (entry.isDirectory() && SKIPPED_DIRECTORIES.has(entry.name)) continue;
      const fullPath = path.join(directory, entry.name);
      if (entry.isDirectory()) visit(fullPath);
      else if (entry.isFile() && entry.name.toLowerCase().endsWith(".md")) {
        files.push(fullPath);
      }
    }
  };
  visit(sourceRoot);
  return files.sort((left, right) => left.localeCompare(right));
}

const pageShell = ({ title, body, appleTitle = false }) => `<!doctype html>
<html xmlns="http://www.w3.org/1999/xhtml" lang="en">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  ${appleTitle ? `<meta name="AppleTitle" content="${BOOK_ID}" />` : ""}
  <meta name="robots" content="index, anchors" />
  <title>${escapeHtml(title)}</title>
  <style>
    :root { color-scheme: light dark; font-family: -apple-system, BlinkMacSystemFont, sans-serif; }
    body { margin: 0 auto; max-width: 880px; padding: 2rem; line-height: 1.55; }
    a { color: #1685d8; }
    code, pre { font-family: ui-monospace, SFMono-Regular, Menlo, monospace; }
    pre { overflow-wrap: anywhere; overflow-x: auto; padding: 1rem; border-radius: .6rem; background: rgba(127,127,127,.13); }
    h1, h2, h3 { line-height: 1.2; }
    .source { color: #777; font-size: .85rem; }
  </style>
</head>
<body>${body}</body>
</html>
`;

export function buildHelpBook({
  sourceRoot,
  outputRoot,
  createIndex = process.platform === "darwin",
}) {
  const files = collectMarkdownFiles(sourceRoot);
  if (!files.length) throw new Error("No StagePilot Markdown files were found.");

  fs.rmSync(outputRoot, { recursive: true, force: true });
  const contents = path.join(outputRoot, BOOK_FOLDER, "Contents");
  const localized = path.join(contents, "Resources", "English.lproj");
  const pages = path.join(localized, "pages");
  fs.mkdirSync(pages, { recursive: true });

  const topics = files.map((file) => {
    const relative = path.relative(sourceRoot, file).replaceAll("\\", "/");
    const markdown = fs.readFileSync(file, "utf8");
    const heading = /^#\s+(.+)$/m.exec(markdown)?.[1]?.trim();
    const title = heading || relative.replace(/\.md$/i, "");
    const filename = `${slug(relative.replace(/\.md$/i, ""))}.html`;
    const body = `<p class="source">Source: ${escapeHtml(relative)}</p>\n${renderMarkdown(markdown)}`;
    fs.writeFileSync(
      path.join(pages, filename),
      pageShell({ title, body }),
      "utf8",
    );
    return { filename, relative, title };
  });

  const topicLinks = topics
    .map(
      ({ filename, relative, title }) =>
        `<li><a href="pages/${filename}">${escapeHtml(title)}</a><br><span class="source">${escapeHtml(relative)}</span></li>`,
    )
    .join("\n");
  fs.writeFileSync(
    path.join(localized, "index.html"),
    pageShell({
      title: "StagePilot Help",
      appleTitle: true,
      body: `<h1>StagePilot Help</h1><p>Search from the macOS Help menu or browse the bundled StagePilot documentation below.</p><ul>${topicLinks}</ul>`,
    }),
    "utf8",
  );
  fs.writeFileSync(
    path.join(localized, "InfoPlist.strings"),
    'CFBundleName = "StagePilot Help";\nHPDBookTitle = "StagePilot Help";\n',
    "utf8",
  );
  fs.writeFileSync(
    path.join(contents, "Info.plist"),
    `<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleDevelopmentRegion</key><string>en</string>
<key>CFBundleIdentifier</key><string>${BOOK_ID}</string>
<key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
<key>CFBundleName</key><string>StagePilot Help</string>
<key>CFBundlePackageType</key><string>BNDL</string>
<key>CFBundleShortVersionString</key><string>1</string>
<key>CFBundleSignature</key><string>hbwr</string>
<key>CFBundleVersion</key><string>1</string>
<key>HPDBookAccessPath</key><string>index.html</string>
<key>HPDBookIndexPath</key><string>StagePilot.helpindex</string>
<key>HPDBookTitle</key><string>StagePilot Help</string>
<key>HPDBookType</key><string>3</string>
</dict></plist>
`,
    "utf8",
  );

  if (createIndex) {
    const indexPath = path.join(localized, "StagePilot.helpindex");
    fs.rmSync(indexPath, { force: true });
    execFileSync("/usr/bin/hiutil", ["-Caf", indexPath, localized], {
      stdio: "inherit",
    });
    if (!fs.existsSync(indexPath) || fs.statSync(indexPath).size === 0) {
      throw new Error("macOS Help Indexer did not create StagePilot.helpindex.");
    }
  }
  return { files: topics.length, outputRoot };
}

const thisFile = fileURLToPath(import.meta.url);
if (process.argv[1] && path.resolve(process.argv[1]) === thisFile) {
  const sourceRoot = path.resolve(import.meta.dirname, "..");
  const outputRoot = path.join(sourceRoot, "desktop", "src-tauri", "generated-help");
  const result = buildHelpBook({
    sourceRoot,
    outputRoot,
    createIndex:
      process.platform === "darwin" &&
      process.env.STAGEPILOT_SKIP_HELP_INDEX !== "1",
  });
  console.log(`Built searchable StagePilot Help from ${result.files} Markdown files.`);
}

