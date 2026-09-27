import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig } from 'vite';

/** The crate users add to a build, whose releases the site shows. */
const crate = 'lungo-build';

/**
 * The latest release of `crate` on crates.io. The site shows the published version users get
 * from `cargo add`, not the version of the sources it is built from; a build that cannot
 * determine it fails.
 */
async function latestRelease(): Promise<string> {
	const url = `https://crates.io/api/v1/crates/${crate}`;
	// crates.io requires a User-Agent that identifies the client.
	const response = await fetch(url, {
		headers: { 'User-Agent': 'lungo-docs (https://github.com/machinefabric/lungo)' }
	});
	if (!response.ok) {
		throw new Error(`${url}: ${response.status} ${response.statusText}`);
	}
	const version = (await response.json())?.crate?.max_stable_version;
	if (typeof version !== 'string' || version === '') {
		throw new Error(`${url}: ${crate} has no released version`);
	}
	return version;
}

export default defineConfig(async () => ({
	plugins: [sveltekit()],
	define: {
		__LUNGO_CRATE__: JSON.stringify(crate),
		__LUNGO_VERSION__: JSON.stringify(await latestRelease())
	},
	build: {
		target: 'esnext'
	}
}));
