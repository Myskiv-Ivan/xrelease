/** Shared page / section layout tokens for consistent spacing. */
export const PAGE_STACK = 'flex flex-col gap-6';
export const PANEL_GRID = 'grid gap-4 lg:grid-cols-2';
export const PANEL_STACK = 'flex flex-col gap-4';
/** Vertical stack inside a form / settings panel. */
export const FIELD_STACK = 'flex flex-col gap-4';
/** Label + control pair. */
export const FIELD_GROUP = 'flex flex-col gap-1.5';
/**
 * Site content width — wide enough for ops tables (Sources Details column)
 * without forcing horizontal scroll on typical laptop viewports.
 */
export const LAYOUT_MAX = 'max-w-screen-2xl';

/**
 * Site-wide type scale (ops dashboard density).
 *
 * Two families only (see app.css) — Fira cohesion:
 * - `font-sans` / `font-heading` → Fira Sans (UI copy, titles)
 * - `font-mono` → Fira Code (IDs, tags, hashes, versions, code)
 *
 * Caps policy: CSS `uppercase` is allowed ONLY via `TYPE_OVERLINE`
 * (panel labels, stat labels). Table column headers use `TYPE_TABLE_HEAD`
 * (sentence case) so multi-word labels and hashes stay readable.
 * All other copy is sentence case; acronyms (API, SSO, CSV, UTC) stay as written.
 */
export const TYPE_PAGE_TITLE =
	'font-heading text-2xl font-semibold tracking-tight text-foreground';
export const TYPE_PAGE_DESC = 'text-sm leading-normal text-muted-foreground';
/** Overline labels — sole CSS-uppercase role. */
export const TYPE_OVERLINE =
	'text-xs font-medium uppercase tracking-[0.08em] text-muted-foreground';
/** Panel / card section label — prefer `TYPE_SECTION` for visible panel titles. */
export const TYPE_PANEL_TITLE = TYPE_OVERLINE;
/**
 * Table column headers — sentence case, muted, compact.
 * Do not uppercase: "Content SHA-256" / "Applied by" must stay readable.
 */
export const TYPE_TABLE_HEAD =
	'text-xs font-medium tracking-wide text-muted-foreground';
/** In-panel section heading (sentence case, no uppercase). */
export const TYPE_SECTION =
	'font-heading text-base font-semibold tracking-tight text-foreground';
export const TYPE_BODY = 'text-sm text-foreground';
/** Compact secondary copy (ids, meta, captions). */
export const TYPE_MUTED = 'text-xs text-muted-foreground';
/** Form / panel hints at body size (sentence case). */
export const TYPE_HINT = 'text-sm text-muted-foreground';
/** Compact field-level validation (under inputs). */
export const TYPE_FIELD_ERROR = 'text-xs text-destructive';
export const TYPE_FIELD_WARNING = 'text-xs text-warning';
/** Body-size status lines (parse errors, empty-state warnings, save confirmations). */
export const TYPE_STATUS_ERROR = 'text-sm text-destructive';
export const TYPE_STATUS_WARNING = 'text-sm text-warning';
export const TYPE_STATUS_SUCCESS = 'text-sm text-success';
/** Empty / placeholder titles. */
export const TYPE_EMPTY_TITLE =
	'font-heading text-base font-semibold tracking-tight text-foreground';
/** Technical data: mono + compact size + tabular nums. */
export const TYPE_CODE = 'font-mono text-xs tabular-nums tracking-tight';
