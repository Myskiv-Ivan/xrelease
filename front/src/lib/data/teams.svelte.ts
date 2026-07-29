import { api } from '$lib/api/client';
import type { TeamListResponse, TeamView } from '$lib/api/types';
import { organizationParam } from '$lib/data/organization-scope';
import { createResourceStore } from '$lib/data/resource.svelte';
import { t } from '$lib/i18n';

const resource = createResourceStore<TeamListResponse>({
	fetcher: () => api.listTeams(organizationParam()),
	fallbackError: t('errors.loadTeams')
});

/** Cached name map rebuilt only when the query data identity changes. */
let cachedTeams: TeamView[] | null = null;
let cachedNameByTag: Map<string, string> = new Map();

function nameByTag(): Map<string, string> {
	const teams = resource.data?.teams ?? [];
	if (teams === cachedTeams) return cachedNameByTag;

	const map = new Map<string, string>();
	for (const team of teams) {
		if (team.name && team.name.trim()) {
			map.set(team.tag, team.name.trim());
		}
	}
	cachedTeams = teams;
	cachedNameByTag = map;
	return map;
}

const teamsStore = {
	get teams() {
		return resource.data?.teams ?? [];
	},
	get error() {
		return resource.error;
	},
	get isLoading() {
		return resource.isLoading;
	},
	get isRefreshing() {
		return resource.isRefreshing;
	},
	get lastUpdated() {
		return resource.lastUpdated;
	},
	/** Human-readable label for a routing tag, falling back to the raw tag. */
	nameFor: (tag: string | null | undefined): string | null => {
		if (!tag) return null;
		return nameByTag().get(tag) ?? tag;
	},
	start: () => resource.start(),
	stop: () => resource.stop(),
	reload: () => resource.reload(),
	refresh: () => resource.refresh()
};

export function getTeamsStore() {
	return teamsStore;
}

export function resetTeamsStore() {
	cachedTeams = null;
	cachedNameByTag = new Map();
	resource.reset();
}
