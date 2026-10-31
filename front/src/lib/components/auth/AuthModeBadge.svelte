<script lang="ts">
	import Badge from '#lib/components/kit/Badge.svelte';
	import { getAuthMode } from '#lib/auth/config.js';
	import { t } from '#lib/i18n/index.js';

	const mode = getAuthMode();
</script>

<Badge tone="muted" class="font-normal">
	{t(`auth.mode.${mode}`)}
</Badge>
