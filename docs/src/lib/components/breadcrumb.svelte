<script lang="ts">
	import { page } from '$app/state';
	import { resolve } from '$app/paths';
	import { siteConfig } from '$lib/config';
	import * as Breadcrumb from '$lib/components/ui/breadcrumb/index.js';

	// The page's path within the docs (`how-to/bound-the-worker`), which never includes the
	// site's base path.
	const segments = $derived((page.params.slug ?? '').split('/').filter(Boolean));
	const title = (segment: string, index: number) =>
		(index === 0 && siteConfig.sections.find((s) => s.dir === segment)?.title) ||
		segment.replace(/-/g, ' ').replace(/\b\w/g, (char) => char.toUpperCase());
	// Only the docs root and the current page are pages; the sections between them are not.
	const breadcrumbs: { name: string; href?: string }[] = $derived([
		{ name: 'Docs', href: resolve('/docs') },
		...segments.map((segment, index) => ({ name: title(segment, index) }))
	]);
</script>

<Breadcrumb.Root>
	<Breadcrumb.List>
		{#each breadcrumbs as breadcrumb, i}
			{#if i > 0}
				<Breadcrumb.Separator class="hidden md:block" />
			{/if}
			<Breadcrumb.Item>
				<!-- Check if it's the last breadcrumb item to render as Page instead of Link -->
				{#if i === breadcrumbs.length - 1}
					<Breadcrumb.Page>{breadcrumb.name}</Breadcrumb.Page>
				{:else if breadcrumb.href === undefined}
					<span class="hidden md:block">{breadcrumb.name}</span>
				{:else}
					<Breadcrumb.Link href={breadcrumb.href} class="hidden md:block"
						>{breadcrumb.name}</Breadcrumb.Link
					>
				{/if}
			</Breadcrumb.Item>
		{/each}
	</Breadcrumb.List>
</Breadcrumb.Root>
