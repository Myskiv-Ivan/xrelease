import type { SurfaceTone } from '$lib/components/kit/surface-styles';

export interface KeyValueItem {
	label: string;
	value: string | number | boolean;
	/**
	 * Reuses [`SurfaceTone`] rather than repeating its members: `KeyValueList`
	 * indexes `toneTextClass` with this value, so an inline copy could drift
	 * from the record it is a key for.
	 */
	tone?: SurfaceTone;
}
