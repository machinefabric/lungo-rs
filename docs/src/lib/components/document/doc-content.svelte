<script lang="ts">
	import { onMount } from 'svelte';
	import { TableOfContents } from './toc.svelte.js';

	let contentRef: HTMLElement | undefined = $state();
	let { data }: { data: any } = $props();

	let toc = $state(TableOfContents.getInstance());

	onMount(() => {
		if (contentRef) toc.updateContentRef(contentRef);
	});
</script>

<div class="prose prose-slate max-w-none dark:prose-invert" bind:this={contentRef}>
	<data.doc />
</div>
