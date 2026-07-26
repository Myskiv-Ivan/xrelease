import type { Permission } from '$lib/auth/types';

export type NavGroup = 'primary' | 'more';

export type NavItem = {
	href: string;
	labelKey: string;
	permission?: Permission;
	badge?: boolean;
	/** Primary = always visible; more = overflow menu. */
	group: NavGroup;
};

export const NAV_ITEMS: NavItem[] = [
	{ href: '/', labelKey: 'nav.overview', permission: 'status:read', group: 'primary' },
	{ href: '/sources', labelKey: 'nav.sources', permission: 'sources:read', group: 'primary' },
	{ href: '/outbox', labelKey: 'nav.outbox', permission: 'outbox:read', badge: true, group: 'primary' },
	{ href: '/config', labelKey: 'nav.config', permission: 'config:read', group: 'primary' },
	{
		href: '/diagnostics',
		labelKey: 'nav.diagnostics',
		permission: 'diagnostics:read',
		group: 'more'
	},
	{ href: '/docs', labelKey: 'nav.apiDocs', permission: 'about:read', group: 'more' },
	{ href: '/settings', labelKey: 'nav.settings', group: 'more' },
	{ href: '/about', labelKey: 'nav.about', permission: 'about:read', group: 'more' }
];
