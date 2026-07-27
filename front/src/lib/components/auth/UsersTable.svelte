<script lang="ts">
	import type { AuthUserView } from '$lib/api/types';
	import Badge from '$lib/components/kit/Badge.svelte';
	import RelativeTime from '$lib/components/kit/RelativeTime.svelte';
	import TableShell from '$lib/components/kit/TableShell.svelte';
	import {
		TABLE_BODY_CELL,
		TABLE_BODY_ROW,
		TABLE_DATE_CELL,
		TABLE_HEAD_CELL,
		TABLE_HEAD_ROW
	} from '$lib/components/kit/table-styles';
	import { TYPE_CODE, TYPE_MUTED } from '$lib/components/kit/layout-styles';
	import { EMPTY_VALUE } from '$lib/core/format';
	import { roleLabel } from '$lib/auth/roles';
	import { t } from '$lib/i18n';

	interface Props {
		users: AuthUserView[];
	}

	let { users }: Props = $props();

	function identityLabel(user: AuthUserView): string {
		return (
			user.display_name ??
			user.username ??
			user.email ??
			(user.oidc_sub ? `oidc:${user.oidc_sub.slice(0, 12)}` : `#${user.id}`)
		);
	}

	function sourceTone(source: string): 'accent' | 'success' | 'muted' {
		if (source === 'oidc') return 'accent';
		if (source === 'local') return 'success';
		return 'muted';
	}

	function sourceLabel(source: string): string {
		if (source === 'oidc') return t('users.sourceOidc');
		if (source === 'local') return t('users.sourceLocal');
		return source;
	}
</script>

<TableShell>
	<thead class={TABLE_HEAD_ROW}>
		<tr>
			<th scope="col" class={TABLE_HEAD_CELL}>{t('users.colUser')}</th>
			<th scope="col" class={TABLE_HEAD_CELL}>{t('users.colEmail')}</th>
			<th scope="col" class={TABLE_HEAD_CELL}>{t('users.colRole')}</th>
			<th scope="col" class={TABLE_HEAD_CELL}>{t('users.colSource')}</th>
			<th scope="col" class={TABLE_HEAD_CELL}>{t('users.createdAt')}</th>
			<th scope="col" class={TABLE_HEAD_CELL}>{t('users.lastLogin')}</th>
		</tr>
	</thead>
	<tbody>
		{#each users as user (user.id)}
			<tr class={TABLE_BODY_ROW}>
				<td class={TABLE_BODY_CELL}>
					<div class="flex min-w-0 flex-col gap-0.5">
						<span class="truncate text-sm font-medium">{identityLabel(user)}</span>
						{#if user.username}
							<span class="{TYPE_CODE} {TYPE_MUTED}">{user.username}</span>
						{/if}
					</div>
				</td>
				<td class={TABLE_BODY_CELL}>
					{#if user.email}
						<span class="text-sm">{user.email}</span>
					{:else}
						<span class={TYPE_MUTED}>{EMPTY_VALUE}</span>
					{/if}
				</td>
				<td class={TABLE_BODY_CELL}>
					<Badge tone="muted">{roleLabel(user.role)}</Badge>
				</td>
				<td class={TABLE_BODY_CELL}>
					<div class="flex flex-wrap items-center gap-1.5">
						<Badge tone={sourceTone(user.auth_source)}>{sourceLabel(user.auth_source)}</Badge>
						{#if user.oidc_sub}
							<!-- title carries the opaque sub: useful to an admin, too noisy inline -->
							<span title={user.oidc_sub}>
								<Badge tone="accent">{t('users.oidcLinked')}</Badge>
							</span>
						{/if}
					</div>
				</td>
				<td class={TABLE_DATE_CELL}>
					<RelativeTime value={user.created_at} format="full" />
				</td>
				<td class={TABLE_DATE_CELL}>
					{#if user.last_login_at}
						<RelativeTime value={user.last_login_at} format="full" />
					{:else}
						{t('users.neverLoggedIn')}
					{/if}
				</td>
			</tr>
		{/each}
	</tbody>
</TableShell>
