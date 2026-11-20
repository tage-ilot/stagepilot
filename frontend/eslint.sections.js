// The section-template guard (DESIGN.md §6.6). A configuration page is built ONLY from the templates in src/components/sections/ plus the
// content it passes in; these rules stop a page from drawing its own copy of what a template already does. Every message says which
// template to use. A page-specific exception is allowed only with a reason, on the line itself:
//   // eslint-disable-next-line no-restricted-syntax -- page-specific: <why no template fits>
// A new page file is named like the existing ones (…SetupPanel.tsx, …ApiPanel.tsx, …LiveLink.tsx, …LinkBlock.tsx, …Strip.tsx) or added to PAGE_FILES.

/** The files that are configuration pages or their Live Link blocks. Tests are not pages. */
export const PAGE_FILES = [
  "src/components/*SetupPanel.tsx",
  "src/components/*ApiPanel.tsx",
  "src/components/RemoteAccessPanel.tsx",
  "src/components/LightkeyStrip.tsx",
  "src/components/LightingOscParts.tsx",
  "src/components/PlaybackLiveLink.tsx",
  "src/components/ProPresenterLinkBlock.tsx",
  // The Presenter Link section (in the Presentation page): its frame, options, list, button and result are templates plus its own list.
  "src/components/PresenterLinkSection.tsx",
  "src/components/PresenterSongList.tsx",
  "src/components/AddSongsParts.tsx",
];

const USE = "Use the section templates (import from components/sections, see DESIGN.md 6.6)";
const className = "JSXAttribute[name.name='className']";
const text = (pattern) => `:matches(Literal[value=/${pattern}/], TemplateElement[value.raw=/${pattern}/])`;

/** `no-restricted-syntax` selectors, each with the template that does the job. */
export const SYNTAX = [
  { selector: "JSXOpeningElement[name.name=/^(dl|dt|dd)$/]", message: `A list of facts (API, Configured timer, Last checked) is the StatusSummary template: <StatusSummary items={[{ label, value }]} />. ${USE}.` },
  { selector: `${className} ${text("grid-cols-")}`, message: `A grid of fields is the SettingsSection template: <SettingsSection columns="standard|pair|triple|lead|single"> around the <Field>s. ${USE}. A list or table that is not fields takes an eslint-disable-next-line with the reason.` },
  { selector: `CallExpression[callee.name=/^(cx|clsx)$/] Literal[value=/grid-cols-/]`, message: `A grid of fields is the SettingsSection template (columns="standard|pair|triple|lead|single"). ${USE}.` },
  { selector: `${className} ${text("(^|\\s)live-link(\\s|$)|bg-surface-panel-strong")}`, message: `A Live Link block (the bordered box with the address row, the status line and Details) is the ConnectionSection template: <ConnectionSection title pills connection status details detailsLabel />. ${USE}.` },
  { selector: `${className} ${text("flex-wrap[^\"]*items-end[^\"]*gap-x-2.5|gap-x-2.5[^\"]*gap-y-2")}`, message: `The row of address, port and main buttons is <ConnectionRow> (from components/sections), inside a ConnectionSection.` },
  { selector: "JSXOpeningElement[name.name=/^(button|Button)$/] > JSXAttribute[name.name='aria-expanded']", message: `A Details button is the ConnectionSection's detailsLabel; a collapsible optional block is the OptionalSection (or OptionalBand) template. ${USE}.` },
  { selector: ":matches(JSXOpeningElement[name.name='Input'] > JSXAttribute[name.name='aria-label'][value.value=/host|computer/i], JSXOpeningElement[name.name='Input'] > JSXAttribute[name.name='aria-label'] Identifier[name=/HOST_LABEL/])", message: `The box for a computer's address (a host or IP, with the arrow that lists the recent and scanned ones) is the AddressBox template: <AddressBox aria-label scope value onChange scanned onSubmit />. ${USE}.` },
  { selector: "JSXElement[openingElement.name.name='Button'] > JSXText[value=/^\\s*(Test|Refresh|Reload|Load)\\b/]", message: `A Test / Refresh / Reload / Load button is <ActionButton label busyLabel busy /> inside <ActionRow> (components/sections). ${USE}.` },
  { selector: "JSXElement[openingElement.name.name='Button'] > JSXExpressionContainer :matches(Literal[value=/^(Test|Refresh|Reload|Load)\\b/], Literal[value=/^(Testing|Refreshing|Reloading)/])", message: `A Test / Refresh / Reload / Load button is <ActionButton label busyLabel busy /> inside <ActionRow> (components/sections). ${USE}.` },
];

/** `no-restricted-imports`: the old pieces and the primitives a template wraps. */
export const IMPORTS = {
  paths: [
    { name: "./ui/Panel", importNames: ["Panel"], message: "A page's panel is PanelLayout (components/sections): header, alert lines, then the slots in the one order." },
    { name: "./ui/Disclosure", message: "A collapsible optional block is OptionalSection (or OptionalBand), with the remembered open flag: import it from components/sections." },
    { name: "./ui/InlineAlert", message: "A line of news is SectionNotice (box or line), from components/sections." },
  ],
  patterns: [
    { group: ["**/sections/[A-Z]*"], message: "Import the templates from components/sections (its index), never from one of its files: ./sections." },
    { group: ["**/LightingOscParts"], importNames: ["LiveLinkShell", "LiveLinkPill", "LiveLinkField", "DisclosureButton"], message: "The Live Link pieces live in components/sections only: ConnectionSection draws the block, LiveLinkPill and LiveLinkField fill it." },
  ],
};

/** The flat-config block. */
export const sectionGuard = {
  files: PAGE_FILES,
  ignores: ["**/*.test.{ts,tsx}"],
  rules: {
    "no-restricted-syntax": ["error", ...SYNTAX],
    "no-restricted-imports": ["error", IMPORTS],
  },
};
