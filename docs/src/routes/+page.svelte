<script>
	import Features from '$lib/components/home/features.svelte';
	import Footer from '$lib/components/home/footer.svelte';
	import Hero from '$lib/components/home/hero.svelte';
	import Nav from '$lib/components/home/nav.svelte';
	import { siteConfig } from '$lib/config';
</script>

<svelte:head>
	<title>{siteConfig.title}: Lean projects as Rust crates</title>
	<meta name="description" content={siteConfig.description} />
</svelte:head>

<Nav />
<Hero />
<Features />
<Footer />
