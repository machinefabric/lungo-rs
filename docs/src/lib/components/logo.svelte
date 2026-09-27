<script lang="ts">
	import { asset } from '$app/paths';
	import { siteConfig } from '$lib/config';

	/** `wordmark` adds the site's name after the mark; the mark is black and inverts in dark mode. */
	let { wordmark = true, class: className = '' }: { wordmark?: boolean; class?: string } = $props();
</script>

<span class="inline-flex items-center gap-2 {className}">
	<img src={asset(siteConfig.logo)} alt={wordmark ? '' : siteConfig.title} class="h-6 w-auto dark:invert" />
	{#if wordmark}
		<span class="text-xl font-bold tracking-tight text-foreground">{siteConfig.title}</span>
	{/if}
</span>
