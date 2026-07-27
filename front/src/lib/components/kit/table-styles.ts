import { TYPE_TABLE_HEAD } from '$lib/components/kit/layout-styles';

/**
 * Table header row chrome. Type tokens live on the cell / sort button — not only
 * the `<thead>` — because UA/`button` styles reset inherited text styles and
 * left sortable columns looking different from static ones.
 */
export const TABLE_HEAD_ROW = 'border-b border-border bg-[var(--table-head-bg)]';
/**
 * Shared head cell — left-aligned to match body cells (Applied by, Content SHA-256, …).
 * Sentence case via TYPE_TABLE_HEAD; nowrap so multi-word labels stay one line.
 */
export const TABLE_HEAD_CELL = `px-4 py-3 text-left align-middle ${TYPE_TABLE_HEAD} whitespace-nowrap`;
/** Opaque at rest and on hover so `TABLE_STICKY_END` can inherit it exactly. */
export const TABLE_BODY_ROW =
	'border-b border-border/60 last:border-0 bg-card hover:bg-[var(--table-row-hover-bg)]';
export const TABLE_BODY_CELL = 'px-4 py-3 text-left align-middle';
export const TABLE_DATE_CELL = `${TABLE_BODY_CELL} whitespace-nowrap tabular-nums text-muted-foreground`;
export const TABLE_SORT_BUTTON = `inline-flex w-full cursor-pointer items-center justify-start gap-1 rounded ${TYPE_TABLE_HEAD} whitespace-nowrap transition-colors hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary focus-visible:ring-offset-2 focus-visible:ring-offset-background`;
/**
 * Sticky end column (Details) so wide tables keep the action visible.
 *
 * `bg-inherit` deliberately: the cell takes whatever the row is painted with,
 * so it tracks the hover tint instead of sitting at a fixed colour that reads
 * as a differently-shaded column. Requires opaque rows — see `TABLE_BODY_ROW`.
 */
export const TABLE_STICKY_END =
	'sticky right-0 z-10 bg-inherit shadow-[-8px_0_8px_-8px_rgba(0,0,0,0.12)] dark:shadow-[-8px_0_8px_-8px_rgba(0,0,0,0.4)]';
/**
 * Head variant names the colour instead of inheriting: `TABLE_HEAD_ROW` sits on
 * `<thead>`, but a `<th>`'s background-color inherits from its `<tr>` — which
 * carries no background — so `bg-inherit` would resolve to transparent and let
 * body rows scroll visibly under the pinned header cell. Same token as
 * `TABLE_HEAD_ROW`, so the two still match exactly.
 */
export const TABLE_STICKY_END_HEAD = `${TABLE_STICKY_END} bg-[var(--table-head-bg)]`;
