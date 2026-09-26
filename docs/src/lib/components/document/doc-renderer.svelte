<script lang="ts">
	import PromoCard from './promo-card.svelte';
	import Separator from '../ui/separator/separator.svelte';
	import DocContent from './doc-content.svelte';
	import DocHeader from './doc-header.svelte';
	import TableOfContents from './table-of-contents.svelte';
	import MobileTableOfContents from './mobile-table-of-contents.svelte';

	let { title, description, data }: { title: string; description: string; data: any } = $props();
</script>

<!-- Code blocks arrive highlighted from the build (svelte.config.js), so the page renders
     completely on the server and is prerendered with its content. -->
<div class="flex flex-col gap-6 sm:flex-row">
	<div class="min-w-0">
		<DocHeader {title} {description} />
		{#key title}
			<DocContent {data} />
		{/key}
	</div>
	<div>
		<div class="sticky top-20 flex w-72 flex-col gap-4">
			<div class="hidden sm:block">
				<TableOfContents />
				<Separator />
			</div>
			<div class="block sm:hidden">
				<MobileTableOfContents />
			</div>
			<PromoCard />
		</div>
	</div>
</div>

<style>
	/* Code blocks, highlighted at build time */
	:global(pre.shiki) {
		margin: 1.5em 0;
		padding: 1.25rem;
		border-radius: 0.5rem;
		width: 100%;
		max-width: calc(100vw - 3rem);
		overflow-x: auto;
		display: block;
		background-color: hsl(var(--code-background));
		border: 1px solid hsl(var(--border));
	}

	:global(ol pre.shiki) {
		max-width: calc(100vw - 5rem);
	}

	/* Code highlighting styles */
	:global(.shiki) {
		background-color: transparent !important;
		font-family: 'JetBrains Mono', ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
		font-size: 0.9em;
		line-height: 1.5;
	}

	/* Pre and code styles */
	:global(.prose pre) {
		padding: 0.75rem !important;
		margin: 0;
		background-color: hsl(var(--code-background)) !important;
		color: hsl(var(--code-foreground)) !important;
	}

	:global(.prose :not(pre) > code) {
		background-color: hsl(var(--code-background));
		color: hsl(var(--code-foreground));
		padding: 0.25rem;
		border-radius: 0.25rem;
		font-size: 0.875em;
		font-weight: 400;
		max-width: 100%;
	}

	/* Remove default code decorations */
	:global(.prose code::before),
	:global(.prose code::after) {
		content: '' !important;
	}

	/* Line hover effects */
	:global(.line:hover) {
		background-color: rgb(175 184 193 / 10%);
	}

	:global(.dark .line:hover) {
		background-color: #1f2937;
	}
</style>
