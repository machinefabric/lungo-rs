<script lang="ts">
	import * as Sidebar from '$lib/components/ui/sidebar/index.js';
	import { siteConfig } from '$lib/config';
	import Logo from './logo.svelte';
	import type { ComponentProps } from 'svelte';
	let { ref = $bindable(null), ...restProps }: ComponentProps<typeof Sidebar.Root> = $props();
	import { docsNavigation } from '$lib/components/doc-navigation.svelte';
	import { page } from '$app/state';
	import SocialMedia from './social-media.svelte';
	import { resolve } from '$app/paths';
	import { link } from '$lib/paths';

	const path = $derived(page.url.pathname);
</script>

<Sidebar.Root bind:ref {...restProps}>
	<Sidebar.Header>
		<Sidebar.Menu>
			<Sidebar.MenuItem>
				<Sidebar.MenuButton size="lg">
					{#snippet child({ props })}
						<a href={resolve('/')} {...props}>
							<Logo wordmark={false} class="shrink-0" />
							<div class="flex flex-col gap-0.5 leading-none">
								<span class="font-semibold"> {siteConfig.title} </span>
								<span class="">{siteConfig.version}</span>
							</div>
						</a>
					{/snippet}
				</Sidebar.MenuButton>
			</Sidebar.MenuItem>
		</Sidebar.Menu>
	</Sidebar.Header>
	<Sidebar.Content>
		<Sidebar.Group>
			<Sidebar.Menu>
				{#each docsNavigation.docNav.filter(item => !item.disabled) as groupItem (groupItem.title)}
					<Sidebar.MenuItem>
						<Sidebar.MenuButton class="font-medium" isActive={groupItem.href !== undefined && path === resolve(groupItem.href)}>
							{#snippet child({ props })}
								<a href={link(groupItem.href)} {...props}>
									{groupItem.title}
								</a>
							{/snippet}
						</Sidebar.MenuButton>
						{#if groupItem.items?.length}
							<Sidebar.MenuSub>
								{#each groupItem.items.filter(item => !item.disabled) as item (item.title)}
									<Sidebar.MenuSubItem>
										<Sidebar.MenuSubButton isActive={item.href !== undefined && path === resolve(item.href)}>
											{#snippet child({ props })}
												<a href={link(item.href)} {...props}>{item.title}</a>
											{/snippet}
										</Sidebar.MenuSubButton>
									</Sidebar.MenuSubItem>
								{/each}
							</Sidebar.MenuSub>
						{/if}
					</Sidebar.MenuItem>
				{/each}
			</Sidebar.Menu>
		</Sidebar.Group>
	</Sidebar.Content>
	<div class="block sm:hidden">
		<Sidebar.Footer>
			<SocialMedia />
		</Sidebar.Footer>
	</div>

	<Sidebar.Rail />
</Sidebar.Root>
