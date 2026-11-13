import { describe, expect, it } from 'vitest';

import page from '../public/admin/index.html?raw';

// The admin page is a single static file whose script looks elements up by id.
// These checks catch the markup and the script drifting apart (a renamed id
// would silently break a whole section) and guard the page's key behaviours.
describe('fleet admin page', () => {
  const scriptIds = [...page.matchAll(/getElementById\('([^']+)'\)/g)].map((m) => m[1]);

  it('has an element for every id the script looks up', () => {
    expect(scriptIds.length).toBeGreaterThan(10);
    for (const id of new Set(scriptIds)) {
      expect(page, `missing id="${id}"`).toContain(`id="${id}"`);
    }
  });

  it('keeps the edit fields hidden until the Edit button is pressed', () => {
    expect(page).toContain('editPanel.hidden = true;');
    expect(page).toContain("el('button', 'btn sm', 'Edit')");
    expect(page).toContain('editPanel.hidden = !opening;');
    expect(page).toContain('[hidden] { display: none !important; }');
  });

  it('uses one body font family and the teal accent', () => {
    expect(page).toContain('--teal: #1dc1c7;');
    expect(page.match(/--font:/g)?.length).toBe(1);
    // The only other family is the display face used for headings.
    expect(page.match(/font-family:\s*var\(--(?!font\b|display\b)/g)).toBeNull();
  });

  it('uses the Locations wording and keeps the credit link', () => {
    expect(page).toContain('>Locations<');
    expect(page).not.toContain('>Cities<');
    expect(page).toContain('https://db-ip.com');
  });

  it('keeps the admin token suffix and loads no external resources', () => {
    expect(page).toContain("var ADMIN_TOKEN_SUFFIX = 'JHwUWeCutsi3WioLfAL3CXx4f8bDxlCp';");
    const external = [...page.matchAll(/(?:src|href)="(https?:[^"]+)"/g)].map((m) => m[1]);
    expect(external).toEqual(['https://db-ip.com']);
  });

  it('shows the installation actions, including Pin and Remove', () => {
    for (const label of ['Diagnostics', 'Pin', 'Unpin', 'Archive', 'Unarchive', 'Remove']) {
      expect(page).toContain(`'${label}'`);
    }
  });
});
