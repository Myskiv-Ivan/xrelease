import { api } from '$lib/api/client';
import type { OrganizationView } from '$lib/api/types';
import { STORAGE_KEYS } from '$lib/core/constants';
import { resolveApiError } from '$lib/core/errors';
import { t } from '$lib/i18n';

/**
 * Organization catalogue + the operator's currently-selected org.
 *
 * Multi-org mode is inferred from the catalogue: an empty list means the
 * instance runs in single-document mode, so the switcher and
 * org-scoped views stay hidden and the global `/config` page is authoritative.
 * The catalogue is bootstrap-only, so it is fetched once per authenticated
 * session; selection is persisted in `localStorage` and reconciled against the
 * live list on every load (a stored org that no longer exists falls back to the
 * first entry).
 */
let organizations = $state<OrganizationView[]>([]);
let configSource = $state<'local' | 'api'>('local');
let apiConfigEnabled = $state(false);
let selectedId = $state<string | null>(null);
let isLoaded = $state(false);
let isLoading = $state(false);
let error = $state<string | null>(null);

/** De-dupes concurrent `load` calls (layout effect can re-run). */
let inFlight: Promise<void> | null = null;

function readStoredSelection(): string | null {
	if (typeof localStorage === 'undefined') return null;
	return localStorage.getItem(STORAGE_KEYS.selectedOrg);
}

function persistSelection(id: string | null): void {
	if (typeof localStorage === 'undefined') return;
	if (id) {
		localStorage.setItem(STORAGE_KEYS.selectedOrg, id);
	} else {
		localStorage.removeItem(STORAGE_KEYS.selectedOrg);
	}
}

/** Restore the persisted org when still present; otherwise default to the first. */
function reconcileSelection(): void {
	if (organizations.length === 0) {
		selectedId = null;
		return;
	}
	const stored = readStoredSelection();
	if (stored && organizations.some((org) => org.id === stored)) {
		selectedId = stored;
		return;
	}
	selectedId = organizations[0].id;
	persistSelection(selectedId);
}

async function load(): Promise<void> {
	if (inFlight) return inFlight;
	isLoading = true;
	error = null;
	inFlight = (async () => {
		try {
			const response = await api.listOrganizations();
			organizations = response.organizations;
			configSource = response.config_source;
			apiConfigEnabled = response.api_config_enabled;
			reconcileSelection();
			isLoaded = true;
		} catch (err) {
			error = resolveApiError(err, t('errors.loadOrganizations'));
		} finally {
			isLoading = false;
			inFlight = null;
		}
	})();
	return inFlight;
}

export function getOrganizationsState() {
	return {
		get organizations() {
			return organizations;
		},
		get configSource() {
			return configSource;
		},
		get apiConfigEnabled() {
			return apiConfigEnabled;
		},
		get selectedId() {
			return selectedId;
		},
		get selected(): OrganizationView | null {
			return organizations.find((org) => org.id === selectedId) ?? null;
		},
		/** True when the instance has one or more `[[organizations]]` entries. */
		get isMultiOrg() {
			return organizations.length > 0;
		},
		get isLoaded() {
			return isLoaded;
		},
		get isLoading() {
			return isLoading;
		},
		get error() {
			return error;
		},
		/** Fetch the catalogue once per session (idempotent, de-duped). */
		load,
		/** Force a re-fetch (catalogue is bootstrap-only, but keeps counts fresh). */
		reload() {
			isLoaded = false;
			return load();
		},
		select(id: string) {
			if (!organizations.some((org) => org.id === id)) return;
			selectedId = id;
			persistSelection(id);
		}
	};
}

export function resetOrganizationsStore(): void {
	organizations = [];
	configSource = 'local';
	apiConfigEnabled = false;
	selectedId = null;
	isLoaded = false;
	isLoading = false;
	error = null;
	inFlight = null;
}
