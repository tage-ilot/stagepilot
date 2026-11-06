# Dashboard layout

StagePilot uses a local GridStack-powered dashboard for Service Plan, Now
Playing, Manual Controls, and Recent Event Stream. Layout data never leaves the
browser or desktop WebView. Connection readiness is available from the
translucent checklist shown while hovering or focusing the header's system
status.

## Edit a layout

Select **Edit layout** above the dashboard widgets. Normal mode locks every
widget so a live-service button press cannot accidentally move or resize the
dashboard.

While editing:

- Drag only from the labeled **Move** handle.
- Resize from the lower-right handle.
- Use **Move earlier** and **Move later** for keyboard-accessible ordering.
- Select **Compact layout** to reclaim unintentional gaps.
- Select **Add spacer** to preserve an intentional gap.
- Select **Done** to lock the layout again.

The Event Stream lists the latest 10 real events from the last 5 minutes. Connection
status (`connection.changed`) is kept apart from them: the backend keeps one entry per
integration for a connected status and one for a not-connected status, each replaced
(moved to the top with a fresh time) when the integration reports again. Those entries do not
age out, so you can always see when each integration was last connected, and a periodic
status check can never push song, timer or playback events out of the list.

Service Plan and Recent Event Stream scroll internally when their saved height
is smaller than their content. Each widget has minimum dimensions that protect
its controls and primary information.

## Spacers

A spacer is a real saved grid item. It can be moved and resized in edit mode,
and it prevents compaction from filling that area. Outside edit mode it becomes
visually transparent and does not intercept ordinary pointer input. Remove it
from its edit controls when the gap is no longer needed.

StagePilot does not treat an arbitrary empty cell as intentional space.
Automatic and explicit compaction may reclaim any gap that is not occupied by a
spacer.

## Responsive behavior

- Desktop widths use a saved 12-column grid with drag and resize.
- Tablet widths use a separate saved 6-column grid.
- Mobile widths use one predictable column. Horizontal resizing and free-form
  dragging are disabled; the earlier/later controls update mobile order.

Moving to mobile or tablet width does not overwrite the desktop geometry.
Returning to desktop restores its saved positions and sizes. The grid uses a
28-pixel row unit to provide useful resizing without excessively granular
layout data.

## Persistence and migration

The versioned local key is:

```text
stagepilot.dashboard-layout.v2
```

It stores only item IDs, kinds, positions, sizes, constraints, desktop/tablet
layouts, and mobile order. It contains no credentials, service data, or
analytics.

On first use, StagePilot reads the former
`stagepilot.dashboard-widget-order.v1` value. A complete, unique legacy order is
packed into the new grids and saved as v2. The v1 key is retained temporarily
for rollback and can be removed in a future release after the migration has
been established in production.

Invalid v2 data is copied to `stagepilot.dashboard-layout.invalid` when local
storage permits, then replaced by a safe default. Corrupt layout data never
prevents the dashboard from opening.

## Reset and troubleshooting

**Reset layout** asks for confirmation, removes custom spacers, restores the
current defaults, and does not modify any integration or StagePilot setting.

If the UI cannot be used, open the WebView/browser developer tools and run:

```js
localStorage.removeItem("stagepilot.dashboard-layout.v2");
location.reload();
```

The next load creates a fresh default layout. Leave the v1 key in place unless
you intentionally want to prevent legacy-order migration.

## Implementation

StagePilot pins GridStack 13.0.2 under its MIT license and bundles its JavaScript
and CSS locally. The established GridStack core API owns only the grid
container, item shells, collision handling, touch behavior, resizing, and
compaction. React continues to own all widget contents. Grid listeners are
removed and the instance is destroyed without deleting React-owned content
during unmount.

The earlier fixed-slot implementation was removed because it stored only an
order and mapped array positions to hard-coded CSS cells. It could not
represent dimensions, practical two-dimensional placement, responsive layouts,
or intentional space.

## Now playing: elapsed and remaining

Elapsed time and Time remaining are both computed from one song position (see "Countdown
accuracy" in `docs/playback-api.md`), so they always add up to the song length: while running,
stopped, after a resume or seek, and past the end (Time remaining holds 00:00, Elapsed time
holds the song length).
