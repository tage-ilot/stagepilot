export const confirmClearSelectedLightingCues = (songTitle: string) =>
  window.confirm(`Clear the lighting cues for "${songTitle}"? This cannot be undone.`);

export const confirmClearAllLightingCues = (songTitles: string[], planTitle: string | null) => {
  const target = songTitles.length === 1
    ? `"${songTitles[0]}"`
    : `every song in "${planTitle ?? "the current service plan"}"`;
  return window.confirm(`Clear all lighting cues for ${target}? This cannot be undone.`);
};

export const confirmRerecordLooks = () =>
  window.confirm("Recording replaces this song's captured looks, including your edits. Record anyway?");

export const confirmRemoveFixture = (fixture: string, songTitle: string) =>
  window.confirm(`Remove fixture "${fixture}" from "${songTitle}"? Its messages are deleted from every look row, and rows left empty are removed.`);

export const confirmMergeChangesValues = (addresses: string[]) =>
  window.confirm(`Merging moves later values earlier for ${addresses.length} address${addresses.length === 1 ? "" : "es"} (for example ${addresses[0]}). Merge anyway?`);

