import { getConfigStore } from '$lib/data/config.svelte';
import { getOrgConfigStore } from '$lib/data/org-config.svelte';
import { getOrganizationsState } from '$lib/stores/organizations.svelte';

interface CreateScopedConfigOptions {
	/** Gate the whole controller — e.g. until `config:read` is granted. */
	isEnabled?: () => boolean;
}

/**
 * Drives whichever config store the instance actually has, and reports it as one.
 *
 * A single-document instance reads `/config`; a multi-org instance reads the
 * org routes and must re-fetch whenever the switcher selection changes. Both
 * the config page and the overview panel need that, and both used to own a
 * hand-written copy of the effect — copies that had already drifted (the
 * overview never recovered when the organization catalogue failed to load, so
 * its routing graph stayed empty forever).
 *
 * Owns `$effect`s, so it must be called during component initialisation.
 */
export function createScopedConfig(options: CreateScopedConfigOptions = {}) {
	const orgs = getOrganizationsState();
	const globalStore = getConfigStore();
	const orgStore = getOrgConfigStore();

	/**
	 * Settled either way: a catalogue that failed to load falls back to the
	 * global document rather than leaving the page blank.
	 */
	const isCatalogueReady = $derived(orgs.isLoaded || orgs.error != null);
	const isOrgScoped = $derived(orgs.isMultiOrg && orgs.selectedId != null);
	const active = $derived(isOrgScoped ? orgStore : globalStore);

	/** The org `orgStore` is currently tracking (plain, for switch detection). */
	let activeOrg: string | null = null;

	$effect(() => {
		if (options.isEnabled && !options.isEnabled()) return;
		// Wait for the catalogue so a multi-org instance never fetches the global
		// document just to abandon it; the layout loads it once on auth.
		if (!isCatalogueReady) return;

		if (orgs.isMultiOrg) {
			globalStore.stop();
			if (!orgs.selectedId) {
				orgStore.stop();
				activeOrg = null;
				return;
			}
			if (activeOrg === null) {
				orgStore.start();
			} else if (activeOrg !== orgs.selectedId) {
				void orgStore.reload();
			}
			activeOrg = orgs.selectedId;
			return;
		}

		if (activeOrg !== null) {
			orgStore.stop();
			activeOrg = null;
		}
		globalStore.start();
	});

	// Teardown only. Deliberately dependency-free: folding it into the effect
	// above would stop and restart the store on every selection change, turning
	// a re-target into a full reload flash.
	$effect(() => {
		return () => {
			globalStore.stop();
			orgStore.stop();
			activeOrg = null;
		};
	});

	return {
		/** True once the instance is known to be multi-org with a selection. */
		get isOrgScoped() {
			return isOrgScoped;
		},
		get global() {
			return globalStore;
		},
		get org() {
			return orgStore;
		},
		get error() {
			return active.error;
		},
		/** Catalogue resolution counts as loading — the page cannot pick a store yet. */
		get isLoading() {
			return !isCatalogueReady || active.isLoading;
		},
		get isRefreshing() {
			return active.isRefreshing;
		},
		get lastUpdated() {
			return active.lastUpdated;
		},
		get hasContent() {
			return active.view != null;
		},
		refresh: () => active.refresh()
	};
}
